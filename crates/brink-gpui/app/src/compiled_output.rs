//! Compiled Output — the `.inkt` dump of the compiled story, as a
//! read-only Code-view tab (`docs/studio-shell-spec.md` §4,
//! `CompiledOutputDocument.tsx`; the disassembly-view precedent).
//!
//! A singleton tab, like the Player: opened once, selected thereafter. The
//! text comes from the worker ([`QueryKind::CompiledOutput`]), because the
//! dump is written from a `StoryData` the main thread never holds.
//!
//! **Read-only, not disabled.** The kit separates the two: `disabled` dims
//! the text to half alpha, which is wrong for something meant to be read,
//! while `readonly` refuses the user's edits and still paints normally —
//! and programmatic `set_value` bypasses both, which is what lets this
//! panel replace the text on each compile. So the buffer keeps selection,
//! scrolling and the gutter, and typing into it does nothing.
//!
//! The flag goes on the **element**, not the state: `Editor` pushes its own
//! `readonly` into the state on every render, so `set_readonly` at
//! construction is overwritten by the element's default on the first frame.
//! Setting it there instead of here is the difference between a dump you
//! can read and one you can edit — which the first version of this panel
//! was, until it was typed into.
//!
//! Compile-bound and refreshed on the Program Explorer's rule: while it is
//! the shown tab it re-asks after each analysis; hidden, it marks itself
//! stale and asks when shown. A dump is a whole-file replacement, so there
//! is no incremental path to want here.

use brink_gpui_model::compiled::{CompiledOutput, CompiledStatus};
use brink_gpui_model::query::{QueryKind, QueryResult};
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Subscription, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, PanelId, TabGroup};
use gpui_component::input::{Editor, EditorState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};
use brink_gpui_shell::tool_window::{TabSlot, select_tab};
use gpui::WeakEntity;

pub struct CompiledOutputView {
    project: Entity<Project>,
    editor: Entity<EditorState>,
    /// The last report, for the header line.
    report: Option<CompiledOutput>,
    /// A compile is in flight.
    busy: bool,
    /// An analysis landed while hidden; ask again when shown.
    stale: bool,
    /// Whether the panel is the shown tab (set by `render`).
    shown: bool,
    /// Bumped per request so a stale reply is dropped.
    generation: u64,
    focus: FocusHandle,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for CompiledOutputView {}

impl CompiledOutputView {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .language(crate::inkt_highlight::LANGUAGE)
                // The dump's own line structure is the point; wrapping a
                // bytecode row would hide the column alignment it relies on.
                .soft_wrap(false);
            // Installed before the editor's own `ensure_highlighter_factory`
            // fills the slot, so the tree-sitter path is never consulted —
            // there is no `.inkt` grammar for it to find anyway.
            state.set_highlighter_factory(crate::inkt_highlight::factory(), cx);
            state
        });
        let on_project = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| {
                if matches!(event, ProjectEvent::Analyzed) {
                    this.refresh_if_shown(window, cx);
                }
            },
        );
        Self {
            project,
            editor,
            report: None,
            busy: false,
            stale: true,
            shown: false,
            generation: 0,
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_project],
        }
    }

    /// Whether the panel currently sits in a dock.
    #[must_use]
    pub fn is_docked(&self) -> bool {
        self.tab.group().is_some()
    }

    /// Make this the shown tab of its group.
    pub fn activate(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        if let Some(group) = this.read(cx).tab.group() {
            select_tab(&group, PanelId::from(this.entity_id()), window, cx);
        }
    }

    /// Ask again if shown; otherwise remember to.
    fn refresh_if_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown {
            self.refresh(window, cx);
        } else {
            self.stale = true;
        }
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.project.read(cx).has_analyzed() {
            self.stale = true;
            return;
        }
        self.stale = false;
        self.busy = true;
        self.generation += 1;
        let generation = self.generation;
        let query = self.project.read(cx).query(QueryKind::CompiledOutput, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = query.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.busy = false;
                if let Ok(QueryResult::CompiledOutput(report)) = result {
                    this.apply(*report, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn apply(&mut self, report: CompiledOutput, window: &mut Window, cx: &mut Context<Self>) {
        let text = match &report.status {
            CompiledStatus::Ready { text, .. } => text.clone(),
            // The errors are Problems' business; the buffer says why it is
            // empty rather than showing the last good dump as if current.
            CompiledStatus::Errors(messages) => {
                let mut out = String::from("; the project has errors, so there is no program:\n");
                for m in messages {
                    out.push_str(";   ");
                    out.push_str(m);
                    out.push('\n');
                }
                out
            }
            CompiledStatus::NoEntry => {
                "; no entry file — set `[project] entry` in brink.toml.\n".to_owned()
            }
        };
        self.editor.update(cx, |state, cx| {
            // `set_value` bypasses `readonly` by design (the kit clears both
            // flags around a programmatic write), which is the whole reason
            // this panel can be read-only and still refresh.
            state.set_value(text, window, cx);
        });
        self.report = Some(report);
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let summary: SharedString = match self.report.as_ref().map(|r| &r.status) {
            Some(CompiledStatus::Ready { text, bytes }) => format!(
                "{} · {} lines · {bytes} B",
                self.report
                    .as_ref()
                    .and_then(|r| r.entry.clone())
                    .unwrap_or_else(|| "story".to_owned()),
                text.lines().count(),
            )
            .into(),
            Some(CompiledStatus::Errors(messages)) => {
                format!("{} error(s) — see Problems", messages.len()).into()
            }
            Some(CompiledStatus::NoEntry) => "no entry".into(),
            None if self.busy => "compiling…".into(),
            None => "not compiled yet".into(),
        };
        h_flex()
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().text_xs().text_color(muted).child(summary))
            .child(div().flex_1())
            .child(
                Button::new("inkt-refresh")
                    .ghost()
                    .xsmall()
                    .label("Refresh")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.refresh(window, cx);
                    })),
            )
    }
}

impl Focusable for CompiledOutputView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for CompiledOutputView {
    fn panel_name(&self) -> &'static str {
        "CompiledOutput"
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

impl Panel for CompiledOutputView {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Compiled Output")
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for CompiledOutputView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being rendered is being shown — the dock renders only the shown
        // tab (the Program Explorer's rule, and for the same reason).
        self.shown = true;
        if self.stale && !self.busy {
            self.refresh(window, cx);
        }
        let header = self.header(cx);
        v_flex()
            .id("compiled-output")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .child(header)
            // `flex_1` belongs on the Editor itself, not on a wrapper: the
            // editor computes its visible line range from its OWN height
            // (`continuous.rs` documents the same trap), so an editor
            // inside a flexed div has no height and lays out one line.
            .child(
                Editor::new(&self.editor)
                    // Read-only belongs on the ELEMENT, not the state: the
                    // `Editor` element pushes its own `readonly` into the
                    // state on every render, so a flag set once at
                    // construction is overwritten by the element's default
                    // on the first frame (it was — typing into the dump
                    // edited it).
                    .readonly(true)
                    .flex_1()
                    .bordered(false),
            )
    }
}
