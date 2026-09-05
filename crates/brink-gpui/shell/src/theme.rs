//! Themes — the studio's five, as the native app's paint.
//!
//! The studio's themes are CSS token sheets (`docs/studio-shell-spec.md`
//! §7.4, `packages/studio-shell/src/styles/themes/*.css`): Catppuccin
//! Mocha as the base every other sheet sits on, Catppuccin Latte, and the
//! three from the theme ruling of 2026-08-25 — Manuscript, the writing-first
//! colorway, and faithful ports of Inky's two looks. This module carries
//! **the same values**, read from those sheets, so a theme picked here is
//! the theme the web studio shows. The sheets stay the source of truth: a
//! colour tuned there is tuned here by hand, and the tests below pin the
//! derivations (the CSS fallback chain, the override-sheet inheritance) so
//! a port that drifts from the rules fails rather than looks slightly off.
//!
//! ## How a theme reaches the screen
//!
//! gpui-component paints its chrome from a global [`Theme`] and projects an
//! editor's colours from it on every render (`input.rs`: foreground,
//! background, selection, caret, the active line, the diagnostic colours,
//! and the `highlight_theme` every highlighter's `styles` is handed as its
//! resolver). So a studio theme is applied by **building a
//! [`ThemeConfig`]** — the JSON shape gpui-component loads theme files
//! from — and installing it as the light or dark theme, then `Theme::change`.
//! Nothing paints from a private palette; a switch repaints every surface.
//!
//! ## Syntax roles ride Zed's names
//!
//! The resolver an editor hands the highlighter is gpui-component's
//! [`HighlightTheme`], whose syntax table has Zed's fixed set of names
//! (`keyword`, `punctuation.list_marker`, …), not brink's 19 token types.
//! Rather than a second resolver, each brink role is **carried under one
//! Zed name** — [`syntax_key`] is the one place that mapping lives, used
//! both when a theme's config is built and when the highlighter asks. The
//! studio's CSS fallbacks (a marker falls back to the operator colour in a
//! theme that does not split them; `END`/`DONE` to the keyword colour) are
//! resolved when the theme is built, so every role always has a colour.
//!
//! Choosing a theme is a command per theme ("Theme: Manuscript", the
//! studio's `theme.select.<id>`), and the choice is persisted in the
//! platform's config directory so the next launch opens in it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{Action, App, SharedString, Window};
use gpui_component::{Theme, ThemeConfig, ThemeMode};
use serde_json::{Value, json};

/// Choose a theme by id — `theme.select.<id>` in the studio.
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = brink, no_json)]
pub struct SelectTheme {
    pub id: SharedString,
}

/// The `--bs-*` tokens a theme sets, as `0xRRGGBB`. Names follow the CSS
/// (`--bs-editor-bg` → `editor_bg`, `--bs-syn-keyword` → `syn_keyword`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    // ── Surfaces & chrome ────────────────────────────────────────────
    pub editor_bg: u32,
    pub surface_bg: u32,
    pub panel_bg: u32,
    pub fg: u32,
    pub fg_muted: u32,
    pub border: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub hover_bg: u32,
    pub list_active_bg: u32,
    /// `--bs-active-line-bg`: a colour and the alpha it is laid at.
    pub active_line: (u32, f32),
    // ── Severity & story status ──────────────────────────────────────
    pub error: u32,
    pub warning: u32,
    pub todo: u32,
    pub todo_band: u32,
    pub todo_ink: u32,
    pub draft: u32,
    pub success: u32,
    pub info: u32,
    // ── Story symbols ────────────────────────────────────────────────
    pub symbol_file: u32,
    pub symbol_knot: u32,
    pub symbol_stitch: u32,
    pub symbol_function: u32,
    // ── Syntax ───────────────────────────────────────────────────────
    pub syn_namespace: u32,
    pub syn_function: u32,
    pub syn_variable: u32,
    pub syn_property: u32,
    pub syn_string: u32,
    pub syn_number: u32,
    pub syn_keyword: u32,
    pub syn_operator: u32,
    pub syn_comment: u32,
    pub syn_enum: u32,
    pub syn_parameter: u32,
    pub syn_decorator: u32,
    pub syn_label: u32,
    /// The role tokens of the 2026-08-25 ruling. `None` takes the CSS
    /// fallback: markers and diverts were operators, halts were keywords.
    pub syn_marker: Option<u32>,
    pub syn_divert: Option<u32>,
    pub syn_halt: Option<u32>,
    /// `--bs-cue` / `--bs-cue-weight`: `None` is the classic accent cue.
    pub cue: Option<u32>,
    pub cue_weight: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StudioTheme {
    /// Stable id — the studio's `data-theme` value.
    pub id: &'static str,
    pub label: &'static str,
    pub dark: bool,
    pub tokens: Tokens,
}

