//! The dock's appearance: `gpui-component`'s skin, with two exceptions.
//!
//! The outer area's centre holds one panel, the editor root, and the toolkit
//! draws a title strip over a lone panel (`PanelStyle::Auto`). The editor
//! root is not a tool window and must not wear one — Code view draws its own
//! tab bar inside it — so the group holding it draws no bar at all.
//!
//! The side docks draw no bar either (maintainer, 2026-09-05: "why is it
//! there?"). The rail is the switcher for a rail-addressed dock — its button
//! opens-and-selects the tool window — and each window carries its own
//! header, so a tab strip above them named the same thing a third time. The
//! bottom dock keeps its strip: that row also carries the panels' toolbars
//! (Problems' counts and filters), which have nowhere else to go yet.
//! Everything else delegates to the toolkit's skin unchanged.

use std::{cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

use gpui::{AnyElement, AnyView, App, Axis, Div, Empty, IntoElement as _, Stateful, Window};
use gpui_base::ResizeHandleContext;
use gpui_component::dock::{
    BasePanelView, DockAreaRenderer, DockContext, DockSkin, DropIndicator, NodeId, PanelState,
    TabGroupContext, TabGroupRenderer, TilesRenderer,
};

use crate::editor_view::EDITOR_ROOT_PANEL_NAME;

pub(crate) struct StudioSkin {
    inner: Rc<DockSkin>,
    /// Panel names whose groups draw no tab bar: the side docks' tool
    /// windows, registered by the workspace as they are added.
    barless: Rc<RefCell<HashSet<&'static str>>>,
}

impl StudioSkin {
    pub(crate) fn new(inner: Rc<DockSkin>) -> Self {
        Self {
            inner,
            barless: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    /// The set the workspace adds side-dock tool windows to.
    pub(crate) fn barless(&self) -> Rc<RefCell<HashSet<&'static str>>> {
        self.barless.clone()
    }
}

impl DockAreaRenderer for StudioSkin {
    fn frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner.frame(window, cx)
    }

    fn split_frame(
        &self,
        node: NodeId,
        axis: Axis,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.inner.split_frame(node, axis, window, cx)
    }

    fn center_frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner.center_frame(window, cx)
    }

    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_split_handle(handle, window, cx)
    }

    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.inner.render_dock(dock, content, window, cx)
    }

    fn build_placeholder(
        &self,
        state: &PanelState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<dyn BasePanelView>> {
        self.inner.build_placeholder(state, window, cx)
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(StudioTabGroups {
            inner: self.inner.tab_group_renderer(),
            barless: self.barless.clone(),
        })
    }

    fn tiles_renderer(&self) -> Rc<dyn TilesRenderer> {
        self.inner.tiles_renderer()
    }
}

struct StudioTabGroups {
    inner: Rc<dyn TabGroupRenderer>,
    barless: Rc<RefCell<HashSet<&'static str>>>,
}

/// Whether a group is the editor root's, by the names of the panels in it.
/// Only ever exactly that one panel: the root is unclosable and nothing is
/// added beside it.
fn is_editor_root_group(names: &[&str]) -> bool {
    names == [EDITOR_ROOT_PANEL_NAME]
}

/// A group of nothing but side-dock tool windows: the rail names and
/// switches them, so the strip would be a third copy of the same names.
/// A group that is empty, or holds anything else, keeps the toolkit's bar.
fn is_barless_group(names: &[&str], barless: &HashSet<&'static str>) -> bool {
    !names.is_empty() && names.iter().all(|n| barless.contains(n))
}

impl TabGroupRenderer for StudioTabGroups {
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.inner.frame(group, window, cx)
    }

    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.inner.content_frame(group, window, cx)
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let names: Vec<&str> = group
            .panels()
            .iter()
            .map(|panel| panel.panel_name(cx))
            .collect();
        if is_editor_root_group(&names) || is_barless_group(&names, &self.barless.borrow()) {
            Empty.into_any_element()
        } else {
            self.inner.render_tab_bar(group, window, cx)
        }
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.inner.render_active_panel(panel, group, window, cx)
    }

    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_drop_indicator(indicator, window, cx)
    }

    fn render_empty(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.inner.render_empty(group, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_dock_of_tool_windows_draws_no_bar() {
        let barless: HashSet<&'static str> = ["Binder", "Search"].into_iter().collect();
        assert!(is_barless_group(&["Binder"], &barless));
        assert!(is_barless_group(&["Binder", "Search"], &barless));
        // The bottom dock's windows are not registered, so their row — which
        // carries their toolbars — stays.
        assert!(!is_barless_group(&["Problems", "TODOs"], &barless));
        // Anything unregistered in the group keeps the bar: it is the only
        // way back to that panel.
        assert!(!is_barless_group(&["Binder", "Document"], &barless));
        assert!(!is_barless_group(&[], &barless));
    }

    #[test]
    fn only_the_lone_editor_root_loses_its_bar() {
        assert!(is_editor_root_group(&[EDITOR_ROOT_PANEL_NAME]));
        // A tool window alone in its dock is the skin's other exception,
        // handled by `is_barless_group`, not this one.
        assert!(!is_editor_root_group(&["Binder"]));
        // Nothing is ever docked beside the root, but if something were, the
        // bar is how the user would get back to it.
        assert!(!is_editor_root_group(&[EDITOR_ROOT_PANEL_NAME, "Binder"]));
        assert!(!is_editor_root_group(&[]));
    }
}
