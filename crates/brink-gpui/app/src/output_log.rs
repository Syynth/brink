//! The Output log — the studio's bottom-dock "Output / compile log"
//! (`docs/studio-shell-spec.md` §4: "Compile timings, wasm/runtime errors
//! that aren't source diagnostics. Replaces nothing; today this
//! information is dropped").
//!
//! It is the one place the studio's own working is written down. Nothing
//! here duplicates Problems: a source diagnostic has a file and a span and
//! belongs there, while what lands here has neither — how long a pass took,
//! that a project failed to open, that a running story hit a runtime error.
//!
//! ## What is worth a row
//!
//! Analysis runs on every keystroke, so logging each one would bury the
//! rows that matter under thousands that do not. An analysis is logged
//! when it is the **first**, when the **problem count moved**, or when it
//! was **slow** ([`SLOW_MS`]) — the three cases where the timing is telling
//! you something. Everything else is counted into a "N more analyses" tail
//! on the last analysis row, so the quiet ones are visible as a number
//! without each taking a line. `Verbose` turns the filter off and logs
//! every pass, for when that number is the thing being investigated.
//!
//! ## Bounded by construction
//!
//! [`CAP`] rows, oldest dropped — the project's standing rule that any
//! accumulation has a limit. The counter in the header says how many were
//! dropped, so a truncated log never reads as a complete one.

use std::collections::VecDeque;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, div, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::dock::{BasePanel, Panel, PanelEvent, TabGroup};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use brink_ir::Severity;

use crate::player::PlayerEvent;
use crate::project::{Project, ProjectEvent};
use brink_gpui_shell::tool_window::{TabSlot, ToolWindow};
use gpui::WeakEntity;

/// The most rows kept. Oldest first out — the unbounded-growth guard.
pub const CAP: usize = 500;

/// An analysis at or over this is logged even when nothing else changed.
pub const SLOW_MS: f64 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub level: Level,
    /// Which part of the studio spoke: "project", "analysis", "player".
    pub source: SharedString,
    pub text: SharedString,
    /// Quiet analyses folded into this row since it was written.
    pub also: usize,
}

/// The rows, and the rule for what earns one. Split from the view so the
/// decision is testable without a window.
#[derive(Debug, Default)]
pub struct Log {
    rows: VecDeque<Row>,
    dropped: usize,
    /// The problem count at the last logged analysis, to notice a move.
    last_problems: Option<usize>,
    /// Whether any analysis has been logged yet.
    analyzed_once: bool,
    pub verbose: bool,
}

impl Log {
    #[must_use]
    pub fn rows(&self) -> &VecDeque<Row> {
        &self.rows
    }

    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.dropped = 0;
        // Not `last_problems`/`analyzed_once`: clearing the view does not
        // un-analyze the project, and re-logging an unchanged count as if
        // it were news would be a lie about what happened.
    }

    pub fn push(&mut self, level: Level, source: &str, text: impl Into<SharedString>) {
        self.rows.push_back(Row {
            level,
            source: SharedString::from(source.to_owned()),
            text: text.into(),
            also: 0,
        });
        while self.rows.len() > CAP {
            self.rows.pop_front();
            self.dropped += 1;
        }
    }

    /// An analysis landed. `errors`/`warnings` are counts within
    /// `problems`, and they decide the row's colour: a project whose only
    /// problems are Info notes is not a project in trouble, and colouring
    /// its row amber said it was.
    pub fn analyzed(
        &mut self,
        elapsed_ms: f64,
        problems: usize,
        errors: usize,
        warnings: usize,
    ) -> bool {
        let moved = self.last_problems != Some(problems);
        let notable = self.verbose || !self.analyzed_once || moved || elapsed_ms >= SLOW_MS;
        self.analyzed_once = true;
        self.last_problems = Some(problems);
        if notable {
            let slow = if elapsed_ms >= SLOW_MS { " (slow)" } else { "" };
            let level = if errors > 0 {
                Level::Error
            } else if warnings > 0 {
                Level::Warning
            } else {
                Level::Info
            };
            self.push(
                level,
                "analysis",
                format!("{elapsed_ms:.1} ms · {problems} problem(s){slow}"),
            );
        } else if let Some(last) = self
            .rows
            .iter_mut()
            .rev()
            .find(|r| r.source.as_ref() == "analysis")
        {
            last.also += 1;
        }
        notable
    }
}

