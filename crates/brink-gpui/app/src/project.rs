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
use brink_gpui_model::binder_order::{self, BinderOrder};
use brink_gpui_model::cues::CueLine;
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
    /// A file's prose lints moved. Problems lists them; nothing else does.
    ProseChanged,
    /// The disk moved under the project. The changes are already applied
    /// — this is what the studio should SAY about them.
    DiskChanged(Vec<DiskReport>),
    /// A breakpoint was marked, cleared, or found to bind to nothing.
    /// Every editor over the file repaints its marks.
    BreakpointsChanged,
    /// The set of files changed — one was created, renamed or deleted.
    /// Every surface keyed by path (the Binder, Search, the manuscript)
    /// rebuilds; an analysis follows on its own.
    FilesChanged,
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
    /// Files the config points at — `dialect.json` and any sibling. Held
    /// in `sources` like the config, listed in the Binder, and never in
    /// `files`: the manuscript and Search read `files`, and an artifact
    /// is not part of the story's text.
    artifacts: Vec<String>,
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
    /// Dialect-classified lines per file, from the last analysis — what
    /// the highlighter paints a cue, a parenthetical and a dialogue run
    /// from. Empty for a project with no `[dialogue]` dialect.
    cues: BTreeMap<String, Vec<CueLine>>,
    /// Prose lints per OPEN file, reported by the document that computed
    /// them. Kept apart from `diagnostics`, which is the analysis's:
    /// merging them would double-mark the editor (which lays its own) and
    /// would count prose in the "N problems" the status bar means by
    /// compiler problems.
    prose: BTreeMap<String, Vec<Diagnostic>>,
    /// The file operations the Binder has run, newest last — what
    /// `undo_file_op` inverts. Bounded: this is a way back out of the
    /// last mistake, not a history of the session.
    file_ops: Vec<FileOp>,
    /// Files whose disk text has moved under an unsaved buffer, and what
    /// the disk said when it was reported. Kept so the same conflict is
    /// announced once rather than once per filesystem event.
    conflicted: BTreeMap<String, String>,
    /// The breakpoints the author has marked, as `(path, 1-based line)`.
    /// The PROJECT owns them, not any one editor: the marks outlive a
    /// closed tab and a restarted session, and the worker arms whatever
    /// this holds at the next Start.
    breakpoints: BTreeSet<(String, u32)>,
    /// Marked lines the last arming could not bind — a comment, a blank,
    /// or code that folded away. Drawn differently, so a mark that can
    /// never hit says so instead of looking armed.
    unbound: BTreeSet<(String, u32)>,
    warnings: Vec<String>,
    /// Whether any analysis has landed. Distinct from the closure being
    /// non-empty, which stays false whenever `brink.toml` names no entry
    /// however many times the project has analyzed — reading one for the
    /// other is how Problems said "Not analyzed yet." forever.
    analyzed: bool,
    revision: u64,
    last_analyze_ms: f64,
    worst_analyze_ms: f64,
    /// The authored Binder order, from `.binder.json` beside the project
    /// (`brink_gpui_model::binder_order`). The Project owns it because
    /// the Project owns the disk: a rename has to re-key it and a delete
    /// has to drop it, wherever the operation was asked for.
    binder_order: BinderOrder,
    /// The pump draining the worker's responses. Dropping it stops the pump,
    /// so it is held for its lifetime, not its value.
    _pump: Task<()>,
    empty_kinds: Kinds,
}

/// A file operation, kept so it can be undone. A create is undone by a
/// delete, a rename by the opposite rename, and a delete by writing back
/// the text it had — which is why the text is kept here and nowhere
/// else: once the file is gone, this is the only copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOp {
    Created { path: String },
    Renamed { from: String, to: String },
    Deleted { path: String, text: String },
}

impl FileOp {
    /// How the undo names itself, in the studio's own vocabulary.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Created { path } => format!("creating {path}"),
            Self::Renamed { from, to } => format!("renaming {from} to {to}"),
            Self::Deleted { path, .. } => format!("deleting {path}"),
        }
    }
}

