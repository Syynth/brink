//! The Search tool window — project-wide find (studio spec §4 "Search",
//! `docs/search-results-cards-spec.md`, rulings of 2026-08-24).
//!
//! ## What is ported
//!
//! - **The engine** (`packages/ink-editor/src/project-search.ts`): plain
//!   text or regex, case and whole-word options composed into one regex,
//!   matches capped at [`RESULT_CAP`] as the unbounded-growth guard.
//! - **Per-match cards**: a header row (`file:line`, the containing
//!   knot/stitch, reveal `↗`, a collapse chevron) over the match line with
//!   its context window — 1 line above, 2 below, the ruled default.
//! - **The frozen snapshot**: once a search has run, edits never remove or
//!   re-filter cards; only typing a new query, changing an option, or the
//!   summary strip's `↻` replaces the set.
//! - **The summary strip**: "N results · M files" and, reusing the
//!   Binder's controls (ruled), expand-all / collapse-all, plus `↻`.
//! - `search.focus` (`cmd-shift-f`): open the window and focus the query.
//!
//! ## Held back, and why
//!
//! The cards are **read-only** here. The ruling makes inline editing the
//! point of the surface, but an editable card needs one buffer per file
//! that every editor over that file — Code view's document, a manuscript
//! section, a card — is a view of, and the native app has no such thing
//! yet: each surface owns its own `EditorState`. That shared buffer is a
//! model-layer piece (it is also why an edit in the manuscript does not
//! reach the Code view's tab today); cards become editable when it lands,
//! and replace previews with it. References mode waits on a worker query.
//! The context knob, `edited` badges and syntax colouring in cards are
//! deferred with the same buffer.

use std::collections::BTreeSet;
use std::ops::Range;

use brink_gpui_shell::tool_window::{TabSlot, ToolWindow};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, SharedString, Subscription, WeakEntity, Window, div, px, uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, TabGroup};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use regex::{Regex, RegexBuilder};

use crate::icons;
use crate::project::Project;

/// Hard cap on matches per search — the unbounded-growth guard.
pub const RESULT_CAP: usize = 1000;

/// Context lines around the match line: the ruled default, 1 above and 2
/// below.
pub const CONTEXT: (usize, usize) = (1, 2);

/// Activating a card opens its file with the match selected.
#[derive(Debug, Clone)]
pub enum SearchEvent {
    Reveal { path: String, span: Range<usize> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// Compile the query and options into one regex, as the studio does: a
/// plain query is escaped so every option composes through the same
/// path; a regex query is validated before the whole-word wrapping so an
/// error points at the author's input, not the decoration.
pub fn build_pattern(query: &str, options: SearchOptions) -> Result<Regex, String> {
    let source = if options.regex {
        Regex::new(query).map_err(|e| format!("Invalid regex: {e}"))?;
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let source = if options.whole_word {
        format!(r"\b(?:{source})\b")
    } else {
        source
    };
    RegexBuilder::new(&source)
        .case_insensitive(!options.case_sensitive)
        .multi_line(true)
        .build()
        .map_err(|e| format!("Invalid regex: {e}"))
}

/// One match, with what its card shows: the match line and its context,
/// captured at search time so the card is stable under later edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub path: String,
    pub span: Range<usize>,
    /// 1-based line of the match's start.
    pub line: u32,
    /// The knot or stitch the match sits in, when one precedes it.
    pub container: Option<String>,
    /// 1-based line number of `lines[0]`.
    pub first_line: u32,
    pub lines: Vec<String>,
    /// Index into `lines` of the match line, and the hit's range within it.
    pub hit_line: usize,
    pub hit: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub matches: Vec<Match>,
    pub files: usize,
    pub capped: bool,
}

/// Run `pattern` over every file, in the order given.
pub fn search<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
    pattern: &Regex,
    context: (usize, usize),
    cap: usize,
) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for (path, source) in files {
        let starts = line_starts(source);
        // The editor draws the empty line a trailing newline creates, but a
        // card's context window should end at the last line with anything
        // on it.
        let last_line = if source.ends_with('\n') && starts.len() > 1 {
            starts.len() - 2
        } else {
            starts.len() - 1
        };
        let mut any = false;
        for found in pattern.find_iter(source) {
            if snapshot.matches.len() >= cap {
                snapshot.capped = true;
                break;
            }
            // An empty match (a regex like `a*`) would never advance;
            // `find_iter` steps past it, but the card would be nothing.
            if found.is_empty() {
                continue;
            }
            any = true;
            let line_ix = line_index_at(&starts, found.start());
            let first = line_ix.saturating_sub(context.0);
            let last = (line_ix + context.1).min(last_line).max(line_ix);
            let lines: Vec<String> = (first..=last)
                .map(|ix| line_text(source, &starts, ix))
                .collect();
            let line_start = starts[line_ix];
            let hit_end = found.end().min(line_end(source, &starts, line_ix));
            snapshot.matches.push(Match {
                path: path.to_owned(),
                span: found.range(),
                line: line_ix as u32 + 1,
                container: container_at(source, found.start()),
                first_line: first as u32 + 1,
                lines,
                hit_line: line_ix - first,
                hit: (found.start() - line_start)..(hit_end - line_start),
            });
        }
        if any {
            snapshot.files += 1;
        }
        if snapshot.capped {
            break;
        }
    }
    snapshot
}

