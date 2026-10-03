//! The command palette — `docs/studio-shell-spec.md` §6: "a shell overlay
//! listing enabled commands, fuzzy-filtered, showing keybindings".
//!
//! It ranks the registry against what is typed and dispatches the chosen
//! command's action back through the workspace, which restores focus to
//! where it was first — a command must run against the surface the author
//! was in, not against the palette's own input. The grouped menu the spec
//! also asks for is the menu bar now (`crate::menus`, #3624), generated
//! from the same registry.

use gpui::prelude::*;
use gpui::{
    Action, AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, KeyDownEvent, Render, Subscription, Window, div, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::commands::{Command, display_keystroke, rank_titles};

/// What the overlay was opened over: the registry, snapshotted with each
/// command's enablement as gpui reported it at that moment.
pub struct PaletteItem {
    pub command: Command,
    pub enabled: bool,
}

pub enum PaletteEvent {
    /// Run this command — after the overlay has closed and focus is back.
    Run(Box<dyn Action>),
    Dismiss,
}

pub struct Palette {
    items: Vec<PaletteItem>,
    /// Indices into `items`, best match first.
    rows: Vec<usize>,
    /// Index into `rows`.
    selected: usize,
    input: Entity<InputState>,
    query: String,
    focus: FocusHandle,
    _subscription: Subscription,
}

/// Row height, and the most rows shown before the list scrolls.
const ROW_HEIGHT: f32 = 28.0;
const MAX_VISIBLE_ROWS: usize = 12;
pub const PALETTE_WIDTH: f32 = 480.0;

impl Palette {
    pub fn new(items: Vec<PaletteItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Type a command\u{2026}"));
        let subscription = cx.subscribe(
            &input,
            |this: &mut Self, state, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    this.query = state.read(cx).value().to_string();
                    this.rebuild(cx);
                }
                InputEvent::PressEnter { .. } => this.confirm(cx),
                _ => {}
            },
        );
        let mut this = Self {
            items,
            rows: Vec::new(),
            selected: 0,
            input,
            query: String::new(),
            focus: cx.focus_handle(),
            _subscription: subscription,
        };
        this.rebuild(cx);
        this
    }

    /// Keys land in the input.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let titles: Vec<(String, String)> = self
            .items
            .iter()
            .map(|i| (i.command.title.to_string(), i.command.full_title()))
            .collect();
        self.rows = rank_titles(&titles, &self.query);
        self.selected = 0;
        cx.notify();
    }

    /// Step the selection, wrapping.
    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected as isize + step).rem_euclid(n as isize) as usize;
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.rows.get(self.selected)
            && let Some(item) = self.items.get(*ix)
            && item.enabled
        {
            cx.emit(PaletteEvent::Run(item.command.action.boxed_clone()));
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Seen here before the input's own keymap runs, so up/down/escape
        // steer the list rather than the caret.
        match event.keystroke.key.as_str() {
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "enter" => self.confirm(cx),
            "escape" => cx.emit(PaletteEvent::Dismiss),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, accent) = (theme.foreground, theme.muted_foreground, theme.accent);
        let item = &self.items[self.rows[ix]];
        let selected = ix == self.selected;
        let colour = if item.enabled { fg } else { muted };
        let keystroke = item.command.keystroke.as_deref().map(display_keystroke);
        h_flex()
            .id(("palette-row", ix))
            .h(px(ROW_HEIGHT))
            .px_3()
            .gap_2()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .when(selected, |el| el.bg(accent))
            .child(
                div()
                    .text_color(muted)
                    .child(format!("{}:", item.command.group)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(colour)
                    .child(item.command.title.clone()),
            )
            .children(keystroke.map(|k| div().text_xs().text_color(muted).child(k)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.selected = ix;
                this.confirm(cx);
            }))
            .into_any_element()
    }
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Focusable for Palette {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let visible = self.rows.len().min(MAX_VISIBLE_ROWS);
        let count = self.rows.len();
        let empty = self.rows.is_empty();
        let muted = theme.muted_foreground;
        v_flex()
            .id("palette")
            .key_context("Palette")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)))
            .w(px(PALETTE_WIDTH))
            .p_1()
            .gap_1()
            .rounded_md()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .text_sm()
            .child(div().px_1().child(Input::new(&self.input).small()))
            .when(empty, |el| {
                el.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(muted)
                        .child("No matching commands"),
                )
            })
            .when(!empty, |el| {
                el.child(
                    uniform_list(
                        "palette-rows",
                        count,
                        cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                            range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                        }),
                    )
                    .h(px(ROW_HEIGHT * visible as f32)),
                )
            })
    }
}
