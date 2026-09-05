//! **Single File view** — decision log 2026-08-26: one file at a time, no
//! tab strip. Navigating replaces what is on screen instead of accumulating
//! tabs, so nothing needs closing; the Binder, Problems or a jump is how you
//! change file.
//!
//! It shows Code view's active document — the one fact the views share — by
//! rendering the very `Document` entity Code view holds in a tab. Nothing is
//! duplicated: while this view is up, that tab is simply off screen.
//!
//! The companion split — the Player beside the file — is part of this view's
//! definition and deliberately absent. Where the Player sits in each view is
//! an open ruling (HANDOFF.md, "Open, parked by the maintainer"), so until it
//! is settled the view is the file alone.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, div,
};
use gpui_component::ActiveTheme as _;

use crate::code_view::{CodeView, CodeViewEvent};

pub struct SingleFileView {
    code: Entity<CodeView>,
    focus: FocusHandle,
    _subscription: Subscription,
}

impl SingleFileView {
    pub fn new(code: Entity<CodeView>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&code, |_, _, _: &CodeViewEvent, cx| cx.notify());
        Self {
            code,
            focus: cx.focus_handle(),
            _subscription: subscription,
        }
    }
}

impl Focusable for SingleFileView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.code.read(cx).active_document() {
            Some(document) => document.focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

impl Render for SingleFileView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        match self.code.read(cx).active_document().cloned() {
            Some(document) => div().size_full().child(document),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child("Open a file from the Binder"),
        }
    }
}
