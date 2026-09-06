//! Settings ▸ Appearance (App scope): the theme, the two font sizes, and
//! the editor's gutters and inlay hints — the studio's Appearance section.
//!
//! **The theme picker shows the theme, not its name** (#3174): every tile
//! is a snippet painted in that theme's own tokens, the same six lines the
//! studio's picker renders (a definition, a comment, a cue, prose, the
//! structure markers, a divert into a halt) — the roles a theme actually
//! decides between. A tile is built from `theme::Tokens` directly, so a
//! colour tuned there is tuned here; there is no second palette to drift.
//!
//! Every control writes through `settings::update` and then applies the
//! change live: the theme through `theme::select`, the editor size by
//! re-applying the theme (the kit's editor reads `mono_font_size` from the
//! global theme on each render), the app size through the window's rem
//! size, and the editor toggles through the settings global every editor
//! observes.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, FontWeight, IntoElement, Render, SharedString, Window,
    div, px,
};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use crate::settings::{
    self, AppSettings, DEFAULT_APP_FONT_SIZE, DEFAULT_EDITOR_FONT_SIZE, MAX_APP_FONT_SIZE,
    MAX_EDITOR_FONT_SIZE, MIN_APP_FONT_SIZE, MIN_EDITOR_FONT_SIZE,
};
use crate::settings_modal::{setting_group, setting_row, setting_stepper};
use crate::theme::{self, StudioTheme, hsla};

pub struct AppearanceSection;

impl AppearanceSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Redraw on any settings change, wherever it came from — a palette
        // theme command while the window is open, say.
        cx.observe_global::<AppSettings>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<gpui_component::Theme>(|_, cx| cx.notify())
            .detach();
        Self
    }

    /// The tile: the theme's snippet in its own colours, with its name
    /// under it; the ring, not a fill, marks the current one — the tile's
    /// inside belongs to the theme it shows.
    fn tile(&self, theme: &StudioTheme, current: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = theme.tokens;
        let accent = cx.theme().primary;
        let (fg, muted, border) = (
            cx.theme().foreground,
            cx.theme().muted_foreground,
            cx.theme().border,
        );
        let mono = cx.theme().mono_font_family.clone();
        let line = |parts: Vec<(&'static str, u32, FontWeight, bool)>| {
            h_flex()
                .h(px(14.))
                .children(parts.into_iter().map(|(text, colour, weight, italic)| {
                    div()
                        .text_color(hsla(colour))
                        .font_weight(weight)
                        .when(italic, gpui::Styled::italic)
                        .child(text)
                }))
        };
        let normal = FontWeight::NORMAL;
        let cue_weight = if t.cue_weight >= 600 {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let preview = v_flex()
            .w_full()
            .px(px(10.))
            .py(px(9.))
            .rounded_md()
            .border_2()
            .border_color(if current { accent } else { border })
            .bg(hsla(t.editor_bg))
            .font_family(mono)
            .text_size(px(9.))
            .child(line(vec![
                ("= ", t.syn_keyword, FontWeight::SEMIBOLD, false),
                ("haggle", t.syn_namespace, normal, false),
            ]))
            .child(line(vec![(
                "// the lantern scene",
                t.syn_comment,
                normal,
                true,
            )]))
            .child(line(vec![("KID", t.cue(), cue_weight, false)]))
            .child(line(vec![(
                "How much for the lantern?",
                t.fg,
                normal,
                false,
            )]))
            .child(line(vec![
                ("* [", t.marker(), normal, false),
                ("Offer five", t.fg, normal, false),
                ("]", t.marker(), normal, false),
            ]))
            .child(line(vec![
                ("  -> ", t.divert(), normal, false),
                ("DONE", t.halt(), normal, false),
            ]));
        let id = theme.id;
        v_flex()
            .id(SharedString::from(format!("theme-tile-{id}")))
            .w(px(172.))
            .gap_1()
            .cursor_pointer()
            .child(preview)
            .child(
                div()
                    .text_xs()
                    .text_color(if current { fg } else { muted })
                    .child(theme.label),
            )
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                theme::select(id, Some(window), cx);
            }))
            .into_any_element()
    }
}

