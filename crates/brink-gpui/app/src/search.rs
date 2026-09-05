//! The Search tool window — project-wide find (studio spec §4 "Search",
//! `docs/search-results-cards-spec.md`, rulings of 2026-08-24).
//!
//! ## What is ported
//!
//! - **The engine** (`packages/ink-editor/src/project-search.ts`): plain
//!   text or regex, case and whole-word options composed into one regex,
//!   matches capped at [`RESULT_CAP`] as the unbounded-growth guard.
//! - **Per-match cards**: a header row (`file:line`, the containing
//!   knot/stitch, an `edited` badge, reveal `↗`, a collapse chevron) over
//!   an **editable buffer** holding the match line with its context window
//!   — 1 line above, 2 below, the ruled default — syntax-coloured by the
//!   same highlighter every other editor uses, the hit marked.
//! - **Write-through**: a card is one more editor over the shared buffer
//!   (spec §6). An edit in a card is spliced into the file's canonical text
//!   through `Project::edit`, so Code view's tab and the manuscript follow
//!   it; an edit anywhere else reaches the card the same way. Ruled: inline
//!   editing is the point of the surface.
//! - **The frozen snapshot**: once a search has run, edits never remove or
//!   re-filter cards; only typing a new query, changing an option, or the
//!   summary strip's `↻` replaces the set. Every card's window and hit are
//!   **edit-mapped** through each change, so write-through stays aimed at
//!   the right bytes and a card whose text has moved away from what the
//!   search saw is badged `edited` and stays.
//! - **The summary strip**: "N results · M files" and, reusing the
//!   Binder's controls (ruled), expand-all / collapse-all, plus `↻`.
//! - `search.focus` (`cmd-shift-f`): open the window and focus the query.
//!
//! ## How a card follows the file
//!
//! A card's window is a line-aligned byte range of its file. When the file
//! changes ([`ProjectEvent::SourceChanged`]), [`Match::map_edit`] moves the
//! window through the delta: a change wholly before it shifts it, one
//! wholly inside it becomes a local delta applied in place (caret and undo
//! kept), one straddling a boundary re-snaps the window to whole lines and
//! resets the card's text. The card that *made* the change is synced from
//! its own buffer instead of mapped, because a diff cannot tell an
//! insertion at the end of a window from one just past its newline.
//!
//! Only cards that have been scrolled into view own an `EditorState`; the
//! rest are data until they are, which bounds the cost of a thousand-match
//! search to the cards actually looked at.
//!
//! ## Held back
//!
//! Replace previews and references mode (a worker query), and the context
//! knob — the window is the ruled default and not yet tunable.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use brink_gpui_shell::tool_window::{TabSlot, ToolWindow};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    HighlightStyle, IntoElement, ListAlignment, ListState, Render, SharedString, Subscription,
    WeakEntity, Window, div, list, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, TabGroup};
use gpui_component::input::{
    Editor, EditorState, Input, InputEvent, InputState, TextDecoration, TextDecorationCollection,
};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use regex::{Regex, RegexBuilder};

use crate::document::{apply_delta, highlighter_factory};
use crate::icons;
use crate::project::{Project, ProjectEvent, SourceDelta};

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

/// One match and its card: a line-aligned window of the file, kept
/// pointed at the same lines as the file changes underneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub path: String,
    /// 1-based line of the hit's start, kept current.
    pub line: u32,
    /// The knot or stitch the match sat in when the search ran.
    pub container: Option<String>,
    /// 1-based line number of the window's first line, kept current.
    pub first_line: u32,
    /// The context window: a byte range of the file, starting at a line
    /// start and ending at a line end (no trailing newline).
    pub window: Range<usize>,
    /// The hit, in file bytes. Empty once an edit has run through it.
    pub hit: Range<usize>,
    /// The window's text as the search saw it.
    pub frozen: String,
    /// The window's text no longer reads as `frozen` — the `edited` badge.
    pub edited: bool,
}

