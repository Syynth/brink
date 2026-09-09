//! The window — `docs/gpui-studio-spec.md` §4.
//!
//! Five surfaces: two rails, three docks (the rails' four slots address
//! them), the editor centre, and a status bar. The shell owns the frame and
//! the placement rule; it does not know what any particular tool window or
//! editor view is.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Action, AnyElement, AnyView, App, ClickEvent, Entity, FocusHandle, IntoElement, Render,
    SharedString, Subscription, Window, anchored, deferred, div, point, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{DockArea, DockPlacement, DockSkin, PanelId, panel_handle};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Sizable as _, TitleBar, h_flex, v_flex};

use crate::commands::{
    CommandRegistry, OpenSettings, ToggleMenu, TogglePalette, ToggleToolWindow, Unbound,
    bind_chord, keymap_bindings, reset, tool_window_keystroke, unbind,
};
use crate::editor_view::{EditorRoot, EditorView, ViewCode, ViewContinuous, ViewSingle};
use crate::palette::{PALETTE_WIDTH, Palette, PaletteEvent, PaletteItem, PaletteMode};
use crate::rail::{RAIL_WIDTH, RailButton, rail};
use crate::region::RailEdge;
use crate::settings::{self, AppSettings};
use crate::settings_appearance::AppearanceSection;
use crate::settings_editor::EditorSection;
use crate::settings_keymap::KeymapSection;
use crate::settings_modal::{
    MODAL_HEIGHT, MODAL_WIDTH, Scope, Section, SectionMeta, SettingsEvent, SettingsModal,
};
use crate::settings_player::PlayerSection;
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
///
/// `docs/studio-shell-spec.md` §7.3 puts the bar in two groups: what the
/// PROJECT is doing on the left, what the CARET is doing on the right. A
/// cell says which end it belongs to rather than the bar keeping two
/// lists, so a caller builds one vector in the order it thinks in.
#[derive(Debug, Clone)]
pub struct StatusCell {
    pub text: SharedString,
    pub opens: Option<SharedString>,
    pub align_end: bool,
}

impl StatusCell {
    #[must_use]
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            opens: None,
            align_end: false,
        }
    }

    /// Put this cell in the right-hand group.
    #[must_use]
    pub fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }

    /// Clicking the cell opens the tool window with this id.
    #[must_use]
    pub fn opens(mut self, tool_window: impl Into<SharedString>) -> Self {
        self.opens = Some(tool_window.into());
        self
    }
}

/// How much room the window has (`docs/studio-shell-spec.md` §5.3).
///
/// Only the width matters: the docks that give way are the side ones, and
/// what a narrow window cannot afford is columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Everything fits.
    Wide,
    /// One side dock gives way — the right, which holds inspectors
    /// rather than the file you are working in.
    Medium,
    /// Both side docks give way; the editor keeps the window.
    Narrow,
}

impl Tier {
    /// The tier for a viewport width. The thresholds are the width at
    /// which the editor stops having room to read in, not round numbers:
    /// two 260px side docks plus a 600px editor is ~1120, and one dock
    /// plus that editor is ~860.
    #[must_use]
    pub fn of(width: f32) -> Self {
        if width >= 1120. {
            Self::Wide
        } else if width >= 860. {
            Self::Medium
        } else {
            Self::Narrow
        }
    }

    /// Which docks this tier can afford, by placement name.
    #[must_use]
    pub fn allows(self, dock: &str) -> bool {
        match self {
            Self::Wide => true,
            Self::Medium => dock != "right",
            // The bottom dock is a strip, not a column: Problems still
            // fits when nothing beside the editor does.
            Self::Narrow => dock == "bottom",
        }
    }
}

