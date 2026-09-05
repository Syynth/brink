//! The analysis worker — `docs/gpui-studio-spec.md` §3.3.
//!
//! The single [`IdeSession`] lives here, on a thread of its own, and the UI
//! never touches it. That is possible because `IdeSession` is already
//! `Send` (`brink-lsp` runs one as `Arc<Mutex<NativeProjects>>` under a
//! multi-threaded server), so it **moves** rather than being shared: no
//! salsa snapshot handles, no `ProjectDb: Clone`, and above all no
//! `cancel_others` blocking a keystroke for an unbounded cooperative-
//! cancellation interval.
//!
//! Everything crossing back is **plain data** — offsets, strings, enums.
//! No db handle, no salsa reference, nothing borrowing the session.
//!
//! **Offsets, not line/column.** Positions cross the boundary as byte
//! ranges. Converting to line/column needs a `LineIndex` per file, which is
//! O(file); doing that for every file on every keystroke would put an
//! O(project) term back on the hot path for the sake of files nobody is
//! looking at. The UI resolves positions for the one document it paints,
//! where it already holds the text.
//!
//! **Coalescing is not debounce** (ruled 2026-09-04, "No debounce"). The
//! loop never waits before starting work. It does drain everything already
//! queued before analyzing, because an edit superseded by a later edit to
//! the same file has no result anyone will ever see — that is declining to
//! do dead work, not delaying live work.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use brink_ide::session::IdeSession;

use crate::query::{QueryKind, QueryResult};
use brink_ir::hir::projection::range_key;
use brink_ir::{Severity, SymbolKind};

/// Byte-offset range key, as the classifiers want it.
pub type Kinds = BTreeMap<(u32, u32), SymbolKind>;

/// One diagnostic, positioned in bytes. See the module doc on why this is
/// not an `lsp_types::Diagnostic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub start: u32,
    pub end: u32,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

/// What the UI asks of the worker.
#[derive(Debug)]
pub enum Request {
    /// Discard any current project and load the one rooted at `root`.
    Open { root: PathBuf },
    /// Ask a question of the current analysis. Answered **after** every
    /// edit queued ahead of it, so it never sees stale text.
    Query {
        kind: QueryKind,
        /// A one-shot reply channel, so no request-id bookkeeping is needed
        /// on either side.
        reply: async_channel::Sender<QueryResult>,
    },
    /// The full text of one file, as the editor now holds it. For the
    /// project's `brink.toml` (`Opened::config`) this re-applies the config
    /// rather than updating a source — see [`ConfigState`].
    Edit {
        path: String,
        text: String,
        /// Echoed back on the resulting [`Analyzed`] so the UI can tell how
        /// far behind an arriving result is.
        revision: u64,
    },
}

/// What the worker sends back. Plain data only.
#[derive(Debug)]
pub enum Response {
    Opened(Box<Result<Opened, String>>),
    Analyzed(Box<Analyzed>),
}

/// A freshly loaded project.
#[derive(Debug)]
pub struct Opened {
    pub root: PathBuf,
    /// Root-relative, forward-slashed, sorted — the compiler's key
    /// convention. The author's files only; the mounted stdlib is not
    /// listed, because it is not theirs to open, rename or delete.
    pub files: Vec<String>,
    /// Initial text for each entry in `files`, same order.
    pub sources: Vec<String>,
    /// `[project] entry` from `brink.toml`, if one was found.
    pub entry: Option<String>,
    /// Config-file warnings, already prefixed with their source.
    pub warnings: Vec<String>,
    /// The project's `brink.toml`, if it has one — a file the mirror holds
    /// in the shared buffer like any other, so Settings' Project sections
    /// and a raw editor over it are views of one text. Not in `files`: it
    /// is not a source, and the manuscript and search must not read it as
    /// one.
    pub config: Option<ConfigFile>,
    pub elapsed_ms: f64,
}

/// The project's config file as loaded: its root-relative key and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: String,
    pub text: String,
}

/// One `[project] drafts` glob and what it currently matches — the
/// session's `DraftGlobReport` row, as plain data. Ruled 2026-08-29: the
/// split between "drafts" and "matched but the story still reaches it" has
/// one implementation, the session's, and the UI shows it rather than
/// recomputing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftGlob {
    pub glob: String,
    /// Matched, and outside the compile closure — drafts.
    pub drafts: Vec<String>,
    /// Matched, but the entry still reaches them, so not drafts.
    pub in_story: Vec<String>,
}

