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

/// Local wall-clock as `hh:mm:ss`.
///
/// Computed from the Unix epoch rather than pulled from a date library:
/// the log wants "when, roughly" within a session, and a dependency for
/// three fields would be the larger cost. UTC, since the studio has no
/// timezone of its own to be wrong about.
#[must_use]
pub fn clock() -> SharedString {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let day = secs % 86_400;
    SharedString::from(format!(
        "{:02}:{:02}:{:02}",
        day / 3600,
        (day % 3600) / 60,
        day % 60
    ))
}

/// An analysis at or over this is logged even when nothing else changed.
pub const SLOW_MS: f64 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// The toggle a level belongs to.
#[must_use]
pub fn level_ix(level: Level) -> usize {
    match level {
        Level::Error => 0,
        Level::Warning => 1,
        Level::Info => 2,
    }
}

/// The row indices `show` admits, oldest first. Split from the view so
/// the filter can be tested without a window.
#[must_use]
pub fn visible_rows(log: &Log, show: &[bool; 3]) -> Vec<usize> {
    log.rows()
        .iter()
        .enumerate()
        .filter(|(_, r)| show[level_ix(r.level)])
        .map(|(i, _)| i)
        .collect()
}

/// Counts per severity over every row, filtered or not — so a muted
/// toggle still says what turning it back on would restore.
#[must_use]
pub fn counts_of(log: &Log) -> [usize; 3] {
    let mut out = [0usize; 3];
    for row in log.rows() {
        out[level_ix(row.level)] += 1;
    }
    out
}

