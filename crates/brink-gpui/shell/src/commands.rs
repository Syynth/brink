//! Commands — `docs/gpui-studio-spec.md` §4.5, satisfying
//! `docs/studio-shell-spec.md` §6's command contract.
//!
//! **A command is a gpui action with a title.** Keybindings, the palette,
//! the hamburger menu, rail buttons and status cells all dispatch the same
//! action, so nothing binds a key to a function and the palette is complete
//! by construction. There is no second registry: the action IS the command;
//! this module only remembers what to call it and where it starts bound.
//!
//! Enablement is gpui's own: an action is available when something in the
//! focus path (or a global listener) handles it — `Window::is_action_available`
//! — which is the `when` of the studio's contract without a closure per
//! command.
//!
//! ## The keymap layer (studio §6)
//!
//! The registry is the single default table, and the author's overrides
//! (`crate::settings::AppSettings::keymap`, keyed by a command's full
//! title) merge over it: [`effective_keystroke`] is the one rule. A
//! command keeps a **binder** — a closure over its typed action that turns
//! any keystroke into a `KeyBinding` — because gpui's `KeyBinding::new`
//! wants the concrete type and a `no_json` action cannot be rebuilt from a
//! name. gpui's keymap can only grow (later bindings win; nothing removes
//! one), so an override is bound after the default, and a default that an
//! override takes away is **shadowed**: its keystroke is bound to
//! [`Unbound`], which the workspace swallows with a global listener.
//!
//! Rebinding **displaces** (ruled 2026-08-30): a chord taken for one
//! command comes off whichever command held it, and [`bind_chord`] says
//! which, for the UI to say out loud. Two commands on one chord would mean
//! the later-registered silently wins and the other is dead.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::{Action, KeyBinding, SharedString, actions};

use crate::settings::KeymapOverride;

actions!(
    brink,
    [
        /// Open or close the command palette.
        TogglePalette,
        /// Open or close the hamburger menu.
        ToggleMenu,
        /// Open the Settings window.
        OpenSettings,
        /// What a default keystroke is rebound to when an override takes
        /// it away: swallowed by the workspace's global listener, so the
        /// keystroke does nothing rather than falling through to the
        /// default it shadows.
        Unbound,
        /// Move the palette's selection up.
        PaletteUp,
        /// Move the palette's selection down.
        PaletteDown,
        /// Run the palette's selected command.
        PaletteConfirm,
        /// Close the palette without running anything.
        PaletteDismiss,
    ]
);

/// Toggle a tool window by id — `view.toggle.<id>`, generated per tool
/// window at registration (studio §5.2: `Mod-1…9` by registration order).
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = brink, no_json)]
pub struct ToggleToolWindow {
    pub id: SharedString,
}

/// One registered command.
pub struct Command {
    /// Palette group and hamburger section: "View", "File", "Story".
    pub group: SharedString,
    /// What the palette shows; the ruled vocabulary, so it is user-facing.
    pub title: SharedString,
    pub action: Box<dyn Action>,
    /// The default keystroke, gpui syntax ("cmd-shift-p"), if any.
    pub keystroke: Option<SharedString>,
    /// Any keystroke → a binding for this command's typed action.
    binder: Binder,
}

type Binder = Rc<dyn Fn(&str) -> KeyBinding>;

impl Clone for Command {
    fn clone(&self) -> Self {
        Self {
            group: self.group.clone(),
            title: self.title.clone(),
            action: self.action.boxed_clone(),
            keystroke: self.keystroke.clone(),
            binder: self.binder.clone(),
        }
    }
}

impl Command {
    /// "View: Code" — how the studio's palette spells it, and the key a
    /// keymap override is stored under.
    #[must_use]
    pub fn full_title(&self) -> String {
        format!("{}: {}", self.group, self.title)
    }

    /// A binding of this command to `keystroke`.
    #[must_use]
    pub fn bind(&self, keystroke: &str) -> KeyBinding {
        (self.binder)(keystroke)
    }
}

