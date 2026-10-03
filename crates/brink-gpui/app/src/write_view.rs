//! **Write mode's occupant** — the manuscript, and the Player beside it.
//!
//! In Script mode the Player is a centre tab. In Write mode it slides in
//! from the right and pushes the manuscript, so a passage can be read and
//! fixed in the same glance (`docs/gpui-writing-scripting-modes.md` W7,
//! which supersedes the parked "swap, not split" direction). It is the same
//! `Player` entity either way: only its home changes, and only one mode is
//! on screen at a time, so it is never drawn twice.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, ClickEvent, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use crate::continuous::ContinuousView;
use crate::player::Player;

/// The Player panel's width once it has slid in.
pub(crate) const PLAYER_WIDTH: f32 = 400.;

/// How long the slide takes.
const SLIDE: Duration = Duration::from_millis(160);

pub(crate) struct WriteView {
    manuscript: Entity<ContinuousView>,
    player: Entity<Player>,
    player_open: bool,
    /// Bumped on every opening, so the slide's animation id is new and it
    /// plays again rather than resuming at its end.
    openings: usize,
}

impl WriteView {
    pub(crate) fn new(manuscript: Entity<ContinuousView>, player: Entity<Player>) -> Self {
        Self {
            manuscript,
            player,
            player_open: false,
            openings: 0,
        }
    }

    #[must_use]
    pub(crate) fn is_player_open(&self) -> bool {
        self.player_open
    }

    /// Slide the Player in, or leave it where it is if it already is.
    pub(crate) fn open_player(&mut self, cx: &mut Context<Self>) {
        if !self.player_open {
            self.player_open = true;
            self.openings += 1;
            cx.notify();
        }
    }

    /// Put the Player away. The session keeps running; Play or the toggle
    /// brings it back where it was.
    pub(crate) fn close_player(&mut self, cx: &mut Context<Self>) {
        if self.player_open {
            self.player_open = false;
            cx.notify();
        }
    }
}

impl Focusable for WriteView {
    /// The manuscript's: switching to Write puts the caret in the text.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.manuscript.read(cx).focus_handle(cx)
    }
}

impl Render for WriteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, surface, muted) = (theme.border, theme.background, theme.muted_foreground);
        let panel = self.player_open.then(|| {
            v_flex()
                .h_full()
                .flex_none()
                .overflow_hidden()
                .border_l_1()
                .border_color(border)
                .bg(surface)
                .child(
                    h_flex()
                        .w(px(PLAYER_WIDTH))
                        .h(px(30.))
                        .flex_none()
                        .px_2()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(border)
                        .child(div().text_xs().text_color(muted).child("Player"))
                        .child(
                            Button::new("write-player-close")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Close the Player")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.close_player(cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .w(px(PLAYER_WIDTH))
                        .flex_1()
                        .min_h_0()
                        .child(self.player.clone()),
                )
                // The slide: the panel's width grows from nothing, so the
                // manuscript beside it is pushed rather than covered. The
                // contents stay at full width and are clipped meanwhile, so
                // nothing re-wraps mid-slide.
                .with_animation(
                    SharedString::from(format!("write-player-{}", self.openings)),
                    Animation::new(SLIDE).with_easing(gpui::ease_out_quint()),
                    |panel, delta| panel.w(px(PLAYER_WIDTH * delta)),
                )
        });
        h_flex()
            .id("write-view")
            .size_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.manuscript.clone()),
            )
            .children(panel)
    }
}
