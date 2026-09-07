//! The project entity — the UI's mirror of the worker's analysis.
//!
//! An `Entity` with observers, not an `Rc<RefCell<_>>`. The spike used the
//! latter and had to call `rebuild()` by hand everywhere, because a shared
//! cell has no way to tell anyone it changed. Panels observe this instead
//! and re-render themselves.
//!
//! Nothing here holds an `IdeSession`, a `ProjectDb`, or anything borrowing
//! either: the session lives on the worker thread, and what arrives is plain
//! data (`docs/gpui-studio-spec.md` §3.3).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

use anyhow::Result;
use brink_gpui_model::play::{PlayCommand, PlayOutcome};
use brink_gpui_model::query::{QueryKind, QueryResult};
use brink_gpui_model::worker::{Diagnostic, DraftGlob, Kinds, Request, Response, Worker};
use gpui::{App, AppContext as _, Context, EntityId, EventEmitter, Task};

/// What the UI learns from the worker.
#[derive(Debug, Clone)]
pub enum ProjectEvent {
    /// A project finished loading. Carries how long it took.
    Opened {
        elapsed_ms: f64,
    },
    OpenFailed(String),
    /// Fresh analysis landed — diagnostics, kinds and drafts all moved.
    Analyzed,
    /// A file's text changed. Every editor over `path` other than `origin`
    /// applies `delta` to its own buffer, so all of them show one text —
    /// the shared buffer `docs/gpui-studio-spec.md` §6 asks for, with the
    /// mirror as the canonical copy and each `EditorState` a view of it.
    SourceChanged {
        path: String,
        /// The editor the change came from, which already holds it.
        origin: Option<EntityId>,
        delta: SourceDelta,
    },
    /// Dirty files were written to disk.
    Saved,
    /// A dirty file could NOT be written. Nothing else reports this: the
    /// editor keeps the text, so the only sign a save failed is this event
    /// (and the Output row it becomes). It used to go to stderr, where a
    /// windowed studio has no reader.
    SaveFailed {
        path: String,
        message: String,
    },
}

/// One contiguous replacement in a file's text — what a keystroke is, and
/// what any edit reduces to between its unchanged head and tail. `range`
/// and `removed` describe the OLD text; `inserted` is what replaced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDelta {
    pub range: Range<usize>,
    pub removed: String,
    pub inserted: String,
}