/// The default: the studio's, and the base every override sheet sits on.
pub const DEFAULT_ID: &str = "mocha";

/// The built-in themes, in the studio's order.
#[must_use]
pub fn builtin() -> [StudioTheme; 5] {
    [mocha(), latte(), manuscript(), inky(), inky_dark()]
}

#[must_use]
pub fn find(id: &str) -> Option<StudioTheme> {
    builtin().into_iter().find(|t| t.id == id)
}

/// Catppuccin Mocha (`mocha.css`) — theme #1 and the base.
fn mocha() -> StudioTheme {
    StudioTheme {
        id: "mocha",
        label: "Catppuccin Mocha",
        dark: true,
        tokens: Tokens {
            editor_bg: 0x1e1e2e,
            surface_bg: 0x252536,
            panel_bg: 0x2a2a3d,
            fg: 0xcdd6f4,
            fg_muted: 0x6c7086,
            border: 0x45475a,
            accent: 0x89b4fa,
            on_accent: 0x1e1e2e,
            hover_bg: 0x45475a,
            list_active_bg: 0x252536,
            active_line: (0x252536, 0.6),
            error: 0xf38ba8,
            warning: 0xf9e2af,
            todo: 0xe1b23f,
            todo_band: 0xbe973c,
            todo_ink: 0x2a2130,
            draft: 0xfab387,
            success: 0xa6e3a1,
            info: 0x89dceb,
            symbol_file: 0x89b4fa,
            symbol_knot: 0xcba6f7,
            symbol_stitch: 0x89dceb,
            symbol_function: 0xfab387,
            syn_keyword: 0xcba6f7,
            syn_namespace: 0xcba6f7,
            syn_function: 0x89b4fa,
            // Bindings must read differently from prose (author feedback
            // 2026-08-25): maroon, unused by any other token.
            syn_variable: 0xeba0ac,
            syn_property: 0xeba0ac,
            syn_string: 0xa6e3a1,
            syn_number: 0xfab387,
            syn_operator: 0x89dceb,
            syn_comment: 0x6c7086,
            syn_enum: 0xf9e2af,
            syn_parameter: 0xf2cdcd,
            syn_decorator: 0xfab387,
            syn_label: 0xf5c2e7,
            syn_marker: None,
            syn_divert: None,
            syn_halt: None,
            cue: None,
            cue_weight: 700,
        },
    }
}

/// Catppuccin Latte (`latte.css`) — the light theme, a full sheet.
fn latte() -> StudioTheme {
    StudioTheme {
        id: "latte",
        label: "Catppuccin Latte",
        dark: false,
        tokens: Tokens {
            editor_bg: 0xeff1f5,
            surface_bg: 0xe6e9ef,
            panel_bg: 0xdce0e8,
            fg: 0x4c4f69,
            fg_muted: 0x6c6f85,
            border: 0xbcc0cc,
            accent: 0x1e66f5,
            on_accent: 0xeff1f5,
            hover_bg: 0xbcc0cc,
            list_active_bg: 0xccd0da,
            active_line: (0xe6e9ef, 0.6),
            error: 0xd20f39,
            warning: 0xdf8e1d,
            todo: 0xdba01f,
            todo_band: 0xdfaf46,
            todo_ink: 0x2a2130,
            draft: 0xfe640b,
            success: 0x40a02b,
            info: 0x179299,
            symbol_file: 0x1e66f5,
            symbol_knot: 0x8839ef,
            symbol_stitch: 0x179299,
            symbol_function: 0xfe640b,
            syn_keyword: 0x8839ef,
            syn_namespace: 0x8839ef,
            syn_function: 0x1e66f5,
            syn_variable: 0xe64553,
            syn_property: 0xe64553,
            syn_string: 0x40a02b,
            syn_number: 0xfe640b,
            syn_operator: 0x179299,
            syn_comment: 0x6c6f85,
            syn_enum: 0xdf8e1d,
            syn_parameter: 0xdd7878,
            syn_decorator: 0xfe640b,
            syn_label: 0xea76cb,
            syn_marker: None,
            syn_divert: None,
            syn_halt: None,
            cue: None,
            cue_weight: 700,
        },
    }
}

