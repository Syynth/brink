//! The window — `docs/gpui-studio-spec.md` §4.
//!
//! Five surfaces: two rails, three docks (the rails' four slots address
//! them), the editor centre, and a status bar. The shell owns the frame and
//! the placement rule; it does not know what any particular tool window or
//! editor view is.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Action, AnyElement, AnyView, App, Entity, FocusHandle, IntoElement, Render, SharedString,
    Subscription, Window, anchored, deferred, div, point, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{DockArea, DockPlacement, DockSkin, PanelId, panel_handle};
use gpui_component::{ActiveTheme, TitleBar, h_flex, v_flex};

use crate::commands::{
    CommandRegistry, ToggleMenu, TogglePalette, ToggleToolWindow, tool_window_keystroke,
};
use crate::editor_view::{EditorRoot, EditorView, ViewCode, ViewContinuous, ViewSingle};
use crate::palette::{PALETTE_WIDTH, Palette, PaletteEvent, PaletteItem, PaletteMode};
use crate::rail::{RAIL_WIDTH, RailButton, rail};
use crate::region::RailEdge;
use crate::skin::StudioSkin;
use crate::theme::{self, SelectTheme};
use crate::tool_window::{Badge, TabSlot, ToolWindow, ToolWindowSpec, select_tab};

/// Reads a tool window's badge without the shell holding the panel's type.
type BadgeReader = Box<dyn Fn(&App) -> Option<Badge>>;

/// Whether a tool window is its group's displayed tab.
type ActiveReader = Box<dyn Fn(&App) -> bool>;
/// Make a tool window its group's displayed tab.
type TabSelector = Box<dyn Fn(&mut Window, &mut App)>;

/// A registered tool window: its spec, and closures over the panel for what
/// the shell needs without holding the panel's type. The dock owns the
/// panel handle itself.
struct Registered {
    spec: ToolWindowSpec,
    badge: BadgeReader,
    is_active: ActiveReader,
    select: TabSelector,
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
    /// Every command, in registration order (`crate::commands`).
    commands: CommandRegistry,
    /// The palette or the menu while open, with what had focus before it —
    /// restored before the chosen command runs, so it runs where the
    /// author was.
    overlay: Option<(Entity<Palette>, Option<FocusHandle>, Subscription)>,
    /// The window's fallback focus: where keys land before anything has
    /// been clicked, and where they return when the focused surface goes
    /// off screen. Without it a fresh window hears no shortcut at all.
    focus: FocusHandle,
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