/// The result of one analysis pass.
#[derive(Debug, Default)]
pub struct Analyzed {
    /// The highest [`Request::Edit`] revision folded into this pass.
    pub revision: u64,
    /// Keyed by root-relative path. Absent means "no diagnostics".
    pub diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    /// The identity join the token classifiers refine `IDENT` with. This is
    /// the only thing the UI is ever allowed to render stale, and staleness
    /// costs *refinement* alone — an identifier not yet known to name a
    /// knot. Structure is decidable from syntax and never comes from here.
    pub kinds: BTreeMap<String, Kinds>,
    /// `[project] drafts` resolved against the compile closure.
    pub drafts: Vec<String>,
    /// The compile closure — the files the story actually reaches. A file
    /// the project holds but this omits is on disk and not in the story,
    /// which absent diagnostics look exactly like, so the Binder says so.
    pub closure: Vec<String>,
    /// `[project] entry` as the config currently applied resolves it. Rides
    /// every analysis because an edit to `brink.toml` can move it.
    pub entry: Option<String>,
    /// The applied config's warnings, unprefixed. Also reported as
    /// `diagnostics` rows under the config's own path.
    pub config_warnings: Vec<String>,
    /// Per-glob attribution for the Drafts setting, in the author's order.
    pub draft_globs: Vec<DraftGlob>,
    /// Whether `draft_globs` means anything yet: false until a compile
    /// closure exists, when every list is empty and "matches nothing" would
    /// be the wrong reading.
    pub drafts_known: bool,
    pub elapsed_ms: f64,
}

/// What the loaded `brink.toml` resolved to, kept on the worker across
/// edits so the next re-application and the next analysis can report it.
///
/// **Edits re-apply the whole file.** `IdeSession::apply_project_config`
/// follows the "unset means untouched" convention — a key absent from the
/// config leaves the session's current value alone — which is right for a
/// one-shot load and wrong for an editor, where deleting `drafts = [...]`
/// must stop marking drafts. So a re-application first returns the session
/// to its defaults (`clear_project_config`, the dialect, the type policy)
/// and then applies the file as if for the first time.
///
/// **A malformed file keeps the last good config applied.** The author is
/// typing; every intermediate state of a raw edit is invalid TOML, and
/// tearing the applied config down on each of them would blank the entry,
/// empty the closure, and mark every file "not in the story" mid-keystroke.
/// The error is reported as an Error diagnostic on the config's own path
/// (the studio's #3391 shape) and clears with the next parse that succeeds.
#[derive(Debug, Default)]
pub struct ConfigState {
    /// Root-relative key of the config file, if the project has one.
    path: Option<String>,
    entry: Option<String>,
    /// The applied config's warnings, unprefixed.
    warnings: Vec<String>,
    /// The current text's parse error, if it has one: its byte span in
    /// the text (when the parser knows one) and its message.
    error: Option<(Option<Range<usize>>, String)>,
}

impl ConfigState {
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    #[must_use]
    pub fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
    }

    /// The error the current text parses to, if any.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_ref().map(|(_, message)| message.as_str())
    }

    /// The config's diagnostics for the Problems panel: the parse error as
    /// an Error (at its span), each warning as a Warning at the file's
    /// start. Empty when there is nothing to say.
    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut rows = Vec::new();
        if let Some((span, message)) = &self.error {
            let (start, end) = span
                .clone()
                .map_or((0, 0), |r| (offset_u32(r.start), offset_u32(r.end)));
            rows.push(Diagnostic {
                start,
                end,
                severity: Severity::Error,
                code: CONFIG_CODE.to_owned(),
                message: message.clone(),
            });
        }
        rows.extend(self.warnings.iter().map(|w| Diagnostic {
            start: 0,
            end: 0,
            severity: Severity::Warning,
            code: CONFIG_CODE.to_owned(),
            message: w.clone(),
        }));
        rows
    }
}

/// The code a `brink.toml` problem carries. Not a compiler code: the
/// config has none, and a row needs one to sort and to say what it is.
pub const CONFIG_CODE: &str = "CONFIG";

