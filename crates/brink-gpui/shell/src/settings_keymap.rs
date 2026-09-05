//! Settings ▸ Keymap (App scope): rebind commands by pressing the keys —
//! the studio's `KeymapSettings`.
//!
//! A table of every registered command with its current keystroke and
//! where that came from (default, custom, unbound). **Record** on a row
//! takes the keyboard through gpui's keystroke interceptor
//! (`App::intercept_keystrokes`), which runs BEFORE the keymap: an element
//! key listener would be too late, since gpui matches bindings first and
//! the chord being recorded would fire the command it already belongs to
//! (it did — the first version switched views instead of recording). The
//! chord is read from the same keystroke the window dispatches, then
//! spelled back with `Keystroke::unparse` — the spelling gpui binds. A
//! typed spelling can be right and still not be the chord the keyboard
//! produces; recording sidesteps that.
//!
//! Rebinding **displaces** (ruled 2026-08-30): a chord taken for one
//! command comes off the command that held it, and the row says so. The
//! model is `commands::bind_chord`; this is its view. Escape cancels a
//! recording; while recording, nothing else hears the keyboard.

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, IntoElement, Keystroke, Render, SharedString,
    Subscription, WeakEntity, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::commands::{
    KeySource, canonical_chord, display_keystroke, effective_keystroke, key_source,
};
use crate::settings::AppSettings;
use crate::workspace::Workspace;

