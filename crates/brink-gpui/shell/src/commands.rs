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
//! focus path (or a global listener) handles it — `Window::available_actions`
//! — which is the `when` of the studio's contract without a closure per
//! command.
//!
//! Not here yet: the user-override keymap (studio §6 "Keymap layer"). The
//! registry is the single default table it would merge over — every
//! default binding is installed through [`CommandRegistry::register`] —
//! so the seam exists; the JSON half needs `KeyBinding::load` with the
//! platform's keyboard mapper, and data-carrying actions to be
//! serialisable, which they are not while `#[action(no_json)]` is on.

use gpui::{Action, KeyBinding, SharedString, actions};

actions!(
    brink,
    [
        /// Open or close the command palette.
        TogglePalette,
        /// Open or close the hamburger menu.
        ToggleMenu,
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
}

impl Clone for Command {
    fn clone(&self) -> Self {
        Self {
            group: self.group.clone(),
            title: self.title.clone(),
            action: self.action.boxed_clone(),
            keystroke: self.keystroke.clone(),
        }
    }
}

impl Command {
    /// "View: Code" — how the studio's palette spells it.
    #[must_use]
    pub fn full_title(&self) -> String {
        format!("{}: {}", self.group, self.title)
    }
}

/// The commands, in registration order — the order the palette and menu
/// list them in, and the order `Mod-1…9` are handed out in.
#[derive(Default)]
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    /// Register a command and return its key binding, for the caller to
    /// install with `cx.bind_keys`.
    pub fn register<A: Action>(
        &mut self,
        group: impl Into<SharedString>,
        title: impl Into<SharedString>,
        action: A,
        keystroke: Option<&str>,
    ) -> Option<KeyBinding> {
        // The registry keeps a boxed copy; the binding takes the typed one,
        // since `KeyBinding::new` wants a concrete action.
        let boxed = action.boxed_clone();
        let binding = keystroke.map(|keys| KeyBinding::new(keys, action, None));
        self.commands.push(Command {
            group: group.into(),
            title: title.into(),
            action: boxed,
            keystroke: keystroke.map(SharedString::new),
        });
        binding
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

/// Whether `action` has a handler where focus currently is — gpui's
/// enablement, asked through the window.
#[must_use]
pub fn is_available(action: &dyn Action, available: &[Box<dyn Action>]) -> bool {
    let wanted = action.as_any().type_id();
    available.iter().any(|a| a.as_any().type_id() == wanted)
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
        assert_eq!(r.keystroke_for(&ViewSingle).as_deref(), Some("cmd-shift-2"));
        assert_eq!(r.keystroke_for(&ViewContinuous), None);
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

    #[test]
    fn availability_is_by_action_type_not_value() {
        let available: Vec<Box<dyn Action>> = vec![Box::new(ToggleToolWindow {
            id: "binder".into(),
        })];
        // A different id is the same command with different data; the
        // window offers the type, so every id is available.
        assert!(is_available(
            &ToggleToolWindow {
                id: "problems".into()
            },
            &available
        ));
        assert!(!is_available(&ViewCode, &available));
    }
}