/// What [`Match::map_edit`] asks the card's editor to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapped {
    /// The card's text is unchanged; only its position may have moved.
    Untouched,
    /// The change fell inside the window: apply this window-relative delta.
    Inside(SourceDelta),
    /// The window was re-snapped; reload the card's text from the file.
    Reset,
}

impl Match {
    /// The window's current text.
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(self.window.clone()).unwrap_or_default()
    }

    /// The hit relative to the window, when it still lies inside it.
    #[must_use]
    pub fn hit_local(&self) -> Option<Range<usize>> {
        let w = &self.window;
        (!self.hit.is_empty() && self.hit.start >= w.start && self.hit.end <= w.end)
            .then(|| self.hit.start - w.start..self.hit.end - w.start)
    }

    /// The current text of the line holding the hit, for a collapsed
    /// card's inline preview.
    #[must_use]
    pub fn preview(&self, source: &str) -> String {
        let text = self.text(source);
        let at = self.hit_local().map_or(0, |h| h.start).min(text.len());
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
        text[start..end].trim().to_owned()
    }

    /// Rows the card's editor draws — `split`, not `lines`, because the
    /// editor renders the empty row a trailing newline creates.
    #[must_use]
    pub fn rows(&self, source: &str) -> usize {
        self.text(source).split('\n').count().max(1)
    }

    /// Move the card through a change to its file. `source` is the text
    /// AFTER the change.
    pub fn map_edit(&mut self, delta: &SourceDelta, source: &str) -> Mapped {
        let r = delta.range.clone();
        let (ws, we) = (self.window.start, self.window.end);
        let (ins, rem) = (delta.inserted.len(), delta.removed.len());
        let shift = |offset: usize| offset + ins - rem;
        let pure_insertion = r.is_empty();

        let mapped = if r.end < ws || (r.end == ws && !pure_insertion) {
            // Wholly before: the window slides. A deletion that ends
            // exactly at the window's start took the newline before it,
            // which joins the previous line onto the window's first — the
            // snap below catches that and asks for a reset.
            let slid = shift(ws)..shift(we);
            let snapped = snap_to_lines(source, slid.clone());
            self.hit = map_range(&self.hit, delta);
            if snapped == slid {
                self.first_line = (i64::from(self.first_line) + newlines(&delta.inserted)
                    - newlines(&delta.removed))
                .max(1) as u32;
                self.window = slid;
                Mapped::Untouched
            } else {
                self.window = snapped;
                self.first_line = line_number_at(source, self.window.start);
                Mapped::Reset
            }
        } else if r.start > we || (r.start == we && !pure_insertion) {
            if r.start == we {
                // A deletion starting at the window's end took the newline
                // after it: the next line joined the window's last.
                self.window = snap_to_lines(source, ws..we);
                Mapped::Reset
            } else {
                Mapped::Untouched
            }
        } else if ws <= r.start && r.end <= we {
            // Inside — including an insertion at either edge.
            self.window = ws..shift(we);
            self.hit = map_range(&self.hit, delta);
            Mapped::Inside(SourceDelta {
                range: r.start - ws..r.end - ws,
                removed: delta.removed.clone(),
                inserted: delta.inserted.clone(),
            })
        } else {
            // Straddling a boundary: cover both sides, then whole lines.
            let start = ws.min(r.start);
            let end = if r.end >= we {
                r.start + ins
            } else {
                shift(we)
            };
            self.window = snap_to_lines(source, start..end);
            self.first_line = line_number_at(source, self.window.start);
            self.hit = map_range(&self.hit, delta);
            Mapped::Reset
        };
        self.refresh_derived(source);
        mapped
    }

    /// The card that made a change already holds the new text: take the
    /// window from its buffer rather than from the delta, whose position is
    /// ambiguous at the window's end (see the module doc).
    pub fn sync_to_own_edit(&mut self, text_len: usize, delta: &SourceDelta, source: &str) {
        self.window = self.window.start..self.window.start + text_len;
        self.hit = map_range(&self.hit, delta);
        self.refresh_derived(source);
    }

    fn refresh_derived(&mut self, source: &str) {
        let text = self.text(source);
        self.edited = text != self.frozen;
        if let Some(hit) = self.hit_local() {
            self.line = self.first_line + newlines(&text[..hit.start]) as u32;
        }
    }
}