/// How many operations back the Binder can go.
const UNDO_DEPTH: usize = 20;

/// What the studio should SAY about a disk change. The Project applies
/// the change; saying so is the studio's, which owns the notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskReport {
    /// Taken from disk into a clean buffer.
    Reloaded(String),
    /// Changed on disk under unsaved edits; the buffer was kept.
    Conflicted(String),
    Vanished {
        path: String,
        dirty: bool,
    },
    Appeared(String),
}

/// A root-relative path a file operation will accept.
///
/// Refused: an absolute path, anything with a `..` segment, a trailing
/// slash, an empty name. A project's files live under its root, and a
/// path that climbs out of it would write somewhere the author cannot
/// see from the Binder.
fn normalise_path(path: &str) -> Result<String> {
    let path = path.trim().trim_start_matches("./");
    if path.is_empty() {
        anyhow::bail!("a file needs a name");
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains(':') {
        anyhow::bail!("{path} is not inside the project");
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        anyhow::bail!("{path} is not a path inside the project");
    }
    Ok(parts.join("/"))
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
            binder_order: BinderOrder::default(),
            artifacts: Vec::new(),
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
            cues: BTreeMap::new(),
            prose: BTreeMap::new(),
            conflicted: BTreeMap::new(),
            file_ops: Vec::new(),
            breakpoints: BTreeSet::new(),
            unbound: BTreeSet::new(),
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
                    self.artifacts = opened.artifacts.iter().map(|(p, _)| p.clone()).collect();
                    for (path, text) in opened.artifacts {
                        self.sources.insert(path, text);
                    }
                    self.saved = self.sources.clone();
                    self.files = opened.files;
                    self.entry = opened.entry;
                    // Read beside the project, never through the session:
                    // `.json` is not a source, and this is presentation.
                    self.binder_order = std::fs::read_to_string(self.root.join(binder_order::PATH))
                        .map(|text| binder_order::parse(&text))
                        .unwrap_or_default();
                    self.warnings = opened.warnings;
                    // A new project invalidates everything keyed by path.
                    self.diagnostics.clear();
                    self.kinds.clear();
                    self.cues.clear();
                    self.prose.clear();
                    self.conflicted.clear();
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
                self.cues = analyzed.cues;
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
        // A mounted library file is not the author's: it is openable and
        // read-only, and nothing it emits may reach the mirror. Without
        // this the editor's first Change put the text into `sources` and
        // not `saved`, and the tab came up marked unsaved.
        if self.is_library(path) {
            return false;
        }
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
        let mut written: Vec<String> = Vec::new();
        for (path, text) in &self.sources {
            if self.saved.get(path) == Some(text) {
                continue;
            }
            match std::fs::write(self.root.join(path), text) {
                Ok(()) => {
                    self.saved.insert(path.clone(), text.clone());
                    written.push(path.clone());
                    wrote = true;
                }
                Err(err) => failures.push((path.clone(), err)),
            }
        }
        // The save settles the argument: this text IS the disk now, so a
        // conflict reported before it is over.
        for path in &written {
            self.conflicted.remove(path);
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

    /// Create `path` (root-relative) with `text`, on disk and in the
    /// session. Answers what went wrong, if anything.
    ///
    /// The file is written straight away rather than left dirty: a file
    /// that exists in the Binder and not on disk is a file the next
    /// `INCLUDE` cannot find, and the author has no way to tell.
    pub fn create_file(&mut self, path: &str, text: &str, cx: &mut Context<Self>) -> Result<()> {
        let path = normalise_path(path)?;
        if self.sources.contains_key(&path) {
            anyhow::bail!("{path} is already in the project");
        }
        let full = self.root.join(&path);
        if full.exists() {
            anyhow::bail!("{path} already exists on disk");
        }
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, text)?;
        self.sources.insert(path.clone(), text.to_owned());
        self.saved.insert(path.clone(), text.to_owned());
        self.files.push(path.clone());
        self.files.sort();
        self.worker.send(Request::AddFile {
            path: path.clone(),
            text: text.to_owned(),
        });
        self.remember(FileOp::Created { path });
        cx.emit(ProjectEvent::FilesChanged);
        cx.notify();
        Ok(())
    }

    /// Push an operation onto the undo stack, oldest dropped past the cap.
    fn remember(&mut self, op: FileOp) {
        self.file_ops.push(op);
        if self.file_ops.len() > UNDO_DEPTH {
            self.file_ops.remove(0);
        }
    }

    /// What the next undo would take back, for the command's own label.
    #[must_use]
    pub fn undoable_file_op(&self) -> Option<&FileOp> {
        self.file_ops.last()
    }

    /// Take back the last file operation.
    ///
    /// Refused rather than forced when taking it back would lose work: a
    /// created or renamed file with unsaved edits, or a path something
    /// else now occupies. The whole point is to undo a mistake, and an
    /// undo that makes a second one is worse than none.
    pub fn undo_file_op(&mut self, cx: &mut Context<Self>) -> Result<String> {
        let Some(op) = self.file_ops.pop() else {
            anyhow::bail!("nothing to undo");
        };
        let done = op.describe();
        let result = match &op {
            FileOp::Created { path } => {
                if self.is_dirty(path) {
                    anyhow::bail!("{path} has unsaved edits — save or revert it first");
                }
                self.delete_file(path, cx)
            }
            FileOp::Renamed { from, to } => {
                if self.is_dirty(to) {
                    anyhow::bail!("{to} has unsaved edits — save or revert it first");
                }
                self.rename_file(to, from, cx)
            }
            FileOp::Deleted { path, text } => self.create_file(path, text, cx),
        };
        match result {
            Ok(()) => {
                // Undoing is not itself an operation to undo: the inverse
                // pushed one, and leaving it there would make the next
                // undo redo this one.
                self.file_ops.pop();
                Ok(done)
            }
            Err(err) => {
                // Put it back: a refusal must leave the stack as it was.
                self.remember(op);
                Err(err)
            }
        }
    }

    /// Move `from` to `to`, on disk and in the session.
    ///
    /// The text that moves is the text the EDITORS hold, not what is on
    /// disk: renaming a file with unsaved work must not throw that work
    /// away, so the move writes the current text to the new path.
    pub fn rename_file(&mut self, from: &str, to: &str, cx: &mut Context<Self>) -> Result<()> {
        let to = normalise_path(to)?;
        if from == to {
            return Ok(());
        }
        if !self.sources.contains_key(from) {
            anyhow::bail!("{from} is not in the project");
        }
        if self.sources.contains_key(&to) || self.root.join(&to).exists() {
            anyhow::bail!("{to} already exists");
        }
        let text = self.sources.get(from).cloned().unwrap_or_default();
        let target = self.root.join(&to);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &text)?;
        // Only after the new file is safely written.
        let _ = std::fs::remove_file(self.root.join(from));
        self.sources.remove(from);
        self.saved.remove(from);
        self.sources.insert(to.clone(), text.clone());
        self.saved.insert(to.clone(), text.clone());
        self.files.retain(|f| f != from);
        self.files.push(to.clone());
        self.files.sort();
        if self.entry.as_deref() == Some(from) {
            // The config still names the old path; the analysis will say
            // so. Moving the entry is the author's to decide.
            self.entry = None;
        }
        // The arrangement follows the file, so a move does not shuffle
        // the manuscript back to its fallback order.
        self.binder_order = binder_order::rekey(&self.binder_order, from, &to);
        self.write_binder_order(cx);
        self.worker.send(Request::RemoveFile {
            path: from.to_owned(),
        });
        self.worker.send(Request::AddFile {
            path: to.clone(),
            text,
        });
        self.remember(FileOp::Renamed {
            from: from.to_owned(),
            to,
        });
        cx.emit(ProjectEvent::FilesChanged);
        cx.notify();
        Ok(())
    }

    /// Delete `path` from the project and from disk.
    pub fn delete_file(&mut self, path: &str, cx: &mut Context<Self>) -> Result<()> {
        if !self.sources.contains_key(path) {
            anyhow::bail!("{path} is not in the project");
        }
        std::fs::remove_file(self.root.join(path))?;
        // Kept for the undo: once the file is gone this is the only copy,
        // and it is the text the EDITORS held, unsaved edits included.
        let text = self.sources.get(path).cloned().unwrap_or_default();
        self.sources.remove(path);
        self.saved.remove(path);
        self.files.retain(|f| f != path);
        self.binder_order = binder_order::remove(&self.binder_order, path);
        self.write_binder_order(cx);
        self.worker.send(Request::RemoveFile {
            path: path.to_owned(),
        });
        self.remember(FileOp::Deleted {
            path: path.to_owned(),
            text,
        });
        cx.emit(ProjectEvent::FilesChanged);
        cx.notify();
        Ok(())
    }

    /// Write a `brink.toml` for a project that has none, and adopt it.
    ///
    /// The default names the entry and nothing else: every other key has
    /// a default the analysis already applies, and a file full of keys
    /// nobody chose is a file nobody can read later.
    pub fn create_config(&mut self, cx: &mut Context<Self>) -> Result<()> {
        if self.config.is_some() {
            anyhow::bail!("this project already has a brink.toml");
        }
        let entry = self
            .entry
            .clone()
            .or_else(|| self.files.first().cloned())
            .ok_or_else(|| anyhow::anyhow!("the project has no files to point at"))?;
        let path = "brink.toml".to_owned();
        let full = self.root.join(&path);
        if full.exists() {
            anyhow::bail!("brink.toml already exists on disk");
        }
        let text = format!("[project]\nentry = \"{entry}\"\n");
        std::fs::write(&full, &text)?;
        self.sources.insert(path.clone(), text.clone());
        self.saved.insert(path.clone(), text.clone());
        self.config = Some(path.clone());
        self.worker.send(Request::SetConfig { path, text });
        cx.emit(ProjectEvent::FilesChanged);
        cx.notify();
        Ok(())
    }

    /// The mounted stdlib, `(key, text)` — the Binder's Library section.
    /// Not the author's files: they are never in `files`, never dirty,
    /// never saved, and open read-only.
    #[must_use]
    pub fn library(&self) -> &'static [(&'static str, &'static str)] {
        brink_gpui_model::library_sources()
    }

    /// Whether `path` is a mounted library file rather than the author's.
    #[must_use]
    pub fn is_library(&self, path: &str) -> bool {
        self.library().iter().any(|(key, _)| *key == path)
    }

    /// The config's artifacts, root-relative — openable and saveable, but
    /// not sources.
    #[must_use]
    pub fn artifacts(&self) -> &[String] {
        &self.artifacts
    }

    /// The authored Binder order.
    #[must_use]
    pub fn binder_order(&self) -> &BinderOrder {
        &self.binder_order
    }

    /// Record one container's children in their new order, and write the
    /// sidecar. A failed write is reported, not swallowed: an arrangement
    /// that silently does not persist is worse than one that says so.
    pub fn reorder_binder(
        &mut self,
        container: &str,
        ordered: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        binder_order::apply_reorder(&mut self.binder_order, container, ordered);
        self.write_binder_order(cx);
    }

    fn write_binder_order(&mut self, cx: &mut Context<Self>) {
        let path = self.root.join(binder_order::PATH);
        let text = binder_order::serialize(&self.binder_order);
        if let Err(err) = std::fs::write(&path, text) {
            cx.emit(ProjectEvent::SaveFailed {
                path: binder_order::PATH.to_owned(),
                message: format!("{err}"),
            });
        }
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
        self.sources.get(path).map(String::as_str).or_else(|| {
            // A library file is not in the mirror — nothing edits it — but
            // it is openable, so its text has to be reachable by path.
            self.library()
                .iter()
                .find(|(key, _)| *key == path)
                .map(|(_, text)| *text)
        })
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

    /// Apply what happened to these paths on disk.
    ///
    /// The policy is `watch::classify`'s and the web studio's: a change
    /// under a CLEAN buffer is adopted, a change under a DIRTY one keeps
    /// the buffer and is reported. Nothing here writes to disk — this is
    /// the direction that reads.
    pub fn disk_changed(&mut self, paths: &[String], cx: &mut Context<Self>) {
        let mut reports = Vec::new();
        let mut files_changed = false;
        for path in paths {
            let disk = std::fs::read_to_string(self.root.join(path)).ok();
            let change = crate::watch::classify(
                self.saved.get(path).map(String::as_str),
                self.sources.get(path).map(String::as_str),
                disk.as_deref(),
            );
            match change {
                crate::watch::DiskChange::Ignore => {}
                crate::watch::DiskChange::Adopt(text) => {
                    self.conflicted.remove(path);
                    // Through `edit`, so every editor over the file
                    // follows and the worker re-analyses; then `saved` is
                    // put back level, because this text IS what is on
                    // disk and the file is not dirty.
                    self.edit(path, text.clone(), None, cx);
                    self.saved.insert(path.clone(), text);
                    reports.push(DiskReport::Reloaded(path.clone()));
                }
                crate::watch::DiskChange::Conflict(disk) => {
                    // One write often reaches a watcher as several
                    // events; the author needs telling once, not once per
                    // event. Anything that MOVES the disk again is news
                    // again.
                    if self.conflicted.get(path) != Some(&disk) {
                        self.conflicted.insert(path.clone(), disk);
                        reports.push(DiskReport::Conflicted(path.clone()));
                    }
                }
                crate::watch::DiskChange::Vanished => {
                    let dirty = self.is_dirty(path);
                    self.sources.remove(path);
                    self.saved.remove(path);
                    self.files.retain(|f| f != path);
                    self.worker.send(Request::RemoveFile { path: path.clone() });
                    files_changed = true;
                    reports.push(DiskReport::Vanished {
                        path: path.clone(),
                        dirty,
                    });
                }
                crate::watch::DiskChange::Appeared(text) => {
                    self.sources.insert(path.clone(), text.clone());
                    self.saved.insert(path.clone(), text.clone());
                    self.files.push(path.clone());
                    self.files.sort();
                    self.worker.send(Request::AddFile {
                        path: path.clone(),
                        text,
                    });
                    files_changed = true;
                    reports.push(DiskReport::Appeared(path.clone()));
                }
            }
        }
        if files_changed {
            cx.emit(ProjectEvent::FilesChanged);
            cx.notify();
        }
        if !reports.is_empty() {
            // Saying so is the studio's: it owns the notifications and the
            // window they need.
            cx.emit(ProjectEvent::DiskChanged(reports));
        }
    }

    /// Record a file's prose lints — the document that ran the check
    /// reports them here so Problems can list them beside the compiler's.
    /// Only OPEN files have any: nothing else runs the checker.
    pub fn set_prose(&mut self, path: &str, lints: Vec<Diagnostic>, cx: &mut Context<Self>) {
        let changed = if lints.is_empty() {
            self.prose.remove(path).is_some()
        } else {
            self.prose.insert(path.to_owned(), lints) != self.prose.get(path).cloned()
        };
        if changed {
            cx.emit(ProjectEvent::ProseChanged);
            cx.notify();
        }
    }

    /// Every file's prose lints, by path.
    pub fn all_prose(&self) -> impl Iterator<Item = (&String, &Vec<Diagnostic>)> {
        self.prose.iter()
    }

    /// Toggle the breakpoint on `path`'s 1-based `line`, then tell the
    /// worker. Returns whether there is now one there.
    pub fn toggle_breakpoint(&mut self, path: &str, line: u32, cx: &mut Context<Self>) -> bool {
        let key = (path.to_owned(), line);
        let on = if self.breakpoints.remove(&key) {
            self.unbound.remove(&key);
            false
        } else {
            self.breakpoints.insert(key);
            true
        };
        self.send_breakpoints(cx);
        on
    }

    /// Drop every breakpoint.
    pub fn clear_breakpoints(&mut self, cx: &mut Context<Self>) {
        if self.breakpoints.is_empty() {
            return;
        }
        self.breakpoints.clear();
        self.unbound.clear();
        self.send_breakpoints(cx);
    }

    /// Hand the current set to the worker and record which lines bound to
    /// nothing. A set with no story running is not an error — it is the
    /// ordinary case of marking a line before pressing Play.
    fn send_breakpoints(&mut self, cx: &mut Context<Self>) {
        let lines: Vec<(String, u32)> = self.breakpoints.iter().cloned().collect();
        let task = self.play(PlayCommand::SetBreakpoints(lines), cx);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(outcome) = outcome
                    && outcome.error.is_none()
                {
                    this.unbound = outcome.unbound.into_iter().collect();
                }
                cx.emit(ProjectEvent::BreakpointsChanged);
                cx.notify();
            });
        })
        .detach();
        cx.emit(ProjectEvent::BreakpointsChanged);
    }

    /// Record which marks the worker could not bind — from a Start, which
    /// arms them against the program it just compiled.
    pub fn set_unbound(&mut self, unbound: Vec<(String, u32)>, cx: &mut Context<Self>) {
        let next: BTreeSet<(String, u32)> = unbound.into_iter().collect();
        if next == self.unbound {
            return;
        }
        self.unbound = next;
        cx.emit(ProjectEvent::BreakpointsChanged);
        cx.notify();
    }

    /// The marked lines in `path`, each with whether it bound.
    #[must_use]
    pub fn breakpoints_in(&self, path: &str) -> Vec<(u32, bool)> {
        self.breakpoints
            .iter()
            .filter(|(p, _)| p == path)
            .map(|key| (key.1, !self.unbound.contains(key)))
            .collect()
    }

    /// Every breakpoint, in path then line order.
    #[must_use]
    pub fn all_breakpoints(&self) -> Vec<(String, u32, bool)> {
        self.breakpoints
            .iter()
            .map(|key| (key.0.clone(), key.1, !self.unbound.contains(key)))
            .collect()
    }

    /// The file's dialect-classified lines. Like `kinds_for`, this lags by
    /// at most one analysis — a cue typed a keystroke ago paints as prose
    /// until the next pass lands, which is refinement, not error.
    #[must_use]
    pub fn cues_for(&self, path: &str) -> &[CueLine] {
        self.cues.get(path).map_or(&[], Vec::as_slice)
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
    fn a_path_that_climbs_out_of_the_project_is_refused() {
        // A file operation writes to disk. Everything it will accept has
        // to stay under the root, or the Binder would show one thing and
        // the filesystem hold another.
        assert!(normalise_path("../secrets.ink").is_err());
        assert!(normalise_path("a/../../b.ink").is_err());
        assert!(normalise_path("/etc/passwd").is_err());
        assert!(
            normalise_path("C:/x.ink").is_err(),
            "a drive is not a path here"
        );
        assert!(normalise_path("").is_err());
        assert!(normalise_path("   ").is_err());
        assert!(normalise_path("a//b.ink").is_err(), "an empty segment");
        assert!(normalise_path("a/./b.ink").is_err());
    }

    #[test]
    fn an_ordinary_path_survives_normalisation() {
        assert_eq!(normalise_path("scene.ink").unwrap(), "scene.ink");
        assert_eq!(normalise_path("acts/two.ink").unwrap(), "acts/two.ink");
        assert_eq!(normalise_path("  scene.ink  ").unwrap(), "scene.ink");
        assert_eq!(normalise_path("./scene.ink").unwrap(), "scene.ink");
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