/// Manuscript (`manuscript.css`, theme ruling 2026-08-25) — an OVERRIDE
/// sheet over mocha: prose brighter than everything, the pause points hot
/// red, all other machinery in one cool band ordered by conceptual
/// distance, cues as plain prose.
fn manuscript() -> StudioTheme {
    let base = mocha();
    StudioTheme {
        id: "manuscript",
        label: "Manuscript",
        dark: true,
        tokens: Tokens {
            fg: 0xf2f4fc,
            syn_namespace: 0xb9a9e6,
            syn_function: 0xb9a9e6,
            syn_label: 0xa4abdf,
            syn_divert: Some(0xa4abdf),
            syn_keyword: 0x8ba6cb,
            syn_operator: 0x90afcc,
            syn_variable: 0x93b8c8,
            syn_property: 0x93b8c8,
            syn_number: 0x93b8c8,
            syn_enum: 0x93b8c8,
            syn_parameter: 0x93b8c8,
            syn_string: 0x98bab4,
            syn_comment: 0x6c7086,
            syn_marker: Some(0xff5d62),
            syn_halt: Some(0xff5d62),
            syn_decorator: 0xf9e2af,
            cue: Some(0xf2f4fc),
            cue_weight: 400,
            symbol_knot: 0xb9a9e6,
            symbol_stitch: 0xa4abdf,
            symbol_function: 0x93b8c8,
            ..base.tokens
        },
    }
}

/// Inky (`inky.css`) — Inky's default light look, a full sheet: flow in
/// pure blue, logic in green, black prose on white.
fn inky() -> StudioTheme {
    StudioTheme {
        id: "inky",
        label: "Inky",
        dark: false,
        tokens: Tokens {
            editor_bg: 0xffffff,
            surface_bg: 0xf2f2f2,
            panel_bg: 0xebebeb,
            fg: 0x111111,
            fg_muted: 0x737373,
            border: 0xd0d0d0,
            accent: 0x1a53d8,
            on_accent: 0xffffff,
            hover_bg: 0xe2e2e2,
            list_active_bg: 0xe8e8e8,
            active_line: (0xf2f2f2, 0.6),
            error: 0xc22132,
            warning: 0xc07a12,
            todo: 0xb98a1a,
            todo_band: 0xd9b968,
            todo_ink: 0x2a2130,
            draft: 0xc2661a,
            success: 0x1d8348,
            info: 0x1a53d8,
            symbol_file: 0x1a53d8,
            symbol_knot: 0x0000ff,
            symbol_stitch: 0x0000ff,
            symbol_function: 0x008000,
            syn_namespace: 0x0000ff,
            syn_function: 0x0000ff,
            syn_label: 0x0000ff,
            syn_divert: Some(0x0000ff),
            syn_marker: Some(0x0000ff),
            syn_halt: Some(0x0000ff),
            syn_keyword: 0x008000,
            syn_operator: 0x008000,
            syn_variable: 0x008000,
            syn_property: 0x008000,
            syn_number: 0x008000,
            syn_enum: 0x008000,
            syn_parameter: 0x008000,
            syn_string: 0x008000,
            syn_comment: 0x84756c,
            syn_decorator: 0xaaaaaa,
            // Inky has no cue concept; cues read as prose.
            cue: Some(0x111111),
            cue_weight: 700,
        },
    }
}

