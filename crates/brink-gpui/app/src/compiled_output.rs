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
//! ## From a dump row back to the source
//!
//! The dump carries source positions in two shapes, and `Go to Source`
//! (F12, or the header button) reads both off the caret's line:
//!
//! - a line-table row's own `(source "file" a..b)` clause, written when
//!   the entry has a `source_location`; and
//! - a debug-info `(entry off file_idx start len kind flags)` row, which
//!   names its file by INDEX into the same dump's `(file …)` table — the
//!   shape a real dump is mostly made of, and the reason resolving a row
//!   takes the whole text and not just the line.
//!
//! A `(file …)` row itself opens that file at its start. Everything else
//! — a container head, a bytecode row — says it carries no source rather
//! than guessing at the nearest row above, which would send the author to
//! a line they did not click.
//!
//! Compile-bound and refreshed on the Program Explorer's rule: while it is
//! the shown tab it re-asks after each analysis; hidden, it marks itself
//! stale and asks when shown. A dump is a whole-file replacement, so there
//! is no incremental path to want here.

use std::ops::Range;

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

/// What the panel asks the studio to do for it.
pub enum CompiledOutputEvent {
    /// Open a file and reveal a span — a dump row's own source.
    Navigate { path: String, span: Range<usize> },
    /// The caret's row carries no source location.
    NoSource,
}

/// The `(source "file" start..end)` clause on a dump line, if it has one.
///
/// A hand parser rather than a regex or the `.inkt` reader: the reader
/// wants a whole well-formed document (this is one line), and the clause's
/// shape is fixed by `inkt::write` — a quoted path with backslash escapes,
/// then two integers separated by `..`. Anything that does not match is
/// simply "no source", never a guess.
#[must_use]
pub fn source_on_line(line: &str) -> Option<(String, Range<usize>)> {
    let rest = line.find("(source \"").map(|i| &line[i + 9..])?;
    let (path, end) = read_quoted(rest)?;
    let tail = rest[end..].trim_start();
    let (start, stop) = tail.split_once("..")?;
    let start: usize = start.trim().parse().ok()?;
    let stop: usize = stop
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    if path.is_empty() || stop < start {
        return None;
    }
    Some((path, start..stop))
}

/// A `(file <idx> <surface> "path" …)` row's index and path.
///
/// The debug-info file table is how an `(entry …)` row names its file: by
/// index into this table, not by path. Resolving one means reading the
/// table out of the same dump, which is why this takes the whole text.
#[must_use]
pub fn file_table(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("(file ") else {
            continue;
        };
        let mut fields = rest.splitn(3, ' ');
        let Some(Ok(idx)) = fields.next().map(str::parse::<usize>) else {
            continue;
        };
        let Some(tail) = fields.nth(1) else { continue };
        let Some(quoted) = tail.strip_prefix('"') else {
            continue;
        };
        if let Some((path, _)) = read_quoted(quoted) {
            out.push((idx, path));
        }
    }
    out
}

/// An `(entry offset file_idx range_start range_len kind flags)` row — the
/// debug-info form, which names its file by index into [`file_table`].
#[must_use]
pub fn entry_on_line(line: &str) -> Option<(usize, Range<usize>)> {
    let rest = line.trim_start().strip_prefix("(entry ")?;
    let rest = rest.trim_end().strip_suffix(')')?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if fields.len() != 6 {
        return None;
    }
    let file_idx: usize = fields[1].parse().ok()?;
    let start: usize = fields[2].parse().ok()?;
    let len: usize = fields[3].parse().ok()?;
    Some((file_idx, start..start + len))
}

/// The caret's row, resolved against the whole dump: either a line-table
/// row's own `(source …)` clause, or a debug-info `(entry …)` row through
/// the file table, or a `(file …)` row itself (which opens that file at
/// its start). Anything else is no source at all — never the nearest one
/// above, which would send the author somewhere they did not click.
#[must_use]
pub fn target_for(text: &str, line: &str) -> Option<(String, Range<usize>)> {
    if let Some(direct) = source_on_line(line) {
        return Some(direct);
    }
    if let Some((idx, span)) = entry_on_line(line) {
        let path = file_table(text)
            .into_iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, path)| path)?;
        return Some((path, span));
    }
    // A `(file …)` row names a file and nothing finer.
    let quoted = line.trim_start().strip_prefix("(file ")?;
    let mut fields = quoted.splitn(3, ' ');
    fields.next()?;
    let tail = fields.nth(1)?.strip_prefix('"')?;
    let (path, _) = read_quoted(tail)?;
    (!path.is_empty()).then_some((path, 0..0))
}