/// The studio window.
pub struct Workspace {
    dock_area: Entity<DockArea>,
    /// The centre's one panel, holding the three views
    /// (`crate::editor_view`).
    editor_root: Entity<EditorRoot>,
    tools: Vec<Registered>,
    /// Panel names of the side docks' tool windows — the groups the skin
    /// draws no tab strip for (`skin.rs`).
    barless: Rc<std::cell::RefCell<std::collections::HashSet<&'static str>>>,
    /// Rendered along the bottom edge, under everything.
    status: Vec<StatusCell>,
    /// Every command, in registration order (`crate::commands`).
    commands: CommandRegistry,
    /// The palette or the menu while open, with what had focus before it —
    /// restored before the chosen command runs, so it runs where the
    /// author was.
    overlay: Option<(Entity<Palette>, Option<FocusHandle>, Subscription)>,
    /// The Settings window while open, with the focus to restore.
    settings: Option<(Entity<SettingsModal>, Option<FocusHandle>, Subscription)>,
    /// The registered settings sections (`crate::settings_modal`).
    sections: Vec<Section>,
    /// The width tier the window was last laid out at
    /// (`docs/studio-shell-spec.md` §5.3), and the docks that were open
    /// before it narrowed — so widening puts back what the author had,
    /// not a guess at it.
    tier: Tier,
    pre_narrow: Option<Vec<(&'static str, bool)>>,
    /// Whether the notification history popover is open (§7.5's bell).
    notices_open: bool,
    /// Which docks were open before the editor was maximized, so
    /// un-maximizing puts back what was there and not a guess at it.
    /// `None` when not maximized.
    unmaximized: Option<Vec<(&'static str, bool)>>,
    /// The view the AUTHOR chose, which is not always the one on screen.
    ///
    /// The Player, the Story Graph and Compiled Output are Code-view tabs,
    /// so asking for any of them takes the manuscript's place. That switch
    /// is the studio's doing, not a preference, and persisting it meant
    /// pressing `cmd-r` once in Continuous and being in Code the next
    /// morning. What is remembered is this; what is drawn is the root's.
    chosen_view: EditorView,
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
        let mut barless = None;
        let dock_area = cx.new(|cx| {
            let inner = DockSkin::new(cx);
            skin = Some(inner.clone());
            DockArea::new("brink-studio", Some(1), window, cx).with_renderer(Rc::new({
                let studio = StudioSkin::new(inner);
                barless = Some(studio.barless());
                studio
            }))
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
            barless: barless.unwrap_or_default(),
            editor_root,
            tools: Vec::new(),
            status: Vec::new(),
            commands: CommandRegistry::default(),
            overlay: None,
            settings: None,
            sections: Vec::new(),
            tier: Tier::Wide,
            pre_narrow: None,
            notices_open: false,
            unmaximized: None,
            chosen_view: EditorView::Code,
            focus: cx.focus_handle(),
        };
        // A default keystroke an override took away is bound to `Unbound`
        // (`crate::commands`); swallowed here so it falls through to
        // nothing rather than to the default it shadows.
        App::on_action(cx, |_: &Unbound, _| {});
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
        this.register_command("App", "Settings\u{2026}", OpenSettings, Some("cmd-,"), cx);
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
        // The App sections the shell owns. A Project section is the feature
        // crate's to add (`add_settings_section`).
        let me = cx.entity().downgrade();
        let appearance = cx.new(AppearanceSection::new);
        this.add_settings_section(Section::new(
            SectionMeta::new(
                "appearance",
                Scope::App,
                "Appearance",
                &[
                    "theme", "colour", "color", "font", "size", "gutter", "inlay", "format", "save",
                ],
            ),
            appearance,
        ));
        let editor = cx.new(EditorSection::new);
        this.add_settings_section(Section::new(
            SectionMeta::new(
                "editor",
                Scope::App,
                "Editor",
                &["view", "open", "default", "fix", "save"],
            ),
            editor,
        ));
        let player = cx.new(PlayerSection::new);
        this.add_settings_section(Section::new(
            SectionMeta::new(
                "player",
                Scope::App,
                "Player",
                &["play", "player", "follow", "transcript", "font", "size"],
            ),
            player,
        ));
        let keymap = cx.new(|cx| KeymapSection::new(me, window, cx));
        this.add_settings_section(Section::new(
            SectionMeta::new(
                "keymap",
                Scope::App,
                "Keymap",
                &["keys", "shortcut", "binding", "rebind"],
            ),
            keymap,
        ));
        this
    }

    /// Register a settings section; the window lists it under its scope.
    pub fn add_settings_section(&mut self, section: Section) {
        self.sections.push(section);
    }

    /// Open the Settings window on `section` (an id), or its first section.
    pub fn open_settings(
        &mut self,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.is_some() {
            return;
        }
        self.close_overlay(window, cx);
        let previous = window.focused(cx);
        let sections = self.sections.clone();
        let modal = cx.new(|cx| SettingsModal::new(sections, section, window, cx));
        let subscription = cx.subscribe_in(
            &modal,
            window,
            |this, _, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Close => this.close_settings(window, cx),
            },
        );
        modal.update(cx, |modal, cx| modal.focus(window, cx));
        self.settings = Some((modal, previous, subscription));
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, previous, _)) = self.settings.take() {
            if let Some(handle) = previous {
                window.focus(&handle, cx);
            }
            cx.notify();
        }
    }

    /// Install every binding the overrides call for (`keymap_bindings`),
    /// after a change to them. gpui's keymap only grows, and later
    /// bindings win, which is what makes re-installing the whole set the
    /// right move.
    fn apply_keymap(&self, cx: &mut Context<Self>) {
        let overrides = AppSettings::get(cx).keymap;
        cx.bind_keys(keymap_bindings(self.commands.commands(), &overrides));
    }

    /// Give a chord to the command at `index`, displacing whoever held
    /// it; returns the displaced command's title. Persists and rebinds.
    pub fn rebind(&mut self, index: usize, chord: &str, cx: &mut Context<Self>) -> Option<String> {
        let commands = self.commands.commands().to_vec();
        let mut displaced = None;
        settings::update(cx, |s| {
            displaced = bind_chord(&commands, &mut s.keymap, index, chord);
        });
        self.apply_keymap(cx);
        cx.notify();
        displaced
    }

    pub fn unbind_command(&mut self, index: usize, cx: &mut Context<Self>) {
        let commands = self.commands.commands().to_vec();
        settings::update(cx, |s| unbind(&commands, &mut s.keymap, index));
        self.apply_keymap(cx);
        cx.notify();
    }

    pub fn reset_command(&mut self, index: usize, cx: &mut Context<Self>) {
        let commands = self.commands.commands().to_vec();
        settings::update(cx, |s| reset(&commands, &mut s.keymap, index));
        self.apply_keymap(cx);
        cx.notify();
    }

    /// Register a command and install its default binding. Studio §6: a
    /// button, a key and a palette entry are one action, never three
    /// functions.
    pub fn register_command(
        &mut self,
        group: impl Into<SharedString>,
        title: impl Into<SharedString>,
        action: impl Action + Clone,
        keystroke: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        self.register_command_in(group, title, action, keystroke, None, cx);
    }

    /// The same, bound only inside `context` — see
    /// [`CommandRegistry::register_in`]. The command is in the palette
    /// like any other; only its KEY is scoped.
    pub fn register_command_in(
        &mut self,
        group: impl Into<SharedString>,
        title: impl Into<SharedString>,
        action: impl Action + Clone,
        keystroke: Option<&str>,
        context: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        let ix = self
            .commands
            .register_in(group, title, action, keystroke, context);
        // Bound through the overrides, so a persisted rebinding holds from
        // the first frame.
        let overrides = AppSettings::get(cx).keymap;
        let bindings = keymap_bindings(&self.commands.commands()[ix..=ix], &overrides);
        cx.bind_keys(bindings);
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

        // A side dock's tool windows are switched by the rail; their group
        // draws no tab strip (`skin.rs`).
        if spec.slot.group == crate::region::RailGroup::Upper {
            self.barless
                .borrow_mut()
                .insert(panel.read(cx).panel_name());
        }

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
        self.chosen_view = view;
        let focus = self.editor_root.update(cx, |root, cx| {
            root.set_view(view, cx);
            root.occupant_focus()
        });
        window.focus(&focus.unwrap_or_else(|| self.focus.clone()), cx);
        self.persist_layout(cx);
        cx.notify();
    }

    /// Switch views because a SURFACE needs one, not because the author
    /// asked — the Player, the Story Graph and Compiled Output are all
    /// Code-view tabs, so showing one gives the manuscript's place away.
    ///
    /// Deliberately not [`Workspace::set_editor_view`]: it leaves
    /// `chosen_view` alone, so the author still reopens tomorrow in the
    /// view they picked. It also leaves focus alone, because the caller is
    /// about to put focus in the surface it opened this for.
    pub fn require_editor_view(&mut self, view: EditorView, cx: &mut Context<Self>) {
        if self.editor_root.read(cx).view() == view {
            return;
        }
        self.editor_root
            .update(cx, |root, cx| root.set_view(view, cx));
        cx.notify();
    }

    #[must_use]
    pub fn editor_view(&self, cx: &App) -> EditorView {
        self.editor_root.read(cx).view()
    }

    /// The window's current shape, for the settings.
    ///
    /// Only the three docks and the editor view: the panel TREE is not
    /// persisted (see `settings::Layout`), so nothing here has to survive
    /// a panel that no longer exists.
    #[must_use]
    pub fn layout(&self, cx: &App) -> crate::settings::Layout {
        let area = self.dock_area.read(cx);
        let docks = DOCKS
            .iter()
            .map(|(name, placement)| {
                (
                    (*name).to_owned(),
                    crate::settings::DockShape {
                        open: area.is_dock_open(*placement),
                        size: area.dock_size(*placement).map(f32::from),
                    },
                )
            })
            .collect();
        // The scroll and open-document halves belong to whoever owns the
        // documents, not to the shell — so they are carried through from
        // what is already saved rather than blanked.
        // `Workspace::save_layout` is the app's door for replacing them.
        let saved = crate::settings::AppSettings::get(cx).layout;
        crate::settings::Layout {
            docks,
            // The chosen view, NOT the one on screen — see `chosen_view`.
            editor_view: Some(self.chosen_view.persistence_key().to_owned()),
            scroll_root: saved.scroll_root,
            scroll: saved.scroll,
            open_files: saved.open_files,
            active_file: saved.active_file,
        }
    }

    /// Put a persisted shape back. Called once, after the tool windows are
    /// registered — their `ToolWindowSpec::open()` defaults decide the
    /// first run, and this overrides them when there is something saved.
    pub fn apply_layout(
        &mut self,
        layout: &crate::settings::Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (name, placement) in DOCKS {
            let Some(shape) = layout.docks.get(*name) else {
                continue;
            };
            if let Some(size) = shape.size {
                self.dock_area.update(cx, |area, cx| {
                    area.set_dock_size(*placement, px(size), window, cx)
                });
            }
            if self.dock_area.read(cx).is_dock_open(*placement) != shape.open {
                self.dock_area
                    .update(cx, |area, cx| area.toggle_dock(*placement, window, cx));
            }
        }
        // A chosen default view wins over the remembered one: "always
        // open in Continuous" is a preference about every launch, and the
        // last view used is only the memory it replaces.
        let settings = AppSettings::get(cx);
        let key = settings
            .default_view
            .as_ref()
            .or(layout.editor_view.as_ref());
        if let Some(key) = key
            && let Some(view) = EditorView::ALL.iter().find(|v| v.persistence_key() == key)
        {
            self.set_editor_view(*view, window, cx);
        }
        cx.notify();
    }

    /// Write the current shape into the settings. Cheap and idempotent —
    /// `settings::update` compares before writing — so a caller may say
    /// this whenever the layout might have moved.
    pub fn save_layout(
        this: &Entity<Self>,
        documents: Option<crate::settings::Documents>,
        cx: &mut App,
    ) {
        let mut layout = this.read(cx).layout(cx);
        if let Some(documents) = documents {
            layout.scroll_root = Some(documents.root);
            layout.scroll = documents.scroll;
            layout.open_files = documents.open;
            layout.active_file = documents.active;
        }
        crate::settings::update(cx, |settings| settings.layout = layout);
    }

    /// The same, from inside a method. Called after every discrete change
    /// a person makes — a dock toggled, a view switched — so the shape
    /// survives a kill as well as a clean quit; `on_app_quit` alone would
    /// lose it to a crash, and SIGTERM does not run it either.
    fn persist_layout(&self, cx: &mut Context<Self>) {
        let layout = self.layout(cx);
        crate::settings::update(cx, |settings| settings.layout = layout);
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
        self.persist_layout(cx);
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
        self.persist_layout(cx);
        cx.notify();
    }

    /// Replace the status-bar cells, left to right.
    /// Re-lay the window for its current width (§5.3). Closing is
    /// automatic; REOPENING only ever puts back what was open before the
    /// window narrowed, so a dock the author closed themselves stays
    /// closed.
    fn apply_tier(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) {
        let tier = Tier::of(width);
        if tier == self.tier {
            return;
        }
        let widening = self.tier != Tier::Wide && tier == Tier::Wide;
        // Maximized is the author's own "no docks": leave it alone.
        if self.unmaximized.is_some() {
            self.tier = tier;
            return;
        }
        if self.pre_narrow.is_none() && tier != Tier::Wide {
            self.pre_narrow = Some(
                DOCKS
                    .iter()
                    .map(|(name, placement)| {
                        (*name, self.dock_area.read(cx).is_dock_open(*placement))
                    })
                    .collect(),
            );
        }
        for (name, placement) in DOCKS {
            let open = self.dock_area.read(cx).is_dock_open(*placement);
            let want = if widening {
                self.pre_narrow
                    .as_ref()
                    .and_then(|before| before.iter().find(|(n, _)| n == name))
                    .is_some_and(|(_, open)| *open)
            } else {
                open && tier.allows(name)
            };
            if open != want {
                self.dock_area
                    .update(cx, |area, cx| area.toggle_dock(*placement, window, cx));
            }
        }
        if widening {
            self.pre_narrow = None;
        }
        self.tier = tier;
        cx.notify();
    }

    /// Whether the editor is maximized — every dock hidden.
    #[must_use]
    pub fn is_maximized(&self) -> bool {
        self.unmaximized.is_some()
    }

    /// Give the editor the whole window, and give it back
    /// (`docs/studio-shell-spec.md` §5.4).
    ///
    /// Restoring puts back exactly the docks that were open, rather than
    /// opening all three: a writer who works with the Binder closed does
    /// not want it back for having read one scene full-width.
    pub fn toggle_maximize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.unmaximized.take() {
            Some(before) => {
                for (name, open) in before {
                    let Some((_, placement)) = DOCKS.iter().find(|(n, _)| *n == name) else {
                        continue;
                    };
                    if self.dock_area.read(cx).is_dock_open(*placement) != open {
                        self.dock_area
                            .update(cx, |area, cx| area.toggle_dock(*placement, window, cx));
                    }
                }
            }
            None => {
                let before: Vec<(&'static str, bool)> = DOCKS
                    .iter()
                    .map(|(name, placement)| {
                        (*name, self.dock_area.read(cx).is_dock_open(*placement))
                    })
                    .collect();
                // Nothing open is already maximized; toggling then would
                // record "all closed" and lose the way back.
                if before.iter().all(|(_, open)| !open) {
                    return;
                }
                for (_, placement) in DOCKS {
                    if self.dock_area.read(cx).is_dock_open(*placement) {
                        self.dock_area
                            .update(cx, |area, cx| area.toggle_dock(*placement, window, cx));
                    }
                }
                self.unmaximized = Some(before);
            }
        }
        cx.notify();
    }

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

    /// The Settings window: a scrim over the whole window, the modal
    /// centred on it.
    fn render_settings(&self, window: &Window, cx: &App) -> Option<AnyElement> {
        let (modal, _, _) = self.settings.as_ref()?;
        let viewport = window.viewport_size();
        let width = px(MODAL_WIDTH).min(viewport.width - px(64.));
        let height = px(MODAL_HEIGHT).min(viewport.height - px(80.));
        let position = point(
            (viewport.width - width) / 2.,
            (viewport.height - height) / 2.,
        );
        let scrim = cx.theme().background.opacity(0.55);
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    // Occluded: a click on the modal must not also reach
                    // what it covers (it did — a click on the scope switch
                    // was also a click on the Binder row beneath it).
                    div()
                        .occlude()
                        .w(viewport.width)
                        .h(viewport.height)
                        .bg(scrim)
                        .child(
                            anchored()
                                .position(position)
                                .snap_to_window_with_margin(px(16.))
                                .child(modal.clone()),
                        ),
                ),
            )
            .into_any_element(),
        )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        // Copied out before the cells are built: `render` takes `cx`
        // mutably, so a live `cx.theme()` borrow would outlive it.
        let (sidebar, border, muted) = {
            let theme = cx.theme();
            (theme.sidebar, theme.border, theme.muted_foreground)
        };
        let (hover, fg) = {
            let theme = cx.theme();
            (theme.muted.opacity(0.6), theme.foreground)
        };
        let render = |ix: usize, cell: &StatusCell, cx: &mut Context<Self>| -> AnyElement {
            match &cell.opens {
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
            }
        };
        let mut start: Vec<AnyElement> = Vec::new();
        let mut end: Vec<AnyElement> = Vec::new();
        for (ix, cell) in self.status.iter().enumerate() {
            let element = render(ix, cell, cx);
            if cell.align_end {
                end.push(element);
            } else {
                start.push(element);
            }
        }
        h_flex()
            .h(px(24.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(sidebar)
            .border_t_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .children(start)
            // The two groups, held apart: what the project is doing stays
            // at the start, what the caret is doing sits at the far end
            // (§7.3) rather than drifting with the left group's width.
            .child(div().flex_1())
            .child(h_flex().gap_4().items_center().children(end))
            .child(self.render_bell(cx))
            .into_any_element()
    }

    /// The notification bell — §7.5's history, at the far end of the
    /// right group. A toast is gone in seconds; this is what lets an
    /// author come back and ask what the red thing said.
    fn render_bell(&self, cx: &mut Context<Self>) -> AnyElement {
        let unread = crate::notify::Notifications::unread(cx);
        let (accent, muted) = (cx.theme().primary, cx.theme().muted_foreground);
        h_flex()
            .id("status-bell")
            .gap_1()
            .px_1()
            .rounded_sm()
            .cursor_pointer()
            .hover(|s| s.bg(cx.theme().muted.opacity(0.6)))
            .child(
                div()
                    .text_color(if unread > 0 { accent } else { muted })
                    .child("\u{1F514}"),
            )
            .when(unread > 0, |el| {
                el.child(div().text_color(accent).child(format!("{unread}")))
            })
            .on_click(cx.listener(|this, _, _window, cx| {
                this.notices_open = !this.notices_open;
                if this.notices_open {
                    // Opening IS reading: the badge is about what arrived
                    // while you were not looking.
                    crate::notify::Notifications::mark_read(cx);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// The history popover: newest first, capped, with what it dropped.
    fn render_notices(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.notices_open {
            return None;
        }
        let notices = crate::notify::Notifications::get(cx);
        let dropped = crate::notify::Notifications::dropped(cx);
        let theme = cx.theme();
        let (muted, border, popover) = (theme.muted_foreground, theme.border, theme.popover);
        let colour = |severity: crate::notify::Severity| match severity {
            crate::notify::Severity::Error => theme.danger,
            crate::notify::Severity::Warning => theme.warning,
            crate::notify::Severity::Success => theme.primary,
            crate::notify::Severity::Info => theme.muted_foreground,
        };
        let rows: Vec<AnyElement> = notices
            .iter()
            .rev()
            .map(|notice| {
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .py_0p5()
                    .child(
                        div()
                            .w(px(56.))
                            .flex_none()
                            .text_color(muted)
                            .child(notice.at.clone()),
                    )
                    .child(
                        div()
                            .w(px(52.))
                            .flex_none()
                            .text_color(colour(notice.severity))
                            .child(notice.severity.label()),
                    )
                    .child(div().flex_1().child(notice.message.clone()))
                    .child(div().text_color(muted).child(notice.source.clone()))
                    .into_any_element()
            })
            .collect();
        Some(
            v_flex()
                .absolute()
                .right(px(8.))
                .bottom(px(28.))
                .w(px(480.))
                .max_h(px(320.))
                .p_2()
                .gap_1()
                .rounded_md()
                .bg(popover)
                .border_1()
                .border_color(border)
                .text_xs()
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(div().flex_1().text_color(muted).child({
                            let n = rows.len();
                            let plural = if n == 1 { "notice" } else { "notices" };
                            if dropped > 0 {
                                format!("{n} {plural} · {dropped} older dropped")
                            } else {
                                format!("{n} {plural}")
                            }
                        }))
                        .child(
                            Button::new("notices-clear")
                                .ghost()
                                .compact()
                                .label("Clear")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    crate::notify::Notifications::clear(cx);
                                    this.notices_open = false;
                                    cx.notify();
                                })),
                        ),
                )
                .when(rows.is_empty(), |el| {
                    el.child(
                        div()
                            .p_2()
                            .text_color(muted)
                            .child("Nothing has been reported."),
                    )
                })
                .child(
                    v_flex()
                        .id("notices-list")
                        .overflow_y_scroll()
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// The view switcher: three toggles, in the title bar. The studio has no
    /// dedicated widget for this (its views are palette commands); the native
    /// app gives them a permanent home, since which view you are in changes
    /// what the whole centre means.
    fn view_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.editor_view(cx);
        // Hand-built rather than `ButtonGroup`, for two reasons found on
        // screen. Its `outline` variant paints every segment in the accent
        // foreground and puts `selected` in the BORDER, so with icons and
        // no labels all three read as active — the switcher had no visible
        // state at all. And at the kit's own button metrics the control
        // stood half again as tall as the 30px chrome it sits in.
        //
        // These are the Binder's tool metrics instead — 22px cells, a 14px
        // glyph, accent fill and `primary` for the one that is on — which
        // is the idiom already proven legible in this app, and small enough
        // to belong in a title bar.
        let (border, accent, muted, on_colour, off_colour) = {
            let theme = cx.theme();
            (
                theme.border,
                theme.accent,
                theme.muted,
                theme.primary,
                theme.muted_foreground,
            )
        };
        h_flex()
            .rounded_sm()
            .border_1()
            .border_color(border)
            .overflow_hidden()
            .children(EditorView::ALL.iter().enumerate().map(|(ix, &view)| {
                let on = view == current;
                // The label is a glyph now, so the ruled NAME and the
                // keystroke live here — the vocabulary still has a home.
                let hint = SharedString::from(format!("{} ({})", view.title(), view.keystroke()));
                div()
                    .id(SharedString::from(format!(
                        "view-{}",
                        view.persistence_key()
                    )))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.))
                    // Hairlines BETWEEN the segments, not around each: one
                    // control with three cells, rather than three buttons
                    // that happen to touch.
                    .when(ix > 0, |el| el.border_l_1().border_color(border))
                    .when(on, |el| el.bg(accent))
                    .when(!on, |el| el.hover(|s| s.bg(muted.opacity(0.6))))
                    .cursor_pointer()
                    .child(view.icon().with_size(px(14.)).text_color(if on {
                        on_colour
                    } else {
                        off_colour
                    }))
                    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.set_editor_view(view, window, cx);
                    }))
            }))
            .into_any_element()
    }
}