pub struct KeymapSection {
    workspace: WeakEntity<Workspace>,
    filter: Entity<InputState>,
    query: String,
    /// The row recording a chord, if any, and the interceptor that owns
    /// the keyboard while it does — dropped to give it back.
    recording: Option<(usize, Subscription)>,
    /// What the last change did — "Took ⌘K from Search: Find in Files".
    notice: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// Keys that arrive on their own when a modifier goes down; a chord needs
/// a key under them.
const MODIFIER_KEYS: [&str; 7] = [
    "shift", "control", "ctrl", "alt", "cmd", "platform", "function",
];

/// The chord a recorded keystroke spells, or `None` for a modifier alone.
/// The platform modifier is spelled `cmd` whatever the platform calls it
/// (`super` on Linux, `win` on Windows — gpui parses all three), so a
/// persisted chord reads the same everywhere and displays as ⌘.
#[must_use]
pub fn recorded_chord(keystroke: &gpui::Keystroke) -> Option<String> {
    if MODIFIER_KEYS.contains(&keystroke.key.as_str()) {
        return None;
    }
    Some(canonical_chord(&keystroke.unparse()))
}

impl KeymapSection {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter commands"));
        let on_filter = cx.subscribe(&filter, |this: &mut Self, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = state.read(cx).value().to_string();
                cx.notify();
            }
        });
        let on_settings = cx.observe_global::<AppSettings>(|_, cx| cx.notify());
        Self {
            workspace,
            filter,
            query: String::new(),
            recording: None,
            notice: None,
            _subscriptions: vec![on_filter, on_settings],
        }
    }

    fn start_recording(&mut self, index: usize, cx: &mut Context<Self>) {
        self.notice = None;
        let me = cx.entity().downgrade();
        // While recording, the keyboard is ours: a chord being recorded
        // must not also fire, and Escape must cancel rather than close the
        // window over us.
        let interceptor = cx.intercept_keystrokes(move |event, _window, cx| {
            cx.stop_propagation();
            _ = me.update(cx, |this, cx| this.on_keystroke(&event.keystroke, cx));
        });
        self.recording = Some((index, interceptor));
        cx.notify();
    }

    fn on_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some((index, _)) = &self.recording else {
            return;
        };
        let index = *index;
        if keystroke.key == "escape" {
            self.recording = None;
            cx.notify();
            return;
        }
        let Some(chord) = recorded_chord(keystroke) else {
            return;
        };
        self.recording = None;
        let displaced = self
            .workspace
            .update(cx, |workspace, cx| workspace.rebind(index, &chord, cx))
            .ok()
            .flatten();
        self.notice = displaced.map(|title| {
            SharedString::from(format!("Took {} from {title}", display_keystroke(&chord)))
        });
        cx.notify();
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        let workspace = self.workspace.upgrade()?;
        let command = workspace.read(cx).commands().commands().get(index)?.clone();
        let overrides = AppSettings::get(cx).keymap;
        let keys = effective_keystroke(&command, &overrides);
        let source = key_source(&command, &overrides);
        let theme = cx.theme();
        let (fg, muted, border, accent, surface) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.primary,
            theme.sidebar,
        );
        let recording = self.recording.as_ref().is_some_and(|(ix, _)| *ix == index);
        let chip: AnyElement = if recording {
            div()
                .px_2()
                .rounded_sm()
                .border_1()
                .border_color(accent)
                .text_xs()
                .text_color(accent)
                .child("Press keys\u{2026} (Esc cancels)")
                .into_any_element()
        } else {
            match &keys {
                Some(keys) => div()
                    .px_2()
                    .rounded_sm()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .text_color(fg)
                    .child(display_keystroke(keys))
                    .into_any_element(),
                None => div()
                    .text_xs()
                    .text_color(muted)
                    .child("\u{2014}")
                    .into_any_element(),
            }
        };
        let source_label = match source {
            KeySource::Default => "default",
            KeySource::Custom => "custom",
            KeySource::Unbound => "unbound",
        };
        Some(
            h_flex()
                .id(("keymap-row", index))
                .w_full()
                .h(px(30.))
                .px_2()
                .gap_3()
                .items_center()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .text_sm()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(fg)
                        .child(SharedString::from(command.full_title())),
                )
                .child(div().w(px(120.)).flex_shrink_0().child(chip))
                .child(
                    div()
                        .w(px(56.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(muted)
                        .child(source_label),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .child(
                            Button::new(("keymap-record", index))
                                .outline()
                                .xsmall()
                                .label(if recording { "Recording" } else { "Record" })
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.start_recording(index, cx);
                                })),
                        )
                        .when(keys.is_some(), |el| {
                            el.child(
                                Button::new(("keymap-unbind", index))
                                    .ghost()
                                    .xsmall()
                                    .label("Unbind")
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.notice = None;
                                        _ = this.workspace.update(cx, |workspace, cx| {
                                            workspace.unbind_command(index, cx);
                                        });
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(source != KeySource::Default, |el| {
                            el.child(
                                Button::new(("keymap-reset", index))
                                    .ghost()
                                    .xsmall()
                                    .label("Reset")
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.notice = None;
                                        _ = this.workspace.update(cx, |workspace, cx| {
                                            workspace.reset_command(index, cx);
                                        });
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}

impl Render for KeymapSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let warning = cx.theme().warning;
        let needle = self.query.trim().to_lowercase();
        let count = self
            .workspace
            .upgrade()
            .map_or(0, |w| w.read(cx).commands().commands().len());
        let titles: Vec<String> = self.workspace.upgrade().map_or_else(Vec::new, |w| {
            w.read(cx)
                .commands()
                .commands()
                .iter()
                .map(|c| c.full_title().to_lowercase())
                .collect()
        });
        let rows: Vec<AnyElement> = (0..count)
            .filter(|&ix| needle.is_empty() || titles[ix].contains(&needle))
            .filter_map(|ix| self.render_row(ix, cx))
            .collect();
        v_flex()
            .id("settings-keymap")
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Record presses the keys; a chord taken from another command comes off it and says so. Reset returns a command to its shipped default."),
            )
            .child(Input::new(&self.filter).small())
            // A fixed slot, so a notice appearing never shifts the rows
            // under a pointer that is about to click one.
            .child(
                div()
                    .h(px(18.))
                    .text_xs()
                    .text_color(warning)
                    .children(self.notice.clone()),
            )
            .child(v_flex().w_full().children(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modifier_alone_is_not_a_chord() {
        let shift = gpui::Keystroke::parse("shift").unwrap();
        assert_eq!(recorded_chord(&shift), None);
        let chord = gpui::Keystroke::parse("cmd-shift-p").unwrap();
        assert_eq!(
            recorded_chord(&chord),
            Some(canonical_chord("shift-cmd-p")),
            "the platform modifier is spelled cmd on every platform, in one order"
        );
        let minus = gpui::Keystroke::parse("ctrl--").unwrap();
        assert_eq!(recorded_chord(&minus).as_deref(), Some("ctrl--"));
    }
}
