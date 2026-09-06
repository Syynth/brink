//! App settings — this machine's, following the author between projects
//! (`docs/studio-shell-spec.md` §7; "Settings splits App and Project
//! scope", ruled 2026-08-27).
//!
//! What lives here is the **App** scope: the theme, the two font sizes,
//! the editor's gutters and inlay hints, and the keymap overrides. Project
//! settings write `brink.toml` and are the project's, not this file's.
//!
//! One JSON file in the platform's config directory (`settings_dir`),
//! written whole on every change — switches are rare and atomic, so there
//! is nothing to debounce. The file is read once at startup into a gpui
//! global, [`AppSettings`], and every consumer that must follow a change
//! observes that global (`cx.observe_global::<AppSettings>`), the way the
//! editors follow the theme.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{App, Global};
use serde_json::{Value, json};

/// The editor's shipped text size, and its bounds — the studio's
/// (`packages/ink-editor/src/theme.ts`): below 8 the gutter collides with
/// itself; above 32 a line stops fitting a pane.
pub const DEFAULT_EDITOR_FONT_SIZE: f32 = 14.;
pub const MIN_EDITOR_FONT_SIZE: f32 = 8.;
pub const MAX_EDITOR_FONT_SIZE: f32 = 32.;

/// The app-wide UI size — a separate knob from the editor's: "make the UI
/// bigger" and "make the text I write bigger" are different asks.
pub const DEFAULT_APP_FONT_SIZE: f32 = 12.;
pub const MIN_APP_FONT_SIZE: f32 = 9.;
pub const MAX_APP_FONT_SIZE: f32 = 20.;

/// Clamp and round an arbitrary value (garbage from the file included) to
/// a usable size; NaN lands on the default.
#[must_use]
pub fn clamp_font_size(value: f32, default: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.round().clamp(min, max)
    } else {
        default
    }
}

/// A command's keymap override: `Some(chord)` rebinds it, `None` unbinds
/// it. A command absent from the map keeps its shipped default.
pub type KeymapOverride = Option<String>;

#[derive(Debug, Clone, PartialEq)]
pub struct AppSettings {
    /// A built-in theme id (`crate::theme`).
    pub theme: String,
    pub editor_font_size: f32,
    pub app_font_size: f32,
    /// Line numbers in the editor.
    pub show_gutters: bool,
    /// Parameter-name hints drawn inside the line.
    pub show_inlay_hints: bool,
    /// Run the formatter over every dirty file before it is written.
    pub format_on_save: bool,
    /// By the command's full title ("View: Toggle Binder") — the name the
    /// palette shows, and the only identity a data-carrying action has.
    pub keymap: BTreeMap<String, KeymapOverride>,
}

impl Global for AppSettings {}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: crate::theme::DEFAULT_ID.to_owned(),
            editor_font_size: DEFAULT_EDITOR_FONT_SIZE,
            app_font_size: DEFAULT_APP_FONT_SIZE,
            show_gutters: true,
            show_inlay_hints: true,
            format_on_save: false,
            keymap: BTreeMap::new(),
        }
    }
}

impl AppSettings {
    #[must_use]
    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// The settings as they are, or the defaults before `init` has run
    /// (tests, and any read that races startup).
    #[must_use]
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    /// The window's rem size for this app font size: gpui's text scale is
    /// rem-based, and the studio's default UI text is 12px at the default
    /// 16px rem, so the app size scales the rem.
    #[must_use]
    pub fn rem_size(&self) -> f32 {
        16. * self.app_font_size / DEFAULT_APP_FONT_SIZE
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        let keymap: serde_json::Map<String, Value> = self
            .keymap
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().map_or(Value::Null, Value::String)))
            .collect();
        json!({
            "theme": self.theme,
            "editor_font_size": self.editor_font_size,
            "app_font_size": self.app_font_size,
            "show_gutters": self.show_gutters,
            "show_inlay_hints": self.show_inlay_hints,
            "format_on_save": self.format_on_save,
            "keymap": Value::Object(keymap),
        })
    }

    /// Read leniently: a missing or malformed field takes its default, so
    /// a file from an older or newer build never blanks the app.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let defaults = Self::default();
        let num = |key: &str| value.get(key).and_then(Value::as_f64).map(|n| n as f32);
        let theme = value
            .get("theme")
            .and_then(Value::as_str)
            .filter(|id| crate::theme::find(id).is_some())
            .map_or(defaults.theme, str::to_owned);
        let keymap = value
            .get("keymap")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| match v {
                        Value::Null => Some((k.clone(), None)),
                        Value::String(s) if !s.is_empty() => Some((k.clone(), Some(s.clone()))),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            theme,
            editor_font_size: clamp_font_size(
                num("editor_font_size").unwrap_or(defaults.editor_font_size),
                DEFAULT_EDITOR_FONT_SIZE,
                MIN_EDITOR_FONT_SIZE,
                MAX_EDITOR_FONT_SIZE,
            ),
            app_font_size: clamp_font_size(
                num("app_font_size").unwrap_or(defaults.app_font_size),
                DEFAULT_APP_FONT_SIZE,
                MIN_APP_FONT_SIZE,
                MAX_APP_FONT_SIZE,
            ),
            show_gutters: value
                .get("show_gutters")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.show_gutters),
            format_on_save: value
                .get("format_on_save")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.format_on_save),
            show_inlay_hints: value
                .get("show_inlay_hints")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.show_inlay_hints),
            keymap,
        }
    }
}