/// Where `range` ends up after `delta`: shifted when the change is before
/// it, unchanged when after, collapsed to empty when the two overlap.
fn map_range(range: &Range<usize>, delta: &SourceDelta) -> Range<usize> {
    let r = &delta.range;
    let shift = delta.inserted.len() as i64 - delta.removed.len() as i64;
    if r.end <= range.start {
        let start = (range.start as i64 + shift).max(0) as usize;
        let end = (range.end as i64 + shift).max(0) as usize;
        start..end
    } else if r.start >= range.end {
        range.clone()
    } else {
        let at = range.start.min(r.start);
        at..at
    }
}

/// Widen a range to the start of the line it begins in and the end of the
/// line it ends in.
fn snap_to_lines(source: &str, range: Range<usize>) -> Range<usize> {
    let len = source.len();
    let start = range.start.min(len);
    let end = range.end.clamp(start, len);
    let start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let end = source[end..].find('\n').map_or(len, |i| end + i);
    start..end
}

fn newlines(text: &str) -> i64 {
    text.matches('\n').count() as i64
}

/// 1-based line number of the line containing `offset`.
fn line_number_at(source: &str, offset: usize) -> u32 {
    newlines(&source[..offset.min(source.len())]) as u32 + 1
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
            let window = starts[first]..line_end(source, &starts, last);
            let hit = found.start()..found.end().min(window.end);
            snapshot.matches.push(Match {
                path: path.to_owned(),
                line: line_ix as u32 + 1,
                container: container_at(source, found.start()),
                first_line: first as u32 + 1,
                frozen: source[window.clone()].to_owned(),
                window,
                hit,
                edited: false,
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

/// A card's editor, built the first time the card is on screen.
#[derive(Clone)]
struct CardEditor {
    state: Entity<EditorState>,
    /// The hit's highlight. The editor moves it through its own edits;
    /// only a wholesale reset needs it re-laid.
    hit: TextDecorationCollection,
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
    /// Editors for the cards that have been on screen, by card index.
    editors: HashMap<usize, CardEditor>,
    card_subs: Vec<Subscription>,
    list: ListState,
    /// The editor's real row height once one card has laid out — see
    /// `continuous.rs` for why the theme's number is only a first guess.
    measured_line_height: Option<f32>,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SearchEvent> for SearchView {}
impl EventEmitter<PanelEvent> for SearchView {}

const ROW_HEIGHT: f32 = 22.0;
/// See `continuous.rs`: gpui-component lays the editor out at 1.5× the
/// monospace size.
const LINE_HEIGHT_FACTOR: f32 = 1.5;
/// Zero vertical padding, so a card is exactly its rows.
const CARD_SIZE: gpui_component::Size = gpui_component::Size::XSmall;
const GUTTER_WIDTH: f32 = 36.0;

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
        // A theme or font-size change re-sizes every card and recolours
        // the band; the cards are rebuilt lazily, the snapshot kept.
        cx.observe_global::<gpui_component::Theme>(|this, cx| this.restyle(cx))
            .detach();
        // The snapshot never re-runs on a change; it follows it.
        let on_project = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| {
                if let ProjectEvent::SourceChanged {
                    path,
                    origin,
                    delta,
                } = event
                {
                    this.on_source_changed(path, *origin, delta, window, cx);
                }
            },
        );
        Self {
            project,
            query,
            options: SearchOptions::default(),
            snapshot: None,
            error: None,
            collapsed: BTreeSet::new(),
            editors: HashMap::new(),
            card_subs: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(300.)),
            measured_line_height: None,
            tab: TabSlot::default(),
            _subscriptions: vec![on_query, on_project],
        }
    }

    /// Drop every card's editor so the next paint rebuilds it against the
    /// current theme; the snapshot and the collapse state stay.
    fn restyle(&mut self, cx: &mut Context<Self>) {
        self.editors.clear();
        self.card_subs.clear();
        self.measured_line_height = None;
        let count = self.snapshot.as_ref().map_or(0, |s| s.matches.len());
        self.list.splice(0..count, count);
        cx.notify();
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
        self.editors.clear();
        self.card_subs.clear();
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
        let count = self.snapshot.as_ref().map_or(0, |s| s.matches.len());
        self.list = ListState::new(count, ListAlignment::Top, px(300.));
        cx.notify();
    }

    fn toggle_option(&mut self, apply: impl FnOnce(&mut SearchOptions), cx: &mut Context<Self>) {
        apply(&mut self.options);
        self.run(cx);
    }

    fn set_all_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        let count = self.snapshot.as_ref().map_or(0, |s| s.matches.len());
        self.collapsed = if collapsed {
            (0..count).collect()
        } else {
            BTreeSet::new()
        };
        self.list.splice(0..count, count);
        cx.notify();
    }

    fn toggle_collapsed(&mut self, card: usize, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&card) {
            self.collapsed.insert(card);
        }
        self.list.splice(card..card + 1, 1);
        cx.notify();
    }

    fn reveal(&self, card: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.snapshot.as_ref().and_then(|s| s.matches.get(card)) {
            let span = if m.hit.is_empty() {
                m.window.start..m.window.start
            } else {
                m.hit.clone()
            };
            cx.emit(SearchEvent::Reveal {
                path: m.path.clone(),
                span,
            });
        }
    }

    fn line_height(&self, cx: &App) -> f32 {
        self.measured_line_height
            .unwrap_or_else(|| f32::from(cx.theme().mono_font_size) * LINE_HEIGHT_FACTOR)
    }

    /// Adopt the real row height as soon as any card has laid out, and
    /// re-measure every card against it. Runs once per snapshot.
    fn adopt_measured_line_height(&mut self, cx: &mut Context<Self>) {
        if self.measured_line_height.is_some() {
            return;
        }
        let Some(real) = self
            .editors
            .values()
            .find_map(|editor| editor.state.read(cx).line_height())
            .map(f32::from)
        else {
            return;
        };
        self.measured_line_height = Some(real);
        self.list.remeasure();
        cx.notify();
    }

    /// The file changed — in a card, in a tab, in the manuscript. Move
    /// every card of that file, and update the buffers of the ones that
    /// have them.
    fn on_source_changed(
        &mut self,
        path: &str,
        origin: Option<gpui::EntityId>,
        delta: &SourceDelta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.snapshot.as_mut() else {
            return;
        };
        let source = self
            .project
            .read(cx)
            .loaded_source(path)
            .unwrap_or_default()
            .to_owned();
        let hit_style = hit_style(cx);
        let mut touched = Vec::new();
        for (ix, m) in snapshot.matches.iter_mut().enumerate() {
            if m.path != path {
                continue;
            }
            let editor = self.editors.get(&ix).cloned();
            let is_origin = editor
                .as_ref()
                .is_some_and(|e| Some(e.state.entity_id()) == origin);
            if is_origin {
                let len = editor
                    .as_ref()
                    .map_or(0, |e| e.state.read(cx).value().len());
                m.sync_to_own_edit(len, delta, &source);
            } else {
                match (m.map_edit(delta, &source), editor) {
                    (Mapped::Untouched, _) | (_, None) => {}
                    (Mapped::Inside(local), Some(editor)) => {
                        let fallback = m.text(&source).to_owned();
                        editor.state.update(cx, |state, cx| {
                            apply_delta(state, &local, &fallback, window, cx);
                        });
                    }
                    (Mapped::Reset, Some(editor)) => {
                        let text = m.text(&source).to_owned();
                        editor.state.update(cx, |state, cx| {
                            state.set_value(text, window, cx);
                        });
                        editor.hit.set(hit_decorations(m, hit_style), cx);
                    }
                }
            }
            touched.push(ix);
        }
        for ix in touched {
            self.list.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }

    /// A card's buffer changed under the author's hands: splice its window
    /// into the file. The broadcast that follows brings the window's end
    /// back in line with the buffer.
    fn on_card_edited(
        &mut self,
        card: usize,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.snapshot.as_ref().and_then(|s| s.matches.get(card)) else {
            return;
        };
        let new = editor.read(cx).value().to_string();
        let (path, window) = (m.path.clone(), m.window.clone());
        let full = {
            let project = self.project.read(cx);
            let source = project.loaded_source(&path).unwrap_or_default();
            if source.get(window.clone()) == Some(new.as_str()) {
                return;
            }
            let (Some(head), Some(tail)) = (source.get(..window.start), source.get(window.end..))
            else {
                return;
            };
            format!("{head}{new}{tail}")
        };
        let origin = editor.entity_id();
        self.project.update(cx, |project, cx| {
            project.edit(&path, full, Some(origin), cx);
        });
    }

    fn build_editor(
        &mut self,
        card: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<CardEditor> {
        let m = self.snapshot.as_ref()?.matches.get(card)?.clone();
        let text = self
            .project
            .read(cx)
            .loaded_source(&m.path)
            .map(|s| m.text(s).to_owned())
            .unwrap_or_default();
        let key: SharedString = m.path.clone().into();
        let weak: WeakEntity<Project> = self.project.downgrade();
        let decorations = hit_decorations(&m, hit_style(cx));
        let mut hit = None;
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(false)
                .language("brink")
                .soft_wrap(false)
                .scroll_beyond_last_line(Some(0));
            state.set_highlighter_factory(highlighter_factory(weak.clone(), key.clone()), cx);
            state.set_value(text, window, cx);
            hit = Some(state.create_decorations_collection(decorations, cx));
            state
        });
        let hit = hit?;
        self.card_subs.push(
            cx.subscribe(&state, move |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.on_card_edited(card, &state, cx);
                }
            }),
        );
        Some(CardEditor { state, hit })
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

    fn render_header(
        &self,
        card: usize,
        m: &Match,
        source: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, accent, hover, warning) = (
            theme.muted_foreground,
            theme.primary,
            theme.muted.opacity(0.5),
            theme.warning,
        );
        let collapsed = self.collapsed.contains(&card);
        let location = format!("{}:{}", m.path, m.line);
        let preview = collapsed.then(|| m.preview(source));
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
            .when(m.edited, |el| {
                el.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(warning.opacity(0.6))
                        .text_color(warning)
                        .child("edited"),
                )
            })
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

    fn render_card(
        &mut self,
        card: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(m) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.matches.get(card))
            .cloned()
        else {
            return div().into_any_element();
        };
        let source = self
            .project
            .read(cx)
            .loaded_source(&m.path)
            .unwrap_or_default()
            .to_owned();
        let header = self.render_header(card, &m, &source, cx);
        if self.collapsed.contains(&card) {
            return v_flex().w_full().child(header).into_any_element();
        }
        let editor = match self.editors.get(&card) {
            Some(editor) => Some(editor.clone()),
            None => {
                let built = self.build_editor(card, window, cx);
                if let Some(built) = &built {
                    self.editors.insert(card, built.clone());
                }
                built
            }
        };
        let Some(editor) = editor else {
            return v_flex().w_full().child(header).into_any_element();
        };
        let theme = cx.theme();
        let line_height = self.line_height(cx);
        let rows = m.rows(&source);
        let height = rows as f32 * line_height;
        let gutter = v_flex()
            .w(px(GUTTER_WIDTH))
            .flex_shrink_0()
            .pr_2()
            .items_end()
            .font_family(theme.mono_font_family.clone())
            .text_size(theme.mono_font_size)
            .text_color(theme.muted_foreground)
            .children((0..rows).map(|row| {
                div()
                    .h(px(line_height))
                    .flex()
                    .items_center()
                    .child((m.first_line as usize + row).to_string())
            }));
        v_flex()
            .w_full()
            .pb_1()
            .child(header)
            .child(
                h_flex().w_full().pl_4().items_start().child(gutter).child(
                    Editor::new(&editor.state)
                        .bordered(false)
                        .appearance(false)
                        .with_size(CARD_SIZE)
                        .h(px(height))
                        .flex_1()
                        .min_w_0(),
                ),
            )
            .into_any_element()
    }
}