/// The three docks, by the name their shape is persisted under.
const DOCKS: &[(&str, DockPlacement)] = &[
    ("left", DockPlacement::Left),
    ("right", DockPlacement::Right),
    ("bottom", DockPlacement::Bottom),
];

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
        // §5.3: the window is laid out for the room it has. Checked here
        // rather than on a resize event, which gpui does not raise for a
        // view — a render IS the resize notification.
        let width = f32::from(window.viewport_size().width);
        self.apply_tier(width, window, cx);
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
        let notices = self.render_notices(cx);
        let overlay = self.render_overlay(window, cx);
        let settings_window = self.render_settings(window, cx);
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
            // The notifications popover places itself against this box's
            // bottom-right; without `relative` it would resolve against
            // the window and land wherever.
            .relative()
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
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_settings(None, window, cx);
            }))
            .child(
                TitleBar::new().child(
                    h_flex()
                        .flex_1()
                        .items_center()
                        .justify_between()
                        // `justify_between` pins the switcher to the content
                        // edge, which on macOS is also the window's rounded
                        // corner — so it read as cramped against the frame
                        // rather than merely tight. Enough to clear the
                        // radius, not enough to look detached from it.
                        .pr_3()
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
            // Above the status bar, as §7.5 places it, and after the docks
            // so it paints over them.
            .children(notices)
            .children(overlay)
            .children(settings_window)
    }
}

#[cfg(test)]
mod tests {
    use super::Tier;

    #[test]
    fn the_tier_follows_the_width_and_says_what_fits() {
        assert_eq!(Tier::of(1440.), Tier::Wide);
        assert_eq!(Tier::of(1120.), Tier::Wide, "the boundary is inclusive");
        assert_eq!(Tier::of(1000.), Tier::Medium);
        assert_eq!(Tier::of(860.), Tier::Medium);
        assert_eq!(Tier::of(700.), Tier::Narrow);

        // Wide affords everything; medium gives up the right dock, which
        // holds inspectors rather than the file you are working in;
        // narrow keeps only the bottom strip.
        for dock in ["left", "right", "bottom"] {
            assert!(Tier::Wide.allows(dock));
        }
        assert!(Tier::Medium.allows("left"));
        assert!(Tier::Medium.allows("bottom"));
        assert!(!Tier::Medium.allows("right"));
        assert!(!Tier::Narrow.allows("left"));
        assert!(!Tier::Narrow.allows("right"));
        assert!(Tier::Narrow.allows("bottom"), "a strip is not a column");
    }

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
