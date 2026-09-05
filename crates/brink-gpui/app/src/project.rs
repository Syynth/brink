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
use std::path::PathBuf;

use anyhow::Result;
use brink_gpui_model::query::{QueryKind, QueryResult};
use brink_gpui_model::worker::{Diagnostic, Kinds, Request, Response, Worker};
use gpui::{App, AppContext as _, Context, EventEmitter, Task};

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
}

/// The mirror.
pub struct Project {
    worker: Worker,
    root: PathBuf,
    files: Vec<String>,
    /// Text as loaded. A `Document` takes its initial value from here; once
    /// open, the editor's rope is the truth and this is not kept in step.
    sources: BTreeMap<String, String>,
    entry: Option<String>,
    drafts: BTreeSet<String>,
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
            sources: BTreeMap::new(),
            entry: None,
            drafts: BTreeSet::new(),
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
                    self.files = opened.files;
                    self.entry = opened.entry;
                    self.warnings = opened.warnings;
                    // A new project invalidates everything keyed by path.
                    self.diagnostics.clear();
                    self.kinds.clear();
                    self.drafts.clear();
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
                self.closure = analyzed.closure.into_iter().collect();
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

    /// Tell the worker a file's new text. Returns immediately — the analysis
    /// arrives later as [`ProjectEvent::Analyzed`], and nothing waits for it.
    ///
    /// The mirror keeps the text too, so whatever reads sources here —
    /// Search, the Problems panel's line numbers — sees what the author
    /// typed, not what the file was loaded with.
    pub fn edit(&mut self, path: &str, text: String) {
        self.sources.insert(path.to_owned(), text.clone());
        self.revision += 1;
        self.worker.send(Request::Edit {
            path: path.to_owned(),
            text,
            revision: self.revision,
        });
    }

    /// Ask the worker a question. The returned task resolves when the worker
    /// gets to it — after any edit already queued, so the answer is never
    /// against staler text than the caller has.
    pub fn query(&self, kind: QueryKind, cx: &App) -> Task<Result<QueryResult>> {
        let (reply, answer) = async_channel::bounded(1);
        self.worker.send(Request::Query { kind, reply });
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

    /// The text a file was loaded with. `None` once the project is replaced,
    /// or for a path it never held.
    #[must_use]
    pub fn loaded_source(&self, path: &str) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }

    #[must_use]
    pub fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
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
