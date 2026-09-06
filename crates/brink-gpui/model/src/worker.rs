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

use crate::play::{Play, PlayCommand, PlayOutcome};
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
    /// Drive the play session — see [`crate::play`]. Answered after the
    /// queries of the same drain, against the same text.
    Play {
        command: PlayCommand,
        reply: async_channel::Sender<PlayOutcome>,
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
    /// The resolved `[dialogue]` dialect, or `None` when the config
    /// declares none (or it failed — see `dialogue_error`).
    pub dialogue: Option<brink_ir::DialogueDialect>,
    /// Why `[dialogue]` did not resolve, if it did not.
    pub dialogue_error: Option<String>,
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
    root: PathBuf,
    /// Root-relative key of the config file, if the project has one.
    path: Option<String>,
    /// The config text as last applied (or last attempted).
    text: String,
    /// Project files the mirror holds that are neither sources nor the
    /// config — the `[dialogue]` artifact (`dialect.json`) above all. The
    /// session never sees them as documents; the config's reader serves
    /// them from here (an unsaved edit wins) and then from the disk.
    artifacts: BTreeMap<String, String>,
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
    // The author's file keys, for the play session's entry stand-in rule.
    let mut files: Vec<String> = Vec::new();
    let mut play: Option<Play> = None;

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
        let mut plays = Vec::new();
        for request in batch {
            match request {
                Request::Query { kind, reply } => queries.push((kind, reply)),
                Request::Play { command, reply } => plays.push((command, reply)),
                Request::Open { root } => {
                    session = session_with_stdlib();
                    config = ConfigState::default();
                    play = None;
                    files.clear();
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
                    } else if session.file_id(&path).is_some() {
                        session.update_source(&path, text);
                    } else {
                        // Not a source: an artifact the config may point
                        // at, so the config is applied again with it.
                        config.artifacts.insert(path, text);
                        let current = config.text.clone();
                        apply_config_text(&mut session, &mut config, &current);
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
            if let Ok(opened) = &opened {
                files.clone_from(&opened.files);
            }
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
            let result = if !usable {
                QueryResult::Unavailable
            } else if matches!(kind, QueryKind::Program) {
                // Needs the entry and the file list, which only the loop
                // holds — so it is answered here, not in `query::answer`.
                QueryResult::Program(Box::new(crate::program::report(
                    &mut session,
                    config.entry.as_deref(),
                    &files,
                )))
            } else {
                crate::query::answer(&mut session, &kind)
            };
            // A dropped receiver just means the asker moved on.
            let _ = reply.send_blocking(result);
        }

        // The play session last: a start compiles what the edits above
        // produced.
        for (command, reply) in plays {
            let outcome = if usable {
                crate::play::run(
                    &mut session,
                    config.entry.as_deref(),
                    &files,
                    &mut play,
                    command,
                )
            } else {
                PlayOutcome::unavailable()
            };
            let _ = reply.send_blocking(outcome);
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
    let mut state = ConfigState {
        root: root.to_path_buf(),
        ..ConfigState::default()
    };
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
    state.text = text.to_owned();
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
            let root = state.root.clone();
            let artifacts = state.artifacts.clone();
            let read_file = |key: &str| -> Option<String> {
                artifacts
                    .get(key)
                    .cloned()
                    .or_else(|| std::fs::read_to_string(root.join(key)).ok())
            };
            warnings.extend(session.apply_project_config_with_reader(
                &config,
                true,
                true,
                Some(&config_dir),
                &read_file,
            ));
            state.entry.clone_from(&config.entry);
            state.warnings = warnings;
            state.error = None;
            set_compile_entry(session, state.entry.as_deref());
        }
        Err(e) => {
            // One line: `toml`'s `Display` renders a caret-annotated
            // excerpt, which a Problems row has no room for and the
            // editor already shows.
            let message = match &e {
                brink_project_config::ConfigError::Toml { source, .. } => {
                    source.message().to_owned()
                }
                other => other.to_string(),
            };
            state.error = Some((e.span(), message));
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
        dialogue: session.project_settings().dialogue.clone(),
        dialogue_error: session.project_settings().dialogue_error.clone(),
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

    // ── Navigation queries (INVENTORY §0 item 1) ────────────────────

    use crate::query::{Fold, QueryKind, QueryResult, ReferenceKind, answer};

    const MAIN: &str = "INCLUDE greet.ink\nStart here.\n-> greet\n";
    // Sticky choices: a once-only choice without a label carries an
    // advisory (E157) before and after any rename, which would make "clean
    // afterwards" unfair to assert.
    const GREET: &str = "=== greet ===\nHello.\n+ [Again] -> greet\n+ [Stop] -> DONE\n- -> DONE\n";

    fn nav_tree(name: &str) -> Tree {
        Tree::new(name, &[("main.ink", MAIN), ("greet.ink", GREET)])
    }

    fn play(worker: &Worker, command: PlayCommand) -> PlayOutcome {
        let (reply, answer) = async_channel::bounded(1);
        worker.send(Request::Play { command, reply });
        answer.recv_blocking().expect("the worker answers")
    }

    fn line_texts(outcome: &PlayOutcome) -> Vec<&str> {
        outcome
            .steps
            .iter()
            .filter_map(|s| match s {
                crate::play::PlayStep::Line { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn program_report_reads_one_compile_three_ways() {
        use crate::program::ProgramStatus;
        let tree = Tree::new(
            "program",
            &[(
                "main.ink",
                "VAR torch = 3\nLIST moods = calm, (tense)\nEXTERNAL play_se(name)\n\
                 === greet ===\nHello {torch}.\n= wave\nBye.\n-> END\n",
            )],
        );
        let worker = drive(&tree);
        let (reply, answer) = async_channel::bounded(1);
        worker.send(Request::Query {
            kind: QueryKind::Program,
            reply,
        });
        let QueryResult::Program(report) = answer.recv_blocking().expect("answered") else {
            panic!("a program report");
        };
        assert_eq!(report.entry.as_deref(), Some("main.ink"));
        let ProgramStatus::Ready(program) = &report.status else {
            panic!("compiles clean: {report:?}");
        };
        let model = &program.model;
        // The LIST declares a global too — `moods` holds the list value.
        let globals: Vec<&str> = model.globals.iter().map(|g| g.name.as_str()).collect();
        assert!(globals.contains(&"torch"), "{globals:?}");
        assert!(globals.contains(&"moods"), "{globals:?}");
        assert_eq!(model.lists.len(), 1);
        assert_eq!(model.lists[0].items.len(), 2);
        assert_eq!(model.externals.len(), 1);
        let paths: Vec<&str> = model.knots.iter().map(|k| k.path.as_str()).collect();
        let greet = model
            .knots
            .iter()
            .find(|k| k.path == "greet")
            .unwrap_or_else(|| panic!("greet among {paths:?}"));
        assert_eq!(greet.children.len(), 1, "{paths:?}");
        assert!(!greet.disasm.is_empty());
        assert_eq!(paths, ["greet"], "only the author's knot");
        assert!(
            program
                .lines
                .scopes
                .iter()
                .any(|s| s.name.as_deref() == Some("greet")),
            "{:?}",
            program.lines.scopes
        );
        assert!(program.size.total > 0 && program.size.shipping <= program.size.total);
        assert!(!program.size.sections.is_empty());

        worker.send(Request::Edit {
            path: "main.ink".to_owned(),
            text: "-> nowhere\n".to_owned(),
            revision: 1,
        });
        let (reply, answer) = async_channel::bounded(1);
        worker.send(Request::Query {
            kind: QueryKind::Program,
            reply,
        });
        let QueryResult::Program(report) = answer.recv_blocking().expect("answered") else {
            panic!("a program report");
        };
        assert!(matches!(&report.status, ProgramStatus::Errors(e) if !e.is_empty()));
    }

    #[test]
    fn play_runs_to_choices_and_on_through_one() {
        use crate::play::{PlayError, PlayStep};
        let tree = nav_tree("play");
        let worker = drive(&tree);

        // Nothing running yet.
        let early = play(&worker, PlayCommand::Choose(0));
        assert_eq!(early.error, Some(PlayError::NotStarted));

        // No brink.toml: `main.ink` stands in for the entry.
        let started = play(&worker, PlayCommand::Start { at: None });
        assert_eq!(started.error, None, "{started:?}");
        assert_eq!(line_texts(&started), ["Start here.\n", "Hello.\n"]);
        let Some(PlayStep::Choices(choices)) = started.steps.last() else {
            panic!("ends on the choices: {started:?}");
        };
        assert_eq!(choices.len(), 2);
        assert!(choices.iter().all(|c| c.sticky));
        assert!(
            choices[0]
                .source
                .as_ref()
                .is_some_and(|l| l.path == "greet.ink"),
            "the choice knows where it was written: {choices:?}"
        );

        let again = play(&worker, PlayCommand::Choose(0));
        assert_eq!(again.error, None, "{again:?}");
        assert_eq!(line_texts(&again), ["Hello.\n"]);
        assert!(matches!(again.steps.last(), Some(PlayStep::Choices(_))));

        let stop = play(&worker, PlayCommand::Choose(1));
        assert_eq!(stop.error, None, "{stop:?}");
        assert_eq!(stop.steps.last(), Some(&PlayStep::Done));
        assert!(stop.is_over());

        // Play from here: straight into the knot, no "Start here.".
        let from = play(
            &worker,
            PlayCommand::Start {
                at: Some("greet".to_owned()),
            },
        );
        assert_eq!(from.error, None, "{from:?}");
        assert_eq!(line_texts(&from), ["Hello.\n"]);

        // An edit after a start is not folded in until the next start.
        worker.send(Request::Edit {
            path: "greet.ink".to_owned(),
            text: GREET.replace("Hello.", "Hi."),
            revision: 1,
        });
        let stale = play(&worker, PlayCommand::Choose(0));
        assert_eq!(line_texts(&stale), ["Hello.\n"]);
        let fresh = play(&worker, PlayCommand::Start { at: None });
        assert_eq!(line_texts(&fresh), ["Start here.\n", "Hi.\n"]);

        // A broken project has no program to run.
        worker.send(Request::Edit {
            path: "main.ink".to_owned(),
            text: "-> nowhere\n".to_owned(),
            revision: 2,
        });
        let broken = play(&worker, PlayCommand::Start { at: None });
        assert!(
            matches!(&broken.error, Some(PlayError::Compile(errors)) if !errors.is_empty()),
            "{broken:?}"
        );
    }

    fn offset_of(haystack: &str, needle: &str) -> u32 {
        u32::try_from(haystack.find(needle).expect("the fixture names it")).expect("fits")
    }

    #[test]
    fn definition_jumps_across_files_to_the_declaration() {
        let tree = nav_tree("def");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let at = offset_of(MAIN, "greet\n") + 1;
        let QueryResult::Definition(Some(loc)) = answer(
            &mut session,
            &QueryKind::Definition {
                path: "main.ink".to_owned(),
                offset: at,
            },
        ) else {
            panic!("the divert must resolve");
        };
        assert_eq!(loc.path, "greet.ink");
        assert_eq!(
            &GREET[loc.start as usize..loc.end as usize],
            "greet",
            "the target is the declared name, not the whole header"
        );
    }

    #[test]
    fn definition_on_an_include_jumps_to_the_start_of_that_file() {
        // An include is a reference to a file the way a divert is a
        // reference to a knot; Cmd-clicking one lands in it.
        let tree = nav_tree("include");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let QueryResult::Definition(Some(loc)) = answer(
            &mut session,
            &QueryKind::Definition {
                path: "main.ink".to_owned(),
                offset: offset_of(MAIN, "greet.ink") + 3,
            },
        ) else {
            panic!("an INCLUDE must resolve to its file");
        };
        assert_eq!((loc.path.as_str(), loc.start, loc.end), ("greet.ink", 0, 0));
    }

    #[test]
    fn definition_on_prose_is_an_ordinary_none_not_unavailable() {
        let tree = nav_tree("def-none");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let result = answer(
            &mut session,
            &QueryKind::Definition {
                path: "main.ink".to_owned(),
                offset: offset_of(MAIN, "here"),
            },
        );
        assert!(
            matches!(result, QueryResult::Definition(None)),
            "got {result:?}"
        );
        let missing = answer(
            &mut session,
            &QueryKind::Definition {
                path: "nope.ink".to_owned(),
                offset: 0,
            },
        );
        assert!(
            matches!(missing, QueryResult::Unavailable),
            "a file the session does not hold is Unavailable, not None"
        );
    }

    #[test]
    fn references_are_classified_and_ordered_by_file_then_offset() {
        let tree = nav_tree("refs");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        // Asked from the declaration, with it included.
        let QueryResult::References(refs) = answer(
            &mut session,
            &QueryKind::References {
                path: "greet.ink".to_owned(),
                offset: offset_of(GREET, "greet") + 2,
                include_declaration: true,
            },
        ) else {
            panic!("references must answer");
        };
        let summary: Vec<(&str, ReferenceKind)> = refs
            .iter()
            .map(|r| (r.location.path.as_str(), r.kind))
            .collect();
        assert_eq!(
            summary,
            [
                ("greet.ink", ReferenceKind::Decl),
                ("greet.ink", ReferenceKind::Divert),
                ("main.ink", ReferenceKind::Divert),
            ],
            "decl first in its file, then the two diverts, files in order"
        );
        let mut sorted = refs.clone();
        sorted.sort_by(|a, b| {
            (&a.location.path, a.location.start).cmp(&(&b.location.path, b.location.start))
        });
        assert_eq!(refs, sorted);

        // Without the declaration, only the sites remain.
        let QueryResult::References(sites) = answer(
            &mut session,
            &QueryKind::References {
                path: "main.ink".to_owned(),
                offset: offset_of(MAIN, "greet\n") + 1,
                include_declaration: false,
            },
        ) else {
            panic!("references must answer");
        };
        assert!(sites.iter().all(|r| r.kind != ReferenceKind::Decl));
        assert_eq!(sites.len(), 2);
    }

    #[test]
    fn prepare_rename_offers_the_name_and_refuses_prose() {
        let tree = nav_tree("prep");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let at = offset_of(MAIN, "greet\n") + 1;
        let QueryResult::PrepareRename(Some((start, end))) = answer(
            &mut session,
            &QueryKind::PrepareRename {
                path: "main.ink".to_owned(),
                offset: at,
            },
        ) else {
            panic!("a divert target is renameable");
        };
        assert_eq!(&MAIN[start as usize..end as usize], "greet");
        let prose = answer(
            &mut session,
            &QueryKind::PrepareRename {
                path: "main.ink".to_owned(),
                offset: offset_of(MAIN, "here"),
            },
        );
        assert!(
            matches!(prose, QueryResult::PrepareRename(None)),
            "got {prose:?}"
        );
    }

    /// Apply a plan's edits to in-memory sources, last-to-first per file.
    fn apply_plan(plan: &crate::query::RenamePlan, files: &mut BTreeMap<String, String>) {
        let mut edits = plan.edits.clone();
        edits.sort_by(|a, b| (&b.path, b.start).cmp(&(&a.path, a.start)));
        for e in edits {
            let text = files
                .get_mut(&e.path)
                .expect("an edited file is a known file");
            text.replace_range(e.start as usize..e.end as usize, &e.new_text);
        }
    }

    #[test]
    fn a_safe_rename_edits_every_site_across_files_and_stays_clean() {
        let tree = nav_tree("rename");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let QueryResult::Rename(Some(plan)) = answer(
            &mut session,
            &QueryKind::Rename {
                path: "main.ink".to_owned(),
                offset: offset_of(MAIN, "greet\n") + 1,
                new_name: "hello".to_owned(),
            },
        ) else {
            panic!("the rename must be computable");
        };
        assert_eq!(plan.old_name, "greet");
        assert!(plan.is_safe(), "introduced {:?}", plan.introduced);
        assert!(!plan.external);
        assert_eq!(plan.files(), ["greet.ink", "main.ink"]);
        assert_eq!(plan.edits.len(), 3, "the declaration and both diverts");

        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert("main.ink".to_owned(), MAIN.to_owned());
        files.insert("greet.ink".to_owned(), GREET.to_owned());
        apply_plan(&plan, &mut files);
        assert!(files["greet.ink"].starts_with("=== hello ==="));
        assert!(files["main.ink"].contains("-> hello\n"));
        assert!(
            !files["greet.ink"].contains("greet"),
            "no site may be left behind"
        );

        // The renamed program must analyze clean — the promise a safe plan
        // makes, checked against a fresh session rather than the gate's own
        // word for it.
        for (path, text) in &files {
            session.update_source(path, text.clone());
        }
        let after = analyze(&mut session, &ConfigState::default(), 2);
        assert!(after.diagnostics.is_empty(), "got {:?}", after.diagnostics);
    }

    #[test]
    fn a_rename_that_would_collide_is_reported_not_refused() {
        // Two knots; renaming one onto the other's name is computable but
        // breaks the program. The plan comes back WITH its report, so the UI
        // can show what breaks and offer Force (ruled 2026-06-20).
        let tree = Tree::new(
            "collide",
            &[(
                "main.ink",
                "-> a\n=== a ===\nA.\n-> b\n=== b ===\nB.\n-> DONE\n",
            )],
        );
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let src = "-> a\n=== a ===\nA.\n-> b\n=== b ===\nB.\n-> DONE\n";
        let QueryResult::Rename(Some(plan)) = answer(
            &mut session,
            &QueryKind::Rename {
                path: "main.ink".to_owned(),
                offset: offset_of(src, "=== a") + 4,
                new_name: "b".to_owned(),
            },
        ) else {
            panic!("a colliding rename is still computable");
        };
        assert!(!plan.is_safe());
        assert!(
            !plan.introduced.is_empty(),
            "the report must say what breaks"
        );
        assert!(
            plan.introduced
                .iter()
                .all(|d| d.path == "main.ink" && d.line >= 1)
        );
    }

    #[test]
    fn folding_offers_structural_folds_only_sorted_and_non_empty() {
        let tree = nav_tree("fold");
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let QueryResult::FoldingRanges(folds) = answer(
            &mut session,
            &QueryKind::FoldingRanges {
                path: "greet.ink".to_owned(),
            },
        ) else {
            panic!("folding must answer");
        };
        assert!(!folds.is_empty(), "a knot with a body is foldable");
        assert!(
            folds.contains(&Fold {
                start_line: 0,
                end_line: 4
            }),
            "the knot folds from its header to its last line; got {folds:?}"
        );
        assert!(folds.iter().all(|f| f.end_line > f.start_line));
        let mut sorted = folds.clone();
        sorted.sort_by_key(|f| (f.start_line, f.end_line));
        assert_eq!(folds, sorted);
    }

    // ── Fixes (INVENTORY §0 item 3) ────────────────────────────────

    use crate::fixes::{FixScope, Tier};

    /// `greet` takes one parameter; the call over-supplies two — E031, whose
    /// fixer is Safe (`brink_ide::arity_trim_fix`).
    const FIXABLE: &str = "=== greet(name) ===\n~ return \"Hi \" + name\n\n=== main ===\n~ temp r = greet(\"Al\", \"Bob\")\n{r}\n-> DONE\n";
    const FIXED: &str = "=== greet(name) ===\n~ return \"Hi \" + name\n\n=== main ===\n~ temp r = greet(\"Bob\")\n{r}\n-> DONE\n";

    fn fixable_tree(name: &str) -> Tree {
        Tree::new(
            name,
            &[
                (
                    "brink.toml",
                    "[project]\nentry = \"test.ink\"\ndialect = \"brink\"\n",
                ),
                ("test.ink", FIXABLE),
            ],
        )
    }

    #[test]
    fn fixes_under_the_cursor_are_offered_with_their_tier_and_edits() {
        let tree = fixable_tree("fixes-at");
        let (mut session, _, config) = open_tree_with_config(&tree);
        let _ = analyze(&mut session, &config, 1);
        let QueryResult::FixesAt(fixes) = answer(
            &mut session,
            &QueryKind::FixesAt {
                path: "test.ink".to_owned(),
                offset: offset_of(FIXABLE, "greet(\"Al\"") + 2,
            },
        ) else {
            panic!("fixes must answer");
        };
        assert_eq!(fixes.len(), 1, "{fixes:?}");
        assert_eq!(fixes[0].code, "E031");
        assert_eq!(fixes[0].tier, Tier::Safe);
        assert!(fixes[0].caret.is_none());
        let mut text = FIXABLE.to_owned();
        for e in fixes[0].edits.iter().rev() {
            assert_eq!(e.path, "test.ink");
            text.replace_range(e.start as usize..e.end as usize, &e.new_text);
        }
        assert_eq!(text, FIXED);
    }

    #[test]
    fn offers_pair_each_fix_with_its_diagnostic_and_count_the_batch() {
        let tree = fixable_tree("offers");
        let (mut session, _, config) = open_tree_with_config(&tree);
        let analyzed = analyze(&mut session, &config, 1);
        let QueryResult::FixOffers(offers) = answer(&mut session, &QueryKind::FixOffers) else {
            panic!("offers must answer");
        };
        assert_eq!(offers.offers.len(), 1, "{:?}", offers.offers);
        let offer = &offers.offers[0];
        assert!(offer.batchable, "a Safe fix is batchable by default");
        // The pairing key is exactly the diagnostic the Problems row shows.
        let shown = analyzed
            .diagnostics
            .get("test.ink")
            .expect("the row exists")
            .iter()
            .any(|d| d.start == offer.start && d.end == offer.end && d.code == offer.code);
        assert!(shown, "an offer must name a visible row: {offer:?}");
        assert_eq!(offers.batchable, 1, "Fix all safe (1)");
    }

    #[test]
    fn fix_all_answers_the_fixed_text_and_leaves_the_session_as_found() {
        let tree = fixable_tree("fix-all");
        let (mut session, _, config) = open_tree_with_config(&tree);
        let _ = analyze(&mut session, &config, 1);
        let QueryResult::FixAll(report) = answer(
            &mut session,
            &QueryKind::FixAll {
                scope: FixScope::Project,
            },
        ) else {
            panic!("fix all must answer");
        };
        assert_eq!(report.applied, 1);
        assert_eq!(report.remaining, 0);
        assert!(!report.cap_hit);
        assert_eq!(
            report.files,
            vec![("test.ink".to_owned(), FIXED.to_owned())]
        );
        // Rolled back: the host owns the write, and its undo must not
        // snapshot the fixed text.
        let id = session.file_id("test.ink").expect("held");
        assert_eq!(session.source(id), Some(FIXABLE));
        // A file scope naming an unknown file is Unavailable, not a panic.
        assert!(matches!(
            answer(
                &mut session,
                &QueryKind::FixAll {
                    scope: FixScope::File("nope.ink".to_owned()),
                },
            ),
            QueryResult::Unavailable
        ));
    }

    #[test]
    fn refactors_are_ink_only_and_resolve_to_new_text() {
        let tree = Tree::new(
            "refactor",
            &[
                (
                    "main.ink",
                    "=== zeta ===\nZ.\n-> DONE\n=== alpha ===\nA.\n-> DONE\n",
                ),
                ("mod.brink", "flow main {\n  Hello.\n}\n"),
            ],
        );
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let QueryResult::Refactors(native) = answer(
            &mut session,
            &QueryKind::Refactors {
                path: "mod.brink".to_owned(),
                offset: 0,
            },
        ) else {
            panic!("refactors must answer");
        };
        assert!(native.is_empty(), "no ink structure in a .brink file");
        let QueryResult::Refactors(ink) = answer(
            &mut session,
            &QueryKind::Refactors {
                path: "main.ink".to_owned(),
                offset: 4,
            },
        ) else {
            panic!("refactors must answer");
        };
        let sort = ink
            .iter()
            .find(|r| r.title.to_lowercase().contains("sort"))
            .expect("two unsorted knots offer a sort");
        let QueryResult::ResolvedRefactor(Some(text)) = answer(
            &mut session,
            &QueryKind::ResolveRefactor {
                path: "main.ink".to_owned(),
                data: sort.data.clone(),
            },
        ) else {
            panic!("the sort must resolve");
        };
        assert!(text.find("=== alpha").expect("alpha") < text.find("=== zeta").expect("zeta"));
    }

    #[test]
    fn format_answers_the_formatter_output_for_ink_and_none_for_native() {
        let messy = "=== start ===\n  Hello.\n* [Go]\n        Went.\n-> DONE\n";
        let tree = Tree::new(
            "format",
            &[
                ("main.ink", messy),
                ("mod.brink", "flow main {\n     Hello.\n}\n"),
            ],
        );
        let (mut session, _) = open_tree(&tree);
        let _ = analyze(&mut session, &ConfigState::default(), 1);
        let QueryResult::Formatted(Some(text)) = answer(
            &mut session,
            &QueryKind::Format {
                path: "main.ink".to_owned(),
            },
        ) else {
            panic!("messy ink must format");
        };
        assert_ne!(text, messy);
        // The same answer `brink fmt` gives, with the project's indent.
        assert_eq!(
            text,
            brink_fmt::format(messy, &brink_fmt::FormatConfig::default())
        );
        // Formatting is idempotent, and an unchanged file answers None.
        session.update_source("main.ink", text.clone());
        assert!(matches!(
            answer(
                &mut session,
                &QueryKind::Format {
                    path: "main.ink".to_owned()
                }
            ),
            QueryResult::Formatted(None)
        ));
        // The ink formatter never touches a native file.
        assert!(matches!(
            answer(
                &mut session,
                &QueryKind::Format {
                    path: "mod.brink".to_owned()
                }
            ),
            QueryResult::Formatted(None)
        ));
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