/// The smallest single replacement turning `old` into `new`, or `None`
/// when they are equal. Common head and tail are trimmed bytewise and then
/// widened to char boundaries, so a change inside a multi-byte character
/// never splits it.
#[must_use]
pub fn diff(old: &str, new: &str) -> Option<SourceDelta> {
    if old == new {
        return None;
    }
    let (ob, nb) = (old.as_bytes(), new.as_bytes());
    let mut start = ob.iter().zip(nb).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) {
        start -= 1;
    }
    let limit = old.len().min(new.len()) - start;
    let mut tail = ob[start..]
        .iter()
        .rev()
        .zip(nb[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(limit);
    while !old.is_char_boundary(old.len() - tail) || !new.is_char_boundary(new.len() - tail) {
        tail -= 1;
    }
    let range = start..old.len() - tail;
    Some(SourceDelta {
        removed: old[range.clone()].to_owned(),
        inserted: new[start..new.len() - tail].to_owned(),
        range,
    })
}

/// The mirror.
pub struct Project {
    worker: Worker,
    root: PathBuf,
    files: Vec<String>,
    /// The project's `brink.toml`, root-relative, if it has one. Held in
    /// `sources`/`saved` like any file — edited through [`Project::edit`],
    /// dirty per file, written by `save_all` — but never in `files`: it is
    /// not a source, and the manuscript and search read `files`.
    config: Option<String>,
    /// The canonical text of every file — what each editor over the file
    /// mirrors, and what is analysed, searched and saved. An editor pushes
    /// its text through [`Project::edit`]; the others hear the delta.
    sources: BTreeMap<String, String>,
    /// The text on disk, as of load or the last save. Dirty is the
    /// difference — a per-file fact, not a per-editor one, so an edit made
    /// in the manuscript is as unsaved as one made in a Code view tab.
    saved: BTreeMap<String, String>,
    entry: Option<String>,
    drafts: BTreeSet<String>,
    /// Per-glob attribution for `[project] drafts`, from the last analysis.
    draft_globs: Vec<DraftGlob>,
    drafts_known: bool,
    /// The resolved `[dialogue]` dialect, from the last analysis.
    dialogue: Option<brink_ir::DialogueDialect>,
    dialogue_error: Option<String>,
    closure: BTreeSet<String>,
    diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    kinds: BTreeMap<String, Kinds>,
    warnings: Vec<String>,
    /// Whether any analysis has landed. Distinct from the closure being
    /// non-empty, which stays false whenever `brink.toml` names no entry
    /// however many times the project has analyzed — reading one for the
    /// other is how Problems said "Not analyzed yet." forever.
    analyzed: bool,
    revision: u64,
    last_analyze_ms: f64,
    worst_analyze_ms: f64,
    /// The pump draining the worker's responses. Dropping it stops the pump,
    /// so it is held for its lifetime, not its value.
    _pump: Task<()>,
    empty_kinds: Kinds,
}

impl EventEmitter<ProjectEvent> for Project {}

impl Project {
    /// Start the worker and the pump that folds its answers into this
    /// entity.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let worker = Worker::spawn();
        let responses = worker.responses();
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(response) = responses.recv().await {
                if this
                    .update(cx, |project, cx| project.apply(response, cx))
                    .is_err()
                {
                    // The entity is gone; so is the reason to keep pumping.
                    break;
                }
            }
        });
        Self {
            worker,
            root: PathBuf::new(),
            files: Vec::new(),
            config: None,
            sources: BTreeMap::new(),
            saved: BTreeMap::new(),
            entry: None,
            drafts: BTreeSet::new(),
            draft_globs: Vec::new(),
            drafts_known: false,
            dialogue: None,
            dialogue_error: None,
            closure: BTreeSet::new(),
            diagnostics: BTreeMap::new(),
            kinds: BTreeMap::new(),
            warnings: Vec::new(),
            analyzed: false,
            revision: 0,
            last_analyze_ms: 0.0,
            worst_analyze_ms: 0.0,
            _pump: pump,
            empty_kinds: Kinds::new(),
        }
    }

    fn apply(&mut self, response: Response, cx: &mut Context<Self>) {
        match response {
            Response::Opened(opened) => match *opened {
                Ok(opened) => {
                    self.root = opened.root;
                    self.sources = opened.files.iter().cloned().zip(opened.sources).collect();
                    self.config = opened.config.as_ref().map(|c| c.path.clone());
                    if let Some(config) = opened.config {
                        self.sources.insert(config.path, config.text);
                    }
                    self.saved = self.sources.clone();
                    self.files = opened.files;
                    self.entry = opened.entry;
                    self.warnings = opened.warnings;
                    // A new project invalidates everything keyed by path.
                    self.diagnostics.clear();
                    self.kinds.clear();
                    self.drafts.clear();
                    self.draft_globs.clear();
                    self.drafts_known = false;
                    self.closure.clear();
                    self.analyzed = false;
                    cx.emit(ProjectEvent::Opened {
                        elapsed_ms: opened.elapsed_ms,
                    });
                }
                Err(message) => cx.emit(ProjectEvent::OpenFailed(message)),
            },
            Response::Analyzed(analyzed) => {
                self.diagnostics = analyzed.diagnostics;
                self.kinds = analyzed.kinds;
                self.drafts = analyzed.drafts.into_iter().collect();
                self.draft_globs = analyzed.draft_globs;
                self.drafts_known = analyzed.drafts_known;
                self.dialogue = analyzed.dialogue;
                self.dialogue_error = analyzed.dialogue_error;
                self.closure = analyzed.closure.into_iter().collect();
                // The config can move the entry between analyses.
                self.entry = analyzed.entry;
                self.warnings = analyzed.config_warnings;
                self.analyzed = true;
                self.last_analyze_ms = analyzed.elapsed_ms;
                self.worst_analyze_ms = self.worst_analyze_ms.max(analyzed.elapsed_ms);
                cx.emit(ProjectEvent::Analyzed);
            }
        }
        cx.notify();
    }

    pub fn open(&mut self, root: PathBuf) {
        self.worker.send(Request::Open { root });
    }

    /// An editor's new text for a file. Returns whether anything changed.
    ///
    /// The mirror takes the text as canonical, tells the worker (the
    /// analysis arrives later as [`ProjectEvent::Analyzed`]; nothing waits
    /// for it), and broadcasts the delta so every other editor over the
    /// file follows. Identical text is a no-op — which is what makes the
    /// broadcast safe: an editor applying a delta re-reports the same text,
    /// and that echo stops here.
    pub fn edit(
        &mut self,
        path: &str,
        text: String,
        origin: Option<EntityId>,
        cx: &mut Context<Self>,
    ) -> bool {
        let old = self
            .sources
            .get(path)
            .map(String::as_str)
            .unwrap_or_default();
        let Some(delta) = diff(old, &text) else {
            return false;
        };
        self.sources.insert(path.to_owned(), text.clone());
        self.revision += 1;
        self.worker.send(Request::Edit {
            path: path.to_owned(),
            text,
            revision: self.revision,
        });
        cx.emit(ProjectEvent::SourceChanged {
            path: path.to_owned(),
            origin,
            delta,
        });
        cx.notify();
        true
    }

    /// Apply byte-range edits to the files they name, last-to-first within
    /// each file so earlier offsets stay valid, then push each rewritten
    /// file through [`Project::edit`] with no origin — so every editor
    /// showing it, tab or manuscript section, follows the delta. Returns
    /// the number of files that actually changed.
    ///
    /// Edits are in bytes of the text as it was when the plan was
    /// computed; the plan is computed against the same sources this holds,
    /// so an edit that no longer fits (the text moved underneath it) is
    /// skipped rather than applied somewhere wrong.
    pub fn apply_edits(
        &mut self,
        edits: &[brink_gpui_model::query::TextEdit],
        cx: &mut Context<Self>,
    ) -> usize {
        let mut by_file: BTreeMap<&str, Vec<&brink_gpui_model::query::TextEdit>> = BTreeMap::new();
        for e in edits {
            by_file.entry(e.path.as_str()).or_default().push(e);
        }
        let mut changed = 0;
        for (path, mut file_edits) in by_file {
            let Some(mut text) = self.sources.get(path).cloned() else {
                continue;
            };
            file_edits.sort_by_key(|e| std::cmp::Reverse(e.start));
            for e in file_edits {
                let (start, end) = (e.start as usize, e.end as usize);
                if start <= end
                    && end <= text.len()
                    && text.is_char_boundary(start)
                    && text.is_char_boundary(end)
                {
                    text.replace_range(start..end, &e.new_text);
                }
            }
            if self.edit(path, text, None, cx) {
                changed += 1;
            }
        }
        changed
    }

    /// Every file whose text differs from what is on disk, sorted.
    #[must_use]
    pub fn dirty_paths(&self) -> Vec<String> {
        self.sources
            .iter()
            .filter(|(path, text)| self.saved.get(*path) != Some(text))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Whether a file's text differs from what is on disk.
    #[must_use]
    pub fn is_dirty(&self, path: &str) -> bool {
        self.sources.get(path) != self.saved.get(path)
    }

    /// Write every dirty file, relative to the root. Each failure is
    /// returned with its path; the others are still written.
    pub fn save_all(&mut self, cx: &mut Context<Self>) -> Vec<(String, std::io::Error)> {
        let mut failures = Vec::new();
        let mut wrote = false;
        for (path, text) in &self.sources {
            if self.saved.get(path) == Some(text) {
                continue;
            }
            match std::fs::write(self.root.join(path), text) {
                Ok(()) => {
                    self.saved.insert(path.clone(), text.clone());
                    wrote = true;
                }
                Err(err) => failures.push((path.clone(), err)),
            }
        }
        for (path, err) in &failures {
            cx.emit(ProjectEvent::SaveFailed {
                path: path.clone(),
                message: format!("{err}"),
            });
        }
        if wrote {
            cx.emit(ProjectEvent::Saved);
            cx.notify();
        }
        failures
    }

    /// Ask the worker a question. The returned task resolves when the worker
    /// gets to it — after any edit already queued, so the answer is never
    /// against staler text than the caller has.
    pub fn query(&self, kind: QueryKind, cx: &App) -> Task<Result<QueryResult>> {
        let (reply, answer) = async_channel::bounded(1);
        self.worker.send(Request::Query { kind, reply });
        cx.background_spawn(async move { Ok(answer.recv().await?) })
    }

    /// Drive the play session. Same ordering rule as [`Project::query`]:
    /// a start compiles the text every queued edit has already produced.
    pub fn play(&self, command: PlayCommand, cx: &App) -> Task<Result<PlayOutcome>> {
        let (reply, answer) = async_channel::bounded(1);
        self.worker.send(Request::Play { command, reply });
        cx.background_spawn(async move { Ok(answer.recv().await?) })
    }

    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    #[must_use]
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// A file's canonical text, as the editors currently hold it. `None`
    /// for a path the project never held.
    #[must_use]
    pub fn loaded_source(&self, path: &str) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }

    #[must_use]
    pub fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
    }

    /// The project's `brink.toml`, root-relative, if it has one. Its text
    /// is [`Project::loaded_source`]; edits go through [`Project::edit`].
    #[must_use]
    pub fn config_path(&self) -> Option<&str> {
        self.config.as_deref()
    }

    /// Whether `path` is the project's config file.
    #[must_use]
    pub fn is_config(&self, path: &str) -> bool {
        self.config.as_deref() == Some(path)
    }

    /// The project's resolved `[dialogue]` dialect, and why it failed to
    /// resolve if it did — `(None, None)` for a project that declares none.
    #[must_use]
    pub fn dialogue(&self) -> (Option<&brink_ir::DialogueDialect>, Option<&str>) {
        (self.dialogue.as_ref(), self.dialogue_error.as_deref())
    }

    /// `[project] drafts`, glob by glob, with what each currently matches
    /// — and whether that is known yet (false before a compile closure
    /// exists, when every list is empty and means nothing).
    #[must_use]
    pub fn draft_globs(&self) -> (&[DraftGlob], bool) {
        (&self.draft_globs, self.drafts_known)
    }

    #[must_use]
    pub fn is_draft(&self, path: &str) -> bool {
        self.drafts.contains(path)
    }

    /// Whether the story actually reaches this file. False for everything
    /// until the first analysis, and for a project with no `[project] entry`
    /// — in both cases nothing is known rather than nothing being reachable.
    #[must_use]
    pub fn in_story(&self, path: &str) -> bool {
        self.closure.contains(path)
    }

    #[must_use]
    pub fn has_analyzed(&self) -> bool {
        self.analyzed
    }

    /// `(path, offset, is_error)` for every error and warning, as the Binder
    /// wants them for its per-row marks. Info and hint tiers are dropped:
    /// the Binder shows two mark colours, so a third tier would have to be
    /// mapped onto one of them and would read as a false severity.
    #[must_use]
    pub fn diagnostic_points(&self) -> Vec<(String, usize, bool)> {
        let mut out = Vec::new();
        for (path, found) in &self.diagnostics {
            for d in found {
                let is_error = match d.severity {
                    brink_ir::Severity::Error => true,
                    brink_ir::Severity::Warning => false,
                    _ => continue,
                };
                out.push((path.clone(), d.start as usize, is_error));
            }
        }
        out
    }

    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    #[must_use]
    pub fn diagnostics_for(&self, path: &str) -> &[Diagnostic] {
        self.diagnostics.get(path).map_or(&[], Vec::as_slice)
    }

    /// Every diagnostic in the project, path-keyed, for the Problems panel.
    pub fn all_diagnostics(&self) -> impl Iterator<Item = (&String, &Vec<Diagnostic>)> {
        self.diagnostics.iter()
    }

    #[must_use]
    pub fn problem_count(&self) -> usize {
        self.diagnostics.values().map(Vec::len).sum()
    }

    /// The identity join for one file — the only thing the UI renders that
    /// is allowed to lag, and lag costs refinement alone.
    #[must_use]
    pub fn kinds_for(&self, path: &str) -> &Kinds {
        self.kinds.get(path).unwrap_or(&self.empty_kinds)
    }

    #[must_use]
    pub fn timings(&self) -> (f64, f64) {
        (self.last_analyze_ms, self.worst_analyze_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(old: &str, new: &str) -> SourceDelta {
        diff(old, new).expect("texts differ")
    }

    #[test]
    fn a_keystroke_is_one_insertion() {
        let d = delta("hello world", "hello, world");
        assert_eq!(d.range, 5..5);
        assert_eq!(d.removed, "");
        assert_eq!(d.inserted, ",");
    }

    #[test]
    fn deletions_and_replacements_keep_the_unchanged_head_and_tail() {
        // The head and tail are trimmed BYTEWISE, so a delta need not fall
        // on word boundaries: "one two three" -> "one three" shares "one t"
        // and "hree", leaving "wo t" removed. Any such delta is correct as
        // long as applying it reproduces the new text.
        let d = delta("one two three", "one three");
        assert_eq!(d.removed.len(), 4);
        assert_eq!(d.inserted, "");
        let d = delta("one two three", "one 2 three");
        assert_eq!(
            (d.range.clone(), d.removed.as_str(), d.inserted.as_str()),
            (4..7, "two", "2")
        );
        // Two far-apart edits collapse to the one span covering both:
        // coarser than two deltas, never wrong.
        let d = delta("aXbYc", "a1bYc2");
        assert_eq!(&"aXbYc"[d.range.clone()], "XbYc");
        assert_eq!(d.inserted, "1bYc2");
    }

    #[test]
    fn identical_text_is_no_delta_and_overlap_is_handled() {
        assert_eq!(diff("same", "same"), None);
        // Head and tail would overlap on "aa" vs "aaa"; the delta is the one
        // extra character.
        let d = delta("aa", "aaa");
        assert_eq!(d.removed, "");
        assert_eq!(d.inserted, "a");
        let d = delta("", "new");
        assert_eq!((d.range.clone(), d.inserted.as_str()), (0..0, "new"));
        let d = delta("gone", "");
        assert_eq!((d.range.clone(), d.removed.as_str()), (0..4, "gone"));
    }

    #[test]
    fn a_change_inside_a_multibyte_character_never_splits_it() {
        // "é" is C3 A9, "è" is C3 A8: the bytes share a prefix.
        let d = delta("café", "cafè");
        assert_eq!(d.removed, "é");
        assert_eq!(d.inserted, "è");
        assert!("café".is_char_boundary(d.range.start));
        assert!("café".is_char_boundary(d.range.end));
    }

    #[test]
    fn applying_the_delta_reproduces_the_new_text() {
        for (old, new) in [
            ("abc", "abXc"),
            ("héllo wörld", "héllo, wörld!"),
            ("line1\nline2\n", "line1\nLINE2\n"),
            ("", "x"),
            ("x", ""),
        ] {
            let d = delta(old, new);
            let mut applied = old.to_owned();
            applied.replace_range(d.range.clone(), &d.inserted);
            assert_eq!(applied, new, "{old:?} -> {new:?}");
            assert_eq!(&old[d.range.clone()], d.removed);
        }
    }
}
