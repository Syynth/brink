//! The rails — `docs/gpui-studio-spec.md` §4.1.
//!
//! A narrow strip down each side holding one button per tool window, split
//! into an upper group (which opens into that side's own dock) and a lower
//! group (which opens into the bottom dock). The lower group is what
//! replaces the bottom rail, so it is drawn pinned to the bottom of the
//! strip rather than following the upper group.
//!
//! The toolkit has no rail of its own; this is the brink-specific part of
//! the region model.

use gpui::prelude::*;
use gpui::{AnyElement, App, Hsla, Pixels, SharedString, Window, div, px, svg};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants as _},
    v_flex,
};

use crate::commands::display_keystroke;
use crate::region::{RailEdge, RailGroup, RailSlot};
use crate::tool_window::{Badge, BadgeTone};

/// Rail width. Narrow enough to read as chrome, wide enough for a 16px icon
/// with breathing room.
pub const RAIL_WIDTH: Pixels = px(36.);
const ICON_SIZE: Pixels = px(16.);

/// One button's worth of state, as the workspace hands it over.
pub struct RailButton {
    pub id: SharedString,
    pub title: SharedString,
    pub icon: Option<&'static str>,
    pub slot: RailSlot,
    /// Whether this tool window's dock is currently open.
    pub active: bool,
    /// A count bubble on the button's corner — `None` for none (§5.1).
    pub badge: Option<Badge>,
    /// The toggle's keystroke, shown in the tooltip (studio §5.2).
    pub keystroke: Option<SharedString>,
}

/// Render one rail. `on_click` receives the id of the button pressed;
/// `leading` sits above the upper group — the hamburger, on the left rail
/// (studio §6: "a single icon at the top of the left strip").
pub fn rail<F>(
    edge: RailEdge,
    buttons: &[RailButton],
    leading: Option<AnyElement>,
    on_click: F,
    _window: &mut Window,
    cx: &mut App,
) -> impl IntoElement
where
    F: Fn(&SharedString, &mut Window, &mut App) + Clone + 'static,
{
    let theme = cx.theme();
    let studio = crate::theme::current(cx).tokens;
    let badge_colours = BadgeColours {
        danger: (theme.danger, theme.danger_foreground),
        advisory: (
            crate::theme::hsla(studio.todo),
            crate::theme::hsla(studio.editor_bg),
        ),
    };
    let group = |g: RailGroup| {
        v_flex().gap_1().children(
            buttons
                .iter()
                .filter(|b| b.slot.edge == edge && b.slot.group == g)
                .map(|b| {
                    button(
                        b,
                        theme.foreground,
                        theme.muted_foreground,
                        badge_colours,
                        on_click.clone(),
                    )
                }),
        )
    };

    // Both rails are always drawn, occupied or not (maintainer, 2026-09-05):
    // the rail is the region model made visible, and a right dock with no
    // rail beside it reads as a dock that cannot be reached.
    v_flex()
        .w(RAIL_WIDTH)
        .h_full()
        .py_1()
        .gap_1()
        .items_center()
        .justify_between()
        .bg(theme.sidebar)
        .when(edge == RailEdge::Left, |s| {
            s.border_r_1().border_color(theme.border)
        })
        .when(edge == RailEdge::Right, |s| {
            s.border_l_1().border_color(theme.border)
        })
        // Upper group flows from the top; the lower group is pinned to the
        // bottom by `justify_between`, which is the whole visual point of
        // dropping the bottom rail.
        .child(
            v_flex()
                .gap_1()
                .items_center()
                .children(leading)
                .child(group(RailGroup::Upper)),
        )
        .child(group(RailGroup::Lower))
}

/// One rail button. A `Button` rather than a bare `div` so it inherits the
/// toolkit's hover, focus, tooltip and toggled states instead of
/// re-deriving them here — `toggled` is what makes an open tool window read
/// as pressed.
/// Badge fill and text per tone.
#[derive(Clone, Copy)]
struct BadgeColours {
    danger: (Hsla, Hsla),
    advisory: (Hsla, Hsla),
}

fn button<F>(
    b: &RailButton,
    on: Hsla,
    off: Hsla,
    badge_colours: BadgeColours,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&SharedString, &mut Window, &mut App) + 'static,
{
    let id = b.id.clone();
    // A Button's variant colours its own label; a child `svg` does not pick
    // that up through the cascade, so the tint is set here explicitly.
    let colour = if b.active { on } else { off };
    // The badge sits over the button's top-right corner, outside the
    // Button's own box so it neither pads nor tints it.
    let badge = b.badge.clone().map(|badge| {
        let (badge_bg, badge_fg) = match badge.tone {
            BadgeTone::Danger => badge_colours.danger,
            BadgeTone::Advisory => badge_colours.advisory,
        };
        div()
            .absolute()
            .top(px(-2.))
            .right(px(-2.))
            .min_w(px(14.))
            .h(px(14.))
            .px(px(3.))
            .rounded_full()
            .bg(badge_bg)
            .text_color(badge_fg)
            .text_size(px(9.))
            .flex()
            .items_center()
            .justify_center()
            .child(badge.text)
    });
    div()
        .relative()
        .child(
            Button::new(SharedString::from(format!("rail-{}", b.id)))
                .ghost()
                .compact()
                .toggled(b.active)
                .tooltip(match &b.keystroke {
                    Some(keys) => {
                        SharedString::from(format!("{} ({})", b.title, display_keystroke(keys)))
                    }
                    None => b.title.clone(),
                })
                .on_click(move |_, window, cx| on_click(&id, window, cx))
                .child(match b.icon {
                    // A complete SVG document, painted as a monochrome mask tinted
                    // by `colour` — only the alpha the shape covers matters.
                    Some(src) => svg()
                        .size(ICON_SIZE)
                        .text_color(colour)
                        .data(src.as_bytes())
                        .into_any_element(),
                    None => div()
                        .text_xs()
                        .text_color(colour)
                        .child(
                            b.title
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string())
                                .unwrap_or_default(),
                        )
                        .into_any_element(),
                }),
        )
        .children(badge)
}
