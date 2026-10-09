//! Settings ▸ Spelling & Grammar (App scope): which checker does what on
//! this machine (`docs/gpui-prose-checker-spec.md` §6.1, §8).
//!
//! App scope because it picks between the checkers this machine has: a
//! collaborator on Linux has no macOS checker to pick, so the choice cannot
//! live in `brink.toml`. What the project asks for — whether its prose is
//! checked at all, in which English, with which invented words — stays in
//! Project ▸ Prose.
//!
//! Three things, the first and last of which are facts rather than
//! choices:
//!
//! - **Spelling** — which checker finds misspellings: macOS's own on
//!   macOS, Harper elsewhere.
//! - **Grammar while typing** — `AppSettings.prose_grammar`.
//! - **Apple Intelligence** (macOS) — whether "Check Grammar with Apple
//!   Intelligence" is offered here, and if not, why not.

use std::time::Duration;

use brink_gpui_model::prose::{Grammar, ModelState, model_state};
use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, Window, div};
use gpui_component::{ActiveTheme as _, v_flex};

use crate::settings::{self, AppSettings};
use crate::settings_modal::{Segment, setting_group, setting_row, setting_segments};

/// How long the section keeps asking whether the model answers. The
/// question is settled in the background within the canary's 30 s; this
/// only bounds the asking if it never is.
const ASK_FOR: Duration = Duration::from_secs(90);

pub struct SpellingSection;

impl SpellingSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<AppSettings>(|_, cx| cx.notify())
            .detach();
        // Whether the model answers is found out in the background and
        // announces nothing, so the section asks until it knows.
        brink_gpui_model::prose::probe_model();
        cx.spawn(async move |this, cx| {
            let started = std::time::Instant::now();
            while model_state() == ModelState::Unknown && started.elapsed() < ASK_FOR {
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
            _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        Self
    }
}

/// A grammar choice's name on its segment.
fn label(grammar: Grammar) -> &'static str {
    match grammar {
        Grammar::Harper => "Harper",
        #[cfg(target_os = "macos")]
        Grammar::MacOs => "macOS",
        Grammar::Off => "Off",
    }
}

/// What a grammar choice does, said under the buttons.
fn hint(grammar: Grammar) -> &'static str {
    match grammar {
        Grammar::Harper => {
            "Harper's rules. The strongest of the quick checks, and the only one that catches spacing and punctuation slips; it can also flag a character's dialect in dialogue."
        }
        #[cfg(target_os = "macos")]
        Grammar::MacOs => {
            "macOS's quick rules, plus anything Apple Intelligence has already checked in the same sentences. Quieter than Harper, and it misses more."
        }
        Grammar::Off => "No grammar while you type: misspellings only.",
    }
}

fn spelling_checker() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS. Words you have taught macOS count as words; the project's own come from Project \u{25b8} Prose."
    } else {
        "Harper. The project's own words come from Project \u{25b8} Prose."
    }
}

fn model_status(state: ModelState) -> &'static str {
    match state {
        ModelState::Available => {
            "Available. Select some prose, right-click, and choose Check Grammar with Apple Intelligence. It reads whole sentences and takes a few seconds; its suggestions stay until you edit the sentence."
        }
        ModelState::Unknown => "Finding out whether it is available on this Mac\u{2026}",
        ModelState::Unavailable => {
            "Not available on this Mac. It needs macOS 27 or later with Apple Intelligence turned on."
        }
    }
}

impl Render for SpellingSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = AppSettings::get(cx).prose_grammar;
        let muted = cx.theme().muted_foreground;
        let note = |text: &'static str| div().pb_1().text_xs().text_color(muted).child(text);
        let choices = Grammar::ALL
            .into_iter()
            .map(|grammar| Segment {
                id: format!("prose-grammar-{}", grammar.as_str()).into(),
                label: label(grammar).into(),
                on: grammar == current,
                choose: Box::new(move |_, _, cx| {
                    settings::update(cx, |s| s.prose_grammar = grammar);
                }),
            })
            .collect();
        let mut section = v_flex()
            .w_full()
            .child(setting_group("Spelling", cx))
            .child(note(spelling_checker()))
            .child(setting_group("Grammar", cx))
            .child(setting_row(
                "While typing",
                "Which grammar is checked as you write. Spelling is checked whichever you pick.",
                setting_segments(choices, cx),
                cx,
            ))
            .child(note(hint(current)));
        if cfg!(target_os = "macos") {
            section = section
                .child(setting_group("Apple Intelligence", cx))
                .child(note(model_status(model_state())));
        }
        section
    }
}
