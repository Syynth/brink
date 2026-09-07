//! The State View — the studio's debugger pane
//! (`docs/studio-shell-spec.md` §4), over the Player's running story.
//!
//! What it shows is the runtime's own `DebugSnapshot`, read through
//! `PlayCommand::Snapshot`: where the story is, how many turns it has
//! taken, every global and its value, the call stack innermost first,
//! visit counts by path, the choices on offer, and the story RNG.
//!
//! **No engine work was needed.** `INVENTORY.md` recorded this surface as
//! blocked on "nothing in the shared layer exposes a running `Story`'s
//! state"; that was stale — `Story::debug_snapshot` assembles all of it
//! and has for a long time. What was missing was a way for the UI to ASK,
//! which is one command on the play session.
//!
//! **It reads; it does not step.** Setting a variable, stepping an
//! instruction and breakpoints are the next questions (the Program
//! Explorer's executing-instruction overlay is the same session's), and
//! each is a control that changes a running story — worth its own slice
//! and its own thought, not a button added in passing here.
//!
//! Refreshed on the Player's own moves (it observes the Player entity) and
//! on demand. Nothing polls: a story that is waiting for a choice is not
//! changing, and a panel that re-asks every second would be lying about
//! how much is happening.

use std::collections::BTreeSet;

use brink_gpui_model::play::{PlayCommand, PlayState};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, SharedString, Subscription, Window, div, px, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, TabGroup};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::player::Player;
use crate::project::Project;
use brink_gpui_shell::tool_window::{TabSlot, ToolWindow};
use gpui::WeakEntity;

/// One rendered row — the panel flattens every section to these, so one
/// uniform list draws the whole thing (the Program Explorer's shape).
#[derive(Debug, Clone)]
enum Row {
    Section {
        key: String,
        title: SharedString,
        collapsed: bool,
    },
    /// `name  value` — a global, a frame, a visit count.
    Pair {
        key: SharedString,
        value: SharedString,
        /// Drawn in the accent: the innermost call frame, which is where
        /// the story actually is.
        accent: bool,
    },
    Text {
        text: SharedString,
        dim: bool,
    },
}

pub struct StateView {
    project: Entity<Project>,
    player: Entity<Player>,
    state: Option<PlayState>,
    /// A snapshot has been asked for and not answered.
    busy: bool,
    /// Bumped per request so a stale answer is dropped.
    generation: u64,
    collapsed: BTreeSet<String>,
    rows: Vec<Row>,
    focus: FocusHandle,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for StateView {}

impl StateView {
    pub fn new(project: Entity<Project>, player: Entity<Player>, cx: &mut Context<Self>) -> Self {
        // The Player changes the story's state without an event of its
        // own — it notifies — so this observes the entity.
        let watch = cx.observe(&player, |this: &mut Self, _, cx| this.refresh(cx));
        let mut this = Self {
            project,
            player,
            state: None,
            busy: false,
            generation: 0,
            collapsed: BTreeSet::new(),
            rows: Vec::new(),
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![watch],
        };
        this.relayout();
        this
    }

