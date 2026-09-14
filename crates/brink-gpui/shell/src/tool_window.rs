//! What a feature registers with the shell.
//!
//! The shell never learns what a Binder is. Zed's `workspace` depends on
//! `project` and not on `editor`; features implement the shell's traits and
//! the concrete wiring happens once, at the top. This is the brink
//! equivalent: a feature hands over a [`gpui_component::dock::Panel`] and a
//! [`ToolWindowSpec`] saying where it lives, and that is the whole contract.

use gpui::{App, Pixels, SharedString, WeakEntity, Window};
use gpui_component::dock::{Panel, PanelId, TabGroup};

use crate::region::RailSlot;

/// The height of a tool window's own header row.
///
/// It is the kit's TAB-STRIP height (`dock/tab_panel.rs`, `px(30.)`), not a
/// number of our own. The side docks draw no tab strip (`skin.rs`, ruled
/// 2026-09-05), so a panel's header is the thing that has to line up with
/// the centre's tabs straight across the window — and four panels had each
/// picked their own value (32, and three different content-plus-`py_1`
/// heights), which is what made the seam visible.
pub const HEADER_HEIGHT: f32 = 30.;

/// A centre tab's title for a panel that is NOT a file.
///
/// A document tab is named by its filename and wants no glyph. The Player,
/// the Story Graph and Compiled Output share the centre with those
/// documents, and the icon is what tells them apart at a glance: this tab
/// is a SURFACE, not something you opened off the disk. Their glyphs come
/// from the kit's lucide set, per the icon ruling — none of the three is a
/// brink concept.
pub fn tab_title(
    icon: impl Into<gpui_component::Icon>,
    label: impl Into<gpui::SharedString>,
) -> impl gpui::IntoElement {
    use gpui::{ParentElement as _, Styled as _};
    use gpui_component::Sizable as _;
    gpui_component::h_flex()
        .gap_1p5()
        .items_center()
        .child(icon.into().small())
        .child(label.into())
}

/// The key context every tool window's root carries
/// (`div().key_context(TOOL_WINDOW_CONTEXT)`).
///
/// It exists so a key that already means something everywhere else can
/// mean one more thing HERE without being taken away: `escape` dismisses
/// the palette, closes the find panel and cancels the code-action menu,
/// and each of those has its own context deeper in the tree, so each
/// still wins where it applies. Bound at this level it is "leave the tool
/// window", which is what `escape` means when nothing else has claimed it.
pub const TOOL_WINDOW_CONTEXT: &str = "ToolWindow";

/// Where a tool window sits in its dock: the tab group the dock placed it
/// in. A panel records it from `on_added_to` and clears it in `on_removed`;
/// the shell reads it to select the tab, since the toolkit exposes no way
/// to activate one panel in a group from outside the group (HANDOFF.md
/// "Known broken" #3, fixed by this).
#[derive(Default)]
pub struct TabSlot {
    group: Option<WeakEntity<TabGroup>>,
}

impl TabSlot {
    pub fn added_to(&mut self, group: WeakEntity<TabGroup>) {
        self.group = Some(group);
    }

    pub fn removed(&mut self) {
        self.group = None;
    }

    /// Whether `me` is the tab its group shows.
    #[must_use]
    pub fn is_active(&self, me: PanelId, cx: &App) -> bool {
        self.group
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .is_some_and(|group| {
                group
                    .read(cx)
                    .active_panel(cx)
                    .is_some_and(|panel| panel.panel_id(cx) == me)
            })
    }

    /// The group, for a caller that needs `&mut App` to select in it.
    #[must_use]
    pub fn group(&self) -> Option<WeakEntity<TabGroup>> {
        self.group.clone()
    }
}

/// Make `me` the displayed tab of `group`.
pub fn select_tab(group: &WeakEntity<TabGroup>, me: PanelId, window: &mut Window, cx: &mut App) {
    _ = group.update(cx, |group, cx| {
        let ix = group
            .panels()
            .iter()
            .position(|panel| panel.panel_id(cx) == me);
        if let Some(ix) = ix {
            group.select_tab(ix, window, cx);
        }
    });
}

/// What a badge means, which is what colours it: an error count is the
/// danger colour; an advisory count (TODO notes) the theme's TODO amber —
/// the studio's `.shell-strip-badge.is-todo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Danger,
    Advisory,
}

/// A count bubble on a rail button's corner (`docs/studio-shell-spec.md`
/// §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Badge {
    pub text: SharedString,
    pub tone: BadgeTone,
}

impl Badge {
    /// A count, hidden at zero and capped at "99+".
    #[must_use]
    pub fn count(n: usize, tone: BadgeTone) -> Option<Self> {
        match n {
            0 => None,
            n if n > 99 => Some(Self {
                text: "99+".into(),
                tone,
            }),
            n => Some(Self {
                text: n.to_string().into(),
                tone,
            }),
        }
    }
}

/// What a tool window is, over and above a dock panel: the rail button's
/// badge (`docs/studio-shell-spec.md` §5.1 — "icons show badges where
/// meaningful (Problems: error count)").
///
/// A trait rather than a field on [`ToolWindowSpec`] because the badge is
/// live state the panel owns; the shell reads it each frame and never
/// learns what it counts.
pub trait ToolWindow: Panel {
    /// The rail button's badge, or `None` for no badge.
    fn badge(&self, _cx: &App) -> Option<Badge> {
        None
    }

    /// The tab the dock placed this window in, so the rail can select it
    /// when a dock holds more than one. A window that does not track it is
    /// toggled dock-wide, as every window was before.
    fn tab_slot(&self) -> Option<&TabSlot> {
        None
    }
}

/// A tool window's registration.
#[derive(Debug, Clone)]
pub struct ToolWindowSpec {
    /// Stable identity, for persistence and for commands to name it.
    pub id: SharedString,
    /// Shown in the tab and as the rail button's tooltip.
    pub title: SharedString,
    /// A complete SVG document, painted as a monochrome mask tinted by the
    /// button's text colour. `None` falls back to the title's first letter.
    pub icon: Option<crate::icons::BrinkIcon>,
    /// The one place this tool window's home is declared.
    pub slot: RailSlot,
    /// Dock size on first open; `None` takes the dock's own default.
    pub default_size: Option<Pixels>,
    /// Whether its dock starts open.
    pub open_by_default: bool,
}

impl ToolWindowSpec {
    #[must_use]
    pub fn new(
        id: impl Into<SharedString>,
        title: impl Into<SharedString>,
        slot: RailSlot,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
            slot,
            default_size: None,
            open_by_default: false,
        }
    }

    #[must_use]
    pub fn icon(mut self, icon: crate::icons::BrinkIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    #[must_use]
    pub fn size(mut self, size: Pixels) -> Self {
        self.default_size = Some(size);
        self
    }

    #[must_use]
    pub fn open(mut self) -> Self {
        self.open_by_default = true;
        self
    }
}