        let mut this = Self {
            dock_area,
            editor_root,
            tools: Vec::new(),
            status: Vec::new(),
            commands: CommandRegistry::default(),
            overlay: None,
            focus: cx.focus_handle(),
        };
        // The shell's own commands. Features add theirs through
        // `register_command`; tool windows get a toggle each on registration.
        let (code, single, continuous) =
            (EditorView::Code, EditorView::Single, EditorView::Continuous);
        this.register_command("View", code.title(), ViewCode, Some(code.keystroke()), cx);
        this.register_command(
            "View",
            single.title(),
            ViewSingle,
            Some(single.keystroke()),
            cx,
        );
        this.register_command(
            "View",
            continuous.title(),
            ViewContinuous,
            Some(continuous.keystroke()),
            cx,
        );
        this.register_command(
            "View",
            "Command Palette",
            TogglePalette,
            Some("cmd-shift-p"),
            cx,
        );
        // One command per theme — the studio's `theme.select.<id>`.
        for theme in theme::builtin() {
            this.register_command(
                "Theme",
                theme.label,
                SelectTheme {
                    id: theme.id.into(),
                },
                None,
                cx,
            );
        }
        this
    }

    /// Register a command and install its default binding. Studio §6: a
    /// button, a key and a palette entry are one action, never three
    /// functions.
    pub fn register_command(
        &mut self,
        group: impl Into<SharedString>,
        title: impl Into<SharedString>,
        action: impl Action,
        keystroke: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if let Some(binding) = self.commands.register(group, title, action, keystroke) {
            cx.bind_keys([binding]);
        }
    }

    #[must_use]
    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
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

        let me = PanelId::from(panel.entity_id());
        let badge = {
            let panel = panel.clone();
            Box::new(move |cx: &App| panel.read(cx).badge(cx))
        };
        let is_active = {
            let panel = panel.clone();
            Box::new(move |cx: &App| {
                panel
                    .read(cx)
                    .tab_slot()
                    .is_none_or(|slot| slot.is_active(me, cx))
            })
        };
        let select = {
            let panel = panel.clone();
            Box::new(move |window: &mut Window, cx: &mut App| {
                let group = panel.read(cx).tab_slot().and_then(TabSlot::group);
                if let Some(group) = group {
                    select_tab(&group, me, window, cx);
                }
            })
        };
        self.dock_area.update(cx, |area, cx| {
            // `panel_handle`, not the bare entity: base's `add_panel` stores
            // the entity alone, and the skin cannot recover a title from it —
            // the tab would read the panel's registered name instead.
            area.add_panel_view(panel_handle(panel), placement, size, window, cx);
            // `open_by_default` opens; it never closes a dock another tool
            // window already opened.
            if open && !area.is_dock_open(placement) {
                area.toggle_dock(placement, window, cx);
            }
        });
        // The dock shows the newest panel. A window that does not open by
        // default must not take the tab from one that does.
        if !open
            && let Some(previous) = self
                .tools
                .iter()
                .rev()
                .find(|t| t.spec.dock_placement() == placement && t.spec.open_by_default)
        {
            (previous.select)(window, cx);
        }
        // `view.toggle.<id>`, `cmd-1…9` by registration order (studio §5.2).
        let ordinal = self.tools.len() + 1;
        let keystroke = tool_window_keystroke(ordinal);
        self.register_command(
            "View",
            format!("Toggle {}", spec.title),
            ToggleToolWindow {
                id: spec.id.clone(),
            },
            keystroke.as_deref(),
            cx,
        );
        self.tools.push(Registered {
            spec,
            badge,
            is_active,
            select,
        });
    }

    /// Hand the shell what a view shows, and what to focus when it is
    /// shown. It learns nothing about the view beyond that it renders.
    pub fn set_view_occupant(
        &mut self,
        view: EditorView,
        occupant: AnyView,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) {
        self.editor_root
            .update(cx, |root, cx| root.set_occupant(view, occupant, focus, cx));
    }

    /// Switch views, and move focus into the one now showing: the view that
    /// just left the screen cannot keep it, or every shortcut goes dead
    /// until the next click.
    pub fn set_editor_view(
        &mut self,
        view: EditorView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.editor_root.update(cx, |root, cx| {
            root.set_view(view, cx);
            root.occupant_focus()
        });
        window.focus(&focus.unwrap_or_else(|| self.focus.clone()), cx);
        cx.notify();
    }

    #[must_use]
    pub fn editor_view(&self, cx: &App) -> EditorView {
        self.editor_root.read(cx).view()
    }

    /// The rail-button gesture. Tab-level: a closed dock opens showing this
    /// window; an open dock showing another window switches to it; an open
    /// dock already showing it closes.
    pub fn toggle_tool_window(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.tools.iter().find(|t| t.spec.id == id) else {
            return;
        };
        let placement = tool.spec.dock_placement();
        let open = self.dock_area.read(cx).is_dock_open(placement);
        let active = (tool.is_active)(cx);
        if open && active {
            self.dock_area
                .update(cx, |area, cx| area.toggle_dock(placement, window, cx));
        } else {
            if !open {
                self.dock_area
                    .update(cx, |area, cx| area.toggle_dock(placement, window, cx));
            }
            (tool.select)(window, cx);
        }
        cx.notify();
    }

    /// Show a tool window: open its dock if closed and select its tab. What
    /// a status cell or a command wants — never a toggle, since "show me
    /// the problems" must not close them.
    pub fn open_tool_window(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.tools.iter().find(|t| t.spec.id == id) else {
            return;
        };
        let placement = tool.spec.dock_placement();
        if !self.dock_area.read(cx).is_dock_open(placement) {
            self.dock_area
                .update(cx, |area, cx| area.toggle_dock(placement, window, cx));
        }
        (tool.select)(window, cx);
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
                // Pressed when this window is the one on screen: its dock
                // open AND its tab the displayed one.
                active: area.is_dock_open(t.spec.dock_placement()) && (t.is_active)(cx),
                badge: (t.badge)(cx),
                keystroke: self.commands.keystroke_for(&ToggleToolWindow {
                    id: t.spec.id.clone(),
                }),
            })
            .collect()
    }

    /// Open the palette or the menu, or close it if that one is already up.
    pub fn toggle_overlay(
        &mut self,
        mode: PaletteMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((palette, _, _)) = &self.overlay {
            let same = palette.read(cx).mode() == mode;
            self.close_overlay(window, cx);
            if same {
                return;
            }
        }
        // Enablement is asked of the window NOW, against the focus the
        // author has — before the overlay takes it. Per action, not from
        // `available_actions()`: that list is built by constructing each
        // listener's action type from nothing, which a data-carrying
        // `no_json` action (`ToggleToolWindow`, `SelectTheme`) cannot do,
        // so it never appears there and read as disabled.
        let items: Vec<PaletteItem> = self
            .commands
            .commands()
            .iter()
            .map(|c| PaletteItem {
                enabled: window.is_action_available(c.action.as_ref(), cx),
                command: c.clone(),
            })
            .collect();
        let previous = window.focused(cx);
        let palette = cx.new(|cx| Palette::new(mode, items, window, cx));
        let subscription = cx.subscribe_in(
            &palette,
            window,
            |this, _, event: &PaletteEvent, window, cx| match event {
                PaletteEvent::Run(action) => {
                    let action = action.boxed_clone();
                    this.close_overlay(window, cx);
                    window.dispatch_action(action, cx);
                }
                PaletteEvent::Dismiss => this.close_overlay(window, cx),
            },
        );
        palette.update(cx, |palette, cx| palette.focus(window, cx));
        self.overlay = Some((palette, previous, subscription));
        cx.notify();
    }

    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, previous, _)) = self.overlay.take() {
            if let Some(handle) = previous {
                window.focus(&handle, cx);
            }
            cx.notify();
        }
    }

    fn render_overlay(&self, window: &Window, cx: &App) -> Option<AnyElement> {
        let (palette, _, _) = self.overlay.as_ref()?;
        // The palette floats top-centre; the menu hangs off the hamburger.
        let position = match palette.read(cx).mode() {
            PaletteMode::Palette => {
                let width = window.viewport_size().width;
                point((width - px(PALETTE_WIDTH)) / 2., px(64.))
            }
            PaletteMode::Menu => point(RAIL_WIDTH + px(4.), px(40.)),
        };
        Some(
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(palette.clone()),
            )
            .into_any_element(),
        )
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_editor_view(view, window, cx);
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