/// One spelling for a chord, whatever order its modifiers were written in
/// and whatever the platform calls its command key: parsed and unparsed
/// by gpui, then `super`/`win` folded into `cmd`. "cmd-alt-3" and
/// "alt-cmd-3" are the same binding, and must compare equal for the
/// displacement rule to see that one command holds another's chord. An
/// unparsable string is returned as written.
#[must_use]
pub fn canonical_chord(chord: &str) -> String {
    let spelled = gpui::Keystroke::parse(chord).map_or_else(|_| chord.to_owned(), |k| k.unparse());
    spelled
        .split('-')
        .map(|part| match part {
            "super" | "win" => "cmd",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// A command's keystroke after the overrides: the override when there is
/// one (`None` inside means unbound), else the shipped default.
#[must_use]
pub fn effective_keystroke(
    command: &Command,
    overrides: &BTreeMap<String, KeymapOverride>,
) -> Option<String> {
    match overrides.get(&command.full_title()) {
        Some(over) => over.clone(),
        None => command.keystroke.as_ref().map(ToString::to_string),
    }
}

/// Where a command's current keystroke comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Default,
    Custom,
    Unbound,
}

#[must_use]
pub fn key_source(command: &Command, overrides: &BTreeMap<String, KeymapOverride>) -> KeySource {
    match overrides.get(&command.full_title()) {
        Some(Some(_)) => KeySource::Custom,
        Some(None) => KeySource::Unbound,
        None => KeySource::Default,
    }
}

/// Give `chord` to the command at `index`, taking it off whichever other
/// command held it. Returns the displaced command's full title, if any.
/// An override that equals the default is dropped, so a round trip leaves
/// no residue.
pub fn bind_chord(
    commands: &[Command],
    overrides: &mut BTreeMap<String, KeymapOverride>,
    index: usize,
    chord: &str,
) -> Option<String> {
    let mut displaced = None;
    for (ix, other) in commands.iter().enumerate() {
        if ix != index && effective_keystroke(other, overrides).as_deref() == Some(chord) {
            let title = other.full_title();
            overrides.insert(title.clone(), None);
            displaced = Some(title);
        }
    }
    let command = &commands[index];
    if command.keystroke.as_deref() == Some(chord) {
        overrides.remove(&command.full_title());
    } else {
        overrides.insert(command.full_title(), Some(chord.to_owned()));
    }
    displaced
}

/// Take the command's keystroke away.
pub fn unbind(
    commands: &[Command],
    overrides: &mut BTreeMap<String, KeymapOverride>,
    index: usize,
) {
    let command = &commands[index];
    if command.keystroke.is_none() {
        overrides.remove(&command.full_title());
    } else {
        overrides.insert(command.full_title(), None);
    }
}

/// Drop the command's override, back to its shipped default.
pub fn reset(commands: &[Command], overrides: &mut BTreeMap<String, KeymapOverride>, index: usize) {
    overrides.remove(&commands[index].full_title());
}

/// The bindings that make the overrides hold, in the order to install
/// them: every shadow first (a default some override took away, bound to
/// [`Unbound`]), then every live keystroke — so a chord moved from one
/// command to another is shadowed for the first and then bound for the
/// second, and the second wins.
#[must_use]
pub fn keymap_bindings(
    commands: &[Command],
    overrides: &BTreeMap<String, KeymapOverride>,
) -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for command in commands {
        if let Some(default) = &command.keystroke
            && effective_keystroke(command, overrides).as_deref() != Some(default.as_ref())
        {
            out.push(KeyBinding::new(default, Unbound, None));
        }
    }
    for command in commands {
        if let Some(keys) = effective_keystroke(command, overrides) {
            out.push(command.bind(&keys));
        }
    }
    out
}

/// The commands, in registration order — the order the palette and menu
/// list them in, and the order `Mod-1…9` are handed out in.
#[derive(Default)]
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    /// Register a command. Returns its index; the caller installs its
    /// keystroke (`Workspace::register_command` does, through the
    /// overrides).
    pub fn register<A: Action + Clone>(
        &mut self,
        group: impl Into<SharedString>,
        title: impl Into<SharedString>,
        action: A,
        keystroke: Option<&str>,
    ) -> usize {
        // The registry keeps a boxed copy; the binder keeps the typed one,
        // since `KeyBinding::new` wants a concrete action.
        let boxed = action.boxed_clone();
        let binder: Binder = Rc::new(move |keys| KeyBinding::new(keys, action.clone(), None));
        self.commands.push(Command {
            group: group.into(),
            title: title.into(),
            action: boxed,
            keystroke: keystroke.map(|keys| SharedString::from(canonical_chord(keys))),
            binder,
        });
        self.commands.len() - 1
    }

    #[must_use]
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// The keystroke bound to an action, for tooltips.
    #[must_use]
    pub fn keystroke_for(&self, action: &dyn Action) -> Option<SharedString> {
        self.commands
            .iter()
            .find(|c| c.action.partial_eq(action))
            .and_then(|c| c.keystroke.clone())
    }
}

