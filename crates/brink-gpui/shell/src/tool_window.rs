//! What a feature registers with the shell.
//!
//! The shell never learns what a Binder is. Zed's `workspace` depends on
//! `project` and not on `editor`; features implement the shell's traits and
//! the concrete wiring happens once, at the top. This is the brink
//! equivalent: a feature hands over a [`gpui_component::dock::Panel`] and a
//! [`ToolWindowSpec`] saying where it lives, and that is the whole contract.

use gpui::{App, Pixels, SharedString};
use gpui_component::dock::Panel;

use crate::region::RailSlot;

/// What a tool window is, over and above a dock panel: the rail button's
/// badge (`docs/studio-shell-spec.md` §5.1 — "icons show badges where
/// meaningful (Problems: error count)").
///
/// A trait rather than a field on [`ToolWindowSpec`] because the badge is
/// live state the panel owns; the shell reads it each frame and never
/// learns what it counts.
pub trait ToolWindow: Panel {
    /// Text for the rail button's badge, or `None` for no badge.
    fn badge(&self, _cx: &App) -> Option<SharedString> {
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
    pub icon: Option<&'static str>,
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
    pub fn icon(mut self, svg: &'static str) -> Self {
        self.icon = Some(svg);
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
