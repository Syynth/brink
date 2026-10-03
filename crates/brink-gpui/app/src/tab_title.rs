//! A centre tab's title, with the close affordance the tab strip lacks.
//!
//! The kit's strip draws no ✕, and its only Close sits in the `…` menu,
//! refused outright for the last tab of a lone group (base's rule for a
//! dock that must not be emptied — wrong for an editor, where no file open
//! is a perfectly good state). So the close lives in the one part of a tab
//! a panel draws itself, its title: a ✕ revealed on hover, and a middle
//! click anywhere on the title.
//!
//! Neither closes anything here. Both dispatch [`CloseTabById`], which the
//! studio handles beside `cmd-w`'s `CloseTab` — one path for every way a
//! tab is closed, so the unsaved-edits prompt and `CodeView`'s bookkeeping
//! cannot be skipped by whichever way the author happened to use.

use gpui::prelude::*;
use gpui::{App, EntityId, IntoElement, MouseButton, SharedString, div, px};
use gpui_component::{ActiveTheme as _, h_flex};

/// Close the centre tab whose panel is entity `id` — a document, or one of
/// the Player, Compiled Output and the Story Graph. Not a palette command:
/// it names a tab, and only a tab's own title knows which.
#[derive(Clone, Copy, PartialEq, Eq, Debug, gpui::Action)]
#[action(namespace = brink, no_json)]
pub struct CloseTabById {
    pub id: EntityId,
}

/// The `group` the ✕ reveals itself on: hovering the title, not the ✕.
const GROUP: &str = "brink-tab-title";

/// `name`, then the ✕. `dirty` marks the name the way the tab always has
/// (`name •`); the ✕ stays a ✕, so what it does never depends on state.
pub fn tab_title(
    id: EntityId,
    name: impl Into<SharedString>,
    dirty: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, fg, hover) = (
        theme.muted_foreground,
        theme.foreground,
        theme.secondary_hover,
    );
    let name: SharedString = name.into();
    let label = if dirty {
        SharedString::from(format!("{name} \u{2022}"))
    } else {
        name
    };
    h_flex()
        .id(("tab-title", id))
        .group(GROUP)
        .gap_1()
        .items_center()
        // Middle-click closes, as in every tabbed editor. On release, so a
        // press that turns into something else closes nothing.
        .on_mouse_up(MouseButton::Middle, move |_, window, cx| {
            cx.stop_propagation();
            window.dispatch_action(Box::new(CloseTabById { id }), cx);
        })
        .child(label)
        .child(
            div()
                .id(("tab-close", id))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .text_xs()
                .text_color(muted)
                // Hidden rather than absent: the tab keeps its width, so
                // the strip does not shuffle under the pointer.
                .invisible()
                .group_hover(GROUP, |style| style.visible())
                .hover(|style| style.bg(hover).text_color(fg))
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    // Before the tab's own click, which would select the
                    // tab first.
                    cx.stop_propagation();
                    window.dispatch_action(Box::new(CloseTabById { id }), cx);
                })
                .child("\u{2715}"),
        )
}
