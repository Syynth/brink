//! The editor root and its three views — decision log 2026-08-26, "The
//! three editor views are named Code, Single File, and Continuous" and
//! "The editor root area has one occupant".
//!
//! The centre of the window holds exactly one panel, [`EditorRoot`], and it
//! renders whichever of three occupants the current [`EditorView`] names.
//! The shell owns the choice and the switching; the feature crate hands
//! over the occupants and the shell never learns what they are — the same
//! one-way edge as tool windows.
//!
//! ## Why the views are occupants of one panel, not centre layouts
//!
//! The toolkit's `DockArea` folds the centre and the three docks into one
//! layout tree, so a switchable centre has to be a panel in it. The
//! alternative — `set_center` with a fresh layout on every switch — tears
//! the centre down each time (`on_removed` on every panel) and would need
//! Code view's splits and tab order dumped and restored around every glance
//! at the manuscript. Zed's terminal panel nests a pane tree inside a dock
//! panel for the same reason; this is that shape at the centre.
//!
//! ## Reversible
//!
//! Nothing outside this crate depends on the nesting. A view arrives as an
//! `AnyView`; Code view's pane tree is the feature crate's own. Moving to
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

actions!(editor_view, [ViewCode, ViewSingle, ViewContinuous]);

/// The three views, in switcher order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorView {
    /// Tabs, groups, splits — a writer working across files.
    Code,
    /// One file at a time, no tab strip; navigating replaces what is shown.
    Single,
    /// Every file as one manuscript.
    Continuous,
}

impl EditorView {
    pub const ALL: [Self; 3] = [Self::Code, Self::Single, Self::Continuous];

    /// The user-facing name — the ruled vocabulary, so it is also what the
    /// switcher shows.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Code => "Code",
            Self::Single => "Single File",
            Self::Continuous => "Continuous",
        }
    }

    /// A stable key for layout persistence.
    #[must_use]
    pub const fn persistence_key(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Single => "single",
            Self::Continuous => "continuous",
        }
    }

    /// The default keystroke. `cmd-1…9` is what the studio gives tool
    /// windows, so the views take the alt row. NOT `cmd-shift-<digit>`:
    /// on Linux a shifted digit arrives as its symbol (`shift-2` is `@`,
    /// verified in gpui's own x11 tests), so such a binding never matches
    /// there. Registered as commands by the workspace, which is where the
    /// binding is installed.
    #[must_use]
    pub const fn keystroke(self) -> &'static str {
        match self {
            Self::Code => "cmd-alt-1",
            Self::Single => "cmd-alt-2",
            Self::Continuous => "cmd-alt-3",
        }
    }

    const fn slot(self) -> usize {
        match self {
            Self::Code => 0,
            Self::Single => 1,
            Self::Continuous => 2,
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
    occupants: [Option<(AnyView, FocusHandle)>; 3],
    current: EditorView,
    focus: FocusHandle,
}

impl EditorRoot {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            occupants: [None, None, None],
            current: EditorView::Code,
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

    /// The occupant fills the panel edge to edge; Code view's own tab bar
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
                    "Nothing registered for the {} view",
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
    fn the_three_views_have_distinct_keys_and_slots() {
        let mut keys: Vec<&str> = EditorView::ALL
            .iter()
            .map(|v| v.persistence_key())
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 3, "a collision would merge two views on reload");

        let mut slots: Vec<usize> = EditorView::ALL.iter().map(|v| v.slot()).collect();
        slots.sort_unstable();
        assert_eq!(slots, [0, 1, 2], "each view needs its own occupant slot");

        let mut strokes: Vec<&str> = EditorView::ALL.iter().map(|v| v.keystroke()).collect();
        strokes.sort_unstable();
        strokes.dedup();
        assert_eq!(strokes.len(), 3, "two views on one keystroke");
    }

    #[test]
    fn titles_are_the_ruled_vocabulary() {
        // Decision log 2026-08-26 names them; the switcher shows these.
        assert_eq!(EditorView::Code.title(), "Code");
        assert_eq!(EditorView::Single.title(), "Single File");
        assert_eq!(EditorView::Continuous.title(), "Continuous");
    }
}