/// Inky Dark (`inky-dark.css`) — an OVERRIDE sheet over mocha: cream prose
/// on `#282828`, red markers, sage flow, leaf-green logic.
fn inky_dark() -> StudioTheme {
    let base = mocha();
    StudioTheme {
        id: "inky-dark",
        label: "Inky Dark",
        dark: true,
        tokens: Tokens {
            editor_bg: 0x282828,
            surface_bg: 0x1f1f1f,
            panel_bg: 0x181818,
            fg: 0xfaf1c6,
            fg_muted: 0x8f8878,
            border: 0x3d3d3d,
            accent: 0x6d9e8e,
            on_accent: 0x181818,
            hover_bg: 0x3a3a3a,
            list_active_bg: 0x2f2f2f,
            // Inky's own active line: white at 5%.
            active_line: (0xffffff, 0.05),
            todo: 0xd19a2f,
            draft: 0xd98f4f,
            todo_band: 0xa97410,
            todo_ink: 0x000000,
            info: 0x6d9e8e,
            symbol_file: 0x6d9e8e,
            symbol_knot: 0x6d9e8e,
            symbol_stitch: 0x6d9e8e,
            symbol_function: 0x8eb865,
            syn_namespace: 0x6d9e8e,
            syn_function: 0x6d9e8e,
            syn_label: 0x6d9e8e,
            syn_divert: Some(0x6d9e8e),
            syn_halt: Some(0x6d9e8e),
            syn_marker: Some(0xec3c2f),
            syn_keyword: 0x8eb865,
            syn_operator: 0x8eb865,
            syn_variable: 0x8eb865,
            syn_property: 0x8eb865,
            syn_number: 0x8eb865,
            syn_enum: 0x8eb865,
            syn_parameter: 0x8eb865,
            syn_string: 0x8eb865,
            syn_comment: 0x9a9186,
            syn_decorator: 0x6f6f6f,
            cue: Some(0xfaf1c6),
            cue_weight: 700,
            ..base.tokens
        },
    }
}

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

fn hexa(c: u32, alpha: f32) -> String {
    let a = (alpha.clamp(0., 1.) * 255.).round() as u8;
    format!("#{c:06x}{a:02x}")
}

/// The Zed syntax name a brink token type is carried under in the theme's
/// highlight table — the one place the two vocabularies meet. `None` for a
/// role the studio leaves as prose (`struct` has no `tok-*` rule).
#[must_use]
pub fn syntax_key(role: &str) -> Option<&'static str> {
    Some(match role {
        "namespace" => "type",
        "function" => "function",
        "variable" => "variable",
        "property" => "property",
        "string" => "string",
        "number" => "number",
        "keyword" => "keyword",
        "operator" => "operator",
        "comment" => "comment",
        "enum" => "enum",
        "enumMember" => "variant",
        "parameter" => "variable.special",
        "decorator" => "attribute",
        "label" => "label",
        "marker" => "punctuation.list_marker",
        "divert" => "punctuation.special",
        "halt" => "constant",
        "escape" => "string.escape",
        _ => return None,
    })
}

impl Tokens {
    /// The marker colour after the CSS fallback (`--bs-syn-marker,
    /// var(--bs-syn-operator)`).
    #[must_use]
    pub fn marker(&self) -> u32 {
        self.syn_marker.unwrap_or(self.syn_operator)
    }

    #[must_use]
    pub fn divert(&self) -> u32 {
        self.syn_divert.unwrap_or(self.syn_operator)
    }

    /// `--bs-syn-halt, var(--bs-syn-keyword)`.
    #[must_use]
    pub fn halt(&self) -> u32 {
        self.syn_halt.unwrap_or(self.syn_keyword)
    }

    /// `--bs-cue, var(--bs-accent)`.
    #[must_use]
    pub fn cue(&self) -> u32 {
        self.cue.unwrap_or(self.accent)
    }

    /// The highlight table: every brink role under its Zed name, with the
    /// studio's per-role dressing — keywords semibold, comments italic, the
    /// escape mark receding to 40% of the prose colour (`editor.css`).
    fn syntax(&self) -> Value {
        let c = |v: u32| json!({ "color": hex(v) });
        let mut table = serde_json::Map::new();
        let mut put = |role: &str, style: Value| {
            if let Some(key) = syntax_key(role) {
                table.insert(key.to_owned(), style);
            }
        };
        put("namespace", c(self.syn_namespace));
        put("function", c(self.syn_function));
        put("variable", c(self.syn_variable));
        put("property", c(self.syn_property));
        put("string", c(self.syn_string));
        put("number", c(self.syn_number));
        put(
            "keyword",
            json!({ "color": hex(self.syn_keyword), "font_weight": 600 }),
        );
        put("operator", c(self.syn_operator));
        put(
            "comment",
            json!({ "color": hex(self.syn_comment), "font_style": "italic" }),
        );
        put("enum", c(self.syn_enum));
        put("enumMember", c(self.syn_enum));
        put("parameter", c(self.syn_parameter));
        put("decorator", c(self.syn_decorator));
        put("label", c(self.syn_label));
        put("marker", c(self.marker()));
        put("divert", c(self.divert()));
        put("halt", c(self.halt()));
        put("escape", json!({ "color": hexa(self.fg, 0.4) }));
        Value::Object(table)
    }
}