pub struct OutputLog {
    project: Entity<Project>,
    log: Log,
    scroll: UniformListScrollHandle,
    /// Rows at the last render, so a row added since can be scrolled to.
    /// A log that does not follow its own tail makes you scroll to find
    /// what just happened, which is the one thing it exists to show.
    shown_rows: usize,
    focus: FocusHandle,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for OutputLog {}

impl OutputLog {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let on_project = cx.subscribe(&project, |this: &mut Self, project, event, cx| {
            match event {
                ProjectEvent::Opened { elapsed_ms } => {
                    let files = project.read(cx).files().len();
                    this.log.push(
                        Level::Info,
                        "project",
                        format!("opened in {elapsed_ms:.1} ms · {files} file(s)"),
                    );
                }
                ProjectEvent::OpenFailed(message) => {
                    this.log.push(Level::Error, "project", message.clone());
                }
                ProjectEvent::Analyzed => {
                    let project = project.read(cx);
                    let (last, _worst) = project.timings();
                    let (mut errors, mut warnings, mut problems) = (0, 0, 0);
                    for (_, diagnostics) in project.all_diagnostics() {
                        for d in diagnostics {
                            problems += 1;
                            match d.severity {
                                Severity::Error => errors += 1,
                                Severity::Warning => warnings += 1,
                                _ => {}
                            }
                        }
                    }
                    this.log.analyzed(last, problems, errors, warnings);
                }
                ProjectEvent::Saved => {
                    this.log.push(Level::Info, "project", "saved");
                }
                ProjectEvent::SourceChanged { .. } => return,
            }
            cx.notify();
        });
        Self {
            project,
            log: Log::default(),
            scroll: UniformListScrollHandle::new(),
            shown_rows: 0,
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_project],
        }
    }

    /// Listen to the Player, once it exists. Runtime and compile failures
    /// are exactly the "errors that aren't source diagnostics" this window
    /// is for; the Player shows them in its transcript, and they are kept
    /// here too so the record survives a Restart clearing it.
    pub fn watch_player(&mut self, player: &Entity<crate::player::Player>, cx: &mut Context<Self>) {
        let subscription = cx.subscribe(player, |this: &mut Self, _, event: &PlayerEvent, cx| {
            if let PlayerEvent::Log { level, text } = event {
                this.log.push(*level, "player", text.clone());
                cx.notify();
            }
        });
        self._subscriptions.push(subscription);
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let dropped = self.log.dropped();
        let count = self.log.rows().len();
        let summary: SharedString = if dropped > 0 {
            format!("{count} rows · {dropped} older dropped").into()
        } else {
            format!("{count} rows").into()
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
                Checkbox::new("output-verbose")
                    .label("Every analysis")
                    .checked(self.log.verbose)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.log.verbose = *checked;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("output-clear")
                    .ghost()
                    .xsmall()
                    .label("Clear")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.log.clear();
                        cx.notify();
                    })),
            )
    }

    fn render_row(&self, index: usize, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let Some(row) = self.log.rows().get(index) else {
            return div();
        };
        let colour = match row.level {
            Level::Info => theme.muted_foreground,
            Level::Warning => theme.warning,
            Level::Error => theme.danger,
        };
        let tail: Option<SharedString> = (row.also > 0).then(|| {
            let n = row.also;
            format!("+{n} more").into()
        });
        div().child(
            h_flex()
                .w_full()
                .gap_2()
                .px_2()
                .py_0p5()
                .child(
                    div()
                        .w(gpui::px(56.))
                        .text_color(theme.muted_foreground)
                        .child(row.source.clone()),
                )
                .child(div().flex_1().text_color(colour).child(row.text.clone()))
                .children(
                    tail.map(|t| div().text_color(theme.muted_foreground).text_xs().child(t)),
                ),
        )
    }
}

