//! The window — `docs/gpui-studio-spec.md` §4.
//!
//! Five surfaces: two rails, three docks (the rails' four slots address
//! them), the editor center, and a status bar. The shell owns the frame and
//! the placement rule; it does not know what any particular tool window is.

use gpui::prelude::*;
use gpui::{App, Entity, IntoElement, Render, SharedString, Window, div, px};
use gpui_component::dock::{DockArea, DockPlacement, Panel};
use gpui_component::{ActiveTheme, h_flex, v_flex};

use crate::rail::{RailButton, rail};
use crate::region::RailEdge;
use crate::tool_window::ToolWindowSpec;

/// A registered tool window. Only the spec is kept: the dock owns the panel
/// handle, and holding a second one here purely against a future
/// select-that-tab call would be dead state today.
struct Registered {
    spec: ToolWindowSpec,
}

/// The studio window.
pub struct Workspace {
    dock_area: Entity<DockArea>,
    tools: Vec<Registered>,
    /// Rendered along the bottom edge, under everything.
    status: Vec<SharedString>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dock_area = cx.new(|cx| DockArea::new("brink-studio", Some(1), window, cx));
        Self {
            dock_area,
            tools: Vec::new(),
            status: Vec::new(),
        }
    }

    #[must_use]
    pub fn dock_area(&self) -> &Entity<DockArea> {
        &self.dock_area
    }

    /// Register a tool window and place it in the dock its rail slot names.
    ///
    /// The shell learns nothing about `panel` beyond the `Panel` trait — the
    /// edge this crate exists to keep one-way.
    pub fn add_tool_window<P: Panel>(
        &mut self,
        spec: ToolWindowSpec,
        panel: Entity<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placement = spec.dock_placement();
        let size = spec.default_size;
        let open = spec.open_by_default;

        self.dock_area.update(cx, |area, cx| {
            area.add_panel(panel, placement, size, window, cx);
            if area.is_dock_open(placement) != open {
                area.toggle_dock(placement, window, cx);
            }
        });
        self.tools.push(Registered { spec });
    }

    /// Put a panel in the editor center.
    pub fn set_center<P: Panel>(
        &mut self,
        panel: Entity<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock_area.update(cx, |area, cx| {
            area.add_panel(panel, DockPlacement::Center, None, window, cx);
        });
    }

    /// Toggle the dock a tool window lives in.
    ///
    /// Dock-level, not tab-level: the toolkit exposes no way to activate one
    /// panel inside a tab group from outside it, and with the docks the
    /// slice fills the two are the same thing. When a dock grows a second
    /// tab this wants to become "open the dock AND select that tab".
    pub fn toggle_tool_window(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.tools.iter().find(|t| t.spec.id == id) else {
            return;
        };
        let placement = tool.spec.dock_placement();
        self.dock_area.update(cx, |area, cx| {
            area.toggle_dock(placement, window, cx);
        });
        cx.notify();
    }

    /// Replace the status-bar cells, left to right.
    pub fn set_status(&mut self, cells: Vec<SharedString>, cx: &mut Context<Self>) {
        self.status = cells;
        cx.notify();
    }

    fn buttons(&self, cx: &App) -> Vec<RailButton> {
        let area = self.dock_area.read(cx);
        self.tools
            .iter()
            .map(|t| RailButton {
                id: t.spec.id.clone(),
                title: t.spec.title.clone(),
                icon: t.spec.icon,
                slot: t.spec.slot,
                active: area.is_dock_open(t.spec.dock_placement()),
            })
            .collect()
    }
}

impl ToolWindowSpec {
    fn dock_placement(&self) -> DockPlacement {
        self.slot.dock()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.buttons(cx);
        let this = cx.entity();
        let click = {
            let this = this.clone();
            move |id: &SharedString, window: &mut Window, cx: &mut App| {
                let id = id.clone();
                this.update(cx, |workspace, cx| {
                    workspace.toggle_tool_window(&id, window, cx);
                });
            }
        };

        let theme = cx.theme();
        let status = h_flex()
            .h(px(24.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(theme.sidebar)
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .children(self.status.iter().map(|cell| div().child(cell.clone())));

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(rail(RailEdge::Left, &buttons, click.clone(), window, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.dock_area.clone()),
                    )
                    .child(rail(RailEdge::Right, &buttons, click, window, cx)),
            )
            .child(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::RailSlot;

    #[test]
    fn a_specs_dock_follows_its_rail_slot() {
        // The shell must read placement from the slot, never carry a second
        // copy of it — that redundancy is what dropping the bottom rail
        // removed.
        let left = ToolWindowSpec::new("binder", "Binder", RailSlot::LEFT_UPPER);
        let bottom = ToolWindowSpec::new("problems", "Problems", RailSlot::LEFT_LOWER);
        assert_eq!(left.dock_placement(), DockPlacement::Left);
        assert_eq!(bottom.dock_placement(), DockPlacement::Bottom);
    }
}