impl gpui::Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
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
        let overlay = self.render_overlay(window, cx);
        // Studio §6: the hamburger at the top of the left strip, opening the
        // registry-generated menu.
        let hamburger = Button::new("hamburger")
            .ghost()
            .compact()
            .tooltip("Menu")
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_overlay(PaletteMode::Menu, window, cx);
            }))
            .child("\u{2630}")
            .into_any_element();

        let theme = cx.theme();
        v_flex()
            .id("workspace")
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            // The shell's actions dispatch from wherever focus is; this is
            // an ancestor of everything in the window, so it hears them all.
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ViewCode, window, cx| {
                this.set_editor_view(EditorView::Code, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ViewSingle, window, cx| {
                this.set_editor_view(EditorView::Single, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ViewContinuous, window, cx| {
                this.set_editor_view(EditorView::Continuous, window, cx);
            }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                this.toggle_overlay(PaletteMode::Palette, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleMenu, window, cx| {
                this.toggle_overlay(PaletteMode::Menu, window, cx);
            }))
            .on_action(cx.listener(|this, action: &ToggleToolWindow, window, cx| {
                this.toggle_tool_window(&action.id, window, cx);
            }))
            .on_action(cx.listener(|_, action: &SelectTheme, window, cx| {
                theme::select(&action.id, Some(window), cx);
                cx.notify();
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
                    .child(rail(
                        RailEdge::Left,
                        &buttons,
                        Some(hamburger),
                        click.clone(),
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.dock_area.clone()),
                    )
                    .child(rail(RailEdge::Right, &buttons, None, click, window, cx)),
            )
            .child(status)
            .children(overlay)
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