/// Set the editor's text size: persist, then re-apply the theme, whose
/// config carries the size the kit's editors read.
pub fn set_editor_font_size(size: f32, window: &mut Window, cx: &mut App) {
    let size = settings::clamp_font_size(
        size,
        DEFAULT_EDITOR_FONT_SIZE,
        MIN_EDITOR_FONT_SIZE,
        MAX_EDITOR_FONT_SIZE,
    );
    settings::update(cx, |s| s.editor_font_size = size);
    if let Err(err) = theme::apply(&theme::current(cx), Some(window), cx) {
        eprintln!("editor font size: {err:#}");
    }
}

/// Set the app's UI size: persist, then scale every window's rem.
pub fn set_app_font_size(size: f32, cx: &mut App) {
    let size = settings::clamp_font_size(
        size,
        DEFAULT_APP_FONT_SIZE,
        MIN_APP_FONT_SIZE,
        MAX_APP_FONT_SIZE,
    );
    settings::update(cx, |s| s.app_font_size = size);
    apply_app_font_size(cx);
}

/// Push the settings' app size onto every open window.
pub fn apply_app_font_size(cx: &mut App) {
    let rem = AppSettings::get(cx).rem_size();
    for handle in cx.windows() {
        _ = handle.update(cx, |_, window, _| window.set_rem_size(px(rem)));
    }
    cx.refresh_windows();
}

impl Render for AppearanceSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = AppSettings::get(cx);
        let current = theme::current_id(cx);
        let tiles: Vec<AnyElement> = theme::builtin()
            .iter()
            .map(|t| self.tile(t, t.id == current.as_ref(), cx))
            .collect();
        v_flex()
            .w_full()
            .gap_1()
            .child(setting_group("Theme", cx))
            .child(
                div()
                    .pb_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Applies immediately. Each tile is the real theme."),
            )
            .child(h_flex().flex_wrap().gap_3().children(tiles))
            .child(setting_group("Editor", cx))
            .child(setting_row(
                "Show gutters",
                "Line numbers beside the text.",
                Switch::new("show-gutters")
                    .checked(settings.show_gutters)
                    .on_click(|on, _, cx| {
                        let on = *on;
                        settings::update(cx, |s| s.show_gutters = on);
                    }),
                cx,
            ))
            .child(setting_row(
                "Show inlay hints",
                "Parameter names drawn inside the line. Applies live to every open editor.",
                Switch::new("show-inlay-hints")
                    .checked(settings.show_inlay_hints)
                    .on_click(|on, _, cx| {
                        let on = *on;
                        settings::update(cx, |s| s.show_inlay_hints = on);
                    }),
                cx,
            ))
            .child(setting_row(
                "Format on save",
                "Run the formatter over every changed .ink file before it is written, with the project's indent.",
                Switch::new("format-on-save")
                    .checked(settings.format_on_save)
                    .on_click(|on, _, cx| {
                        let on = *on;
                        settings::update(cx, |s| s.format_on_save = on);
                    }),
                cx,
            ))
            .child(setting_row(
                "Editor font size",
                format!(
                    "{MIN_EDITOR_FONT_SIZE:.0}–{MAX_EDITOR_FONT_SIZE:.0} px, default {DEFAULT_EDITOR_FONT_SIZE:.0}. The text you write."
                ),
                setting_stepper(
                    "editor-font",
                    settings.editor_font_size,
                    " px",
                    set_editor_font_size,
                    cx,
                ),
                cx,
            ))
            .child(setting_row(
                "App font size",
                format!(
                    "{MIN_APP_FONT_SIZE:.0}–{MAX_APP_FONT_SIZE:.0} px, default {DEFAULT_APP_FONT_SIZE:.0}. Sizes the studio's own chrome."
                ),
                setting_stepper(
                    "app-font",
                    settings.app_font_size,
                    " px",
                    |size, _, cx| set_app_font_size(size, cx),
                    cx,
                ),
                cx,
            ))
    }
}