const SETTINGS_FILE: &str = "settings.json";
/// The theme's own file from before there was a settings file — read once
/// when `settings.json` is absent, so a chosen theme survives the upgrade.
const LEGACY_THEME_FILE: &str = "theme";

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

/// Read the settings in `dir`, or the defaults when there are none.
#[must_use]
pub fn load_from(dir: &Path) -> AppSettings {
    if let Ok(raw) = std::fs::read_to_string(dir.join(SETTINGS_FILE))
        && let Ok(value) = serde_json::from_str::<Value>(&raw)
    {
        return AppSettings::from_json(&value);
    }
    let mut settings = AppSettings::default();
    if let Ok(raw) = std::fs::read_to_string(dir.join(LEGACY_THEME_FILE)) {
        let id = raw.trim();
        if crate::theme::find(id).is_some() {
            settings.theme = id.to_owned();
        }
    }
    settings
}

pub fn save_to(dir: &Path, settings: &AppSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(&settings.to_json())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(dir.join(SETTINGS_FILE), text)
}

/// Load the settings into the global. Before `theme::init`, which reads
/// them.
pub fn init(cx: &mut App) {
    let settings = settings_dir().map_or_else(AppSettings::default, |dir| load_from(&dir));
    cx.set_global(settings);
}

/// Change the settings: `f` edits them, the file is rewritten, and every
/// observer of the global is told. No-op when `f` changes nothing.
pub fn update(cx: &mut App, f: impl FnOnce(&mut AppSettings)) {
    let mut next = AppSettings::get(cx);
    let before = next.clone();
    f(&mut next);
    if next == before {
        return;
    }
    if let Some(dir) = settings_dir()
        && let Err(err) = save_to(&dir, &next)
    {
        eprintln!("settings: could not persist: {err}");
    }
    cx.set_global(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "brink-gpui-settings-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ))
    }

    #[test]
    fn settings_round_trip_through_json() {
        let mut s = AppSettings {
            theme: "inky-dark".to_owned(),
            editor_font_size: 16.,
            app_font_size: 13.,
            show_gutters: false,
            show_inlay_hints: true,
            format_on_save: false,
            keymap: BTreeMap::new(),
        };
        s.keymap
            .insert("File: Save".to_owned(), Some("cmd-shift-s".to_owned()));
        s.keymap.insert("View: Toggle Binder".to_owned(), None);
        let back = AppSettings::from_json(&s.to_json());
        assert_eq!(back, s);
    }

    #[test]
    fn garbage_falls_back_field_by_field() {
        let value = json!({
            "theme": "no-such-theme",
            "editor_font_size": 400,
            "app_font_size": "big",
            "show_gutters": "yes",
            "keymap": { "File: Save": 3, "X: Y": "", "Z: W": null }
        });
        let s = AppSettings::from_json(&value);
        assert_eq!(s.theme, crate::theme::DEFAULT_ID);
        assert_eq!(
            s.editor_font_size, MAX_EDITOR_FONT_SIZE,
            "clamped, not dropped"
        );
        assert_eq!(s.app_font_size, DEFAULT_APP_FONT_SIZE);
        assert!(s.show_gutters, "a non-bool is the default");
        assert_eq!(
            s.keymap.len(),
            1,
            "only the unbound entry survives: {:?}",
            s.keymap
        );
        assert_eq!(s.keymap.get("Z: W"), Some(&None));
        assert_eq!(AppSettings::from_json(&json!(42)), AppSettings::default());
    }

    #[test]
    fn the_file_round_trips_and_the_legacy_theme_file_is_read_once() {
        let dir = scratch("file");
        assert_eq!(load_from(&dir), AppSettings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LEGACY_THEME_FILE), "latte\n").unwrap();
        let legacy = load_from(&dir);
        assert_eq!(legacy.theme, "latte", "the old theme file is honoured");
        let mut s = legacy;
        s.app_font_size = 14.;
        save_to(&dir, &s).unwrap();
        assert_eq!(load_from(&dir), s, "settings.json now wins");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_rem_size_scales_from_the_studio_default() {
        let mut s = AppSettings::default();
        assert!((s.rem_size() - 16.).abs() < f32::EPSILON);
        s.app_font_size = 18.;
        assert!((s.rem_size() - 24.).abs() < f32::EPSILON);
        assert_eq!(clamp_font_size(f32::NAN, 14., 8., 32.), 14.);
        assert_eq!(clamp_font_size(13.6, 14., 8., 32.), 14.);
    }
}
