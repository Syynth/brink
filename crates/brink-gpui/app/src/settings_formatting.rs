//! Settings ▸ Formatting (Project scope): `[project] indent` — the studio's
//! `FormattingSettings` (#3149, design round 2026-08-27).
//!
//! Indent size is the one setting every component reads (the formatter
//! emits it, the editor types it, the guides are drawn against it), which
//! is exactly why it deserves a control rather than a line an author has
//! to know exists. When unset the width is 4 (ruled 2026-08-27), and
//! **removing the key is not the same as writing the default**: one says
//! "this project has no opinion", the other pins it, and only the first
//! follows a later change to the default — so a configured value gets a
//! reset that removes the key.
//!
//! Its own section rather than a group inside General: it is about how
//! the project's TEXT is shaped, which is a different question from what
//! it compiles and how it is typed.

use brink_gpui_shell::settings_modal::{setting_group, setting_row, setting_stepper};
use brink_project_config::DEFAULT_INDENT;
use brink_project_config::edit::ConfigDocument;
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, IntoElement, Render, Subscription, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};
use crate::settings_config::{broken_notice, config_text, edit_config, no_config, parse_error};

/// `brink_project_config::INDENT_RANGE`, which is private there.
pub const MIN_INDENT: u8 = 1;
pub const MAX_INDENT: u8 = 16;

/// The indent the file configures, or `None` when it has no opinion (or
/// an opinion outside the range, which the parser already warns about).
#[must_use]
pub fn configured_indent(doc: &ConfigDocument) -> Option<u8> {
    doc.integer("project", "indent")
        .and_then(|n| u8::try_from(n).ok())
        .filter(|n| (MIN_INDENT..=MAX_INDENT).contains(n))
}

/// Clamp a stepped value into the config's range.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into 1..=16 before the cast"
)]
pub fn clamp_indent(value: f32) -> u8 {
    if value.is_finite() {
        // The stepper hands back a float; the value is a small integer.
        value
            .round()
            .clamp(f32::from(MIN_INDENT), f32::from(MAX_INDENT)) as u8
    } else {
        DEFAULT_INDENT
    }
}

pub struct FormattingSection {
    project: Entity<Project>,
    _subscriptions: Vec<Subscription>,
}

impl FormattingSection {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let on_project = cx.subscribe(&project, |_, _, event: &ProjectEvent, cx| {
            if matches!(
                event,
                ProjectEvent::Opened { .. } | ProjectEvent::SourceChanged { .. }
            ) {
                cx.notify();
            }
        });
        Self {
            project,
            _subscriptions: vec![on_project],
        }
    }

    fn set_indent(&self, value: Option<u8>, cx: &mut Context<Self>) {
        edit_config(&self.project, cx, |doc| match value {
            Some(value) => {
                doc.set_integer("project", "indent", i64::from(value))?;
                Ok(true)
            }
            None => doc.remove_key("project", "indent"),
        });
    }
}

impl Render for FormattingSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let Some((_, text)) = config_text(&self.project, cx) else {
            return no_config("formatting settings", cx);
        };
        let doc = match ConfigDocument::parse(&text) {
            Ok(doc) => doc,
            Err(_) => {
                return v_flex()
                    .w_full()
                    .children(parse_error(&text).map(|reason| broken_notice(&reason, cx)))
                    .into_any_element();
            }
        };
        let configured = configured_indent(&doc);
        let indent = configured.unwrap_or(DEFAULT_INDENT);
        let me = cx.entity().downgrade();
        let stepper = setting_stepper(
            "indent",
            f32::from(indent),
            " spaces",
            move |next, _, cx| {
                let next = clamp_indent(next);
                _ = me.update(cx, |this, cx| this.set_indent(Some(next), cx));
            },
            cx,
        );
        let control: AnyElement = h_flex()
            .gap_2()
            .items_center()
            .child(stepper)
            .when(configured.is_some(), |el| {
                el.child(
                    Button::new("indent-reset")
                        .ghost()
                        .xsmall()
                        .label("Reset")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.set_indent(None, cx);
                        })),
                )
            })
            .into_any_element();
        let description = if configured.is_some() {
            "One value, read by everything that indents — the formatter writes it, the editor types it, and the guides are drawn against it. Reset removes the key, so the default applies.".to_owned()
        } else {
            format!(
                "One value, read by everything that indents — the formatter writes it, the editor types it, and the guides are drawn against it. Not set, so the default of {DEFAULT_INDENT} applies."
            )
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(setting_group("Indentation", cx))
            .child(setting_row("Indent size", description, control, cx))
            .child(setting_row(
                "Indent character (not ruled)",
                "The formatter already models tabs, but brink.toml has no key for it — tabs-vs-spaces was deliberately left out of the indent-size ruling.",
                h_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .rounded_sm()
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .text_color(muted)
                            .child("spaces"),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(muted.opacity(0.6))
                            .child("tabs"),
                    ),
                cx,
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indent_is_read_clamped_and_reset_by_removal() {
        let doc = ConfigDocument::parse("[project]\nentry = \"a.ink\"\n").expect("valid");
        assert_eq!(configured_indent(&doc), None, "no opinion");
        let mut doc = doc;
        doc.set_integer("project", "indent", 2).expect("a table");
        assert_eq!(configured_indent(&doc), Some(2));
        assert!(doc.remove_key("project", "indent").expect("a table"));
        assert_eq!(configured_indent(&doc), None);
        assert_eq!(clamp_indent(0.), MIN_INDENT);
        assert_eq!(clamp_indent(99.), MAX_INDENT);
        assert_eq!(clamp_indent(3.), 3);
        assert_eq!(clamp_indent(f32::NAN), DEFAULT_INDENT);
        let wide = ConfigDocument::parse("[project]\nindent = 40\n").expect("valid");
        assert_eq!(
            configured_indent(&wide),
            None,
            "out of range reads as no opinion"
        );
    }
}
