//! Settings ▸ Prose (Project scope): `[prose]` — the project's spell and
//! grammar checking, the studio's `ProseSettings` (#3211).
//!
//! Dialect is not a nicety and not deferrable: measured against the checker
//! with the American dialect, "The colour of the harbour at night." reports
//! BOTH words as misspellings — so a British-English author with no way to
//! say so gets their entire manuscript underlined, which is
//! indistinguishable from the feature being broken.
//!
//! `enable` is separate from whether a checker is registered at all: an
//! embedder decides whether the engine is available, and this decides
//! whether a project that HAS it wants its prose checked. Those are
//! different decisions by different people. (The native studio has no
//! prose checker yet — the settings are the project's and are written the
//! same, so a project opened in both studios reads the same.)
//!
//! The dictionary lives in `brink.toml` (ruled 2026-08-28: a character's
//! name is a fact about the manuscript, shared by collaborators, surviving
//! a fresh clone), matched literally — so `Griswold` and `GRISWOLD` are two
//! entries, shown as written rather than folded, because folding them in
//! the view would misrepresent what the checker accepts.

use brink_gpui_shell::settings_modal::{setting_group, setting_row};
use brink_project_config::ProseDialect;
use brink_project_config::edit::ConfigDocument;
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, IntoElement, Render, SharedString, Subscription,
    Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};
use crate::settings_config::{broken_notice, config_text, edit_config, no_config, parse_error};
use crate::settings_general::Opt;

/// The dialects, as `ProseDialect::as_str` spells them, with their labels.
pub const DIALECTS: [(ProseDialect, &str); 4] = [
    (ProseDialect::American, "American"),
    (ProseDialect::British, "British"),
    (ProseDialect::Canadian, "Canadian"),
    (ProseDialect::Australian, "Australian"),
];

/// `[prose] dialect` as written, or the default. Read from the `[prose]`
/// table by name — `[project]` has a `dialect` of its own, the surface
/// dialect, and reading that one here would show the wrong value and then
/// overwrite it.
#[must_use]
pub fn dialect_of(doc: &ConfigDocument) -> String {
    doc.string("prose", "dialect")
        .unwrap_or_else(|| ProseDialect::default().as_str().to_owned())
}

/// `[prose] enable`, defaulting to on.
#[must_use]
pub fn enabled_in(doc: &ConfigDocument) -> bool {
    doc.bool("prose", "enable").unwrap_or(true)
}

/// The dictionary as written, in the author's order; a malformed key
/// reads as empty rather than failing the section.
#[must_use]
pub fn dictionary_of(doc: &ConfigDocument) -> Vec<String> {
    doc.string_array("prose", "dictionary").unwrap_or_default()
}

pub struct ProseSection {
    project: Entity<Project>,
    dialect: Entity<SelectState<Vec<Opt>>>,
    word: Entity<InputState>,
    /// True while the select is being set from the file, whose Confirm is
    /// an echo, not a choice.
    syncing: bool,
    _subscriptions: Vec<Subscription>,
}