impl StudioTheme {
    /// This theme as gpui-component's theme-file shape. The chrome keys map
    /// the studio's surfaces onto the kit's (`editor-bg` is the window's
    /// background, `surface-bg` the docks/bars/tabs, `panel-bg` the
    /// popovers), the `highlight` block is what the editor paints from.
    #[must_use]
    pub fn config_value(&self) -> Value {
        let t = &self.tokens;
        let transparent = "#00000000".to_owned();
        // Built as a list, not one `json!` literal: a literal this size
        // blows the macro's recursion limit.
        let colors: [(&str, String); 76] = [
            ("background", hex(t.editor_bg)),
            ("foreground", hex(t.fg)),
            ("border", hex(t.border)),
            ("muted.background", hex(t.hover_bg)),
            ("muted.foreground", hex(t.fg_muted)),
            ("accent.background", hex(t.hover_bg)),
            ("accent.foreground", hex(t.fg)),
            ("primary.background", hex(t.accent)),
            ("primary.foreground", hex(t.on_accent)),
            ("primary.hover.background", hex(t.accent)),
            ("primary.active.background", hex(t.accent)),
            ("secondary.background", hex(t.panel_bg)),
            ("secondary.foreground", hex(t.fg)),
            ("secondary.hover.background", hex(t.hover_bg)),
            ("secondary.active.background", hex(t.hover_bg)),
            ("popover.background", hex(t.panel_bg)),
            ("popover.foreground", hex(t.fg)),
            ("sidebar.background", hex(t.surface_bg)),
            ("sidebar.foreground", hex(t.fg)),
            ("sidebar.border", hex(t.border)),
            ("sidebar.accent.background", hex(t.hover_bg)),
            ("sidebar.accent.foreground", hex(t.fg)),
            ("sidebar.primary.background", hex(t.accent)),
            ("sidebar.primary.foreground", hex(t.on_accent)),
            ("list.background", hex(t.surface_bg)),
            ("list.active.background", hex(t.list_active_bg)),
            ("list.active.border", hex(t.accent)),
            ("list.hover.background", hex(t.hover_bg)),
            ("list.even.background", hex(t.surface_bg)),
            ("list.head.background", hex(t.surface_bg)),
            ("tab_bar.background", hex(t.surface_bg)),
            ("tab_bar.segmented.background", hex(t.surface_bg)),
            ("tab.background", transparent.clone()),
            ("tab.active.background", hex(t.editor_bg)),
            ("tab.active.foreground", hex(t.fg)),
            ("tab.foreground", hex(t.fg_muted)),
            ("title_bar.background", hex(t.surface_bg)),
            ("title_bar.border", hex(t.border)),
            ("status_bar.background", hex(t.surface_bg)),
            ("status_bar.border", hex(t.border)),
            ("danger.background", hex(t.error)),
            ("danger.foreground", hex(t.on_accent)),
            ("warning.background", hex(t.warning)),
            ("warning.foreground", hex(t.on_accent)),
            ("success.background", hex(t.success)),
            ("success.foreground", hex(t.on_accent)),
            ("info.background", hex(t.info)),
            ("info.foreground", hex(t.on_accent)),
            ("selection.background", hex(t.accent)),
            ("caret", hex(t.accent)),
            ("ring", hex(t.accent)),
            ("input.border", hex(t.border)),
            ("link", hex(t.accent)),
            ("link.hover", hex(t.accent)),
            ("link.active", hex(t.accent)),
            ("drag.border", hex(t.accent)),
            ("drop_target.background", hexa(t.accent, 0.25)),
            ("scrollbar.background", transparent),
            ("scrollbar.thumb.background", hexa(t.fg_muted, 0.6)),
            ("scrollbar.thumb.hover.background", hex(t.fg_muted)),
            ("group_box.background", hex(t.panel_bg)),
            ("group_box.foreground", hex(t.fg)),
            ("accordion.background", hex(t.panel_bg)),
            ("skeleton.background", hex(t.hover_bg)),
            ("switch.background", hex(t.border)),
            ("slider.background", hex(t.accent)),
            ("progress.bar.background", hex(t.accent)),
            (
                "overlay",
                hexa(0x00_0000, if self.dark { 0.45 } else { 0.3 }),
            ),
            ("window.border", hex(t.border)),
            ("table.background", hex(t.surface_bg)),
            ("table.active.background", hex(t.list_active_bg)),
            ("table.active.border", hex(t.accent)),
            ("table.even.background", hex(t.surface_bg)),
            ("table.head.background", hex(t.surface_bg)),
            ("table.hover.background", hex(t.hover_bg)),
            ("table.row.border", hex(t.border)),
        ];
        let colors: serde_json::Map<String, Value> = colors
            .into_iter()
            .map(|(k, v)| (k.to_owned(), Value::String(v)))
            .chain([
                ("base.red".to_owned(), Value::String(hex(t.error))),
                ("base.yellow".to_owned(), Value::String(hex(t.warning))),
                ("base.green".to_owned(), Value::String(hex(t.success))),
                ("base.blue".to_owned(), Value::String(hex(t.accent))),
            ])
            .collect();
        let highlight = json!({
            "editor.foreground": hex(t.fg),
            "editor.background": hex(t.editor_bg),
            "editor.gutter.background": hex(t.editor_bg),
            "editor.active_line.background": hexa(t.active_line.0, t.active_line.1),
            "editor.line_number": hex(t.fg_muted),
            "editor.active_line_number": hex(t.fg),
            "editor.invisible": hexa(t.fg_muted, 0.4),
            "error": hex(t.error),
            "warning": hex(t.warning),
            "info": hex(t.info),
            "hint": hex(t.fg_muted),
            "success": hex(t.success),
            "syntax": t.syntax(),
        });
        json!({
            "name": self.label,
            "mode": if self.dark { "dark" } else { "light" },
            // The studio's editor size (`DEFAULT_EDITOR_FONT_SIZE`).
            "mono_font.size": 14,
            "colors": Value::Object(colors),
            "highlight": highlight,
        })
    }