fn offset_u32(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

/// A handle on the worker thread. Dropping it closes the request channel,
/// which ends the loop and drops the session.
pub struct Worker {
    requests: async_channel::Sender<Request>,
    responses: async_channel::Receiver<Response>,
}

impl Worker {
    /// Start the worker. The thread is detached: it ends when `self` drops
    /// and the request channel closes.
    #[must_use]
    pub fn spawn() -> Self {
        let (req_tx, req_rx) = async_channel::unbounded::<Request>();
        let (res_tx, res_rx) = async_channel::unbounded::<Response>();
        std::thread::Builder::new()
            .name("brink-analysis".to_owned())
            .spawn(move || run(&req_rx, &res_tx))
            .expect("spawning the analysis worker");
        Self {
            requests: req_tx,
            responses: res_rx,
        }
    }

    /// Queue a request. Never blocks (the channel is unbounded); fails only
    /// once the worker is gone, which is not an error worth surfacing.
    pub fn send(&self, request: Request) {
        let _ = self.requests.send_blocking(request);
    }

    /// The response stream, for the UI to drive from `cx.spawn`.
    #[must_use]
    pub fn responses(&self) -> async_channel::Receiver<Response> {
        self.responses.clone()
    }
}

/// A fresh session with the stdlib mounted.
///
/// Seeded **before** any project file, and with `update_source` rather than
/// `update_and_analyze`: a real project file at the same key then wins by
/// simply overwriting the embedded copy, with no analysis having run in
/// between against a stdlib-less file set. Same ordering as
/// `EditorSession::new` and `brink-cli`'s `Project::ide_session`.
fn session_with_stdlib() -> IdeSession {
    let mut session = IdeSession::new();
    for (key, text) in brink_environment::stdlib_sources() {
        session.update_source(key, (*text).to_owned());
        if let Some(id) = session.file_id(key) {
            // Which files are the author's is a session-layer fact; rules
            // are written against it (drafts, rename, delete).
            session.mark_mounted_std(id);
        }
    }
    session
}

fn run(requests: &async_channel::Receiver<Request>, responses: &async_channel::Sender<Response>) {
    let mut session = session_with_stdlib();
    let mut config = ConfigState::default();
    let mut revision = 0_u64;

    while let Ok(first) = requests.recv_blocking() {
        // Drain what is already queued. See the module doc: this declines
        // superseded work, it does not delay live work.
        let mut batch = vec![first];
        while let Ok(next) = requests.try_recv() {
            batch.push(next);
        }

        let mut reopened = None;
        let mut edited = false;
        let mut queries = Vec::new();
        for request in batch {
            match request {
                Request::Query { kind, reply } => queries.push((kind, reply)),
                Request::Open { root } => {
                    session = session_with_stdlib();
                    config = ConfigState::default();
                    let opened = match open(&mut session, root) {
                        Ok((opened, state)) => {
                            config = state;
                            Ok(opened)
                        }
                        Err(e) => {
                            // Leave the session empty rather than half-loaded.
                            session = session_with_stdlib();
                            Err(e)
                        }
                    };
                    reopened = Some(opened);
                }
                Request::Edit {
                    path,
                    text,
                    revision: rev,
                } => {
                    if config.path.as_deref() == Some(path.as_str()) {
                        apply_config_text(&mut session, &mut config, &text);
                    } else {
                        session.update_source(&path, text);
                    }
                    revision = revision.max(rev);
                    edited = true;
                }
            }
        }

        let mut usable = true;
        let mut opened_ok = false;
        if let Some(opened) = reopened {
            usable = opened.is_ok();
            opened_ok = usable;
            if responses
                .send_blocking(Response::Opened(Box::new(opened)))
                .is_err()
            {
                return;
            }
        }

        // Only a real change produces an `Analyzed`. A drain carrying
        // nothing but queries must not emit one: `analyze` is memoized and
        // so nearly free, but the UI would repaint diagnostics on every
        // hover.
        if usable && (edited || opened_ok) {
            let analyzed = analyze(&mut session, &config, revision);
            if responses
                .send_blocking(Response::Analyzed(Box::new(analyzed)))
                .is_err()
            {
                return;
            }
        }

        // Queries last, so they read the analysis the same drain produced.
        for (kind, reply) in queries {
            let result = if usable {
                crate::query::answer(&session, &kind)
            } else {
                QueryResult::Unavailable
            };
            // A dropped receiver just means the asker moved on.
            let _ = reply.send_blocking(result);
        }
    }
}

/// Load every source file under `root`, then apply its `brink.toml`.
fn open(session: &mut IdeSession, root: PathBuf) -> Result<(Opened, ConfigState), String> {
    let started = Instant::now();

    let mut files = Vec::new();
    collect_sources(&root, &root, &mut files);
    files.sort();
    if files.is_empty() {
        return Err(format!("no .brink or .ink files under {}", root.display()));
    }

    let mut sources = Vec::with_capacity(files.len());
    for key in &files {
        let text =
            std::fs::read_to_string(root.join(key)).map_err(|e| format!("reading {key}: {e}"))?;
        session.update_source(key, text.clone());
        sources.push(text);
    }

    let (config, state) = load_config(session, &root, &files);
    session.refresh_analysis();

    let warnings = state
        .error
        .iter()
        .map(|(_, message)| message.clone())
        .chain(state.warnings.iter().cloned())
        .map(|w| format!("{}: {w}", state.path.as_deref().unwrap_or("brink.toml")))
        .collect();
    Ok((
        Opened {
            root,
            files,
            sources,
            entry: state.entry.clone(),
            warnings,
            config,
            elapsed_ms: started.elapsed().as_secs_f64() * 1e3,
        },
        state,
    ))
}

/// Establish the compile closure by naming the entry.
///
/// `refresh_analysis` alone never sets one, and `compilation_closure` is
/// "empty when no entry is set" — so without this every draft query returns
/// nothing at all, indistinguishable from a project with no drafts. The
/// entry is a db *input*: setting it once here keeps the closure live and
/// recomputing as `INCLUDE`s change, so this does not belong on the
/// per-keystroke path.
///
/// A project whose `brink.toml` names no entry gets no closure, and
/// `DraftGlobReport::compiled` reports that honestly rather than guessing at
/// one.
fn set_compile_entry(session: &mut IdeSession, entry: Option<&str>) {
    let Some(entry) = entry else {
        return;
    };
    let options = session.db().analysis_options().clone();
    if let Err(e) = session.compile(entry, &options) {
        // Not fatal: analysis stands, only the closure-derived surfaces
        // (drafts, "not in the story" marks) go quiet.
        eprintln!("warning: [project] entry {entry:?}: {e}");
    }
}

/// Discover `brink.toml`, read it, and apply it.
///
/// Discovery is the shared session-level road (decision log 2026-09-04,
/// "Both studio consumers sit on the same layer"), not the analyzer-level
/// `AnalysisOptions::apply_project_config` the spike reached for — that one
/// resolves `[lints]` and `[conventions]` and nothing else, so `[project]
/// entry`, `drafts`, `[prose]`, `[fix]` and `[dialogue]` were all silently
/// dropped.
fn load_config(
    session: &mut IdeSession,
    root: &Path,
    files: &[String],
) -> (Option<ConfigFile>, ConfigState) {
    let mut state = ConfigState::default();
    let tree = brink_driver::RealFs::new(root);
    let Ok(Some(key)) = brink_project_config::discover_from_entry_in_tree(&tree, &files[0]) else {
        return (None, state);
    };
    let Ok(text) = std::fs::read_to_string(root.join(&key)) else {
        state.error = Some((None, "could not be read".to_owned()));
        return (None, state);
    };
    state.path = Some(key.clone());
    apply_config_text(session, &mut state, &text);
    (Some(ConfigFile { path: key, text }), state)
}

/// Apply `text` as the project's `brink.toml` — on load and on every edit
/// to it. See [`ConfigState`] for the two rules: whole-file semantics, and
/// a malformed text keeping the last good config.
fn apply_config_text(session: &mut IdeSession, state: &mut ConfigState, text: &str) {
    let Some(path) = state.path.clone() else {
        return;
    };
    match brink_project_config::parse_str_at(path.clone(), text) {
        Ok((config, parsed)) => {
            let mut warnings: Vec<String> = parsed.into_iter().map(|w| w.0).collect();
            let config_dir = path.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
            // Back to defaults first, so a key deleted from the file stops
            // applying. The dialect and the type policy are resolved here
            // wholesale (`apply_project_config`'s own "explicit" tier keeps
            // whatever the session already had, which is the one-shot
            // convention this is not).
            session.clear_project_config();
            let dialect = config.dialect.unwrap_or_default();
            session.set_language_dialect(dialect);
            session.set_type_policy(brink_analyzer::resolve_type_policy(dialect, config.types));
            warnings.extend(session.apply_project_config(&config, true, true, Some(&config_dir)));
            state.entry.clone_from(&config.entry);
            state.warnings = warnings;
            state.error = None;
            set_compile_entry(session, state.entry.as_deref());
        }
        Err(e) => {
            state.error = Some((e.span(), e.to_string()));
        }
    }
}

/// Re-establish analysis and project everything the UI mirrors.
fn analyze(session: &mut IdeSession, config: &ConfigState, revision: u64) -> Analyzed {
    let started = Instant::now();
    session.refresh_analysis();

    let mut kinds: BTreeMap<String, Kinds> = BTreeMap::new();
    if let Some(analysis) = session.analysis() {
        // ONE pass over the resolutions, grouped by file. Calling
        // `build_resolution_index` per file would rescan the whole list each
        // time — O(files x resolutions), quadratic in project size.
        for rref in &analysis.resolutions {
            let Some(info) = analysis.index.symbols.get(&rref.target) else {
                continue;
            };
            if session.is_mounted_std(rref.file) {
                // Never painted, so never worth shipping — the same
                // exclusion the diagnostics loop below makes.
                continue;
            }
            let Some(path) = session.db().file_path(rref.file) else {
                continue;
            };
            kinds
                .entry(path.to_owned())
                .or_default()
                .insert(range_key(rref.range), info.kind);
        }
    }

    let types = session.type_policy();
    let lints = session.lint_policy().clone();
    let mut diagnostics: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
    for id in session.db().file_ids() {
        if session.is_mounted_std(id) {
            continue;
        }
        let Some(path) = session.db().file_path(id) else {
            continue;
        };
        let path = path.to_owned();
        let found: Vec<Diagnostic> = session
            .db()
            .diagnostics(id)
            .unwrap_or(&[])
            .iter()
            .filter_map(|d| {
                let severity = brink_analyzer::effective_severity(d.code, types, &lints)?;
                Some(Diagnostic {
                    start: d.range.start().into(),
                    end: d.range.end().into(),
                    severity,
                    code: d.code.as_str().to_owned(),
                    message: d.message.clone(),
                })
            })
            .collect();
        if !found.is_empty() {
            diagnostics.insert(path, found);
        }
    }

    // The config's own problems, on the config's own path: the Problems
    // panel lists them beside the sources', and a click opens Settings.
    if let Some(path) = &config.path {
        let rows = config.diagnostics();
        if rows.is_empty() {
            diagnostics.remove(path);
        } else {
            diagnostics.insert(path.clone(), rows);
        }
    }

    let report = session.draft_glob_report();
    Analyzed {
        revision,
        diagnostics,
        kinds,
        drafts: session.draft_paths(),
        closure: session.compilation_closure_paths(),
        entry: config.entry.clone(),
        config_warnings: config.warnings.clone(),
        draft_globs: report
            .globs
            .into_iter()
            .map(|g| DraftGlob {
                glob: g.glob,
                drafts: g.drafts,
                in_story: g.in_story,
            })
            .collect(),
        drafts_known: report.compiled,
        elapsed_ms: started.elapsed().as_secs_f64() * 1e3,
    }
}

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "node_modules"
}

