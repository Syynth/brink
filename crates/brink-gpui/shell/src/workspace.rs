//! The window — `docs/gpui-studio-spec.md` §4.
//!
//! Five surfaces: two rails, three docks (the rails' four slots address
//! them), the editor centre, and a status bar. The shell owns the frame and
//! the placement rule; it does not know what any particular tool window or
//! editor view is.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, AnyView, App, Entity, IntoElement, Render, SharedString, Window, div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{DockArea, DockPlacement, DockSkin, panel_handle};
use gpui_component::{ActiveTheme, TitleBar, h_flex, v_flex};

use crate::editor_view::{EditorRoot, EditorView, ViewCode, ViewContinuous, ViewSingle};
use crate::rail::{RailButton, rail};
use crate::region::RailEdge;
use crate::skin::StudioSkin;
use crate::tool_window::{ToolWindow, ToolWindowSpec};

/// Reads a tool window's badge without the shell holding the panel's type.
type BadgeReader = Box<dyn Fn(&App) -> Option<SharedString>>;

/// A registered tool window: its spec, and its badge reader. The dock owns
/// the panel handle itself.
struct Registered {
    spec: ToolWindowSpec,
    badge: BadgeReader,
}

/// One cell of the status bar. A cell that `opens` a tool window is drawn
/// as a button — the spec's "N errors — click → Problems" (§4 status bar).
#[derive(Debug, Clone)]
pub struct StatusCell {
    pub text: SharedString,
    pub opens: Option<SharedString>,
}

impl StatusCell {
    #[must_use]
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            opens: None,
        }
    }

    /// Clicking the cell opens the tool window with this id.
    #[must_use]
    pub fn opens(mut self, tool_window: impl Into<SharedString>) -> Self {
        self.opens = Some(tool_window.into());
        self
    }
}

/// The studio window.
pub struct Workspace {
    dock_area: Entity<DockArea>,
    /// The centre's one panel, holding the three views
    /// (`crate::editor_view`).
    editor_root: Entity<EditorRoot>,
    tools: Vec<Registered>,
    /// Rendered along the bottom edge, under everything.
    status: Vec<StatusCell>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A `DockArea` built without a skin is gpui-base's bare area, which
        // docks and drags but draws no chrome at all — no tab bar anywhere
        // (HANDOFF.md "Known broken" #1, fixed). The toolkit's skin goes on
        // wrapped (`skin.rs`), so the editor root's group draws no title
        // strip. `DockSkin::new` needs the area's own context, hence the
        // capture — the toolkit's `DockSkin::dock_area` does the same dance.
        let mut skin = None;
        let dock_area = cx.new(|cx| {
            let inner = DockSkin::new(cx);
            skin = Some(inner.clone());
            DockArea::new("brink-studio", Some(1), window, cx)
                .with_renderer(Rc::new(StudioSkin::new(inner)))
        });
        if let Some(skin) = skin {
            // The rails are the one affordance for opening and closing a dock
            // (`docs/gpui-studio-spec.md` §4.1); the toolkit's own collapse
            // buttons in every title strip would be a second, disagreeing one.
            skin.set_toggle_button_visible(false, cx);
        }