/// A `.inkt` quoted string starting after its opening quote: the unescaped
/// content, and the byte offset just past the closing quote.
fn read_quoted(rest: &str) -> Option<(String, usize)> {
    let mut path = String::new();
    let mut chars = rest.char_indices();
    loop {
        let (i, c) = chars.next()?;
        match c {
            '\\' => {
                // The writer escapes `\` and `"`; anything else it emits
                // literally, so an unknown escape keeps both characters.
                let (_, next) = chars.next()?;
                match next {
                    'n' => path.push('\n'),
                    't' => path.push('\t'),
                    '\\' | '"' => path.push(next),
                    other => {
                        path.push('\\');
                        path.push(other);
                    }
                }
            }
            '"' => return Some((path, i + 1)),
            _ => path.push(c),
        }
    }
}

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
impl EventEmitter<CompiledOutputEvent> for CompiledOutputView {}

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

    /// The caret's row, back to the source it was compiled from.
    pub fn go_to_source(&mut self, cx: &mut Context<Self>) {
        let state = self.editor.read(cx);
        let line_ix = state.cursor_position().line as usize;
        let text = state.value();
        // A debug-info row names its file by INDEX into the dump's own
        // file table, so resolving one needs the whole text, not just the
        // caret's line.
        let line = text.lines().nth(line_ix).map(str::to_owned);
        let resolved = line
            .as_deref()
            .and_then(|line| target_for(text.as_ref(), line));
        match resolved {
            Some((path, span)) => cx.emit(CompiledOutputEvent::Navigate { path, span }),
            None => cx.emit(CompiledOutputEvent::NoSource),
        }
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
                Button::new("inkt-source")
                    .ghost()
                    .xsmall()
                    .label("Go to Source")
                    .tooltip("Open the file this row was compiled from (F12)")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.go_to_source(cx);
                    })),
            )
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
        // F12 means the same thing here as in a source file — "show me
        // where this came from" — and this handler is deeper in the tree
        // than the studio's, so it wins while the dump has focus. Taken
        // before the header, which borrows `cx` for as long as it lives.
        let go_to_source = cx.listener(|this: &mut Self, _: &crate::GoToDefinition, _, cx| {
            this.go_to_source(cx);
        });
        let header = self.header(cx);
        v_flex()
            .id("compiled-output")
            .track_focus(&self.focus)
            .on_action(go_to_source)
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

#[cfg(test)]
mod tests {
    use super::{entry_on_line, source_on_line, target_for};

    #[test]
    fn a_line_table_row_gives_up_its_file_and_span() {
        let line = r#"      3 "You went left." @00000000deadbeef (source "story.ink" 120..135)"#;
        let (path, span) = source_on_line(line).expect("the row carries a source");
        assert_eq!(path, "story.ink");
        assert_eq!(span, 120..135);
    }

    #[test]
    fn a_quoted_path_survives_its_escapes() {
        let line = r#"      0 "hi" (source "a\"b/c.ink" 1..2)"#;
        let (path, span) = source_on_line(line).expect("escaped quote is still a path");
        assert_eq!(path, r#"a"b/c.ink"#);
        assert_eq!(span, 1..2);
    }

    #[test]
    fn a_row_with_no_source_is_no_source_not_a_guess() {
        // A container head, a bytecode row, and the clause half-written:
        // none of these may borrow the span of some other row.
        assert!(source_on_line("  (container $c0 story").is_none());
        assert!(source_on_line("      0004  emit_line 3").is_none());
        assert!(source_on_line(r#"(source "story.ink" 12)"#).is_none());
        assert!(source_on_line(r#"(source "story.ink")"#).is_none());
        assert!(source_on_line(r#"(source "" 1..2)"#).is_none(), "no path");
        assert!(
            source_on_line(r#"(source "a.ink" 9..2)"#).is_none(),
            "backwards"
        );
        assert!(source_on_line("").is_none());
    }

    #[test]
    fn a_debug_info_row_resolves_its_file_through_the_dump_table() {
        let dump = r#"  (debug_info
    (files
      (file 0 ink "story.ink" 1966225 (lines 0 12 30))
      (file 1 native "chapter.brink" 1769568)
    )
    (dcontainer 18
      (entry 0 1 754 32 1769568 3)
      (entry 4 0 769 16 1966225 1)
    )
"#;
        assert_eq!(
            target_for(dump, "      (entry 4 0 769 16 1966225 1)"),
            Some(("story.ink".to_owned(), 769..785)),
            "start + len, and the file by index"
        );
        assert_eq!(
            target_for(dump, "      (entry 0 1 754 32 1769568 3)"),
            Some(("chapter.brink".to_owned(), 754..786)),
            "the second file is index 1, not the first one found"
        );
        // An index the table does not hold is no source, not file 0.
        assert!(target_for(dump, "      (entry 0 7 1 2 3 4)").is_none());
        // A `(file …)` row opens the file it names.
        assert_eq!(
            target_for(dump, r#"      (file 1 native "chapter.brink" 1769568)"#),
            Some(("chapter.brink".to_owned(), 0..0))
        );
        assert!(target_for(dump, "    (dcontainer 18").is_none());
    }

    #[test]
    fn a_direct_source_clause_wins_and_needs_no_table() {
        assert_eq!(
            target_for("", r#"  0 "hi" (source "a.ink" 4..9)"#),
            Some(("a.ink".to_owned(), 4..9))
        );
    }

    #[test]
    fn a_malformed_entry_row_is_no_source() {
        let dump = r#"      (file 0 ink "story.ink" 1)"#;
        assert!(entry_on_line("      (entry 0 0 1)").is_none(), "too few");
        assert!(entry_on_line("      (entry a b c d e f)").is_none());
        assert!(
            entry_on_line("      (entry 0 0 1 2 3 4").is_none(),
            "unclosed"
        );
        assert!(target_for(dump, "      (entry 0 0 1 2 3)").is_none());
    }

    #[test]
    fn a_clause_followed_by_more_of_the_row_still_reads() {
        // `(source ...)` is written last today, but the reader must not
        // depend on being at the end of the line to find its numbers.
        let line = r#"  1 "x" (source "a.ink" 4..9) (audio "cue")"#;
        assert_eq!(
            source_on_line(line),
            Some(("a.ink".to_owned(), 4..9)),
            "the span ends at its own paren"
        );
    }
}