fn collect_sources(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !is_ignored_dir(&name) {
                collect_sources(&path, root, out);
            }
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if (ext == "brink" || ext == "ink")
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway project tree, removed on drop.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "brink-gpui-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            for (path, text) in files {
                let full = root.join(path);
                std::fs::create_dir_all(full.parent().expect("a parent directory"))
                    .expect("creating the fixture directory");
                std::fs::write(&full, text).expect("writing a fixture file");
            }
            Self(root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Open synchronously, without the worker thread — the load path is what
    /// these assert, and a channel adds only flakiness.
    fn open_tree(tree: &Tree) -> (IdeSession, Opened) {
        let (session, opened, _) = open_tree_with_config(tree);
        (session, opened)
    }

    fn open_tree_with_config(tree: &Tree) -> (IdeSession, Opened, ConfigState) {
        let mut session = session_with_stdlib();
        let (opened, state) =
            open(&mut session, tree.0.clone()).expect("the fixture project must load");
        (session, opened, state)
    }

    #[test]
    fn the_stdlib_is_mounted_but_is_not_one_of_the_authors_files() {
        let tree = Tree::new("std", &[("main.ink", "Hello.\n-> DONE\n")]);
        let (session, opened) = open_tree(&tree);

        let mounted: Vec<String> = session
            .mounted_std_ids()
            .filter_map(|id| session.db().file_path(id).map(str::to_owned))
            .collect();
        assert!(
            mounted.iter().any(|p| p.starts_with("std/")),
            "the stdlib must be mounted into the analysis universe; got {mounted:?}"
        );
        assert_eq!(
            opened.files,
            vec!["main.ink".to_owned()],
            "the mounted stdlib is not the author's file and must not be listed"
        );
    }

    #[test]
    fn brink_toml_supplies_entry_and_drafts() {
        // The spike applied `AnalysisOptions::apply_project_config`, which
        // resolves `[lints]`/`[conventions]` and silently drops everything
        // else. Both assertions below failed under it.
        let tree = Tree::new(
            "config",
            &[
                (
                    "brink.toml",
                    "[project]\nentry = \"start.ink\"\ndrafts = [\"notes/**\"]\n",
                ),
                ("start.ink", "Hello.\n-> DONE\n"),
                (
                    "notes/scratch.ink",
                    "=== scratch ===\nUnreached.\n-> DONE\n",
                ),
            ],
        );
        let (session, opened) = open_tree(&tree);

        assert_eq!(opened.entry.as_deref(), Some("start.ink"));
        assert_eq!(
            session.draft_globs(),
            ["notes/**"],
            "`[project] drafts` must reach the session"
        );
        assert_eq!(
            session.draft_paths(),
            vec!["notes/scratch.ink".to_owned()],
            "a glob match outside the compile closure is a draft"
        );
    }

    #[test]
    fn analysis_reports_diagnostics_by_path_in_bytes() {
        let tree = Tree::new("diags", &[("main.ink", "Hello.\n-> nowhere\n")]);
        let (mut session, _) = open_tree(&tree);
        let analyzed = analyze(&mut session, &ConfigState::default(), 7);

        assert_eq!(analyzed.revision, 7, "the revision must round-trip");
        let found = analyzed
            .diagnostics
            .get("main.ink")
            .expect("an unresolved divert must be diagnosed");
        assert!(
            found.iter().any(|d| d.severity == Severity::Error),
            "got {found:?}"
        );
        let source = std::fs::read_to_string(tree.0.join("main.ink")).expect("the fixture");
        for d in found {
            assert!(
                usize::try_from(d.end).is_ok_and(|e| e <= source.len()),
                "byte offsets must index the file they came from: {d:?}"
            );
        }
    }

    #[test]
    fn an_edit_clears_the_diagnostic_it_caused() {
        let tree = Tree::new("edit", &[("main.ink", "Hello.\n-> nowhere\n")]);
        let (mut session, _) = open_tree(&tree);
        assert!(
            analyze(&mut session, &ConfigState::default(), 1)
                .diagnostics
                .contains_key("main.ink")
        );

        session.update_source("main.ink", "Hello.\n-> DONE\n".to_owned());
        let after = analyze(&mut session, &ConfigState::default(), 2);
        assert!(
            !after.diagnostics.contains_key("main.ink"),
            "fixing the divert must clear it; got {:?}",
            after.diagnostics
        );
        assert_eq!(after.revision, 2);
    }

    #[test]
    fn kinds_are_grouped_by_file_in_one_pass() {
        let tree = Tree::new(
            "kinds",
            &[
                ("main.ink", "INCLUDE greet.ink\n-> greet\n"),
                ("greet.ink", "=== greet ===\nHello.\n-> DONE\n"),
            ],
        );
        let (mut session, opened) = open_tree(&tree);
        assert_eq!(opened.files.len(), 2);
        let analyzed = analyze(&mut session, &ConfigState::default(), 1);
        assert!(
            analyzed.kinds.keys().all(|k| opened.files.contains(k)),
            "kinds must be keyed by the author's own root-relative paths, \
             with the mounted stdlib excluded: {:?}",
            analyzed.kinds.keys().collect::<Vec<_>>()
        );
        assert!(
            analyzed.kinds.contains_key("main.ink"),
            "the resolved divert must be keyed: {:?}",
            analyzed.kinds.keys().collect::<Vec<_>>()
        );
    }

    /// Drive the real worker thread, blocking on its channels.
    fn drive(tree: &Tree) -> Worker {
        let worker = Worker::spawn();
        worker.send(Request::Open {
            root: tree.0.clone(),
        });
        worker
    }

    fn next(worker: &Worker) -> Response {
        worker
            .responses()
            .recv_blocking()
            .expect("the worker must answer")
    }

    #[test]
    fn the_worker_answers_an_open_with_opened_then_analyzed() {
        let tree = Tree::new("thread", &[("main.ink", "Hello.\n-> DONE\n")]);
        let worker = drive(&tree);
        match next(&worker) {
            Response::Opened(opened) => {
                let opened = opened.expect("the fixture must load");
                assert_eq!(opened.files, vec!["main.ink".to_owned()]);
            }
            other => panic!("expected Opened first, got {other:?}"),
        }
        assert!(
            matches!(next(&worker), Response::Analyzed(_)),
            "an open must be followed by exactly one analysis"
        );
    }

    #[test]
    fn a_query_never_sees_text_older_than_the_edit_queued_before_it() {
        // The ordering claim the whole query design rests on: the channel is
        // FIFO and the worker drains a batch applying edits BEFORE answering
        // queries, so hover can never read the pre-keystroke source.
        let tree = Tree::new("order", &[("main.ink", "=== alpha ===\nHi.\n-> DONE\n")]);
        let worker = drive(&tree);
        let _ = next(&worker);
        let _ = next(&worker);

        worker.send(Request::Edit {
            path: "main.ink".to_owned(),
            text: "=== renamed ===\nHi.\n-> DONE\n".to_owned(),
            revision: 1,
        });
        let (tx, rx) = async_channel::bounded(1);
        worker.send(Request::Query {
            kind: QueryKind::DocumentSymbols {
                path: "main.ink".to_owned(),
            },
            reply: tx,
        });

        let result = rx.recv_blocking().expect("the query must be answered");
        let QueryResult::DocumentSymbols(symbols) = result else {
            panic!("expected symbols, got {result:?}");
        };
        assert_eq!(
            symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["renamed"],
            "the query must see the edit that was queued ahead of it"
        );
    }

    #[test]
    fn a_query_alone_does_not_produce_an_analysis_response() {
        // Otherwise every hover would repaint the diagnostics.
        let tree = Tree::new("quiet", &[("main.ink", "Hello.\n-> DONE\n")]);
        let worker = drive(&tree);
        let _ = next(&worker);
        let _ = next(&worker);

        let (tx, rx) = async_channel::bounded(1);
        worker.send(Request::Query {
            kind: QueryKind::Hover {
                path: "main.ink".to_owned(),
                offset: 0,
            },
            reply: tx,
        });
        let _ = rx.recv_blocking().expect("the query must be answered");
        assert!(
            worker.responses().is_empty(),
            "a drain carrying only queries must emit no Analyzed"
        );
    }

    #[test]
    fn opening_a_tree_with_no_sources_is_an_error_not_a_panic() {
        let tree = Tree::new("empty", &[("README.md", "nothing here\n")]);
        let mut session = session_with_stdlib();
        let err = open(&mut session, tree.0.clone()).expect_err("no sources must be an error");
        assert!(err.contains("no .brink or .ink files"), "got {err}");
    }

    #[test]
    fn editing_brink_toml_reapplies_the_whole_file() {
        let tree = Tree::new(
            "reapply",
            &[
                (
                    "brink.toml",
                    "[project]\nentry = \"start.ink\"\ndrafts = [\"notes/**\"]\n",
                ),
                ("start.ink", "Hello.\n-> DONE\n"),
                ("other.ink", "Other.\n-> DONE\n"),
                (
                    "notes/scratch.ink",
                    "=== scratch ===\nUnreached.\n-> DONE\n",
                ),
            ],
        );
        let (mut session, opened, mut state) = open_tree_with_config(&tree);
        assert_eq!(
            opened.config.as_ref().map(|c| c.path.as_str()),
            Some("brink.toml")
        );
        assert!(
            !opened.files.iter().any(|f| f == "brink.toml"),
            "the config is not a source"
        );
        assert_eq!(state.entry(), Some("start.ink"));
        assert!(
            session
                .compilation_closure_paths()
                .contains(&"start.ink".to_owned())
        );

        // Repoint the entry and drop `drafts`: the closure moves, and the
        // draft mark goes — "unset means untouched" would have kept it.
        apply_config_text(
            &mut session,
            &mut state,
            "[project]\nentry = \"other.ink\"\n",
        );
        assert_eq!(state.entry(), Some("other.ink"));
        let closure = session.compilation_closure_paths();
        assert!(closure.contains(&"other.ink".to_owned()), "{closure:?}");
        assert!(!closure.contains(&"start.ink".to_owned()), "{closure:?}");
        assert!(
            session.draft_globs().is_empty(),
            "a key deleted from the file stops applying"
        );
        assert!(session.draft_paths().is_empty());
        let analyzed = analyze(&mut session, &state, 3);
        assert_eq!(analyzed.entry.as_deref(), Some("other.ink"));
        assert!(
            !analyzed.diagnostics.contains_key("brink.toml"),
            "{:?}",
            analyzed.diagnostics
        );
    }

    #[test]
    fn a_malformed_brink_toml_keeps_the_last_config_and_reports_it() {
        let tree = Tree::new(
            "malformed",
            &[
                ("brink.toml", "[project]\nentry = \"start.ink\"\n"),
                ("start.ink", "Hello.\n-> DONE\n"),
            ],
        );
        let (mut session, _, mut state) = open_tree_with_config(&tree);
        apply_config_text(&mut session, &mut state, "[project]\nentry = \"start.ink\n");
        assert!(state.error().is_some());
        assert_eq!(
            state.entry(),
            Some("start.ink"),
            "the last good config stays applied while the text is broken"
        );
        assert!(
            session
                .compilation_closure_paths()
                .contains(&"start.ink".to_owned())
        );
        let analyzed = analyze(&mut session, &state, 4);
        let rows = analyzed
            .diagnostics
            .get("brink.toml")
            .expect("the parse error is a Problems row on the config's path");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].severity, Severity::Error);
        assert_eq!(rows[0].code, CONFIG_CODE);

        // And it clears with the next parse that succeeds.
        apply_config_text(
            &mut session,
            &mut state,
            "[project]\nentry = \"start.ink\"\n",
        );
        assert!(state.error().is_none());
        assert!(
            !analyze(&mut session, &state, 5)
                .diagnostics
                .contains_key("brink.toml")
        );
    }

    #[test]
    fn the_draft_report_rides_the_analysis() {
        let tree = Tree::new(
            "report",
            &[
                (
                    "brink.toml",
                    "[project]\nentry = \"start.ink\"\ndrafts = [\"notes/**\", \"start.ink\", \"nothing/**\"]\n",
                ),
                ("start.ink", "Hello.\n-> DONE\n"),
                (
                    "notes/scratch.ink",
                    "=== scratch ===\nUnreached.\n-> DONE\n",
                ),
            ],
        );
        let (mut session, _, state) = open_tree_with_config(&tree);
        let analyzed = analyze(&mut session, &state, 1);
        assert!(analyzed.drafts_known);
        assert_eq!(
            analyzed.draft_globs,
            vec![
                DraftGlob {
                    glob: "notes/**".to_owned(),
                    drafts: vec!["notes/scratch.ink".to_owned()],
                    in_story: vec![],
                },
                DraftGlob {
                    glob: "start.ink".to_owned(),
                    drafts: vec![],
                    in_story: vec!["start.ink".to_owned()],
                },
                DraftGlob {
                    glob: "nothing/**".to_owned(),
                    drafts: vec![],
                    in_story: vec![],
                },
            ],
            "three states, in the author's order: drafts, reached, nothing"
        );
    }
}