/// A command's index in the registry with its rank for `query`, lowest
/// first; unmatched commands are absent. The studio's quick-pick rule:
/// title preferred over the group-qualified title, ranked by how tightly
/// the query's characters sit as a subsequence.
#[must_use]
pub fn rank(commands: &[Command], query: &str) -> Vec<usize> {
    let titles: Vec<(String, String)> = commands
        .iter()
        .map(|c| (c.title.to_string(), c.full_title()))
        .collect();
    rank_titles(&titles, query)
}

/// [`rank`] over `(title, full title)` pairs, for a caller holding a
/// snapshot rather than the registry.
#[must_use]
pub fn rank_titles(titles: &[(String, String)], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..titles.len()).collect();
    }
    let mut ranked: Vec<(usize, usize)> = titles
        .iter()
        .enumerate()
        .filter_map(|(ix, (title, full))| {
            let by_title = subsequence_score(&title.to_lowercase(), &q);
            let by_full = subsequence_score(&full.to_lowercase(), &q).map(|s| s + 1);
            match (by_title, by_full) {
                (Some(a), Some(b)) => Some((ix, a.min(b))),
                (Some(a), None) => Some((ix, a)),
                (None, Some(b)) => Some((ix, b)),
                (None, None) => None,
            }
        })
        .collect();
    ranked.sort_by_key(|&(ix, score)| (score, ix));
    ranked.into_iter().map(|(ix, _)| ix).collect()
}

/// Lower is tighter: the span the query's characters cover as an in-order
/// subsequence of `text`, or `None` when they do not all appear. A
/// contiguous substring scores its own length, so "code" ranks "View: Code"
/// above "View: Continuous" even though both contain c…o…d…e.
fn subsequence_score(text: &str, query: &str) -> Option<usize> {
    if let Some(at) = text.find(query) {
        return Some(query.len() + at / 8);
    }
    let mut chars = text.char_indices();
    let mut first = None;
    let mut last = 0;
    for needle in query.chars() {
        let (at, _) = chars.by_ref().find(|(_, c)| *c == needle)?;
        first.get_or_insert(at);
        last = at;
    }
    // A subsequence is always looser than any substring of the same query.
    Some(last - first.unwrap_or(0) + text.len() / 4 + query.len() * 2)
}

/// "cmd-shift-p" as the symbols a Mac user reads: "⌘⇧P". A display, not a
/// parser; gpui keeps its own syntax for the binding itself.
#[must_use]
pub fn display_keystroke(keystroke: &str) -> String {
    let mut out = String::new();
    let mut parts = keystroke.split('-').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        match (part, last) {
            ("cmd", false) => out.push('\u{2318}'),
            ("shift", false) => out.push('\u{21E7}'),
            ("alt", false) => out.push('\u{2325}'),
            ("ctrl", false) => out.push('\u{2303}'),
            ("escape", true) => out.push_str("Esc"),
            ("enter", true) => out.push('\u{21A9}'),
            (key, _) => out.push_str(&key.to_uppercase()),
        }
    }
    out
}