fn hit_style(cx: &App) -> HighlightStyle {
    HighlightStyle {
        background_color: Some(cx.theme().warning.opacity(0.35)),
        ..Default::default()
    }
}

fn hit_decorations(m: &Match, style: HighlightStyle) -> Vec<TextDecoration> {
    m.hit_local()
        .map(|hit| TextDecoration::new(hit, style))
        .into_iter()
        .collect()
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
        self.adopt_measured_line_height(cx);
        let theme = cx.theme();
        let (muted, danger) = (theme.muted_foreground, theme.danger);
        let summary = self.render_summary(cx);
        let count = self.snapshot.as_ref().map_or(0, |s| s.matches.len());
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
                    list(
                        self.list.clone(),
                        cx.processor(|this, card: usize, window, cx| {
                            this.render_card(card, window, cx)
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

    fn delta(range: Range<usize>, removed: &str, inserted: &str) -> SourceDelta {
        SourceDelta {
            range,
            removed: removed.to_owned(),
            inserted: inserted.to_owned(),
        }
    }

    /// Apply `delta` to `source` the way the project does, returning the
    /// new text.
    fn apply(source: &str, delta: &SourceDelta) -> String {
        assert_eq!(&source[delta.range.clone()], delta.removed);
        format!(
            "{}{}{}",
            &source[..delta.range.start],
            delta.inserted,
            &source[delta.range.end..]
        )
    }

    fn one(query: &str, source: &str) -> Match {
        let p = build_pattern(query, opts(false, false, false)).unwrap();
        let s = search([("story.ink", source)], &p, CONTEXT, RESULT_CAP);
        assert_eq!(s.matches.len(), 1, "{query:?} should match once");
        s.matches.into_iter().next().unwrap()
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
    fn a_match_carries_its_window_and_container() {
        let m = one("went", INK);
        assert_eq!(m.line, 8);
        assert_eq!(m.container.as_deref(), Some("left"));
        // One line above, two below — clamped to the file's end.
        assert_eq!(m.first_line, 7);
        assert_eq!(m.text(INK), "=== left ===\nYou went left.\nhello again");
        assert_eq!(m.frozen, m.text(INK));
        assert_eq!(&INK[m.hit.clone()], "went");
        assert_eq!(m.hit_local(), Some(17..21));
        assert_eq!(m.preview(INK), "You went left.");
        assert_eq!(m.rows(INK), 3);
        assert!(!m.edited);
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
    fn an_edit_before_the_window_slides_it() {
        let mut m = one("went", INK);
        let before = m.clone();
        // Two lines added at the top of the file.
        let d = delta(0..0, "", "// intro\n// more\n");
        let after = apply(INK, &d);
        assert_eq!(m.map_edit(&d, &after), Mapped::Untouched);
        assert_eq!(m.text(&after), before.text(INK));
        assert_eq!(m.first_line, before.first_line + 2);
        assert_eq!(m.line, before.line + 2);
        assert_eq!(&after[m.hit.clone()], "went");
        assert!(!m.edited);
        // And back again.
        let d = delta(0..17, "// intro\n// more\n", "");
        let back = apply(&after, &d);
        assert_eq!(m.map_edit(&d, &back), Mapped::Untouched);
        assert_eq!(m, before);
    }

    #[test]
    fn an_edit_inside_the_window_is_a_local_delta() {
        let mut m = one("went", INK);
        let ws = m.window.start;
        // Type " really" after "You" on the hit line, from another editor.
        let at = INK.find("You").unwrap() + 3;
        let d = delta(at..at, "", " really");
        let after = apply(INK, &d);
        assert_eq!(
            m.map_edit(&d, &after),
            Mapped::Inside(delta(at - ws..at - ws, "", " really"))
        );
        assert_eq!(
            m.text(&after),
            "=== left ===\nYou really went left.\nhello again"
        );
        assert_eq!(&after[m.hit.clone()], "went");
        assert_eq!(m.line, 8);
        assert!(m.edited, "the window no longer reads as it did");
        assert_eq!(m.preview(&after), "You really went left.");
        // An insertion at the very end of the window grows it.
        let end = m.window.end;
        let d = delta(end..end, "", "\nand again");
        let after2 = apply(&after, &d);
        assert!(matches!(m.map_edit(&d, &after2), Mapped::Inside(_)));
        assert_eq!(m.rows(&after2), 4);
        assert!(m.text(&after2).ends_with("hello again\nand again"));
    }

    #[test]
    fn an_edit_through_the_hit_empties_it() {
        let mut m = one("went", INK);
        let at = INK.find("went").unwrap();
        let d = delta(at..at + 4, "went", "turned");
        let after = apply(INK, &d);
        assert!(matches!(m.map_edit(&d, &after), Mapped::Inside(_)));
        assert!(m.hit.is_empty());
        assert_eq!(m.hit_local(), None);
        assert!(m.edited);
        assert_eq!(m.preview(&after), "=== left ===", "no hit: the first line");
    }

    #[test]
    fn an_edit_across_the_boundary_resnaps_to_lines() {
        let mut m = one("went", INK);
        // Delete from inside the previous line into the window's first line:
        // "\n\n=== left ===" -> "\n== left ===" (the blank line and the
        // knot header's first `=` go).
        let start = INK.find("\n\n=== left").unwrap() + 1;
        let d = delta(start..start + 2, "\n=", "");
        let after = apply(INK, &d);
        assert_eq!(m.map_edit(&d, &after), Mapped::Reset);
        assert_eq!(m.text(&after), "== left ===\nYou went left.\nhello again");
        assert_eq!(m.first_line, 6);
        assert_eq!(&after[m.hit.clone()], "went");
        assert_eq!(m.line, 7);
        assert!(m.edited);
        // Deleting the newline that closes the window joins the next line
        // onto it, which is a reset too.
        let mut m = one("Hello there", INK);
        assert_eq!(
            m.text(INK),
            "=== start ===\nHello there.\n* [Go left] -> left\n= inner"
        );
        let end = m.window.end;
        let d = delta(end..end + 1, "\n", "");
        let after = apply(INK, &d);
        assert_eq!(m.map_edit(&d, &after), Mapped::Reset);
        assert!(m.text(&after).ends_with("= innerstill here"));
    }

    #[test]
    fn an_edit_after_the_window_is_nothing_to_it() {
        let mut m = one("Hello there", INK);
        let before = m.clone();
        let at = INK.find("hello again").unwrap();
        let d = delta(at..at + 5, "hello", "HELLO");
        let after = apply(INK, &d);
        assert_eq!(m.map_edit(&d, &after), Mapped::Untouched);
        assert_eq!(m, before);
    }

    #[test]
    fn the_editing_card_takes_its_window_from_its_own_buffer() {
        // The author adds a newline at the end of the card. The diff cannot
        // tell that from a newline after the window's own newline, and
        // places the change one byte past the window.
        let mut m = one("Hello there", INK);
        let end = m.window.end;
        let new_text = format!("{}\n", m.text(INK));
        let full = format!("{}{}{}", &INK[..end], "\n", &INK[end..]);
        let d = crate::project::diff(INK, &full).unwrap();
        assert!(d.range.start > end, "the ambiguity this test is about");
        m.sync_to_own_edit(new_text.len(), &d, &full);
        assert_eq!(m.text(&full), new_text);
        assert!(m.edited);
        assert_eq!(&full[m.hit.clone()], "Hello there");
    }
}