    /// Ask the worker for the running story's state.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.generation += 1;
        let generation = self.generation;
        let query = self.project.read(cx).play(PlayCommand::Snapshot, cx);
        cx.spawn(async move |this, cx| {
            let outcome = query.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.busy = false;
                this.state = outcome.ok().and_then(|o| o.state);
                this.relayout();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        self.relayout();
        cx.notify();
    }

    fn relayout(&mut self) {
        let mut rows = Vec::new();
        let Some(state) = self.state.clone() else {
            rows.push(Row::Text {
                text: "No story is running. Play (cmd-r) starts one.".into(),
                dim: true,
            });
            self.rows = rows;
            return;
        };
        let section = |rows: &mut Vec<Row>, key: &str, title: String| -> bool {
            let collapsed = self.collapsed.contains(key);
            rows.push(Row::Section {
                key: key.to_owned(),
                title: title.into(),
                collapsed,
            });
            !collapsed
        };

        if section(&mut rows, "where", "Where".to_owned()) {
            rows.push(Row::Pair {
                key: "status".into(),
                value: state.status.clone().into(),
                accent: false,
            });
            rows.push(Row::Pair {
                key: "location".into(),
                value: state
                    .location
                    .clone()
                    .unwrap_or_else(|| "(unresolved)".to_owned())
                    .into(),
                accent: true,
            });
            rows.push(Row::Pair {
                key: "turn".into(),
                value: state.turn.to_string().into(),
                accent: false,
            });
            rows.push(Row::Pair {
                key: "position".into(),
                // The same `(container, offset)` the Program Explorer
                // marks in its disassembly — said here in the numbers, so
                // the two panels can be checked against each other.
                value: match state.position {
                    Some((container, offset)) => {
                        format!("container {container} · {offset:#06x}").into()
                    }
                    None => SharedString::from("(no open container)"),
                },
                accent: false,
            });
            rows.push(Row::Pair {
                key: "rng".into(),
                // Both halves: a seed alone does not say how far the
                // story has drawn from it, and a divergence hunt needs
                // the second number as much as the first.
                value: format!("seed {} · previous {}", state.rng.0, state.rng.1).into(),
                accent: false,
            });
        }

        if section(
            &mut rows,
            "globals",
            format!("Globals ({})", state.globals.len()),
        ) {
            if state.globals.is_empty() {
                rows.push(Row::Text {
                    text: "none".into(),
                    dim: true,
                });
            }
            for (name, value) in &state.globals {
                rows.push(Row::Pair {
                    key: name.clone().into(),
                    value: value.clone().into(),
                    accent: false,
                });
            }
        }

        if section(
            &mut rows,
            "stack",
            format!("Call stack ({})", state.call_stack.len()),
        ) {
            if state.call_stack.is_empty() {
                rows.push(Row::Text {
                    text: "none".into(),
                    dim: true,
                });
            }
            for (i, (kind, location)) in state.call_stack.iter().enumerate() {
                rows.push(Row::Pair {
                    key: kind.clone().into(),
                    value: location
                        .clone()
                        .unwrap_or_else(|| "(no position)".to_owned())
                        .into(),
                    // Innermost first, and the innermost frame is where
                    // the story is.
                    accent: i == 0,
                });
            }
        }

        if section(
            &mut rows,
            "choices",
            format!("Choices ({})", state.choices.len()),
        ) {
            if state.choices.is_empty() {
                rows.push(Row::Text {
                    text: "none on offer".into(),
                    dim: true,
                });
            }
            for (i, text) in state.choices.iter().enumerate() {
                rows.push(Row::Pair {
                    key: format!("{}", i + 1).into(),
                    value: text.clone().into(),
                    accent: false,
                });
            }
        }

        if section(
            &mut rows,
            "visits",
            format!("Visits ({})", state.visits.len()),
        ) {
            if state.visits.is_empty() {
                rows.push(Row::Text {
                    text: "nothing visited yet".into(),
                    dim: true,
                });
            }
            for (path, count) in &state.visits {
                rows.push(Row::Pair {
                    key: path.clone().into(),
                    value: count.to_string().into(),
                    accent: false,
                });
            }
        }
        self.rows = rows;
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, accent, hover) = (
            theme.foreground,
            theme.muted_foreground,
            theme.primary,
            theme.muted.opacity(0.5),
        );
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let base = h_flex()
            .id(("state-row", ix))
            .w_full()
            .h(px(22.))
            .px_2()
            .gap_2()
            .items_center()
            .text_xs();
        match row {
            Row::Section {
                key,
                title,
                collapsed,
            } => {
                let key = key.clone();
                base.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(div().w(px(10.)).text_color(muted).child(if *collapsed {
                        "\u{25B8}"
                    } else {
                        "\u{25BE}"
                    }))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(title.clone()),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle(&key, cx);
                    }))
                    .into_any_element()
            }
            Row::Pair {
                key,
                value,
                accent: on,
            } => base
                .pl(px(20.))
                .child(
                    div()
                        .w(px(120.))
                        .flex_none()
                        .truncate()
                        .text_color(muted)
                        .child(key.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(if *on { accent } else { fg })
                        .child(value.clone()),
                )
                .into_any_element(),
            Row::Text { text, dim } => base
                .pl(px(20.))
                .child(
                    div()
                        .text_color(if *dim { muted } else { fg })
                        .child(text.clone()),
                )
                .into_any_element(),
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let summary: SharedString = match (&self.state, self.busy) {
            (Some(state), _) => format!(
                "{} · turn {}{}",
                state.status,
                state.turn,
                state
                    .location
                    .as_ref()
                    .map(|l| format!(" · {l}"))
                    .unwrap_or_default()
            )
            .into(),
            (None, true) => "reading…".into(),
            (None, false) => "no session".into(),
        };
        h_flex()
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .text_xs()
            .child(div().flex_1().text_color(muted).child(summary))
            .child(
                Button::new("state-refresh")
                    .ghost()
                    .xsmall()
                    .label("Refresh")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }
}

impl Focusable for StateView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for StateView {
    fn panel_name(&self) -> &'static str {
        "State"
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tab.added_to(group);
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tab.removed();
    }
}

impl Panel for StateView {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("State")
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl ToolWindow for StateView {
    fn tab_slot(&self) -> Option<&TabSlot> {
        Some(&self.tab)
    }
}

impl Render for StateView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being rendered is being shown, and a state read is cheap — but
        // only ask when there is nothing to show, or the panel would
        // re-ask on every frame.
        if self.state.is_none() && !self.busy && self.player.read(cx).is_docked() {
            self.refresh(cx);
        }
        let header = self.render_header(cx);
        let count = self.rows.len();
        v_flex()
            .id("state-view")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .child(header)
            .child(
                uniform_list(
                    "state-rows",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                        range.map(|i| this.render_row(i, cx)).collect::<Vec<_>>()
                    }),
                )
                .p_1()
                .flex_1(),
            )
    }
}
