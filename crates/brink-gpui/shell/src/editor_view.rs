//! The editor root and its two modes — decision log 2026-10-03, "The
//! native studio has two modes, Writing and Scripting; Single File is
//! removed" (`docs/gpui-writing-scripting-modes.md`), which revises the
//! 2026-08-26 three-view naming. "The editor root area has one occupant"
//! still holds.
//!
//! The centre of the window holds exactly one panel, [`EditorRoot`], and it
//! renders whichever of two occupants the current [`EditorView`] names.
//! The shell owns the choice and the switching; the feature crate hands
//! over the occupants and the shell never learns what they are — the same
//! one-way edge as tool windows.
//!
//! ## Why the modes are occupants of one panel, not centre layouts
//!
//! The toolkit's `DockArea` folds the centre and the three docks into one
//! layout tree, so a switchable centre has to be a panel in it. The
//! alternative — `set_center` with a fresh layout on every switch — tears
//! the centre down each time (`on_removed` on every panel) and would need
//! Script mode's splits and tab order dumped and restored around every
//! glance at the manuscript. Zed's terminal panel nests a pane tree inside a
//! dock panel for the same reason; this is that shape at the centre.
//!
//! ## Reversible
//!
//! Nothing outside this crate depends on the nesting. A mode arrives as an
//! `AnyView`; Script mode's pane tree is the feature crate's own. Moving to
//! Zed's arrangement — the shell owning the centre directly, with the docks
//! rendered beside it — changes `workspace.rs` and this file, and nothing
//! in `app/`.

use gpui::prelude::*;
use gpui::{
    AnyView, App, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Window,
    actions, div,
};
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{BasePanel, Panel, PanelControl, PanelEvent};

actions!(editor_view, [ModeWrite, ModeScript]);

/// The two modes, in switcher order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorView {
    /// Every file as one manuscript, with almost no chrome — drafting prose.
    Write,
    /// Tabs, groups, splits and the full tool set — structure and logic.
    Script,
}

impl EditorView {
    pub const ALL: [Self; 2] = [Self::Write, Self::Script];

    /// The user-facing name — the ruled vocabulary. The switcher is
    /// icon-only, so this is its tooltip and the command's title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Write => "Write",
            Self::Script => "Script",
        }
    }

    /// A stable key for layout persistence.
    #[must_use]
    pub const fn persistence_key(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Script => "script",
        }
    }

    /// The mode a persisted key names — including the three views' keys
    /// from before the two modes, so a saved layout or default survives:
    /// `continuous` is Write; `code` and the removed `single` are Script.
    #[must_use]
    pub fn from_persistence_key(key: &str) -> Option<Self> {
        match key {
            "write" | "continuous" => Some(Self::Write),
            "script" | "code" | "single" => Some(Self::Script),
            _ => None,
        }
    }

    /// The default keystroke. Default keys for the modes are deferred
    /// (decision log 2026-10-03); each mode keeps the chord its view had —
    /// Script keeps Code's `cmd-alt-1` and Write keeps Continuous's `cmd-alt-3` — so
    /// nothing a hand has learned moves before the keys are ruled.
    /// `cmd-1…9` is what the studio gives tool windows, so the modes take
    /// the alt row. NOT `cmd-shift-<digit>`: on Linux a shifted digit
    /// arrives as its symbol (`shift-2` is `@`, verified in gpui's own x11
    /// tests), so such a binding never matches there. Registered as
    /// commands by the workspace, which is where the binding is installed.
    #[must_use]
    pub const fn keystroke(self) -> &'static str {
        match self {
            Self::Write => "cmd-alt-3",
            Self::Script => "cmd-alt-1",
        }
    }

    /// The switcher's glyph: a pen for Write, `</>` for Script (decision log
    /// 2026-10-03). Neither is in the kit's lucide subset, so both are drawn
    /// to lucide's own conventions (pen-line, code-xml) in `assets/icons/`,
    /// as the icon ruling (2026-09-09) handles a glyph lucide does not ship.
    #[must_use]
    pub fn icon(self) -> gpui_component::Icon {
        match self {
            Self::Write => crate::icons::BrinkIcon::ModeWrite.into(),
            Self::Script => crate::icons::BrinkIcon::ModeScript.into(),
        }
    }

    const fn slot(self) -> usize {
        match self {
            Self::Script => 0,
            Self::Write => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorRootEvent {
    ViewChanged(EditorView),
}

/// The registered panel name. `skin.rs` recognises the editor root's group
/// by it, to draw no tab bar there.
pub(crate) const EDITOR_ROOT_PANEL_NAME: &str = "EditorRoot";

/// The centre's one panel.
pub struct EditorRoot {
    /// Each view, with where focus goes when it is shown — a view that is
    /// not rendered cannot hold focus, and a key pressed while focus sits
    /// in a hidden view reaches nothing.
    occupants: [Option<(AnyView, FocusHandle)>; 2],
    current: EditorView,
    focus: FocusHandle,
}

impl EditorRoot {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            occupants: [None, None],
            current: EditorView::Script,
            focus: cx.focus_handle(),
        }
    }

    /// What a view shows, and what to focus when it is shown. The shell
    /// holds the view as an `AnyView` and never asks what it is.
    pub fn set_occupant(
        &mut self,
        view: EditorView,
        occupant: AnyView,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) {
        self.occupants[view.slot()] = Some((occupant, focus));
        cx.notify();
    }

    /// Where focus belongs while the current view is showing.
    #[must_use]
    pub fn occupant_focus(&self) -> Option<FocusHandle> {
        self.occupants[self.current.slot()]
            .as_ref()
            .map(|(_, focus)| focus.clone())
    }

    pub fn set_view(&mut self, view: EditorView, cx: &mut Context<Self>) {
        if self.current == view {
            return;
        }
        self.current = view;
        cx.emit(EditorRootEvent::ViewChanged(view));
        cx.notify();
    }

    #[must_use]
    pub fn view(&self) -> EditorView {
        self.current
    }

    fn occupant(&self) -> Option<&AnyView> {
        self.occupants[self.current.slot()]
            .as_ref()
            .map(|(view, _)| view)
    }
}