/// Byte offset of every line's first character; a source always has at
/// least one line.
fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(source.match_indices('\n').map(|(at, _)| at + 1));
    starts
}

fn line_index_at(starts: &[usize], offset: usize) -> usize {
    match starts.binary_search(&offset) {
        Ok(ix) => ix,
        Err(ix) => ix - 1,
    }
}

/// End of a line, excluding its newline.
fn line_end(source: &str, starts: &[usize], ix: usize) -> usize {
    let end = starts.get(ix + 1).map_or(source.len(), |next| next - 1);
    end.max(starts[ix])
}

fn line_text(source: &str, starts: &[usize], ix: usize) -> String {
    let text = &source[starts[ix]..line_end(source, starts, ix)];
    text.strip_suffix('\r').unwrap_or(text).to_owned()
}

/// The knot or stitch whose header most recently precedes `offset` —
/// `knot`, `knot.stitch`, or a native `flow`/`fn` name. A text scan over
/// header shapes, not a parse: it names where a match sits, and a header
/// the parser would reject still reads as a heading to the author.
pub fn container_at(source: &str, offset: usize) -> Option<String> {
    let mut knot: Option<String> = None;
    let mut stitch: Option<String> = None;
    let mut pos = 0;
    for line in source.split_inclusive('\n') {
        if pos > offset {
            break;
        }
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("===") {
            knot = ident_after(rest);
            stitch = None;
        } else if let Some(rest) = t.strip_prefix('=').filter(|r| !r.starts_with('=')) {
            stitch = ident_after(rest);
        } else if let Some(rest) = t.strip_prefix("flow ").or_else(|| t.strip_prefix("fn ")) {
            knot = ident_after(rest);
            stitch = None;
        }
        pos += line.len();
    }
    match (knot, stitch) {
        (Some(k), Some(s)) => Some(format!("{k}.{s}")),
        (Some(k), None) => Some(k),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

/// The first identifier in a header's remainder, skipping ink's
/// `function` keyword and the `=`s that decorate a knot header.
fn ident_after(rest: &str) -> Option<String> {
    let rest = rest.trim_start_matches(['=', ' ', '\t']);
    let rest = rest.strip_prefix("function ").unwrap_or(rest).trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// What the list draws: a card's header, or one of its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Header(usize),
    Line { card: usize, line: usize },
}

fn layout(snapshot: &Snapshot, collapsed: &BTreeSet<usize>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (card, m) in snapshot.matches.iter().enumerate() {
        rows.push(Row::Header(card));
        if !collapsed.contains(&card) {
            rows.extend((0..m.lines.len()).map(|line| Row::Line { card, line }));
        }
    }
    rows
}

pub struct SearchView {
    project: Entity<Project>,
    query: Entity<InputState>,
    options: SearchOptions,
    /// The last search's result — frozen until the next search.
    snapshot: Option<Snapshot>,
    /// A regex the author has not finished typing, shown under the input.
    error: Option<String>,
    collapsed: BTreeSet<usize>,
    rows: Vec<Row>,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SearchEvent> for SearchView {}
impl EventEmitter<PanelEvent> for SearchView {}

const ROW_HEIGHT: f32 = 22.0;

impl SearchView {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find in files\u{2026}"));
        // Live search on every keystroke: the sources are in memory and the
        // pattern runs in well under a frame at project scale, so there is
        // nothing to hide behind a timer (no debounce, ruled).
        let on_query = cx.subscribe(&query, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.run(cx);
            }
        });
        Self {
            project,
            query,
            options: SearchOptions::default(),
            snapshot: None,
            error: None,
            collapsed: BTreeSet::new(),
            rows: Vec::new(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_query],
        }
    }

    /// Put the caret in the query box — what `search.focus` wants.
    pub fn focus_query(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |query, cx| query.focus(window, cx));
    }

    /// Run the query against the sources as they are now. Replaces the
    /// snapshot; nothing else does.
    fn run(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_string();
        self.collapsed.clear();
        if query.is_empty() {
            self.snapshot = None;
            self.error = None;
        } else {
            match build_pattern(&query, self.options) {
                Ok(pattern) => {
                    let project = self.project.read(cx);
                    let files = project
                        .files()
                        .iter()
                        .filter_map(|path| project.loaded_source(path).map(|s| (path.as_str(), s)));
                    self.snapshot = Some(search(files, &pattern, CONTEXT, RESULT_CAP));
                    self.error = None;
                }
                Err(error) => {
                    self.snapshot = None;
                    self.error = Some(error);
                }
            }
        }
        self.relayout(cx);
    }

    fn relayout(&mut self, cx: &mut Context<Self>) {
        self.rows = self
            .snapshot
            .as_ref()
            .map(|s| layout(s, &self.collapsed))
            .unwrap_or_default();
        cx.notify();
    }

    fn toggle_option(&mut self, apply: impl FnOnce(&mut SearchOptions), cx: &mut Context<Self>) {
        apply(&mut self.options);
        self.run(cx);
    }

    fn set_all_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.collapsed = if collapsed {
            (0..self.snapshot.as_ref().map_or(0, |s| s.matches.len())).collect()
        } else {
            BTreeSet::new()
        };
        self.relayout(cx);
    }

    fn toggle_collapsed(&mut self, card: usize, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&card) {
            self.collapsed.insert(card);
        }
        self.relayout(cx);
    }

    fn reveal(&self, card: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.snapshot.as_ref().and_then(|s| s.matches.get(card)) {
            cx.emit(SearchEvent::Reveal {
                path: m.path.clone(),
                span: m.span.clone(),
            });
        }
    }

    /// An option toggle in the title strip: a two-letter glyph, pressed
    /// when on.
    fn option_toggle(
        &self,
        id: &'static str,
        glyph: &'static str,
        tooltip: &'static str,
        on: bool,
        apply: impl Fn(&mut SearchOptions) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let colour = if on {
            theme.primary
        } else {
            theme.muted_foreground
        };
        Button::new(id)
            .ghost()
            .compact()
            .toggled(on)
            .tooltip(tooltip)
            .child(div().text_xs().text_color(colour).child(glyph))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.toggle_option(|o| apply(o), cx);
            }))
            .into_any_element()
    }

    /// A summary-strip affordance — the Binder's own control, as ruled.
    fn tool(
        id: &'static str,
        src: &'static str,
        tooltip: &'static str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let colour = cx.theme().muted_foreground;
        Button::new(id)
            .ghost()
            .compact()
            .tooltip(tooltip)
            .child(icons::icon(src, px(14.), colour))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
            .into_any_element()
    }

    fn render_summary(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snapshot = self.snapshot.as_ref()?;
        let theme = cx.theme();
        let n = snapshot.matches.len();
        let text = format!(
            "{n} result{} \u{B7} {} file{}{}",
            if n == 1 { "" } else { "s" },
            snapshot.files,
            if snapshot.files == 1 { "" } else { "s" },
            if snapshot.capped { " (capped)" } else { "" }
        );
        Some(
            h_flex()
                .h(px(28.))
                .px_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(div().flex_1().child(text))
                .child(Self::tool(
                    "search-expand-all",
                    icons::EXPAND_ALL,
                    "Expand all",
                    cx,
                    |this, cx| this.set_all_collapsed(false, cx),
                ))
                .child(Self::tool(
                    "search-collapse-all",
                    icons::COLLAPSE_ALL,
                    "Collapse all",
                    cx,
                    |this, cx| this.set_all_collapsed(true, cx),
                ))
                .child(Self::tool(
                    "search-refresh",
                    icons::REFRESH,
                    "Search again against the current sources",
                    cx,
                    |this, cx| this.run(cx),
                ))
                .into_any_element(),
        )
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, accent, hover, hit_bg) = (
            theme.foreground,
            theme.muted_foreground,
            theme.primary,
            theme.muted.opacity(0.5),
            theme.warning.opacity(0.35),
        );
        let Some(snapshot) = self.snapshot.as_ref() else {
            return div().into_any_element();
        };
        match self.rows[ix] {
            Row::Header(card) => {
                let m = &snapshot.matches[card];
                let collapsed = self.collapsed.contains(&card);
                let location = format!("{}:{}", m.path, m.line);
                let preview = collapsed.then(|| m.lines[m.hit_line].trim().to_owned());
                h_flex()
                    .id(("search-card", card))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .text_xs()
                    .child(
                        div()
                            .id(("search-chevron", card))
                            .w(px(12.))
                            .text_color(muted)
                            .cursor_pointer()
                            .child(if collapsed { "\u{25B8}" } else { "\u{25BE}" })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.toggle_collapsed(card, cx);
                            })),
                    )
                    .child(
                        div()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(accent)
                            .child(location),
                    )
                    .children(
                        m.container
                            .clone()
                            .map(|c| div().text_color(muted).child(c)),
                    )
                    .children(preview.map(|p| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(muted)
                            .child(p)
                    }))
                    .child(div().flex_1())
                    .child(div().text_color(muted).child("\u{2197}"))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.reveal(card, cx);
                    }))
                    .into_any_element()
            }
            Row::Line { card, line } => {
                let m = &snapshot.matches[card];
                let text = &m.lines[line];
                let number = m.first_line as usize + line;
                let is_hit = line == m.hit_line;
                let mono = theme.mono_font_family.clone();
                let mut content = h_flex()
                    .min_w_0()
                    .whitespace_nowrap()
                    .font_family(mono.clone());
                if is_hit {
                    let hit = m.hit.start.min(text.len())..m.hit.end.min(text.len());
                    content = content
                        .child(text[..hit.start].to_owned())
                        .child(
                            div()
                                .bg(hit_bg)
                                .rounded_sm()
                                .child(text[hit.clone()].to_owned()),
                        )
                        .child(text[hit.end..].to_owned());
                } else {
                    content = content.child(text.clone());
                }
                // The whole card reveals, not just its header: a line is
                // where the eye lands, and a click there should go to it.
                h_flex()
                    .id(("search-line", ix))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .pl_6()
                    .pr_2()
                    .gap_2()
                    .items_center()
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.reveal(card, cx);
                    }))
                    .text_xs()
                    .text_color(if is_hit { fg } else { muted })
                    .child(
                        div()
                            .w(px(32.))
                            .flex_shrink_0()
                            .text_right()
                            .font_family(mono)
                            .text_color(muted)
                            .child(number.to_string()),
                    )
                    .child(content)
                    .into_any_element()
            }
        }
    }
}