impl ProseSection {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let options: Vec<Opt> = DIALECTS
            .iter()
            .map(|(d, label)| Opt {
                value: d.as_str().to_owned(),
                label: SharedString::from(*label),
            })
            .collect();
        let dialect = cx.new(|cx| SelectState::new(options, None, window, cx));
        let word = cx.new(|cx| InputState::new(window, cx).placeholder("Add a word\u{2026}"));
        let on_dialect = cx.subscribe(
            &dialect,
            |this: &mut Self, _, event: &SelectEvent<Vec<Opt>>, cx| {
                if this.syncing {
                    return;
                }
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                let value = value.clone();
                edit_config(&this.project, cx, |doc| {
                    doc.set_string("prose", "dialect", &value)?;
                    Ok(true)
                });
            },
        );
        let on_word = cx.subscribe_in(&word, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.add_word(window, cx);
            }
        });
        let on_project = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| {
                if matches!(
                    event,
                    ProjectEvent::Opened { .. } | ProjectEvent::SourceChanged { .. }
                ) {
                    this.sync(window, cx);
                }
            },
        );
        let mut this = Self {
            project,
            dialect,
            word,
            syncing: false,
            _subscriptions: vec![on_dialect, on_word, on_project],
        };
        this.sync(window, cx);
        this
    }

    /// Set the select from the file.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialect = config_text(&self.project, cx)
            .and_then(|(_, text)| ConfigDocument::parse(&text).ok())
            .map(|doc| dialect_of(&doc));
        if let Some(dialect) = dialect {
            self.syncing = true;
            self.dialect.update(cx, |select, cx| {
                select.set_selected_value(&dialect, window, cx);
            });
            self.syncing = false;
        }
        cx.notify();
    }

    fn add_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let word = self.word.read(cx).value().trim().to_string();
        if word.is_empty() {
            return;
        }
        edit_config(&self.project, cx, |doc| {
            doc.add_to_string_array("prose", "dictionary", &word)
        });
        self.word.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
    }

    fn render_dictionary(&self, words: &[String], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.mono_font_family.clone(),
        );
        let rows: Vec<AnyElement> = words
            .iter()
            .enumerate()
            .map(|(ix, word)| {
                let word_for_remove = word.clone();
                h_flex()
                    .w_full()
                    .h(px(26.))
                    .gap_3()
                    .items_center()
                    .border_b_1()
                    .border_color(border.opacity(0.5))
                    .child(
                        div()
                            .flex_1()
                            .font_family(mono.clone())
                            .text_sm()
                            .text_color(fg)
                            .child(word.clone()),
                    )
                    .child(
                        Button::new(("dict-remove", ix))
                            .ghost()
                            .xsmall()
                            .label("\u{00d7}")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let word = word_for_remove.clone();
                                edit_config(&this.project, cx, |doc| {
                                    doc.remove_from_string_array("prose", "dictionary", &word)
                                });
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Your project's own names — knots, stitches, structs and flows — are known words automatically. This list is for everything else: place names, in-world jargon, and character names the checker has not picked up."),
            )
            .when(words.is_empty(), |el| {
                el.child(div().text_xs().text_color(muted).child("No words yet."))
            })
            .children(rows)
            .child(
                h_flex()
                    .w_full()
                    .pt_1()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(Input::new(&self.word).small()))
                    .child(
                        Button::new("dict-add")
                            .outline()
                            .small()
                            .label("Add")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.add_word(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ProseSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((_, text)) = config_text(&self.project, cx) else {
            return no_config("prose settings", cx);
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
        let enabled = enabled_in(&doc);
        let words = dictionary_of(&doc);
        let project = self.project.clone();
        v_flex()
            .w_full()
            .gap_1()
            .child(setting_group("Checking", cx))
            .child(setting_row(
                "Check prose",
                "Spelling and light grammar over the manuscript's prose — never over diverts, tags, or logic.",
                Switch::new("prose-enable")
                    .checked(enabled)
                    .on_click(move |on, _, cx| {
                        let on = *on;
                        edit_config(&project, cx, |doc| {
                            doc.set_bool("prose", "enable", on)?;
                            Ok(true)
                        });
                    }),
                cx,
            ))
            .child(setting_row(
                "Dialect",
                "Which English the checker judges by. Set this before anything else — under the wrong dialect an author who writes \u{201c}colour\u{201d} sees their whole manuscript underlined.",
                Select::new(&self.dialect).small().w(px(200.)),
                cx,
            ))
            .child(setting_group("Dictionary", cx))
            .child(self.render_dictionary(&words, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_keys_read_with_their_defaults_and_never_the_project_dialect() {
        let doc = ConfigDocument::parse("[project]\ndialect = \"brink\"\n").expect("valid");
        assert_eq!(
            dialect_of(&doc),
            "american",
            "[project] dialect is not ours"
        );
        assert!(enabled_in(&doc));
        assert!(dictionary_of(&doc).is_empty());

        let doc = ConfigDocument::parse(
            "[prose]\ndialect = \"british\"\nenable = false\ndictionary = [\"Griswold\", \"GRISWOLD\"]\n",
        )
        .expect("valid");
        assert_eq!(dialect_of(&doc), "british");
        assert!(!enabled_in(&doc));
        assert_eq!(
            dictionary_of(&doc),
            ["Griswold", "GRISWOLD"],
            "literal, unfolded"
        );
        assert!(
            DIALECTS.iter().any(|(d, _)| d.as_str() == "british"),
            "the select's values are the config's spellings"
        );
    }
}