/// `Mod-1…9` for the first nine tool windows, none after (studio §5.2).
#[must_use]
pub fn tool_window_keystroke(ordinal: usize) -> Option<String> {
    (1..=9).contains(&ordinal).then(|| format!("cmd-{ordinal}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_view::{ViewCode, ViewContinuous, ViewSingle};

    fn registry() -> CommandRegistry {
        let mut r = CommandRegistry::default();
        r.register("View", "Code", ViewCode, Some("cmd-shift-1"));
        r.register("View", "Single File", ViewSingle, Some("cmd-shift-2"));
        r.register("View", "Continuous", ViewContinuous, None);
        r.register("Palette", "Toggle", TogglePalette, Some("cmd-shift-p"));
        r
    }

    #[test]
    fn an_empty_query_lists_everything_in_registration_order() {
        let r = registry();
        assert_eq!(rank(r.commands(), ""), [0, 1, 2, 3]);
        assert_eq!(rank(r.commands(), "   "), [0, 1, 2, 3]);
    }

    #[test]
    fn a_substring_of_the_title_outranks_a_scattered_subsequence() {
        let r = registry();
        // "code" is a substring of "Code" and a subsequence of
        // "View: Continuous"… no — 'd' is absent there; but it IS a
        // subsequence of "Single File"? no 'o'. Use "ile": substring of
        // "Single File", subsequence of nothing else.
        assert_eq!(rank(r.commands(), "ile"), [1]);
        // "vc" matches nothing as a substring, but "View: Code" and
        // "View: Continuous" as subsequences of the full title; Code is
        // tighter.
        let ranked = rank(r.commands(), "vc");
        assert_eq!(ranked.first(), Some(&0));
        assert!(ranked.contains(&2));
        assert!(!ranked.contains(&1), "Single File has no 'c' after its 'v'");
    }

    #[test]
    fn keystrokes_are_looked_up_by_action_value() {
        let r = registry();
        assert_eq!(
            r.keystroke_for(&ViewSingle).map(|k| k.to_string()),
            Some(canonical_chord("cmd-shift-2"))
        );
        assert_eq!(r.keystroke_for(&ViewContinuous), None);
    }

    #[test]
    fn overrides_merge_over_defaults_and_rebinding_displaces() {
        let mut r = CommandRegistry::default();
        r.register("View", "Code", ViewCode, Some("cmd-alt-1"));
        r.register("View", "Single File", ViewSingle, Some("cmd-alt-2"));
        r.register("View", "Continuous", ViewContinuous, None);
        let commands = r.commands().to_vec();
        let mut overrides = BTreeMap::new();
        let code_default = effective_keystroke(&commands[0], &overrides).unwrap();
        assert_eq!(code_default, canonical_chord("alt-cmd-1"), "one spelling");
        // Give Continuous the chord Code holds: Code is displaced.
        let displaced = bind_chord(&commands, &mut overrides, 2, &canonical_chord("cmd-alt-1"));
        assert_eq!(displaced.as_deref(), Some("View: Code"));
        assert_eq!(effective_keystroke(&commands[0], &overrides), None);
        assert_eq!(key_source(&commands[0], &overrides), KeySource::Unbound);
        assert_eq!(
            effective_keystroke(&commands[2], &overrides),
            Some(code_default.clone())
        );
        assert_eq!(key_source(&commands[2], &overrides), KeySource::Custom);
        // Shadows first, then live keys: Code's default is shadowed once,
        // Single File and Continuous are bound.
        let bindings = keymap_bindings(&commands, &overrides);
        assert_eq!(bindings.len(), 3);
        assert!(bindings[0].action().partial_eq(&Unbound));
        // Reset drops the override; an override equal to the default is
        // never stored.
        reset(&commands, &mut overrides, 0);
        assert_eq!(key_source(&commands[0], &overrides), KeySource::Default);
        assert_eq!(
            bind_chord(&commands, &mut overrides, 1, &canonical_chord("cmd-alt-2")),
            None
        );
        assert!(!overrides.contains_key("View: Single File"));
        unbind(&commands, &mut overrides, 1);
        assert_eq!(key_source(&commands[1], &overrides), KeySource::Unbound);
        unbind(&commands, &mut overrides, 2);
        // Continuous had a custom chord; unbinding a command with no
        // default leaves no override.
        assert!(!overrides.contains_key("View: Continuous"));
    }

    #[test]
    fn a_chord_has_one_spelling() {
        assert_eq!(canonical_chord("cmd-alt-3"), canonical_chord("alt-cmd-3"));
        assert_eq!(canonical_chord("super-s"), canonical_chord("cmd-s"));
        assert!(!canonical_chord("cmd-s").contains("super"));
        assert_eq!(canonical_chord("not a chord"), "not a chord");
    }

    #[test]
    fn keystrokes_display_as_symbols() {
        assert_eq!(display_keystroke("cmd-shift-p"), "\u{2318}\u{21E7}P");
        assert_eq!(display_keystroke("cmd-1"), "\u{2318}1");
        assert_eq!(display_keystroke("escape"), "Esc");
        assert_eq!(
            display_keystroke("ctrl-alt-enter"),
            "\u{2303}\u{2325}\u{21A9}"
        );
    }

    #[test]
    fn only_the_first_nine_tool_windows_get_a_number() {
        assert_eq!(tool_window_keystroke(1).as_deref(), Some("cmd-1"));
        assert_eq!(tool_window_keystroke(9).as_deref(), Some("cmd-9"));
        assert_eq!(tool_window_keystroke(10), None);
        assert_eq!(tool_window_keystroke(0), None);
    }
}