impl Focusable for SearchView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl BasePanel for SearchView {
    fn panel_name(&self) -> &'static str {
        "Search"
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

impl Panel for SearchView {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Search")
    }

    /// The options, in the title strip.
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let o = self.options;
        Some(
            h_flex()
                .gap_0p5()
                .child(self.option_toggle(
                    "search-case",
                    "Aa",
                    "Match case",
                    o.case_sensitive,
                    |o| o.case_sensitive = !o.case_sensitive,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-word",
                    "W",
                    "Whole word",
                    o.whole_word,
                    |o| o.whole_word = !o.whole_word,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-regex",
                    ".*",
                    "Regular expression",
                    o.regex,
                    |o| o.regex = !o.regex,
                    cx,
                )),
        )
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl ToolWindow for SearchView {
    fn tab_slot(&self) -> Option<&TabSlot> {
        Some(&self.tab)
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let summary = self.render_summary(cx);
        let count = self.rows.len();
        let empty: Option<SharedString> = match (&self.error, &self.snapshot) {
            (Some(error), _) => Some(error.clone().into()),
            (None, None) => Some("Type to search the project.".into()),
            (None, Some(s)) if s.matches.is_empty() => Some("No results.".into()),
            _ => None,
        };
        let is_error = self.error.is_some();
        v_flex()
            .id("search")
            .size_full()
            .text_xs()
            .child(div().px_2().py_1().child(Input::new(&self.query).small()))
            .children(summary)
            .when_some(empty, |el, text| {
                el.child(
                    div()
                        .p_3()
                        .text_color(if is_error { danger } else { muted })
                        .child(text),
                )
            })
            .when(count > 0, |el| {
                el.child(
                    uniform_list(
                        "search-rows",
                        count,
                        cx.processor(|this, range: Range<usize>, _window, cx| {
                            range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                        }),
                    )
                    .py_1()
                    .flex_1(),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: &str = "=== start ===\nHello there.\n* [Go left] -> left\n= inner\nstill here\n\n=== left ===\nYou went left.\nhello again\n";

    fn opts(case: bool, word: bool, regex: bool) -> SearchOptions {
        SearchOptions {
            case_sensitive: case,
            whole_word: word,
            regex,
        }
    }

    #[test]
    fn options_compose_through_one_pattern() {
        let p = build_pattern("hello", opts(false, false, false)).unwrap();
        assert_eq!(p.find_iter(INK).count(), 2, "case-insensitive by default");
        let p = build_pattern("hello", opts(true, false, false)).unwrap();
        assert_eq!(p.find_iter(INK).count(), 1);
        let p = build_pattern("left", opts(false, true, false)).unwrap();
        assert_eq!(
            p.find_iter(INK).count(),
            4,
            "whole word: [Go left], -> left, === left, went left"
        );
        // A plain query is escaped: `.` is a dot, not any character.
        let p = build_pattern("there.", opts(false, false, false)).unwrap();
        assert_eq!(p.find_iter(INK).count(), 1);
        let p = build_pattern("t.ere", opts(false, false, false)).unwrap();
        assert_eq!(p.find_iter(INK).count(), 0);
        // A regex query is a regex, and a broken one reports at the input.
        assert!(
            build_pattern("h.llo", opts(false, false, true))
                .unwrap()
                .is_match(INK)
        );
        assert!(
            build_pattern("(", opts(false, false, true))
                .unwrap_err()
                .starts_with("Invalid regex")
        );
    }

    #[test]
    fn a_match_carries_its_line_context_and_container() {
        let p = build_pattern("went", opts(false, false, false)).unwrap();
        let s = search([("story.ink", INK)], &p, CONTEXT, RESULT_CAP);
        assert_eq!(s.files, 1);
        assert_eq!(s.matches.len(), 1);
        let m = &s.matches[0];
        assert_eq!(m.line, 8);
        assert_eq!(m.container.as_deref(), Some("left"));
        // One line above, two below — clamped to the file's end.
        assert_eq!(m.first_line, 7);
        assert_eq!(m.lines, ["=== left ===", "You went left.", "hello again"]);
        assert_eq!(m.hit_line, 1);
        assert_eq!(&m.lines[m.hit_line][m.hit.clone()], "went");
    }

    #[test]
    fn stitches_qualify_the_knot_and_native_headers_count() {
        let at = INK.find("still").unwrap();
        assert_eq!(container_at(INK, at).as_deref(), Some("start.inner"));
        assert_eq!(container_at(INK, 0).as_deref(), Some("start"));
        let native = "flow main() {\n  VENDOR\n}\nfn cue(name: string) {\n  return name;\n}\n";
        assert_eq!(
            container_at(native, native.find("return").unwrap()).as_deref(),
            Some("cue")
        );
        assert_eq!(
            container_at("=== function f(x) ===\nx\n", 24).as_deref(),
            Some("f")
        );
        assert_eq!(container_at("no headers\n", 3), None);
    }

    #[test]
    fn the_cap_stops_the_search_and_says_so() {
        let source = "a a a a a a a a a a\n";
        let p = build_pattern("a", opts(false, false, false)).unwrap();
        let s = search([("x.ink", source), ("y.ink", source)], &p, CONTEXT, 5);
        assert_eq!(s.matches.len(), 5);
        assert!(s.capped);
        assert_eq!(s.files, 1, "the second file was never reached");
        let s = search([("x.ink", source)], &p, CONTEXT, RESULT_CAP);
        assert!(!s.capped);
        assert_eq!(s.matches.len(), 10);
    }

    #[test]
    fn collapsing_folds_a_card_to_its_header() {
        let p = build_pattern("hello", opts(false, false, false)).unwrap();
        let s = search([("story.ink", INK)], &p, CONTEXT, RESULT_CAP);
        let open = layout(&s, &BTreeSet::new());
        let headers = open.iter().filter(|r| matches!(r, Row::Header(_))).count();
        assert_eq!(headers, 2);
        assert!(open.len() > 2);
        let folded = layout(&s, &BTreeSet::from([0, 1]));
        assert_eq!(folded, [Row::Header(0), Row::Header(1)]);
    }
}
