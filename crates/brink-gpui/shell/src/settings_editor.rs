//! Settings ▸ Editor (App scope): which view the studio opens in, and
//! what happens to a file on the way to disk.
//!
//! The web studio's `EditorViewSection` + `EditorSection`. The parts of
//! its Editor section that are about type and gutters live in Appearance
//! here (they landed with the theme picker, beside the font sizes they
//! belong with) — this section is the two that were missing: the default
//! view, and fix-on-save beside the format-on-save already there.

use gpui::prelude::*;
use gpui::{App, ClickEvent, Context, IntoElement, Render, Window, div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::editor_view::EditorView;
use crate::settings::{self, AppSettings};
use crate::settings_modal::{setting_group, setting_row};

pub struct EditorSection;

impl EditorSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<AppSettings>(|_, cx| cx.notify())
            .detach();
        Self
    }

    /// One choice in the default-view row: the three views, plus the
    /// "restore the last one" that is the default.
    fn choice(
        id: &'static str,
        label: &'static str,
        on: bool,
        key: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Button::new(id)
            .ghost()
            .xsmall()
            .toggled(on)
            .label(label)
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                let key = key.map(str::to_owned);
                settings::update(cx, |s| s.default_view = key.clone());
            }))
    }
}

impl Render for EditorSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = AppSettings::get(cx);
        let current = settings.default_view.clone();
        let restore = current.is_none();
        let hint = match &current {
            None => "The studio opens in whichever view you left it in.".to_owned(),
            Some(key) => format!(
                "The studio always opens in {}.",
                EditorView::ALL
                    .iter()
                    .find(|v| v.persistence_key() == key)
                    .map_or("that view", |v| v.title())
            ),
        };
        let mut views = Vec::new();
        for view in EditorView::ALL {
            let key = view.persistence_key();
            let on = current.as_deref() == Some(key);
            views.push(
                Self::choice(
                    match view {
                        EditorView::Code => "default-view-code",
                        EditorView::Single => "default-view-single",
                        EditorView::Continuous => "default-view-continuous",
                    },
                    view.title(),
                    on,
                    Some(key),
                    cx,
                )
                .into_any_element(),
            );
        }
        v_flex()
            .w_full()
            .child(setting_group("View", cx))
            .child(setting_row(
                "Open in",
                "Which of the three views a new window starts in.",
                h_flex()
                    .gap_1()
                    .child(Self::choice(
                        "default-view-last",
                        "Restore last",
                        restore,
                        None,
                        cx,
                    ))
                    // Built with a plain loop, not a `map` over `cx`: a
                    // closure capturing the context cannot hand out
                    // listeners that outlive its own call.
                    .children(views),
                cx,
            ))
            .child(
                div()
                    .pb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            )
            .child(setting_group("On save", cx))
            .child(setting_row(
                "Fix on save",
                "Apply every Safe fix to the changed files before they are written. Runs before Format on save, so the formatter lays out what the fixes wrote.",
                Switch::new("fix-on-save")
                    .checked(settings.fix_on_save)
                    .on_click(|on, _, cx: &mut App| {
                        let on = *on;
                        settings::update(cx, |s| s.fix_on_save = on);
                    }),
                cx,
            ))
    }
}
