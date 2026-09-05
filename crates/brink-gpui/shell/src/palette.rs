//! The command palette and the hamburger menu — `docs/studio-shell-spec.md`
//! §6: "a shell overlay listing enabled commands, fuzzy-filtered, showing
//! keybindings", and "a grouped menu generated from the command registry —
//! no hand-maintained menu structure".
//!
//! One overlay, two modes. The palette ranks the registry against what is
//! typed; the menu lists it grouped, with no input. Both dispatch the
//! command's action back through the workspace, which restores focus to
//! where it was first — a command must run against the surface the author
//! was in, not against the palette's own input.

use gpui::prelude::*;
use gpui::{
    Action, AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, KeyDownEvent, Render, SharedString, Subscription, Window, div, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::commands::{Command, display_keystroke, rank_titles};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    /// Fuzzy-filtered, flat.
    Palette,
    /// Grouped, complete, no input — the hamburger.
    Menu,
}

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

/// A drawn row: a group heading (menu mode) or a command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Heading(SharedString),
    Item(usize),
}

pub struct Palette {
    mode: PaletteMode,
    items: Vec<PaletteItem>,
    rows: Vec<Row>,
    /// Index into `rows`; always an `Item` when there is one.
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
    pub fn new(
        mode: PaletteMode,
        items: Vec<PaletteItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            mode,
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

    #[must_use]
    pub fn mode(&self) -> PaletteMode {
        self.mode
    }

    /// Where keys should land: the input in palette mode, the list itself
    /// in menu mode.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            PaletteMode::Palette => self.input.update(cx, |input, cx| input.focus(window, cx)),
            PaletteMode::Menu => window.focus(&self.focus, cx),
        }
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.rows = match self.mode {
            PaletteMode::Palette => {
                let titles: Vec<(String, String)> = self
                    .items
                    .iter()
                    .map(|i| (i.command.title.to_string(), i.command.full_title()))
                    .collect();
                rank_titles(&titles, &self.query)
                    .into_iter()
                    .map(Row::Item)
                    .collect()
            }
            PaletteMode::Menu => {
                // Groups in first-appearance order — registration order,
                // which is the order features started in.
                let mut rows = Vec::new();
                let mut seen: Vec<&SharedString> = Vec::new();
                for item in &self.items {
                    if !seen.contains(&&item.command.group) {
                        seen.push(&item.command.group);
                    }
                }
                for group in seen {
                    rows.push(Row::Heading(group.clone()));
                    rows.extend(
                        self.items
                            .iter()
                            .enumerate()
                            .filter(|(_, i)| &i.command.group == group)
                            .map(|(ix, _)| Row::Item(ix)),
                    );
                }
                rows
            }
        };
        self.selected = self.next_item_from(0, 1).unwrap_or(0);
        cx.notify();
    }

    /// The nearest `Item` row from `from` stepping by `step`, wrapping.
    fn next_item_from(&self, from: usize, step: isize) -> Option<usize> {
        let n = self.rows.len();
        if n == 0 {
            return None;
        }
        let mut ix = from;
        for _ in 0..n {
            if matches!(self.rows[ix], Row::Item(_)) {
                return Some(ix);
            }
            ix = (ix as isize + step).rem_euclid(n as isize) as usize;
        }
        None
    }

    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let start = (self.selected as isize + step).rem_euclid(n as isize) as usize;
        if let Some(ix) = self.next_item_from(start, step) {
            self.selected = ix;
            cx.notify();
        }
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(Row::Item(ix)) = self.rows.get(self.selected)
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
        match &self.rows[ix] {
            Row::Heading(group) => div()
                .h(px(ROW_HEIGHT))
                .px_3()
                .flex()
                .items_end()
                .pb_1()
                .text_xs()
                .text_color(muted)
                .child(group.to_uppercase())
                .into_any_element(),
            Row::Item(item_ix) => {
                let item = &self.items[*item_ix];
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
                    .when(self.mode == PaletteMode::Palette, |el| {
                        el.child(
                            div()
                                .text_color(muted)
                                .child(format!("{}:", item.command.group)),
                        )
                    })
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
            .when(self.mode == PaletteMode::Palette, |el| {
                el.child(div().px_1().child(Input::new(&self.input).small()))
            })
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
