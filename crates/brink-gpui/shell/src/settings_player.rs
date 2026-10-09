//! Settings ▸ Player (App scope): how the story behaves when you press
//! Play, and how its prose is set.
//!
//! App scope throughout, as the web studio rules it: how fast lines land
//! in *your* Player and how large *your* transcript is are machine
//! preferences, never properties of the project.
//!
//! Three settings, each of which something reads:
//!
//! - **Follow in editor** — the Player reveals each line's source as it
//!   plays, which is what makes the Player and the editor one tool rather
//!   than two windows onto the same story. It pauses itself while you are
//!   editing and Play or Restart resumes it (`app/src/player.rs`).
//! - **Autoplay pace** — how long `>>` waits between lines, in the web's
//!   unit (milliseconds per line).
//! - **Player font size** — the transcript's prose only. `0` follows the
//!   app type scale, so the default needs no second number to keep in step
//!   with the app's.
//!
//! What the web has here and this does not: paced auto-reveal (the native
//! Player advances a line per press, and `>>` is its pacing), debug info (the compile always carries it) and the
//! external-function check (nothing binds externals in the studio yet).

use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, Window, div};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, v_flex};

use crate::settings::{
    self, AppSettings, MAX_AUTOPLAY_MS, MAX_EDITOR_FONT_SIZE, MIN_AUTOPLAY_MS,
    MIN_EDITOR_FONT_SIZE, clamp_font_size,
};
use crate::settings_modal::{setting_group, setting_row, setting_stepper, setting_stepper_by};

pub struct PlayerSection;

impl PlayerSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<AppSettings>(|_, cx| cx.notify())
            .detach();
        Self
    }
}

impl Render for PlayerSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = AppSettings::get(cx);
        let size = settings.player_font_size;
        v_flex()
            .w_full()
            .child(setting_group("Playback", cx))
            .child(setting_row(
                "Follow in editor",
                "As the story plays, reveal each line's source in the editor. Pauses while you are editing; Play or Restart resumes it.",
                Switch::new("follow-in-editor")
                    .checked(settings.follow_in_editor)
                    .on_click(|on, _, cx| {
                        let on = *on;
                        settings::update(cx, |s| s.follow_in_editor = on);
                    }),
                cx,
            ))
            .child(setting_row(
                "Autoplay pace",
                "How long >> waits from one line to the next. It stops at a breakpoint, a choice, or the end.",
                setting_stepper_by(
                    "autoplay-pace",
                    settings.autoplay_ms,
                    100.,
                    format!("{:.1} s / line", settings.autoplay_ms / 1000.),
                    |next, _, cx| {
                        let next = next.clamp(MIN_AUTOPLAY_MS, MAX_AUTOPLAY_MS);
                        settings::update(cx, |s| s.autoplay_ms = next);
                    },
                    cx,
                ),
                cx,
            ))
            .child(setting_row(
                "Keep new saves on this computer",
                "Save state writes to the project's .brink/saves/ folder, shared through the repo; on, new saves stay in this computer's app data. Both are always listed.",
                Switch::new("saves-on-this-computer")
                    .checked(settings.saves_on_this_computer)
                    .on_click(|on, _, cx| {
                        let on = *on;
                        settings::update(cx, |s| s.saves_on_this_computer = on);
                    }),
                cx,
            ))
            .child(setting_group("Reading", cx))
            .child(setting_row(
                "Player font size",
                format!(
                    "{MIN_EDITOR_FONT_SIZE:.0}–{MAX_EDITOR_FONT_SIZE:.0} px; 0 follows the app type scale. Sizes the transcript's prose only, not the studio's chrome."
                ),
                setting_stepper(
                    "player-font-size",
                    size,
                    "px",
                    move |next, _, cx| {
                        // 0 is "follow the app", and it sits below the
                        // smallest real size — so the step off it in
                        // either direction has to be spelled out, or a
                        // press of `+` from 0 lands on 1, clamps back to
                        // 0, and the control looks broken (it did).
                        let next = if next < MIN_EDITOR_FONT_SIZE {
                            if next > size {
                                MIN_EDITOR_FONT_SIZE
                            } else {
                                0.
                            }
                        } else {
                            clamp_font_size(next, 0., MIN_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE)
                        };
                        settings::update(cx, |s| s.player_font_size = next);
                    },
                    cx,
                ),
                cx,
            ))
            .child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if size > 0. {
                        format!("The transcript is set at {size:.0} px.")
                    } else {
                        format!(
                            "The transcript follows the app: {:.0} px.",
                            settings.player_size()
                        )
                    }),
            )
    }
}