    /// The parsed config — what `Theme` installs.
    pub fn config(&self) -> anyhow::Result<ThemeConfig> {
        Ok(serde_json::from_value(self.config_value())?)
    }
}

/// Which studio theme is on screen.
#[derive(Debug, Clone)]
pub struct CurrentTheme {
    pub id: SharedString,
}

impl gpui::Global for CurrentTheme {}

/// The id of the theme on screen, `DEFAULT_ID` before any was applied.
#[must_use]
pub fn current_id(cx: &App) -> SharedString {
    cx.try_global::<CurrentTheme>()
        .map_or_else(|| SharedString::from(DEFAULT_ID), |c| c.id.clone())
}

/// Paint the app in `theme`: install it as the kit's light or dark theme
/// and switch to that mode. Every window repaints.
pub fn apply(theme: &StudioTheme, window: Option<&mut Window>, cx: &mut App) -> anyhow::Result<()> {
    let config = Rc::new(theme.config()?);
    let mode = if theme.dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    {
        let global = Theme::global_mut(cx);
        if theme.dark {
            global.dark_theme = config;
        } else {
            global.light_theme = config;
        }
    }
    Theme::change(mode, window, cx);
    cx.set_global(CurrentTheme {
        id: theme.id.into(),
    });
    cx.refresh_windows();
    Ok(())
}

/// `theme.select.<id>`: apply a built-in theme and remember it. An unknown
/// id — a stale persisted value, a bad argument — is ignored rather than
/// blanking the app (the studio's rule). Returns whether it applied.
pub fn select(id: &str, window: Option<&mut Window>, cx: &mut App) -> bool {
    let Some(theme) = find(id) else {
        return false;
    };
    if let Err(err) = apply(&theme, window, cx) {
        eprintln!("theme {id}: {err:#}");
        return false;
    }
    if let Some(dir) = settings_dir()
        && let Err(err) = persist_to(&dir, id)
    {
        eprintln!("theme {id}: could not persist: {err}");
    }
    true
}

/// At startup: the persisted theme, or the default. Call after
/// `gpui_component::init`, before the first window.
pub fn init(cx: &mut App) {
    let persisted = settings_dir().and_then(|dir| load_from(&dir));
    let theme = persisted
        .as_deref()
        .and_then(find)
        .unwrap_or_else(|| find(DEFAULT_ID).unwrap_or_else(mocha));
    if let Err(err) = apply(&theme, None, cx) {
        eprintln!("theme {}: {err:#}", theme.id);
    }
}