impl Focusable for EditorRoot {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for EditorRoot {}
impl EventEmitter<EditorRootEvent> for EditorRoot {}

impl BasePanel for EditorRoot {
    fn panel_name(&self) -> &'static str {
        EDITOR_ROOT_PANEL_NAME
    }

    /// The centre always has its occupant; there is nothing to close it to.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> bool {
        false
    }
}

impl Panel for EditorRoot {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(self.current.title())
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

    /// The occupant fills the panel edge to edge; Script mode's own tab bar
    /// sits at the top of it.
    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for EditorRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let content = match self.occupant() {
            Some(view) => view.clone().into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(format!(
                    "Nothing registered for {} mode",
                    self.current.title()
                ))
                .into_any_element(),
        };
        div().size_full().min_h_0().child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_modes_have_distinct_keys_and_slots() {
        let mut keys: Vec<&str> = EditorView::ALL
            .iter()
            .map(|v| v.persistence_key())
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 2, "a collision would merge two modes on reload");

        let mut slots: Vec<usize> = EditorView::ALL.iter().map(|v| v.slot()).collect();
        slots.sort_unstable();
        assert_eq!(slots, [0, 1], "each mode needs its own occupant slot");

        let mut strokes: Vec<&str> = EditorView::ALL.iter().map(|v| v.keystroke()).collect();
        strokes.sort_unstable();
        strokes.dedup();
        assert_eq!(strokes.len(), 2, "two modes on one keystroke");
    }

    #[test]
    fn titles_are_the_ruled_vocabulary() {
        // Decision log 2026-10-03 names them.
        assert_eq!(EditorView::Write.title(), "Write");
        assert_eq!(EditorView::Script.title(), "Script");
    }

    #[test]
    fn every_key_ever_persisted_still_names_a_mode() {
        for view in EditorView::ALL {
            assert_eq!(
                EditorView::from_persistence_key(view.persistence_key()),
                Some(view)
            );
        }
        // The three views' keys from before the two modes.
        assert_eq!(
            EditorView::from_persistence_key("continuous"),
            Some(EditorView::Write)
        );
        assert_eq!(
            EditorView::from_persistence_key("code"),
            Some(EditorView::Script)
        );
        assert_eq!(
            EditorView::from_persistence_key("single"),
            Some(EditorView::Script),
            "a saved Single File reopens in Script, which shows one file per tab"
        );
        assert_eq!(EditorView::from_persistence_key("nonsense"), None);
    }
}