        let editor_root = cx.new(EditorRoot::new);
        dock_area.update(cx, |area, cx| {
            area.add_panel_view(
                panel_handle(editor_root.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });

        Self {
            dock_area,
            editor_root,
            tools: Vec::new(),
            status: Vec::new(),
        }
    }

    /// The centre's panel. Subscribe to it for `EditorRootEvent`.
    #[must_use]
    pub fn editor_root(&self) -> &Entity<EditorRoot> {
        &self.editor_root
    }

    /// Register a tool window and place it in the dock its rail slot names.
    ///
    /// The shell learns nothing about `panel` beyond the `Panel` trait — the
    /// edge this crate exists to keep one-way.
    pub fn add_tool_window<P: ToolWindow>(
        &mut self,
        spec: ToolWindowSpec,
        panel: Entity<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placement = spec.dock_placement();
        let size = spec.default_size;
        let open = spec.open_by_default;

        let badge = {
            let panel = panel.clone();
            Box::new(move |cx: &App| panel.read(cx).badge(cx))
        };
        self.dock_area.update(cx, |area, cx| {
            // `panel_handle`, not the bare entity: base's `add_panel` stores
            // the entity alone, and the skin cannot recover a title from it —
            // the tab would read the panel's registered name instead.
            area.add_panel_view(panel_handle(panel), placement, size, window, cx);
            if area.is_dock_open(placement) != open {
                area.toggle_dock(placement, window, cx);
            }
        });
        self.tools.push(Registered { spec, badge });
    }

    /// Hand the shell what a view shows. It learns nothing about the view
    /// beyond that it renders.
    pub fn set_view_occupant(
        &mut self,
        view: EditorView,
        occupant: AnyView,
        cx: &mut Context<Self>,
    ) {
        self.editor_root
            .update(cx, |root, cx| root.set_occupant(view, occupant, cx));
    }

    pub fn set_editor_view(&mut self, view: EditorView, cx: &mut Context<Self>) {
        self.editor_root
            .update(cx, |root, cx| root.set_view(view, cx));
        cx.notify();
    }

    #[must_use]
    pub fn editor_view(&self, cx: &App) -> EditorView {
        self.editor_root.read(cx).view()
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

    /// Open the dock a tool window lives in, if it is closed. What a status
    /// cell or a badge click wants: never a toggle, since "show me the
    /// problems" must not close them.
    pub fn open_tool_window(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.tools.iter().find(|t| t.spec.id == id) else {
            return;
        };
        let placement = tool.spec.dock_placement();
        self.dock_area.update(cx, |area, cx| {
            if !area.is_dock_open(placement) {
                area.toggle_dock(placement, window, cx);
            }
        });
        cx.notify();
    }

    /// Replace the status-bar cells, left to right.
    pub fn set_status(&mut self, cells: Vec<StatusCell>, cx: &mut Context<Self>) {
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
                badge: (t.badge)(cx),
            })
            .collect()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (hover, fg) = (theme.muted.opacity(0.6), theme.foreground);
        let cells: Vec<AnyElement> = self
            .status
            .iter()
            .enumerate()
            .map(|(ix, cell)| match &cell.opens {
                None => div().child(cell.text.clone()).into_any_element(),
                Some(tool) => {
                    let tool = tool.clone();
                    div()
                        .id(("status-cell", ix))
                        .px_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .child(cell.text.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_tool_window(&tool, window, cx);
                        }))
                        .into_any_element()
                }
            })
            .collect();
        h_flex()
            .h(px(24.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(theme.sidebar)
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .children(cells)
            .into_any_element()
    }

    /// The view switcher: three toggles, in the title bar. The studio has no
    /// dedicated widget for this (its views are palette commands); the native
    /// app gives them a permanent home, since which view you are in changes
    /// what the whole centre means.
    fn view_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.editor_view(cx);
        h_flex()
            .gap_0p5()
            .children(EditorView::ALL.iter().map(|&view| {
                Button::new(SharedString::from(format!(
                    "view-{}",
                    view.persistence_key()
                )))
                .ghost()
                .compact()
                .toggled(view == current)
                .tooltip(format!("{} ({})", view.title(), view.keystroke()))
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.set_editor_view(view, cx);
                }))
                .child(view.title())
            }))
            .into_any_element()
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
        let switcher = self.view_switcher(cx);
        let status = self.render_status(cx);

        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            // The view actions dispatch from wherever focus is; this is an
            // ancestor of everything in the window, so it hears them all.
            .on_action(cx.listener(|this, _: &ViewCode, _, cx| {
                this.set_editor_view(EditorView::Code, cx);
            }))
            .on_action(cx.listener(|this, _: &ViewSingle, _, cx| {
                this.set_editor_view(EditorView::Single, cx);
            }))
            .on_action(cx.listener(|this, _: &ViewContinuous, _, cx| {
                this.set_editor_view(EditorView::Continuous, cx);
            }))
            .child(
                TitleBar::new().child(
                    h_flex()
                        .flex_1()
                        .items_center()
                        .justify_between()
                        .child(gpui_component::label::Label::new("brink"))
                        .child(switcher),
                ),
            )
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