// ── Persistence ──────────────────────────────────────────────────────

const THEME_FILE: &str = "theme";

/// Where the app keeps its settings: `$BRINK_STUDIO_CONFIG_DIR` if set,
/// otherwise the platform's config directory plus `brink-studio`.
#[must_use]
pub fn settings_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("BRINK_STUDIO_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
    };
    base.map(|b| b.join("brink-studio"))
}

pub fn persist_to(dir: &Path, id: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(THEME_FILE), id)
}

#[must_use]
pub fn load_from(dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join(THEME_FILE)).ok()?;
    let id = raw.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses_as_a_kit_theme() {
        for theme in builtin() {
            let config = theme.config().expect("valid theme config");
            assert_eq!(config.name.as_ref(), theme.label);
            assert_eq!(config.mode.is_dark(), theme.dark, "{}", theme.id);
            let highlight = config.highlight.expect("highlight block");
            assert!(highlight.editor_background.is_some(), "{}", theme.id);
            // Every brink role that has a colour resolves through the table.
            for role in brink_ir::semantic_tokens::token_type_names() {
                let Some(key) = syntax_key(role) else {
                    assert_eq!(*role, "struct", "only struct is left as prose");
                    continue;
                };
                assert!(
                    highlight.syntax.style(key).is_some(),
                    "{}: {role} (as {key}) has no style",
                    theme.id
                );
            }
        }
    }

    #[test]
    fn the_role_fallbacks_follow_the_css() {
        // Catppuccin themes do not split the roles: markers and diverts
        // wear the operator colour, halts the keyword colour.
        let m = mocha().tokens;
        assert_eq!(m.marker(), m.syn_operator);
        assert_eq!(m.divert(), m.syn_operator);
        assert_eq!(m.halt(), m.syn_keyword);
        assert_eq!(m.cue(), m.accent);
        // Manuscript splits them: hot red pause points, the flow band.
        let s = manuscript().tokens;
        assert_eq!(s.marker(), 0xff5d62);
        assert_eq!(s.halt(), 0xff5d62);
        assert_eq!(s.divert(), 0xa4abdf);
        assert_eq!(s.cue(), s.fg);
        assert_eq!(s.cue_weight, 400);
    }

    #[test]
    fn override_sheets_inherit_from_mocha() {
        let base = mocha().tokens;
        let s = manuscript().tokens;
        assert_eq!(s.editor_bg, base.editor_bg);
        assert_eq!(s.error, base.error);
        assert_eq!(s.active_line, base.active_line);
        assert_ne!(s.fg, base.fg);
        let d = inky_dark().tokens;
        assert_eq!(d.error, base.error, "severity inherits");
        assert_eq!(d.warning, base.warning);
        assert_ne!(d.editor_bg, base.editor_bg);
        assert_eq!(d.active_line, (0xffffff, 0.05));
    }

    #[test]
    fn the_highlight_table_carries_the_studio_dressing() {
        let config = mocha().config().unwrap();
        let syntax = config.highlight.unwrap().syntax;
        let keyword = syntax.style("keyword").unwrap();
        assert_eq!(keyword.font_weight, Some(gpui::FontWeight::SEMIBOLD));
        let comment = syntax.style("comment").unwrap();
        assert_eq!(comment.font_style, Some(gpui::FontStyle::Italic));
        let escape = syntax.style(syntax_key("escape").unwrap()).unwrap();
        let alpha = escape.color.unwrap().a;
        assert!((alpha - 0.4).abs() < 0.01, "escape recedes to 40%: {alpha}");
        assert!(syntax.style(syntax_key("enumMember").unwrap()).is_some());
    }

    #[test]
    fn ids_are_unique_and_the_default_exists() {
        let ids: Vec<_> = builtin().iter().map(|t| t.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert!(find(DEFAULT_ID).is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn the_choice_round_trips_through_the_settings_dir() {
        let dir = std::env::temp_dir().join(format!(
            "brink-gpui-theme-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        assert_eq!(load_from(&dir), None);
        persist_to(&dir, "inky-dark").unwrap();
        assert_eq!(load_from(&dir).as_deref(), Some("inky-dark"));
        persist_to(&dir, "").unwrap();
        assert_eq!(load_from(&dir), None, "an empty file is no choice");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