impl Focusable for OutputLog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for OutputLog {
    fn panel_name(&self) -> &'static str {
        "Output"
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

impl Panel for OutputLog {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Output")
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl ToolWindow for OutputLog {
    fn tab_slot(&self) -> Option<&TabSlot> {
        Some(&self.tab)
    }
}

impl Render for OutputLog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let count = self.log.rows().len();
        // Follow the tail: a row arrived since the last frame, so show it.
        if count > self.shown_rows && count > 0 {
            self.scroll.scroll_to_item(count - 1, ScrollStrategy::Top);
        }
        self.shown_rows = count;
        let header = self.render_header(cx);
        let _ = &self.project;
        v_flex()
            .id("output-log")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .child(header)
            .when(count == 0, |el| {
                el.child(div().p_3().text_color(muted).child("Nothing logged yet."))
            })
            .when(count > 0, |el| {
                el.child(
                    uniform_list(
                        "output-rows",
                        count,
                        cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                            range
                                .map(|i| this.render_row(i, cx).into_any_element())
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .p_1()
                    .flex_1(),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_analysis_is_always_logged() {
        let mut log = Log::default();
        assert!(log.analyzed(1.0, 0, 0, 0), "the first analysis is news");
        assert_eq!(log.rows().len(), 1);
    }

    #[test]
    fn a_quiet_analysis_folds_into_the_last_row() {
        let mut log = Log::default();
        log.analyzed(1.0, 0, 0, 0);
        assert!(!log.analyzed(1.0, 0, 0, 0), "nothing moved, so no new row");
        assert!(!log.analyzed(2.0, 0, 0, 0));
        assert_eq!(log.rows().len(), 1, "still one analysis row");
        assert_eq!(log.rows()[0].also, 2, "and it counts the quiet ones");
    }

    #[test]
    fn a_moved_problem_count_earns_a_row() {
        let mut log = Log::default();
        log.analyzed(1.0, 0, 0, 0);
        assert!(log.analyzed(1.0, 3, 0, 1), "0 -> 3 problems is news");
        assert_eq!(log.rows().len(), 2);
        assert_eq!(log.rows()[1].level, Level::Warning);
    }

    #[test]
    fn a_slow_analysis_earns_a_row_even_when_nothing_moved() {
        let mut log = Log::default();
        log.analyzed(1.0, 0, 0, 0);
        assert!(log.analyzed(SLOW_MS, 0, 0, 0), "slow is worth saying");
        assert!(
            log.rows()[1].text.contains("(slow)"),
            "and it says why: {}",
            log.rows()[1].text
        );
    }

    #[test]
    fn verbose_logs_every_pass() {
        let mut log = Log {
            verbose: true,
            ..Log::default()
        };
        log.analyzed(1.0, 0, 0, 0);
        assert!(log.analyzed(1.0, 0, 0, 0));
        assert_eq!(log.rows().len(), 2);
    }

    #[test]
    fn info_only_problems_do_not_colour_the_row_as_trouble() {
        let mut log = Log::default();
        // Six Info notes and nothing else: not a project in trouble.
        log.analyzed(1.0, 6, 0, 0);
        assert_eq!(log.rows()[0].level, Level::Info);
        log.analyzed(1.0, 7, 1, 0);
        assert_eq!(log.rows()[1].level, Level::Error, "an error is trouble");
    }

    #[test]
    fn the_log_is_capped_and_says_how_many_it_dropped() {
        let mut log = Log::default();
        for i in 0..CAP + 10 {
            log.push(Level::Info, "test", format!("row {i}"));
        }
        assert_eq!(log.rows().len(), CAP, "capped");
        assert_eq!(log.dropped(), 10, "and honest about the rest");
        assert_eq!(
            log.rows()[0].text.as_ref(),
            "row 10",
            "oldest dropped first"
        );
    }

    #[test]
    fn clearing_does_not_re_log_an_unchanged_count_as_news() {
        let mut log = Log::default();
        log.analyzed(1.0, 2, 0, 0);
        log.clear();
        assert!(
            !log.analyzed(1.0, 2, 0, 0),
            "the count did not move, so it is not news again"
        );
    }
}