/// The visible rows as plain text, one per line — what Copy puts on the
/// clipboard. The filter is honoured: what you copy is what you can see,
/// which is the point of having muted a level in the first place.
#[must_use]
pub fn transcript(log: &Log, show: &[bool; 3]) -> String {
    let mut out = String::new();
    for i in visible_rows(log, show) {
        let Some(row) = log.rows().get(i) else {
            continue;
        };
        // `as_ref` matters: `SharedString`'s own `Display` writes through
        // without honouring the width, so `{:<8}` pads only a `&str`.
        out.push_str(&format!(
            "{} {:<8} {}",
            row.at,
            row.source.as_ref(),
            row.text
        ));
        if row.also > 0 {
            out.push_str(&format!(" (+{} more)", row.also));
        }
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone)]
pub struct Row {
    pub level: Level,
    /// Which part of the studio spoke: "project", "analysis", "player".
    pub source: SharedString,
    pub text: SharedString,
    /// Quiet analyses folded into this row since it was written.
    pub also: usize,
    /// Wall-clock `hh:mm:ss` when the row was written.
    ///
    /// Order alone is enough while you are watching; it stops being enough
    /// the moment you come back to the window and ask whether something
    /// happened just now or an hour ago.
    pub at: SharedString,
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
        self.push_at(level, source, text, clock());
    }

    /// The same, with the clock supplied — so a test can pin the format
    /// without pinning the moment it ran.
    pub fn push_at(
        &mut self,
        level: Level,
        source: &str,
        text: impl Into<SharedString>,
        at: SharedString,
    ) {
        self.rows.push_back(Row {
            level,
            source: SharedString::from(source.to_owned()),
            text: text.into(),
            also: 0,
            at,
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
    /// Which severities are shown. Each toggle carries its count over the
    /// UNFILTERED rows, so a muted one still says what turning it back on
    /// would restore — the Problems panel's rule, and for the same reason.
    show: [bool; 3],
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
                    let project = project.read(cx);
                    let files = project.files().len();
                    this.log.push(
                        Level::Info,
                        "project",
                        format!("opened in {elapsed_ms:.1} ms · {files} file(s)"),
                    );
                    // Load warnings — an unreadable file, a `brink.toml`
                    // key that means nothing. They have no span, so
                    // Problems cannot hold them, and stderr is not a
                    // surface a windowed studio has.
                    for warning in project.warnings() {
                        this.log.push(Level::Warning, "project", warning.clone());
                    }
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
                ProjectEvent::SaveFailed { path, message } => {
                    this.log
                        .push(Level::Error, "project", format!("{path}: {message}"));
                }
                ProjectEvent::SourceChanged { .. } => return,
            }
            cx.notify();
        });
        Self {
            project,
            log: Log::default(),
            show: [true; 3],
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

    /// The row indices the filter admits, newest last.
    fn visible(&self) -> Vec<usize> {
        visible_rows(&self.log, &self.show)
    }

    /// Counts per severity over every row, filtered or not.
    fn counts(&self) -> [usize; 3] {
        counts_of(&self.log)
    }

    /// One toggle per severity, each showing its count over the unfiltered
    /// rows.
    fn toggles(
        show: &[bool; 3],
        counts: &[usize; 3],
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let theme = cx.theme();
        let colours = [theme.danger, theme.warning, theme.muted_foreground];
        let labels = ["errors", "warnings", "info"];
        (0..3)
            .map(|i| {
                let on = show[i];
                let colour = colours[i];
                Button::new(("output-level", i))
                    .ghost()
                    .xsmall()
                    .toggled(on)
                    .tooltip(format!(
                        "{} {}",
                        if on { "Hide" } else { "Show" },
                        labels[i]
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if on { colour } else { theme.muted_foreground })
                            .child(format!("{}", counts[i])),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.show[i] = !this.show[i];
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
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
            .children(Self::toggles(&self.show, &self.counts(), cx))
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
                Button::new("output-copy")
                    .ghost()
                    .xsmall()
                    .label("Copy")
                    .tooltip("Copy the visible rows")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        let text = transcript(&this.log, &this.show);
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
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

    /// `slot` indexes the FILTERED list, not the log.
    fn render_row(&self, slot: usize, visible: &[usize], cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let Some(row) = visible.get(slot).and_then(|i| self.log.rows().get(*i)) else {
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
                    // Wide enough for `hh:mm:ss` at this size — narrower and
                    // the clock wraps onto two lines, which it did at 52px.
                    div()
                        .w(gpui::px(64.))
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(theme.muted_foreground)
                        .child(row.at.clone()),
                )
                .child(
                    div()
                        .w(gpui::px(56.))
                        .flex_none()
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
        let visible = self.visible();
        let count = visible.len();
        // Follow the tail: a row arrived since the last frame, so show it.
        if count > self.shown_rows && count > 0 {
            self.scroll.scroll_to_item(count - 1, ScrollStrategy::Top);
        }
        self.shown_rows = count;
        let header = self.render_header(cx);
        let total = self.log.rows().len();
        let _ = &self.project;
        v_flex()
            .id("output-log")
            .track_focus(&self.focus)
            .size_full()
            .text_xs()
            .child(header)
            .when(count == 0, |el| {
                el.child(div().p_3().text_color(muted).child(if total == 0 {
                    "Nothing logged yet."
                } else {
                    "Every row is filtered out."
                }))
            })
            .when(count > 0, |el| {
                el.child(
                    uniform_list(
                        "output-rows",
                        count,
                        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                            let visible = this.visible();
                            range
                                .map(|i| this.render_row(i, &visible, cx).into_any_element())
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
    fn the_clock_column_is_a_fixed_width_wall_clock() {
        let at = clock();
        let parts: Vec<&str> = at.split(':').collect();
        assert_eq!(parts.len(), 3, "hh:mm:ss, got {at}");
        for part in parts {
            assert_eq!(part.len(), 2, "each field is padded to two: {at}");
            assert!(part.chars().all(|c| c.is_ascii_digit()), "digits: {at}");
        }
    }

    #[test]
    fn a_row_carries_the_clock_it_was_written_at() {
        let mut log = Log::default();
        log.push_at(Level::Info, "test", "hello", SharedString::from("01:02:03"));
        assert_eq!(log.rows()[0].at.as_ref(), "01:02:03");
    }

    #[test]
    fn the_filter_hides_a_level_without_forgetting_it() {
        let mut log = Log::default();
        log.push(Level::Error, "test", "boom");
        log.push(Level::Warning, "test", "hmm");
        log.push(Level::Info, "test", "fyi");
        log.push(Level::Info, "test", "also fyi");

        assert_eq!(counts_of(&log), [1, 1, 2], "counts are over every row");
        assert_eq!(visible_rows(&log, &[true; 3]).len(), 4, "nothing hidden");

        // Info muted: the two info rows go, the counts do not.
        let show = [true, true, false];
        assert_eq!(visible_rows(&log, &show), vec![0, 1]);
        assert_eq!(
            counts_of(&log),
            [1, 1, 2],
            "a muted toggle still says what it would restore"
        );

        assert!(
            visible_rows(&log, &[false; 3]).is_empty(),
            "everything can be muted; the view says so rather than looking empty"
        );
    }

    #[test]
    fn copy_takes_the_visible_rows_and_only_those() {
        let mut log = Log::default();
        log.push_at(
            Level::Error,
            "player",
            "boom",
            SharedString::from("01:00:00"),
        );
        log.push_at(
            Level::Info,
            "project",
            "saved",
            SharedString::from("01:00:01"),
        );
        let all = transcript(&log, &[true; 3]);
        assert!(all.contains("01:00:00 player   boom"), "got {all:?}");
        assert_eq!(all.lines().count(), 2);

        let errors_only = transcript(&log, &[true, true, false]);
        assert_eq!(errors_only.lines().count(), 1, "muted rows are not copied");
        assert!(errors_only.contains("boom"));
    }

    #[test]
    fn a_folded_tail_is_copied_with_its_count() {
        let mut log = Log::default();
        log.analyzed(1.0, 0, 0, 0);
        log.analyzed(1.0, 0, 0, 0);
        assert!(
            transcript(&log, &[true; 3]).contains("(+1 more)"),
            "the fold is part of what the row says"
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
