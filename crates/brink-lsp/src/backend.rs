use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use brink_analyzer::{AnalysisOptions, AnalysisResult, Dialect, LintLevel, LintPolicy, TypePolicy};
use brink_syntax::ast::AstNode;
use tokio::sync::{Notify, watch};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CodeLens, CodeLensOptions, CodeLensParams, CompletionItem,
    CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, DocumentRangeFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, FileChangeType, FileSystemWatcher, FoldingRange, FoldingRangeKind,
    FoldingRangeParams, FoldingRangeProviderCapability, GlobPattern, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, InlayHint as LspInlayHint, InlayHintLabel,
    InlayHintParams, Location, MarkupContent, MarkupKind, OneOf, ParameterInformation,
    ParameterLabel, Position, PrepareRenameResponse, Range, ReferenceParams, Registration,
    RenameOptions, RenameParams, SaveOptions, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensOptions, SemanticTokensParams, SemanticTokensRangeParams,
    SemanticTokensRangeResult, SemanticTokensResult, SemanticTokensServerCapabilities,
    ServerCapabilities, ServerInfo, SignatureHelp, SignatureHelpOptions, SignatureHelpParams,
    SignatureInformation, SymbolInformation, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, Url, WorkDoneProgressOptions, WorkspaceEdit,
    WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer};

use brink_ide::{
    CompletionContext, cursor_scope, detect_completion_context, is_visible_in_context,
    ref_arg_root_prefix,
};

use crate::backend::adapters::{
    diff_to_lsp_edits, domain_symbol_to_lsp, format_config_from_options, make_completion_item,
    make_stdlib_completion_item, ranges_overlap,
};
use crate::convert::{self, LineIndex};
use crate::semantic_tokens;

mod adapters;

/// Per-project analysis results, keyed by project root.
pub(crate) struct ProjectAnalyses {
    /// Per-project analysis, keyed by root `FileId`.
    by_root: HashMap<brink_ir::FileId, Arc<AnalysisResult>>,
    /// Reverse: file → all project roots that contain it (sorted).
    file_to_roots: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>>,
    /// Project membership: root → member file IDs.
    project_members: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>>,
}

impl ProjectAnalyses {
    /// Primary project for navigation (first/lowest root).
    fn for_file(&self, file: brink_ir::FileId) -> Option<&Arc<AnalysisResult>> {
        let roots = self.file_to_roots.get(&file)?;
        let root = roots.first()?;
        self.by_root.get(root)
    }

    /// All projects containing this file (for diagnostic union).
    fn all_for_file(&self, file: brink_ir::FileId) -> Vec<&Arc<AnalysisResult>> {
        self.file_to_roots
            .get(&file)
            .map(|roots| roots.iter().filter_map(|r| self.by_root.get(r)).collect())
            .unwrap_or_default()
    }

    /// Project members for the primary project of a file.
    fn project_files_for(&self, file: brink_ir::FileId) -> Option<&[brink_ir::FileId]> {
        let roots = self.file_to_roots.get(&file)?;
        let root = roots.first()?;
        self.project_members.get(root).map(Vec::as_slice)
    }
}

/// Client-declared, authoring-time-only compiler policy knobs: the T1b
/// dialect (docs/t1b-surface-spec.md §1, #589) and the TM-3 typed-mode
/// policy (docs/typed-mode-spec.md §1, #660). Bundled into one struct
/// (rather than two loose `Arc<Mutex<_>>` constructor params) because they
/// share identical lifetime/mutability characteristics — both are read once
/// from `initialize`'s `initializationOptions`, then shared unchanged
/// between the foreground `Backend` and the background `analysis_loop` task
/// for the life of the session. `Clone` is shallow (`Arc` clone): every
/// clone reads/writes the same underlying state.
#[derive(Clone)]
pub struct LanguageOptions {
    /// `"brink"` or `"strict-ink"`; defaults to `StrictInk`, matching
    /// `AnalysisOptions::default()`. Tooling-only — gates whether stdlib
    /// slice 1 completion/signature help are offered (#589), and (#599)
    /// feeds `analysis_loop` so its diagnostics analyze under the
    /// client-declared dialect too, instead of always defaulting to
    /// `StrictInk`.
    dialect: Arc<Mutex<Dialect>>,
    /// `"strict"` or `"gradual"`; `None` when neither the client nor a
    /// `brink.toml` ever said — the effective policy is then the
    /// dialect-keyed default (issue #1127, ruled 2026-07-19: brink →
    /// strict, strict-ink → gradual), resolved by
    /// `AnalysisOptions::type_policy()` at each analysis pass. Mirrors
    /// `dialect` exactly (#660: PR #656 left this reachable only via the
    /// compiler CLI's `--types strict`, never via the IDE/LSP surface) —
    /// feeds `analysis_loop` so its diagnostics analyze under the
    /// client-declared types policy too.
    types: Arc<Mutex<Option<TypePolicy>>>,
    /// Resolved `[lints]` policy (issue #1160/#1367): a discovered
    /// `brink.toml`'s `[lints]` table, applied via
    /// `AnalysisOptions::apply_project_config`, then overlaid with any
    /// client-declared `initializationOptions.lints`/`.denyWarnings` (issue
    /// #1417, `AnalysisOptions::apply_lint_overrides`) — both in
    /// `resolve_language_options`. Written only from that function's
    /// output, never read directly off `ConfigOverrides`. Feeds both
    /// `analysis_loop`'s `AnalysisOptions` and every diagnostic-publish
    /// site's `effective_severity` call, so a re-leveled code's
    /// LSP-published severity matches its build-gating severity.
    lints: Arc<Mutex<LintPolicy>>,
}

impl LanguageOptions {
    pub fn new() -> Self {
        Self {
            dialect: Arc::new(Mutex::new(Dialect::default())),
            types: Arc::new(Mutex::new(None)),
            lints: Arc::new(Mutex::new(LintPolicy::default())),
        }
    }

    /// Write a freshly `resolve_language_options`-resolved dialect/types/
    /// lints into the shared session state (poisoned-lock-safe, mirrors
    /// [`Backend::dialect`]) — the common tail of `initialize` and
    /// [`Backend::reload_brink_toml`], both of which compute a fresh
    /// resolution and must publish it identically. Takes `resolved` by value
    /// since neither caller reads it again afterward.
    fn store(&self, resolved: AnalysisOptions) {
        if let Ok(mut guard) = self.dialect.lock() {
            *guard = resolved.dialect;
        }
        if let Ok(mut guard) = self.types.lock() {
            *guard = resolved.types;
        }
        if let Ok(mut guard) = self.lints.lock() {
            *guard = resolved.lints;
        }
    }
}

/// Tier of a `publishDiagnostics` send. The notification handlers
/// (`did_open`/`did_change`/`did_save`) publish the fast **`PerFile`** set
/// (parse + lowering only); the background [`analysis_loop`] publishes the
/// full **`Analysis`** set (adds cross-file analyzer diagnostics). For the
/// same file content, `Analysis` is strictly richer than `PerFile`, so within
/// one generation it wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishTier {
    PerFile,
    Analysis,
}

/// What was last published for a file, plus the ordering key it went out
/// under. `generation` is the content revision (see [`Backend::mutate_db`]) the
/// set was computed against; `tier` breaks ties within one generation.
struct PublishRecord {
    generation: u64,
    tier: PublishTier,
    diags: Vec<tower_lsp::lsp_types::Diagnostic>,
}

/// Outcome of the anti-downgrade rule: whether to actually send the incoming
/// set to the client, and whether to record it as the new authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PublishDecision {
    send: bool,
    record: bool,
}

/// The [`DiagnosticsPublisher`] anti-downgrade rule, factored out pure so it
/// can be unit-tested without a live `Client`. Given what's currently recorded
/// for a file (`prev`) and an incoming `(generation, tier, diags)`, decide
/// whether to send and whether to record. See [`DiagnosticsPublisher`] for the
/// rationale.
fn publish_decision(
    prev: Option<&PublishRecord>,
    generation: u64,
    tier: PublishTier,
    diags: &[tower_lsp::lsp_types::Diagnostic],
) -> PublishDecision {
    match prev {
        // Never published: send (and record) only a non-empty set, so a clean
        // file never generates a spurious empty publish.
        None => {
            let nonempty = !diags.is_empty();
            PublishDecision {
                send: nonempty,
                record: nonempty,
            }
        }
        Some(prev) => {
            let is_downgrade = generation < prev.generation
                || (generation == prev.generation
                    && tier == PublishTier::PerFile
                    && prev.tier == PublishTier::Analysis);
            if is_downgrade {
                // Stale/less-complete relative to what's shown — drop it whole,
                // leaving both the record and the client untouched.
                PublishDecision {
                    send: false,
                    record: false,
                }
            } else {
                // At or above the current authority: send only if the set
                // actually changed, but always re-record so the ordering key
                // advances — a no-op-content upgrade still raises the
                // tier/generation, keeping later downgrade checks correct.
                PublishDecision {
                    send: prev.diags != diags,
                    record: true,
                }
            }
        }
    }
}

/// Serializes every `publishDiagnostics` send through one async critical
/// section so **wire order equals decision order** across the
/// notification-handler tasks and the background [`analysis_loop`] task, and
/// applies a monotone anti-downgrade rule (#615).
///
/// Two independent tasks publish diagnostics for the same file — a handler's
/// per-file publish and the loop's analysis publish — and nothing ordered
/// their sends. Under load the older/less-complete `PerFile` set could land on
/// the wire *after* the richer `Analysis` set; because the previous dedup
/// cache still recorded the `Analysis` set, the next pass computed an
/// identical set and suppressed the correction, permanently stranding the
/// client on the parse-only subset until the next edit.
///
/// Holding the mutex across the send fuses the decision and the send into one
/// atomic step, so no interleaving can reorder them. The rule: a publish
/// applies iff it is not a downgrade of what the file currently shows — a
/// strictly newer `generation` always wins; within one generation `Analysis`
/// beats `PerFile`; a `PerFile` never overwrites a same-or-newer `Analysis`.
/// The `generation` is a content revision advanced under the db lock (see
/// [`Backend::mutate_db`]) and read by both publishers under that same lock,
/// so a per-file set carries exactly its content's revision and the matching
/// background pass reads that revision or a newer one — the full set therefore
/// always wins the exchange for a given content, whichever way the two sends
/// interleave, while a routine edit's per-file set still out-generations the
/// previous analysis and shows instantly.
#[derive(Clone)]
pub struct DiagnosticsPublisher {
    client: Client,
    state: Arc<tokio::sync::Mutex<HashMap<brink_ir::FileId, PublishRecord>>>,
}

impl DiagnosticsPublisher {
    pub(crate) fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Publish `diags` for `file_id` (at `path`) unless doing so would
    /// downgrade what the client currently shows for that file (see the type
    /// docs for the ordering rule). `version` is forwarded verbatim as
    /// `PublishDiagnosticsParams.version`: per-file publishes carry the client
    /// document version, analysis publishes carry `None`.
    async fn publish(
        &self,
        file_id: brink_ir::FileId,
        path: &str,
        diags: Vec<tower_lsp::lsp_types::Diagnostic>,
        generation: u64,
        tier: PublishTier,
        version: Option<i32>,
    ) {
        // Held across the `.await` below — this is the whole point: decide and
        // send under one lock so wire order cannot diverge from decision order.
        let mut state = self.state.lock().await;

        let decision = publish_decision(state.get(&file_id), generation, tier, &diags);

        if decision.send
            && let Ok(uri) = Url::from_file_path(path)
        {
            self.client
                .publish_diagnostics(uri, diags.clone(), version)
                .await;
        }
        if decision.record {
            state.insert(
                file_id,
                PublishRecord {
                    generation,
                    tier,
                    diags,
                },
            );
        }
    }

    /// Forget a file's last-published record (on close/delete) so a later
    /// reopen republishes from scratch instead of being deduped against a
    /// now-stale set.
    async fn forget(&self, file_id: brink_ir::FileId) {
        self.state.lock().await.remove(&file_id);
    }
}

pub struct Backend {
    client: Client,
    db: Arc<Mutex<brink_db::ProjectDb>>,
    analysis_rx: watch::Receiver<Option<Arc<ProjectAnalyses>>>,
    analysis_trigger: Arc<Notify>,
    generation: Arc<AtomicU64>,
    publisher: DiagnosticsPublisher,
    workspace_roots: Arc<Mutex<Vec<PathBuf>>>,
    language: LanguageOptions,
    /// Client-declared `initializationOptions.dialect`/`.types`, captured
    /// once at `initialize` and reused by every later `brink.toml` reload
    /// (#1055 gap 2) — the client never resends `initializationOptions`
    /// mid-session, only the file's own contribution can change.
    config_overrides: Arc<Mutex<ConfigOverrides>>,
    /// The outcome of `initialize`'s one-time `brink.toml` load, stashed
    /// here so `initialized()` can publish its diagnostic (if any) — the LSP
    /// spec forbids the server sending notifications before the client has
    /// the `InitializeResult`, which the `initialized` notification confirms
    /// receipt of (#1055 gap 1).
    initial_config_outcome: Arc<Mutex<Option<ConfigLoadOutcome>>>,
}

impl Backend {
    pub fn new(
        client: Client,
        db: Arc<Mutex<brink_db::ProjectDb>>,
        analysis_rx: watch::Receiver<Option<Arc<ProjectAnalyses>>>,
        analysis_trigger: Arc<Notify>,
        generation: Arc<AtomicU64>,
        publisher: DiagnosticsPublisher,
        language: LanguageOptions,
    ) -> Self {
        Self {
            client,
            db,
            analysis_rx,
            analysis_trigger,
            generation,
            publisher,
            workspace_roots: Arc::new(Mutex::new(Vec::new())),
            language,
            config_overrides: Arc::new(Mutex::new(ConfigOverrides::default())),
            initial_config_outcome: Arc::new(Mutex::new(None)),
        }
    }

    /// The registered T1b compiler dialect (defaults to `StrictInk`, poisoned-
    /// lock-safe via `map_or_else` — never panics on a poisoned mutex).
    fn dialect(&self) -> Dialect {
        self.language
            .dialect
            .lock()
            .map_or_else(|_| Dialect::default(), |g| *g)
    }

    /// The effective TM-3 `types` policy: an explicit client/file value if
    /// one was ever registered, else the dialect-keyed default (mirrors
    /// `IdeSession::type_policy`, poisoned-lock-safe like [`Self::dialect`]).
    fn type_policy(&self) -> TypePolicy {
        let explicit = self.language.types.lock().map_or_else(|_| None, |g| *g);
        brink_analyzer::resolve_type_policy(self.dialect(), explicit)
    }

    /// The resolved `[lints]` policy from the last-loaded `brink.toml`
    /// (poisoned-lock-safe like [`Self::dialect`]; `LintPolicy::default()` —
    /// a no-op — until a project file with a `[lints]` table is discovered).
    fn lints(&self) -> LintPolicy {
        self.language
            .lints
            .lock()
            .map_or_else(|_| LintPolicy::default(), |g| g.clone())
    }

    fn uri_to_path(uri: &Url) -> Option<String> {
        uri.to_file_path()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Publish (or clear) the `textDocument/publishDiagnostics` for a
    /// [`ConfigLoadOutcome`]'s file, if it names one (a missing `brink.toml`
    /// has nothing to publish). An outcome with no diagnostic still
    /// publishes an empty set — clearing a previously shown one once an edit
    /// fixes a malformed `brink.toml` (#1055 gap 1).
    async fn publish_config_outcome(&self, outcome: &ConfigLoadOutcome) {
        let Some(path) = &outcome.path else {
            return;
        };
        let Ok(uri) = Url::from_file_path(path) else {
            return;
        };
        let diags: Vec<_> = outcome.diagnostic.clone().into_iter().collect();
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Re-run `brink.toml` discovery + parse + apply against the current
    /// workspace roots and the client-declared overrides captured at
    /// `initialize`, so edits to the file — or a
    /// `workspace/didChangeConfiguration` notification — take effect without
    /// a client restart (#1055 gap 2). Writes the resolved dialect/types
    /// into the shared [`LanguageOptions`] and publishes (or clears) the
    /// file's diagnostic exactly like `initialized()`'s one-time load
    /// (#1055 gap 1, see [`resolve_language_options`]). Does not itself call
    /// [`Self::trigger_analysis`] — callers trigger re-analysis themselves,
    /// alongside whatever else their notification handler already does.
    async fn reload_brink_toml(&self) {
        let roots = match self.workspace_roots.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let overrides = match self.config_overrides.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };

        let (resolved, outcome) = resolve_language_options(&overrides, &roots);

        self.language.store(resolved);
        // A `brink.toml` appearing, moving, or disappearing moves the native
        // source root with it (#1572), so re-register it here too — otherwise
        // every native module name would stay pinned to whatever root the
        // session started with.
        self.register_native_root(&roots, &outcome);

        self.publish_config_outcome(&outcome).await;
    }

    /// Register this session's native source root with `ProjectDb` (#1572),
    /// so the module identity the editor mints for a `.brink` file equals the
    /// identity a real compile of the same tree mints.
    ///
    /// The LSP keys `ProjectDb` by absolute OS path — it must, since every
    /// path it holds round-trips through a `file://` URI — but a native
    /// file's module is contractually a function of its *root-relative* key.
    /// Declaring the root closes that gap at the one place the identity
    /// function is fed (see [`brink_db::ProjectDb::set_native_root`]).
    ///
    /// Called from `initialize` and from every later
    /// [`reload_brink_toml`](Self::reload_brink_toml). Goes through
    /// [`mutate_db`](Self::mutate_db) so the content generation advances:
    /// changing the root changes every native module name, which is a real
    /// input change the background pass must re-analyze against.
    fn register_native_root(&self, roots: &[PathBuf], outcome: &ConfigLoadOutcome) {
        let root = native_source_root(roots, outcome)
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| !p.is_empty());
        self.mutate_db(|db| db.set_native_root(root));
    }

    /// Publish per-file diagnostics (parse + lowering only, no analysis).
    /// This gives instant syntax error feedback without waiting for background analysis.
    ///
    /// `version` is the client document version this set was computed from,
    /// when the triggering notification carries one (`didOpen`/`didChange`).
    /// Passing it through in `PublishDiagnosticsParams.version` is what the
    /// protocol intends, and it also distinguishes these per-file publishes
    /// from background [`analysis_loop`] publishes (which analyze a db
    /// snapshot, not a specific client document version, and so send no
    /// version). Integration tests rely on that distinction: this publish
    /// runs on the notification-handler task and can land on the wire
    /// *between* a background pass's publish and that pass's
    /// [`BackgroundAnalysisComplete`] signal (#615), so a test waiting for
    /// background-analysis diagnostics must be able to ignore it.
    ///
    /// Routed through [`DiagnosticsPublisher`] (tagged `PerFile`) rather than
    /// sent directly, so the anti-downgrade rule prevents a delayed per-file
    /// send from clobbering a fuller analysis set already on screen.
    async fn publish_perfile_diagnostics(&self, path: &str, version: Option<i32>) {
        let types = self.type_policy();
        let lints = self.lints();
        let (file_id, generation, lsp_diags) = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(path) else {
                return;
            };

            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return;
            };
            let idx = LineIndex::new(&source);

            let raw_diags: Vec<brink_ir::Diagnostic> =
                db.file_diagnostics(file_id).unwrap_or_default().to_vec();
            let suppressions = db.suppressions(file_id).cloned().unwrap_or_default();
            let filtered = brink_ir::suppressions::apply_suppressions(
                file_id,
                &source,
                raw_diags,
                &suppressions,
            );

            let lsp_diags: Vec<_> = filtered
                .iter()
                .map(|d| convert::diagnostic_to_lsp(d, &idx, types, &lints))
                .collect();

            // Generation this set reflects: read under the same db lock as the
            // content, so `(content, generation)` is a consistent pair (the bump
            // happens inside `mutate_db`, under this lock). A per-file publish
            // therefore carries exactly its content's revision, so the matching
            // background pass — which reads the same revision, or a newer one if
            // edits coalesced — ties or beats it and wins the anti-downgrade
            // exchange, while a *routine* edit's set still out-generations the
            // previous analysis and shows instantly.
            let generation = self.generation.load(Ordering::Relaxed);
            (file_id, generation, lsp_diags)
        };

        self.publisher
            .publish(
                file_id,
                path,
                lsp_diags,
                generation,
                PublishTier::PerFile,
                version,
            )
            .await;
    }

    /// Apply a content mutation to the db and advance the content generation
    /// in the **same** critical section.
    ///
    /// The generation is a content revision counter that the
    /// [`DiagnosticsPublisher`] uses to order publishes. It must move in lock
    /// step with the content it versions: every reader that snapshots a
    /// `(content, generation)` pair does so under this same db lock, so
    /// bumping here — rather than later in [`trigger_analysis`] — is what makes
    /// the pair consistent. If the bump happened after the lock was released, a
    /// reader could observe new content with an old generation (or vice-versa),
    /// and the anti-downgrade rule would misfire: a routine edit's per-file
    /// publish would tie the previous analysis and be dropped as a same-
    /// generation downgrade, silently killing instant syntax feedback.
    fn mutate_db<R>(&self, f: impl FnOnce(&mut brink_db::ProjectDb) -> R) -> R {
        let mut db = lock_db(&self.db);
        let out = f(&mut db);
        self.generation.fetch_add(1, Ordering::Relaxed);
        out
    }

    /// Notify the background analysis task that inputs changed. The content
    /// generation is advanced by [`mutate_db`](Self::mutate_db) /
    /// [`load_file_from_disk`](Self::load_file_from_disk) under the db lock,
    /// not here, so it stays a consistent pair with the content snapshot.
    fn trigger_analysis(&self) {
        self.analysis_trigger.notify_one();
    }

    /// Chase INCLUDE directives from a file that's already in the db.
    ///
    /// Deliberately never checks `brink_source_tree::is_ignored_dir` (see
    /// that function's "Admission policy" doc): an `INCLUDE` target is an
    /// explicit reference from source the workspace walk already admitted,
    /// not something this code discovered by scanning a directory, so an
    /// `INCLUDE node_modules/shared/lib.ink` is loaded like any other
    /// include (issue #1424).
    fn chase_includes(&self, path: &str) {
        let includes = {
            let db = lock_db(&self.db);
            let Some(fid) = db.file_id(path) else {
                return;
            };
            let Some(hir) = db.hir(fid) else { return };
            hir.includes
                .iter()
                .map(|inc| inc.file_path.clone())
                .collect::<Vec<_>>()
        };

        let base_dir = std::path::Path::new(path).parent();
        for inc_path in &includes {
            if let Some(resolved) =
                base_dir.map(|d| d.join(inc_path).to_string_lossy().into_owned())
            {
                self.load_file_from_disk(&resolved);
            }
        }
    }

    /// Load a file from disk into the database if not already present.
    /// Recursively chases INCLUDE directives.
    ///
    /// This is the shared admission sink for every explicit-path load —
    /// [`Self::chase_includes`] (an `INCLUDE` target) and [`Self::walk_and_load`]
    /// (a path `collect_source_files` produced while walking the workspace
    /// root) both call it, and it recurses into itself while chasing
    /// further includes. It never applies `brink_source_tree::is_ignored_dir`
    /// itself, because each caller has already decided: `collect_source_files`
    /// prunes ignored directories upstream during the walk, while
    /// `chase_includes` and `did_open` deliberately admit unconditionally,
    /// per `brink_source_tree::is_ignored_dir`'s "Admission policy" doc
    /// (issue #1424).
    fn load_file_from_disk(&self, path: &str) {
        // Check if already loaded
        {
            let db = lock_db(&self.db);
            if db.file_id(path).is_some() {
                return;
            }
        }

        let Ok(contents) = std::fs::read_to_string(path) else {
            tracing::warn!(path, "failed to read file from disk");
            return;
        };

        let mut db = lock_db(&self.db);
        // Double-check under lock
        if db.file_id(path).is_some() {
            return;
        }
        db.set_file(path, contents);
        // Content added — advance the generation under this same lock (see
        // `mutate_db`); this path can't use the helper because it keeps reading
        // the db (include chasing) after the mutation.
        self.generation.fetch_add(1, Ordering::Relaxed);

        // Collect includes to chase (release the lock first)
        let includes = db
            .file_id(path)
            .and_then(|fid| db.hir(fid))
            .map(|hir| {
                hir.includes
                    .iter()
                    .map(|inc| inc.file_path.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let base_dir = std::path::Path::new(path).parent();
        let resolved: Vec<String> = includes
            .iter()
            .filter_map(|inc_path| {
                base_dir
                    .map(|d| d.join(inc_path))
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .collect();
        drop(db);

        for resolved_path in resolved {
            self.load_file_from_disk(&resolved_path);
        }
    }

    /// Scan workspace directories for source files (`.ink` and `.brink`) and
    /// load them all.
    fn load_workspace_files(&self) {
        let roots = match self.workspace_roots.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };

        for root in &roots {
            self.walk_and_load(root);
        }

        // Rebuild include graph now that all files are loaded — set_file
        // can only create edges to files already in the db, so files loaded
        // before their include targets will have missing edges.
        let mut db = lock_db(&self.db);
        db.rebuild_include_graph();
    }

    /// Recursively walk a directory, loading every source file it holds. The
    /// walk itself is delegated to [`collect_source_files`] — a free function
    /// with no `Client` dependency — so pruning can be unit-tested directly
    /// without standing up a full `Backend` (issue #1402).
    fn walk_and_load(&self, dir: &std::path::Path) {
        for path in collect_source_files(dir) {
            let path_str = path.to_string_lossy().into_owned();
            self.load_file_from_disk(&path_str);
        }
    }
}

/// Recursively collect every source file path under `dir` — `.ink` **and**
/// native `.brink` (issue #1562: the scan enumerated `.ink` alone, so in a
/// native workspace only the files the user happened to `didOpen` ever
/// reached the db and every cross-file feature was blind to the rest) — via
/// the shared
/// [`brink_source_tree::Walk`] — so it never descends into a directory
/// [`brink_source_tree::IGNORED_DIR_NAMES`] names (`target/`, `.git/`,
/// `node_modules/`). Before issue #1402, this was the third unpruned
/// recursive walk in the codebase (after #1370's config discovery and
/// #1381's native compile walk), so opening a workspace with a large
/// `target/` or `node_modules/` tree made the LSP enumerate all of it on
/// load; issue #1433 replaced the hand-written recursion #1402 added — and
/// its hand-placed `is_ignored_dir` call — with the shared walk, which
/// applies the same policy by construction.
///
/// `dir` itself — the workspace root a caller starts the walk from — is
/// never checked against the policy, only the entries found *while
/// descending* from it (a [`brink_source_tree::Walk`] contract, not a local
/// convention): a workspace legitimately rooted inside e.g. `node_modules/`
/// (vendored ink content opened directly as a folder) still has its own
/// files admitted; only a genuinely nested ignored directory further below
/// the root is pruned (issue #1424, verifying the root-relative scoping
/// #1415 gave [`path_under_ignored_dir`] holds here too).
///
/// Unreadable directories are skipped rather than reported — a workspace
/// scan is best-effort, exactly as it was when this function swallowed its
/// own `read_dir` errors.
fn collect_source_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    brink_source_tree::Walk::new(dir)
        .flatten()
        .filter(|entry| !entry.is_dir() && is_source_path(entry.path()))
        .map(brink_source_tree::WalkEntry::into_path)
        .collect()
}

/// Whether `path` names a source file this server tracks: ink (`.ink`) or
/// native (`.brink`). The same two extensions the `initialized` handler's
/// file watchers register, and the same axis `brink-db`'s own frontend
/// dispatch splits on (`.brink` → native parser, everything else → ink).
fn is_source_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "ink" || ext == "brink")
}

fn lock_db(db: &Arc<Mutex<brink_db::ProjectDb>>) -> std::sync::MutexGuard<'_, brink_db::ProjectDb> {
    match db.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Map a domain `InlayHintKind` to the LSP's own (`PARAMETER`/`TYPE` are the
/// only two the spec defines). TM-5's new `InferredType` kind is a type
/// hint, not a parameter-name hint — an explicit arm here rather than a
/// blanket default, so a future new variant fails to compile instead of
/// silently inheriting `PARAMETER` (CLAUDE.md's wildcard-arm rule).
fn lsp_inlay_hint_kind(
    kind: &brink_ide::inlay_hints::InlayHintKind,
) -> tower_lsp::lsp_types::InlayHintKind {
    match kind {
        brink_ide::inlay_hints::InlayHintKind::Parameter
        | brink_ide::inlay_hints::InlayHintKind::Value => {
            tower_lsp::lsp_types::InlayHintKind::PARAMETER
        }
        brink_ide::inlay_hints::InlayHintKind::InferredType => {
            tower_lsp::lsp_types::InlayHintKind::TYPE
        }
    }
}

/// Snapshot of analysis + per-file data needed for navigation handlers.
struct NavigationSnapshot {
    analysis: Arc<AnalysisResult>,
    source: String,
    file_id: brink_ir::FileId,
    /// (`FileId`, path, source) for files in the same project.
    project_files: Vec<(brink_ir::FileId, String, String)>,
}

impl Backend {
    /// Take a consistent snapshot without running analysis.
    /// Reads the latest analysis result from the watch channel, scoped to the
    /// project that contains the given file.
    fn navigation_snapshot(&self, path: &str) -> Option<NavigationSnapshot> {
        let projects = self.analysis_rx.borrow().clone()?;
        let db = lock_db(&self.db);
        let file_id = db.file_id(path)?;
        let analysis = Arc::clone(projects.for_file(file_id)?);
        let source = db.source(file_id)?.to_owned();

        // Only include files from the same project
        let project_files: Vec<_> = projects
            .project_files_for(file_id)
            .unwrap_or(&[])
            .iter()
            .filter_map(|&fid| {
                let p = db.file_path(fid)?.to_owned();
                let s = db.source(fid)?.to_owned();
                Some((fid, p, s))
            })
            .collect();

        Some(NavigationSnapshot {
            analysis,
            source,
            file_id,
            project_files,
        })
    }
}

/// Read `initializationOptions.<key>` as a string, if the client set it at
/// all — regardless of whether the value maps to a recognized variant. This
/// is "the client passed an explicit value", the strongest tier of the
/// #1030 precedence rule (see [`resolve_language_options`]). For `dialect`,
/// an unrecognized string still counts as explicit and falls through the
/// `match`'s `_` arm to the same default a missing key would. For `types`
/// (NS-A9, #1127): an unrecognized string is treated as **unset** — the
/// dialect-keyed default applies — because since the strict default landed,
/// coercing garbage to a fixed policy would let a typo silently opt a
/// brink-dialect project out of strict (mirrors the wasm editor's
/// unrecognized-value behavior).
fn explicit_initialization_option<'a>(params: &'a InitializeParams, key: &str) -> Option<&'a str> {
    params
        .initialization_options
        .as_ref()
        .and_then(|opts| opts.get(key))
        .and_then(|v| v.as_str())
}

/// Read `initializationOptions.<key>` as a bool, if the client set it —
/// mirrors [`explicit_initialization_option`] for `denyWarnings` (issue
/// #1417): `Some(_)` only when the client actually set the key to a JSON
/// boolean, `None` for a missing key *or* a key holding a non-bool value
/// (the value is simply not "an explicit bool", same "unset" treatment
/// `types`' unrecognized-string case gets).
fn explicit_initialization_bool(params: &InitializeParams, key: &str) -> Option<bool> {
    params
        .initialization_options
        .as_ref()
        .and_then(|opts| opts.get(key))
        .and_then(serde_json::Value::as_bool)
}

/// Read `initializationOptions.lints` — a client-declared per-code
/// lint-level override map (issue #1417), the LSP's counterpart of the
/// CLI's repeatable `--deny`/`--warn`/`--allow <CODE>` flags (#1373) and
/// `BrinkPlugin::with_config`'s `ProjectConfig.lints` (#1394). Accepts a
/// JSON object `{ "<CODE>": "deny" | "warn" | "allow" }` — the same three
/// strings a `brink.toml` `[lints]` table accepts
/// (`brink_project_config::parse_lint_level`). A missing key resolves to no
/// overrides at all (an empty map, the same as never setting the field). A
/// present but non-object value, or a per-code value that isn't one of the
/// three recognized strings, is skipped with a `tracing::warn!` — the same
/// "warn, never silently drop" channel [`resolve_language_options`] already
/// uses for a `brink.toml`'s own unknown keys — rather than resolving to a
/// hard `initialize` failure; the real code/overridability validation still
/// happens once, downstream, in
/// `AnalysisOptions::apply_lint_overrides` (#1160's `validate_lint_code`
/// gate).
fn explicit_initialization_lints(params: &InitializeParams) -> BTreeMap<String, LintLevel> {
    let mut lints = BTreeMap::new();
    let Some(obj) = params
        .initialization_options
        .as_ref()
        .and_then(|opts| opts.get("lints"))
        .and_then(serde_json::Value::as_object)
    else {
        return lints;
    };
    for (code, value) in obj {
        match value.as_str() {
            Some("deny") => {
                lints.insert(code.clone(), LintLevel::Deny);
            }
            Some("warn") => {
                lints.insert(code.clone(), LintLevel::Warn);
            }
            Some("allow") => {
                lints.insert(code.clone(), LintLevel::Allow);
            }
            _ => {
                tracing::warn!(
                    "initializationOptions.lints.{code}: expected \"allow\" | \"warn\" | \"deny\", ignored"
                );
            }
        }
    }
    lints
}

/// Client-declared `initializationOptions.dialect`/`.types`/`.lints`/
/// `.denyWarnings`, resolved once from `InitializeParams` (`Some(_)`/a
/// non-empty map means the client set the key at all — see
/// [`explicit_initialization_option`]/[`explicit_initialization_lints`]) and
/// reused, unchanged, by every later `brink.toml` reload (#1055 gap 2): the
/// client never resends `initializationOptions` mid-session, only the
/// file's own contribution to [`resolve_language_options`] can change.
///
/// `lints`/`deny_warnings` (issue #1417) extend the same override tier
/// `dialect`/`types` established (#1030) to `[lints]`, closing the gap this
/// type's own doc comment used to name explicitly ("no
/// `initializationOptions` equivalent exists for `[lints]`"). Not `Copy`
/// (unlike the pre-#1417 `dialect`/`types`-only version) — `lints` is a
/// `BTreeMap`, so callers now `.clone()` where they used to deref-copy.
#[derive(Debug, Clone, Default)]
struct ConfigOverrides {
    dialect: Option<Dialect>,
    types: Option<TypePolicy>,
    lints: BTreeMap<String, LintLevel>,
    deny_warnings: Option<bool>,
}

impl ConfigOverrides {
    fn from_initialize_params(params: &InitializeParams) -> Self {
        Self {
            dialect: explicit_initialization_option(params, "dialect").map(|v| match v {
                "brink" => Dialect::Brink,
                _ => Dialect::StrictInk,
            }),
            types: explicit_initialization_option(params, "types").and_then(|v| match v {
                "strict" => Some(TypePolicy::Strict),
                "gradual" => Some(TypePolicy::Gradual),
                _ => None,
            }),
            lints: explicit_initialization_lints(params),
            deny_warnings: explicit_initialization_bool(params, "denyWarnings"),
        }
    }
}

/// The result of one `brink.toml` discovery + load attempt: which file (if
/// any) was found, and — if reading or parsing it failed — the diagnostic to
/// surface on it (#1055 gap 1, see [`config_error_diagnostic`]).
///
/// `path: None` means no `brink.toml` was found anywhere from the workspace
/// root up — byte-identical to pre-#1005 "use defaults", nothing to
/// publish. `path: Some(_)` with `diagnostic: None` means the file loaded
/// cleanly; callers still "publish" that (as an empty diagnostic set) so a
/// fix supersedes a diagnostic shown for a previous, malformed revision of
/// the same file.
#[derive(Debug, Clone, Default)]
struct ConfigLoadOutcome {
    path: Option<PathBuf>,
    diagnostic: Option<tower_lsp::lsp_types::Diagnostic>,
}

/// Best-effort byte-span → LSP range for a `ConfigError` (malformed TOML
/// syntax carries a span — see `ConfigError::span`, #1384). Every other
/// `ConfigError` variant (unreadable file, a recognized key holding the
/// wrong shape/value) has no location narrower than "the whole file", and a
/// span this project's `u32`-based `TextSize` can't represent falls back the
/// same way — `None` here means the caller anchors the diagnostic at the
/// file's start instead.
fn toml_span_to_lsp_range(
    error: &brink_project_config::ConfigError,
    source: Option<&str>,
) -> Option<Range> {
    let span = error.span()?;
    let source = source?;
    let start = u32::try_from(span.start).ok()?;
    let end = u32::try_from(span.end).ok()?;
    let idx = LineIndex::new(source);
    Some(convert::to_lsp_range(
        rowan::TextRange::new(start.into(), end.into()),
        &idx,
    ))
}

/// Convert a `brink_project_config::ConfigError` — surfaced while loading
/// `brink.toml` — into an LSP diagnostic anchored on the file (#1055 gap 1:
/// previously only `tracing::warn!`-ed, so an author editing a malformed
/// `brink.toml` had no client-visible signal that the session had silently
/// fallen back to defaults). `source` is the file's text, when it was read
/// successfully — needed to convert `ConfigError::Toml`'s byte span into an
/// LSP line/column range (see [`toml_span_to_lsp_range`]).
///
/// Severity is always `Error`: every `ConfigError` variant (unreadable
/// file, malformed TOML, a recognized key holding the wrong shape/value) is
/// a genuine config-load failure — `ConfigError` carries no `DiagnosticCode`
/// and has no warning-level variant, unlike `brink_ir::Diagnostic` (whose
/// severity is code-dependent and must go through
/// [`convert::severity_to_lsp`] with
/// [`brink_analyzer::effective_severity`], see
/// [`convert::diagnostic_to_lsp`]). This still routes through
/// `convert::severity_to_lsp` rather than naming the `tower_lsp` variant
/// directly, so the mapping stays centralized in one place (#1163).
fn config_error_diagnostic(
    error: &brink_project_config::ConfigError,
    source: Option<&str>,
) -> tower_lsp::lsp_types::Diagnostic {
    tower_lsp::lsp_types::Diagnostic {
        range: toml_span_to_lsp_range(error, source).unwrap_or_default(),
        severity: Some(convert::severity_to_lsp(brink_ir::Severity::Error)),
        source: Some("brink.toml".to_owned()),
        message: error.to_string(),
        ..Default::default()
    }
}

/// Resolve the effective dialect/types policy for this session, reconciling
/// `overrides` (captured once from `initializationOptions`, see
/// [`ConfigOverrides::from_initialize_params`]) with a discovered
/// `brink.toml` (#1005) under the **same ruled precedence every other mount
/// follows** (#1030): the file supplies the default, an explicit client
/// option always wins over it. Layering, lowest to highest priority:
///
/// 1. `AnalysisOptions::default()` (`strict-ink` / `gradual`);
/// 2. a `brink.toml` discovered from the (first) workspace root — a missing
///    file changes nothing, byte-identical to pre-#1005/#1030 behavior;
/// 3. `overrides.dialect`/`.types`, if the client actually set them at
///    `initialize` — this always overrides the file, never the other way
///    around.
///
/// Discovery is relative to the **workspace root**, not any single open
/// file: at `initialize` time the LSP has no "entry file" the way the CLI's
/// `brink compile <entry>` does, only workspace folders. This reuses
/// `brink-project-config`'s [`brink_project_config::find_config`] walk
/// directly (rather than [`brink_project_config::discover_from_entry`],
/// which expects a file path to take the parent of) — the walking logic
/// itself is not duplicated. With multiple workspace folders, only the
/// first is consulted, mirroring the single-session, single-project-root
/// assumption `Backend::dialect()`/`language` already make elsewhere.
///
/// Unknown keys in the file are logged as warnings (never errors — forward
/// compat, matching the CLI/`brink ide` surfaces) and never earn a
/// diagnostic. A malformed or unreadable `brink.toml` is still only ever a
/// `tracing::warn!` here — a language server must keep serving the session
/// rather than refuse to initialize/reload — but also earns a client-visible
/// diagnostic via the returned [`ConfigLoadOutcome`] (#1055 gap 1): callers
/// publish it (see [`Backend::publish_config_outcome`]), they don't fail on
/// it.
///
/// Called once from `initialize` (via `initialized`, which defers the
/// publish) and again by [`Backend::reload_brink_toml`] on
/// `workspace/didChangeConfiguration` and a file-watched edit to
/// `brink.toml` itself (#1055 gap 2), so edits apply without a client
/// restart.
fn resolve_language_options(
    overrides: &ConfigOverrides,
    roots: &[PathBuf],
) -> (AnalysisOptions, ConfigLoadOutcome) {
    let mut options = AnalysisOptions::default();
    let mut outcome = ConfigLoadOutcome::default();

    if let Some(root) = roots.first()
        && let Some(path) = brink_project_config::find_config(root)
    {
        outcome.path = Some(path.clone());
        match std::fs::read_to_string(&path) {
            Ok(text) => match brink_project_config::parse_str_at(path.display().to_string(), &text)
            {
                Ok((config, warnings)) => {
                    for warning in &warnings {
                        tracing::warn!("[{}] {warning}", path.display());
                    }
                    let lint_warnings = options.apply_project_config(
                        &config,
                        overrides.dialect.is_some(),
                        overrides.types.is_some(),
                    );
                    for warning in &lint_warnings {
                        tracing::warn!("[{}] {warning}", path.display());
                    }
                }
                Err(e) => {
                    // `e`'s own `Display` already names `path` (#1384:
                    // `parse_str_at` threads it into every `ConfigError`),
                    // so this no longer needs its own `path.display()`
                    // prefix the way the pre-#1384 bare `parse_str` did.
                    tracing::warn!("failed to parse: {e}");
                    outcome.diagnostic = Some(config_error_diagnostic(&e, Some(&text)));
                }
            },
            Err(e) => {
                tracing::warn!("failed to read {}: {e}", path.display());
                let err = brink_project_config::ConfigError::Io {
                    path: path.clone(),
                    source: e,
                };
                outcome.diagnostic = Some(config_error_diagnostic(&err, None));
            }
        }
    }

    // T1b compiler dialect (docs/t1b-surface-spec.md §1, #589): an explicit
    // `initializationOptions.dialect` ("brink" or "strict-ink"; any other
    // value keeps whatever the file/default already resolved to) always
    // wins over the file, per the precedence above.
    if let Some(dialect) = overrides.dialect {
        options.dialect = dialect;
    }

    // TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660): same rule,
    // mirroring `dialect` directly above. `Strict` requires `dialect =
    // brink` (a config-error diagnostic otherwise, `E064`) — the client's
    // responsibility, same as the compiler CLI.
    if let Some(types) = overrides.types {
        options.types = Some(types);
    }

    // `[lints]`/`deny-warnings` CLI/API override tier (issue #1417),
    // completing the #1373/#1394 seam for the LSP surface: applied last, on
    // top of whatever the file above just resolved, so an explicit
    // `initializationOptions.lints`/`.denyWarnings` always wins over the
    // same code in a discovered `brink.toml`'s `[lints]` table — the same
    // `CLI/API > file > default` precedence `dialect`/`types` follow.
    let lint_override_warnings =
        options.apply_lint_overrides(&overrides.lints, overrides.deny_warnings);
    for warning in &lint_override_warnings {
        // Same "warn, never silently drop" channel as the file-sourced
        // `[lints]` warnings above (house rule).
        tracing::warn!("{warning}");
    }

    (options, outcome)
}

/// The directory this session's native `.brink` keys are root-relative to
/// (issue #1572), or `None` when there is nothing to anchor to (no workspace
/// folder and no `brink.toml` — a bare `stdin`-only session).
///
/// Mirrors the compiler's own [`brink_driver::native_source_root`] rule at
/// the one input the LSP actually has: the compiler resolves the root from
/// the *entry file* (the governing `brink.toml`'s directory, else the entry's
/// own directory), while a language server has no entry file at all, only
/// workspace folders. So the same two-step applies with the workspace root
/// standing in for the entry directory — the discovered `brink.toml`'s
/// directory wins (`outcome.path` is the very file
/// [`resolve_language_options`] found by walking up from the first workspace
/// root), else the first workspace root itself.
///
/// Consulting only the *first* root matches every other project-scoped
/// decision this server makes (`resolve_language_options`, `Backend::dialect`);
/// a genuinely multi-root native workspace is issue #1572's separate
/// project-extent finding, not this one.
fn native_source_root(roots: &[PathBuf], outcome: &ConfigLoadOutcome) -> Option<PathBuf> {
    outcome
        .path
        .as_ref()
        .and_then(|config| config.parent())
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .or_else(|| roots.first().cloned())
}

/// True if `path`'s file name is exactly
/// `brink_project_config::CONFIG_FILE_NAME` ("brink.toml") — used to route
/// `did_change_watched_files` events for the project config file to
/// [`Backend::reload_brink_toml`] instead of `ProjectDb`, which tracks
/// `.ink` and `.brink` source but never the project config file itself
/// (#1055 gap 2).
fn is_brink_toml_path(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        == Some(brink_project_config::CONFIG_FILE_NAME)
}

/// True if any component of `path`, *below whichever `roots` entry contains
/// it*, is a [`brink_source_tree::is_ignored_dir`] name (`target/`, `.git/`,
/// `node_modules/`) — i.e. `path` lives inside a directory a recursive walk
/// would never descend into. Consulted at two sites in
/// `did_change_watched_files` (see `is_ignored_dir`'s "Admission policy"
/// doc): the `brink.toml` route, which skips unconditionally with no
/// already-tracked exemption (an ignored-dir config is never authoritative,
/// per that branch's own inline comment); and the `.ink` admission gate,
/// where a path already tracked in `ProjectDb` keeps syncing regardless of
/// what this returns.
///
/// `did_change_watched_files` receives whole file paths from the client's
/// file-watcher subscription rather than walking a directory tree itself, so
/// [`collect_source_files`]'s per-entry prune (which only ever sees one path
/// component at a time while descending, and so never tests the workspace
/// root's own name or its ancestors) doesn't apply directly here — this walks every component of the already-complete path
/// instead, using the same shared predicate (issue #1415: `did_change_watched_files`
/// was a fourth unpruned path admitting `target/`/`.git/`/`node_modules/`
/// files into `ProjectDb`, after #1370's config discovery, #1381's native
/// compile walk, and #1402's LSP workspace-load walk).
///
/// `roots` scopes the check to mirror `collect_source_files`'s walk exactly:
/// the longest matching entry of `roots` is stripped from `path` before
/// checking components, so a workspace root that itself lives under e.g.
/// `node_modules/` (vendored ink content opened directly as a folder) still
/// has its own files admitted — only descendants' ignored-dir components
/// count, never the root's own ancestry (#1415 review: path-scope
/// divergence from the prune this helper claims to mirror). A path with no
/// matching root falls back to checking every component, same as before.
fn path_under_ignored_dir(path: &str, roots: &[std::path::PathBuf]) -> bool {
    let full = std::path::Path::new(path);
    let scoped = roots
        .iter()
        .filter(|root| full.starts_with(root))
        .max_by_key(|root| root.as_os_str().len())
        .and_then(|root| full.strip_prefix(root).ok())
        .unwrap_or(full);
    scoped
        .components()
        .any(|c| brink_source_tree::is_ignored_dir(c.as_os_str()))
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Save workspace roots for use in initialized()
        let mut roots = Vec::new();
        if let Some(folders) = &params.workspace_folders {
            for folder in folders {
                if let Ok(path) = folder.uri.to_file_path() {
                    roots.push(path);
                }
            }
        }
        if roots.is_empty() {
            // Fallback: legacy root_uri
            let legacy_uri = params.root_uri.as_ref();
            if let Some(uri) = legacy_uri
                && let Ok(path) = uri.to_file_path()
            {
                roots.push(path);
            }
        }
        // #1030: reconcile `initializationOptions.dialect`/`.types` with a
        // discovered `brink.toml` (#1005), read once here (before `roots`
        // moves into `workspace_roots` below) and shared unchanged for the
        // life of the session — mirrors the previous dialect-only/types-only
        // handling (#589, #660), now layered with the file. See
        // `resolve_language_options` for the full precedence rule and
        // discovery details. `overrides` is stashed for every later reload
        // (#1055 gap 2); a malformed file's diagnostic (`outcome`) is
        // stashed too, rather than published here — the LSP spec forbids
        // sending notifications before this handler returns its
        // `InitializeResult`, so `initialized()` publishes it instead.
        let overrides = ConfigOverrides::from_initialize_params(&params);
        let (resolved, outcome) = resolve_language_options(&overrides, &roots);
        self.language.store(resolved);
        // #1572: declare the native source root before any file is loaded, so
        // the very first analysis pass already mints compile-identical native
        // module identity (`initialized()` runs the workspace scan).
        self.register_native_root(&roots, &outcome);
        if let Ok(mut guard) = self.config_overrides.lock() {
            *guard = overrides;
        }
        if let Ok(mut guard) = self.initial_config_outcome.lock() {
            *guard = Some(outcome);
        }

        if let Ok(mut ws) = self.workspace_roots.lock() {
            *ws = roots;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // ── Sync ──
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),

                // ── Navigation ──
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),

                // ── Info ──
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    ..Default::default()
                }),

                // ── Completion ──
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["->".into(), ".".into()]),
                    resolve_provider: Some(true),
                    ..Default::default()
                }),

                // ── Symbols ──
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),

                // ── Semantic tokens ──
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_tokens::legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(true),
                            ..Default::default()
                        },
                    ),
                ),

                // ── Refactoring ──
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                            CodeActionKind::SOURCE,
                        ]),
                        resolve_provider: Some(true),
                        ..Default::default()
                    },
                )),

                // ── Formatting ──
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),

                // ── Structure ──
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),

                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "brink-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::debug!("initialized");

        // Publish `initialize`'s one-time `brink.toml` load diagnostic, if
        // any (#1055 gap 1) — deferred to here, rather than sent from
        // `initialize` itself, because the LSP spec forbids the server
        // sending notifications before the client has the
        // `InitializeResult`; the `initialized` notification confirms
        // receipt of it. `.take()` so a hypothetical second `initialized`
        // call (not expected from a well-behaved client) doesn't re-publish.
        let stashed_outcome = self
            .initial_config_outcome
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(outcome) = stashed_outcome {
            self.publish_config_outcome(&outcome).await;
        }

        // Register file watchers for **/*.ink, **/*.brink and brink.toml
        // (#1055 gap 2: previously only .ink files were watched, so an
        // on-disk edit to brink.toml never reached the server; #1562: native
        // `.brink` modules were unwatched too, so an on-disk edit to one
        // never reached the server either) — fire-and-forget, some test
        // clients don't respond to server-initiated requests.
        let client = self.client.clone();
        tokio::spawn(async move {
            let ink_watcher = FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.ink".to_owned()),
                kind: None,
            };
            let native_watcher = FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.brink".to_owned()),
                kind: None,
            };
            let toml_watcher = FileSystemWatcher {
                glob_pattern: GlobPattern::String(format!(
                    "**/{}",
                    brink_project_config::CONFIG_FILE_NAME
                )),
                kind: None,
            };
            let registration = Registration {
                id: "ink-file-watcher".to_owned(),
                method: "workspace/didChangeWatchedFiles".to_owned(),
                register_options: serde_json::to_value(
                    tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions {
                        watchers: vec![ink_watcher, native_watcher, toml_watcher],
                    },
                )
                .ok(),
            };
            if let Err(e) = client.register_capability(vec![registration]).await {
                tracing::warn!("failed to register file watcher: {e}");
            }
        });

        // Scan workspace directories for source files
        self.load_workspace_files();
        self.trigger_analysis();
    }

    async fn did_change_configuration(&self, _params: DidChangeConfigurationParams) {
        tracing::debug!("did_change_configuration");
        // #1055 gap 2: re-read brink.toml so a workspace-settings change
        // (which many clients pair with editing it) takes effect without a
        // restart, same as a direct file-watched edit below.
        self.reload_brink_toml().await;
        self.trigger_analysis();
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        tracing::debug!(count = params.changes.len(), "did_change_watched_files");

        let roots = match self.workspace_roots.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };

        let mut changed = false;
        for change in &params.changes {
            let Some(path) = Self::uri_to_path(&change.uri) else {
                continue;
            };

            if is_brink_toml_path(&path) {
                // brink.toml isn't tracked in `ProjectDb` (it's not source —
                // `.ink` or `.brink`), so there's no "already admitted"
                // state to preserve the way there is for the source files
                // below — an ignored-dir brink.toml (e.g. a vendored config
                // under node_modules/) is never authoritative, so skip it
                // unconditionally.
                if path_under_ignored_dir(&path, &roots) {
                    continue;
                }
                // Every change type means the same thing here: re-resolve
                // (#1055 gap 2). A deleted file's own diagnostic is cleared
                // directly, matching the `.ink` DELETED case below;
                // `reload_brink_toml` separately (re-)publishes for
                // whichever ancestor `brink.toml`, if any, is now
                // authoritative.
                if change.typ == FileChangeType::DELETED {
                    self.client
                        .publish_diagnostics(change.uri.clone(), vec![], None)
                        .await;
                }
                self.reload_brink_toml().await;
                changed = true;
                continue;
            }

            match change.typ {
                FileChangeType::CREATED | FileChangeType::CHANGED => {
                    let already_tracked = { lock_db(&self.db).file_id(&path).is_some() };
                    if !already_tracked && path_under_ignored_dir(&path, &roots) {
                        // #1415 + regression fix: only skip *admission* of a
                        // path `ProjectDb` has never seen. A file can be
                        // legitimately tracked despite living under an
                        // ignored dir — `load_file_from_disk`/`chase_includes`
                        // (and `did_open`) resolve `INCLUDE` targets without
                        // pruning, so `INCLUDE node_modules/shared/lib.ink`
                        // is loaded — and such a file must keep syncing on
                        // every later CHANGED event, not just get skipped
                        // here because its path happens to match.
                        continue;
                    }
                    let Ok(contents) = tokio::fs::read_to_string(&path).await else {
                        tracing::warn!(path, "failed to read watched file");
                        continue;
                    };
                    self.mutate_db(|db| {
                        if db.file_id(&path).is_some() {
                            db.update_file(&path, contents);
                        } else {
                            db.set_file(&path, contents);
                        }
                    });
                    changed = true;
                }
                FileChangeType::DELETED => {
                    // Never gated by the ignored-dir guard above: a path
                    // can be legitimately tracked despite living under an
                    // ignored dir (see the CREATED/CHANGED arm), and for an
                    // untracked path `remove_file`/`forget` below are
                    // harmless no-ops — so deletions always run the full
                    // sync + diagnostics clear (#1415 review finding: the
                    // blanket guard was leaving deleted-but-still-tracked
                    // ignored-dir files permanently stale in `ProjectDb`
                    // with never-cleared diagnostics).
                    let file_id = self.mutate_db(|db| {
                        let fid = db.file_id(&path);
                        db.remove_file(&path);
                        fid
                    });
                    if let Some(fid) = file_id {
                        self.publisher.forget(fid).await;
                    }
                    self.client
                        .publish_diagnostics(change.uri.clone(), vec![], None)
                        .await;
                    changed = true;
                }
                _ => {}
            }
        }

        if changed {
            self.trigger_analysis();
        }
    }

    // ── Document sync ────────────────────────────────────────────────

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        tracing::debug!(
            uri = %params.text_document.uri,
            language_id = %params.text_document.language_id,
            "did_open",
        );

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        // Admitted unconditionally, even if `path` lives under `target/`,
        // `.git/`, or `node_modules/` — the client is telling us the user
        // explicitly opened this exact file, which is explicit path
        // admission, not a directory walk, so
        // `brink_source_tree::is_ignored_dir`'s "Admission policy" doc says
        // this never applies here (issue #1424).
        self.mutate_db(|db| db.set_file(&path, params.text_document.text));

        // Chase INCLUDE directives — load referenced files from disk
        self.chase_includes(&path);

        self.publish_perfile_diagnostics(&path, Some(params.text_document.version))
            .await;
        self.trigger_analysis();
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        tracing::debug!(
            uri = %params.text_document.uri,
            version = params.text_document.version,
            "did_change",
        );

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        // Full sync — take the last content change (there should be exactly one)
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };

        self.mutate_db(|db| db.update_file(&path, change.text));

        self.publish_perfile_diagnostics(&path, Some(params.text_document.version))
            .await;
        self.trigger_analysis();
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::debug!(uri = %params.text_document.uri, "did_save");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        if let Some(text) = params.text {
            self.mutate_db(|db| db.update_file(&path, text));
        }

        // `DidSaveTextDocumentParams` carries no document version, so this
        // publish can't be version-tagged like didOpen/didChange's.
        self.publish_perfile_diagnostics(&path, None).await;
        self.trigger_analysis();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        tracing::debug!(uri = %params.text_document.uri, "did_close");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return;
        };

        let file_id = self.mutate_db(|db| {
            let fid = db.file_id(&path);
            db.remove_file(&path);
            fid
        });

        // Drop the last-published record so a reopen republishes from scratch.
        if let Some(fid) = file_id {
            self.publisher.forget(fid).await;
        }

        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
        self.trigger_analysis();
    }

    // ── Navigation ───────────────────────────────────────────────────

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "goto_definition",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position_params.text_document.uri)
        else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position_params.position, &idx);

        // B3a UFCS resolution (issue #1507): a brief, transient lock —
        // `goto_definition` reads the memoized `db.ufcs_verdict` to jump to
        // a UFCS-desugared free function instead of the receiver.
        let db = lock_db(&self.db);
        let Some(loc) =
            brink_ide::navigation::goto_definition(&db, &snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };
        drop(db);

        // Find the target file in our snapshot
        let Some((_, target_path, target_source)) = snap
            .project_files
            .iter()
            .find(|(fid, _, _)| *fid == loc.file)
        else {
            return Ok(None);
        };

        let target_idx = LineIndex::new(target_source);
        let target_range = convert::to_lsp_range(loc.range, &target_idx);
        let Ok(target_uri) = Url::from_file_path(target_path) else {
            return Ok(None);
        };

        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri,
            range: target_range,
        })))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            "references",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position.position, &idx);

        // B3a UFCS resolution (issue #1539): a brief, transient lock — see
        // `goto_definition`'s own comment on the same pattern.
        let db = lock_db(&self.db);
        let refs = brink_ide::navigation::find_references(
            &db,
            &snap.analysis,
            snap.file_id,
            offset,
            params.context.include_declaration,
        );
        drop(db);

        if refs.is_empty() {
            return Ok(None);
        }

        let locations: Vec<_> = refs
            .iter()
            .filter_map(|loc| {
                let (_, file_path, file_source) = snap
                    .project_files
                    .iter()
                    .find(|(fid, _, _)| *fid == loc.file)?;
                let file_idx = LineIndex::new(file_source);
                let uri = Url::from_file_path(file_path).ok()?;
                Some(Location {
                    uri,
                    range: convert::to_lsp_range(loc.range, &file_idx),
                })
            })
            .collect();

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    // ── Info ─────────────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "hover",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position_params.text_document.uri)
        else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position_params.position, &idx);

        // TM-5 (#621): a brief, transient lock — `db.infer_body`/
        // `inferred_signature` are the FG-narrowed per-def fallback hover
        // reads when a param/temp/signature position has no declared type.
        let db = lock_db(&self.db);
        let Some(info) = brink_ide::hover::hover(
            &snap.analysis,
            &db,
            snap.file_id,
            &snap.source,
            offset,
            &snap.project_files,
        ) else {
            return Ok(None);
        };
        drop(db);

        let hover_range = info.range.map(|r| convert::to_lsp_range(r, &idx));

        Ok(Some(Hover {
            contents: tower_lsp::lsp_types::HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: info.content,
            }),
            range: hover_range,
        }))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        tracing::debug!(
            uri = %params.text_document_position_params.text_document.uri,
            "signature_help",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position_params.text_document.uri)
        else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let pos = params.text_document_position_params.position;
        let offset = idx.offset(pos.line, pos.character);
        let byte_offset: usize = offset.into();

        let Some(sig) = brink_ide::signature::signature_help_with_dialect(
            &snap.analysis,
            &snap.source,
            byte_offset,
            self.dialect(),
        ) else {
            return Ok(None);
        };

        let param_infos: Vec<ParameterInformation> = sig
            .parameters
            .iter()
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.label.clone()),
                documentation: None,
            })
            .collect();

        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label: sig.label,
                documentation: sig
                    .documentation
                    .map(tower_lsp::lsp_types::Documentation::String),
                parameters: Some(param_infos),
                active_parameter: Some(sig.active_parameter),
            }],
            active_signature: Some(0),
            active_parameter: Some(sig.active_parameter),
        }))
    }

    // ── Completion ───────────────────────────────────────────────────

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            "completion",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let pos = params.text_document_position.position;
        let idx = LineIndex::new(&snap.source);
        let byte_offset: usize = idx.offset(pos.line, pos.character).into();

        let ctx = detect_completion_context(&snap.source, byte_offset);
        let cursor_scope = cursor_scope(&snap.source, byte_offset);

        let mut items: Vec<CompletionItem> = Vec::new();

        // For dotted paths, show only children of the specified knot.
        if let CompletionContext::DottedPath { ref knot } = ctx {
            let prefix = format!("{knot}.");
            for (name, ids) in &snap.analysis.index.by_name {
                if let Some(suffix) = name.strip_prefix(&*prefix) {
                    for &def_id in ids {
                        let Some(info) = snap.analysis.index.symbols.get(&def_id) else {
                            continue;
                        };
                        if !matches!(
                            info.kind,
                            brink_ir::SymbolKind::Stitch | brink_ir::SymbolKind::Label
                        ) {
                            continue;
                        }
                        items.push(make_completion_item(info, Some(suffix.to_owned())));
                    }
                }
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }

        // T1e (docs/t1e-spec.md §2, issue #850): right after `ref `, only a
        // `VAR` is a legal `ref lvalue-path` root (E080) — narrow the
        // `FunctionArgs` set the same way `brink-web`'s wasm completion path
        // does (`ref_arg_root_prefix`'s own doc explains the "where cheap"
        // scoping: root position only, not `.`/`[` path continuations).
        let ref_root = ref_arg_root_prefix(&snap.source, byte_offset);

        for info in snap.analysis.index.symbols.values() {
            if !is_visible_in_context(&ctx, info, &cursor_scope) {
                continue;
            }
            if ref_root.is_some() && info.kind != brink_ir::SymbolKind::Variable {
                continue;
            }
            items.push(make_completion_item(info, None));
        }

        // Stdlib slice 1 completion (docs/t1b-surface-spec.md §5, #589) —
        // brink dialect only ("never offered in StrictInk"); an
        // author-defined symbol of the same name is already offered above
        // (shadowing, per §5), so this only adds names.
        for f in brink_ide::stdlib_completions(&ctx, self.dialect()) {
            items.push(make_stdlib_completion_item(f));
        }

        // Add synthetic DONE/END for divert context.
        if matches!(
            ctx,
            CompletionContext::Divert | CompletionContext::InlineExpr
        ) {
            for label in &["DONE", "END"] {
                items.push(CompletionItem {
                    label: (*label).to_owned(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("built-in".to_owned()),
                    ..Default::default()
                });
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        tracing::debug!(label = %item.label, "completion_resolve");
        Ok(item)
    }

    // ── Symbols ──────────────────────────────────────────────────────

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "document_symbol");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };

        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(&path) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };
        let Some(source) = db.source(file_id) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };
        let Some(hir) = db.hir(file_id) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };
        let Some(manifest) = db.manifest(file_id) else {
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        };

        let idx = LineIndex::new(source);
        let domain_symbols = brink_ide::document::document_symbols(hir, manifest, source);

        let symbols = domain_symbols
            .into_iter()
            .map(|s| domain_symbol_to_lsp(s, &idx))
            .collect();

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        tracing::debug!(query = %params.query, "workspace_symbol");

        let Some(projects) = self.analysis_rx.borrow().clone() else {
            return Ok(Some(vec![]));
        };

        let all_files = {
            let db = lock_db(&self.db);
            db.file_ids()
                .filter_map(|fid| {
                    let p = db.file_path(fid)?.to_owned();
                    let s = db.source(fid)?.to_owned();
                    Some((fid, p, s))
                })
                .collect::<Vec<_>>()
        };

        let domain_symbols = brink_ide::document::workspace_symbols(
            projects.by_root.values().map(std::convert::AsRef::as_ref),
            &params.query,
        );

        let results = domain_symbols
            .into_iter()
            .filter_map(|ws| {
                let (_, file_path, file_source) =
                    all_files.iter().find(|(fid, _, _)| *fid == ws.file)?;
                let uri = Url::from_file_path(file_path).ok()?;
                let idx = LineIndex::new(file_source);

                #[expect(deprecated, reason = "SymbolInformation requires this field")]
                let sym = SymbolInformation {
                    name: ws.name,
                    kind: convert::symbol_kind_to_lsp(ws.kind),
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri,
                        range: convert::to_lsp_range(ws.range, &idx),
                    },
                    container_name: None,
                };
                Some(sym)
            })
            .collect();

        Ok(Some(results))
    }

    // ── Semantic tokens ──────────────────────────────────────────────

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        tracing::debug!(uri = %params.text_document.uri, "semantic_tokens_full");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let (analysis, source, root, file_id) = {
            let projects = self.analysis_rx.borrow().clone();
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            let analysis = projects.and_then(|p| p.for_file(file_id).cloned());
            let Some(analysis) = analysis else {
                return Ok(None);
            };
            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return Ok(None);
            };
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            (analysis, source, root, file_id)
        };

        let data = semantic_tokens::compute_semantic_tokens(&source, &root, &analysis, file_id);

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        tracing::debug!(uri = %params.text_document.uri, "semantic_tokens_range");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let (analysis, source, root, file_id) = {
            let projects = self.analysis_rx.borrow().clone();
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            let analysis = projects.and_then(|p| p.for_file(file_id).cloned());
            let Some(analysis) = analysis else {
                return Ok(None);
            };
            let Some(source) = db.source(file_id).map(str::to_owned) else {
                return Ok(None);
            };
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            (analysis, source, root, file_id)
        };

        let range = params.range;
        let data = semantic_tokens::compute_semantic_tokens_range(
            &source,
            &root,
            &analysis,
            file_id,
            range.start.line,
            range.end.line,
        );

        Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    // ── Refactoring ──────────────────────────────────────────────────

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "prepare_rename");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.position, &idx);

        // B3a UFCS resolution (issue #1539): a brief, transient lock — see
        // `goto_definition`'s own comment on the same pattern.
        let db = lock_db(&self.db);
        let Some(range) =
            brink_ide::rename::prepare_rename(&db, &snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };
        drop(db);

        Ok(Some(PrepareRenameResponse::Range(convert::to_lsp_range(
            range, &idx,
        ))))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        tracing::debug!(
            uri = %params.text_document_position.text_document.uri,
            new_name = %params.new_name,
            "rename",
        );

        let Some(path) = Self::uri_to_path(&params.text_document_position.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let offset = convert::to_text_size(params.text_document_position.position, &idx);

        // B3a UFCS resolution (issue #1539): a brief, transient lock — see
        // `goto_definition`'s own comment on the same pattern.
        let db = lock_db(&self.db);
        let Some(result) =
            brink_ide::rename::rename(&db, &snap.analysis, snap.file_id, offset, &params.new_name)
        else {
            return Ok(None);
        };
        drop(db);

        // Convert domain edits to LSP WorkspaceEdit
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for edit in &result.edits {
            if let Some((_, file_path, file_source)) = snap
                .project_files
                .iter()
                .find(|(fid, _, _)| *fid == edit.file)
                && let Ok(uri) = Url::from_file_path(file_path)
            {
                let file_idx = LineIndex::new(file_source);
                changes.entry(uri).or_default().push(TextEdit {
                    range: convert::to_lsp_range(edit.range, &file_idx),
                    new_text: edit.new_text.clone(),
                });
            }
        }

        if changes.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        tracing::debug!(uri = %params.text_document.uri, "code_action");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(Some(vec![]));
        };

        let (source, import_actions) = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(Some(vec![]));
            };
            let Some(source) = db.source(file_id).map(String::from) else {
                return Ok(Some(vec![]));
            };
            let idx = LineIndex::new(&source);
            let offset: u32 = idx
                .offset(params.range.start.line, params.range.start.character)
                .into();
            // Auto-import quick-fix (M-4): session-aware, so it reads the
            // module-qualified db while the lock is held, then merges into the
            // same code-action list as the source-only actions below.
            let import_actions = brink_ide::import_fix::import_actions(&db, file_id, offset);
            (source, import_actions)
        };

        let idx = LineIndex::new(&source);
        let cursor_offset: usize = idx
            .offset(params.range.start.line, params.range.start.character)
            .into();

        let mut domain_actions = brink_ide::code_actions::code_actions(&source, cursor_offset);
        domain_actions.extend(import_actions);

        let uri = params.text_document.uri.as_str();
        let lsp_actions = domain_actions
            .into_iter()
            .map(|a| {
                let kind = match a.kind {
                    brink_ide::code_actions::CodeActionKind::QuickFix => CodeActionKind::QUICKFIX,
                    brink_ide::code_actions::CodeActionKind::Refactor => CodeActionKind::REFACTOR,
                    brink_ide::code_actions::CodeActionKind::Source => CodeActionKind::SOURCE,
                };
                let data = code_action_data_to_json(&a.data, uri);
                tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(CodeAction {
                    title: a.title,
                    kind: Some(kind),
                    data: Some(data),
                    ..Default::default()
                })
            })
            .collect();

        Ok(Some(lsp_actions))
    }

    async fn code_action_resolve(&self, mut action: CodeAction) -> Result<CodeAction> {
        tracing::debug!(title = %action.title, "code_action_resolve");

        let data = match &action.data {
            Some(obj) => obj.clone(),
            None => return Ok(action),
        };

        let kind = data.get("kind").and_then(serde_json::Value::as_str);
        let uri_str = data
            .get("uri")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        let Ok(uri) = Url::parse(uri_str) else {
            return Ok(action);
        };

        let Some(path) = Self::uri_to_path(&uri) else {
            return Ok(action);
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(action);
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(action);
        };

        let knot_name = data
            .get("knot")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        let action_data = match kind {
            Some("sort_knots") => brink_ide::code_actions::CodeActionData::SortKnots,
            Some("sort_stitches") => brink_ide::code_actions::CodeActionData::SortStitches {
                knot: knot_name.to_owned(),
            },
            Some("format_knot") => brink_ide::code_actions::CodeActionData::FormatKnot {
                knot: knot_name.to_owned(),
            },
            Some("format_stitch") => {
                let stitch_name = data
                    .get("stitch")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                brink_ide::code_actions::CodeActionData::FormatStitch {
                    knot: knot_name.to_owned(),
                    stitch: stitch_name.to_owned(),
                }
            }
            Some("add_import") => {
                let module = data
                    .get("module")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let name = data
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                brink_ide::code_actions::CodeActionData::AddImport {
                    module: module.to_owned(),
                    name: name.to_owned(),
                }
            }
            _ => return Ok(action),
        };

        let Some(new_source) = brink_ide::code_actions::resolve_code_action(&source, &action_data)
        else {
            return Ok(action);
        };

        let edits = diff_to_lsp_edits(&source, &new_source);
        let mut changes = HashMap::new();
        changes.insert(uri, edits);

        action.edit = Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        });

        Ok(action)
    }

    // ── Formatting ───────────────────────────────────────────────────

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        tracing::debug!(uri = %params.text_document.uri, "formatting");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(None);
        };

        let config = format_config_from_options(&params.options);
        let formatted = brink_fmt::format(&source, &config);

        if formatted == source {
            return Ok(None);
        }

        Ok(Some(diff_to_lsp_edits(&source, &formatted)))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        tracing::debug!(uri = %params.text_document.uri, "range_formatting");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let source = {
            let db = lock_db(&self.db);
            let Some(file_id) = db.file_id(&path) else {
                return Ok(None);
            };
            db.source(file_id).map(String::from)
        };

        let Some(source) = source else {
            return Ok(None);
        };

        let config = format_config_from_options(&params.options);
        let formatted = brink_fmt::format(&source, &config);

        if formatted == source {
            return Ok(None);
        }

        let all_edits = diff_to_lsp_edits(&source, &formatted);
        let range = params.range;

        // Filter edits to those that overlap the requested range.
        let filtered: Vec<TextEdit> = all_edits
            .into_iter()
            .filter(|edit| ranges_overlap(&edit.range, &range))
            .collect();

        if filtered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(filtered))
        }
    }

    // ── Structure ────────────────────────────────────────────────────

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        tracing::debug!(uri = %params.text_document.uri, "folding_range");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(&path) else {
            return Ok(None);
        };
        let Some(source) = db.source(file_id) else {
            return Ok(None);
        };
        let Some(hir) = db.hir(file_id) else {
            return Ok(None);
        };

        let projection = brink_ide::hir_projection::project_hir_structural(hir, source);
        let mut domain_ranges = brink_ide::folding::folding_ranges(hir, source, &projection);
        // `~ { … }` blocks + nested control bodies (docs/t1b-surface-spec.md
        // §2, #589) — a separate pass, see `block_folds`'s doc comment.
        domain_ranges.extend(brink_ide::folding::block_folds(hir, source));

        let ranges = domain_ranges
            .into_iter()
            .map(|r| FoldingRange {
                start_line: r.start_line,
                start_character: None,
                end_line: r.end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: r.collapsed_text,
            })
            .collect();

        Ok(Some(ranges))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<LspInlayHint>>> {
        tracing::debug!(uri = %params.text_document.uri, "inlay_hint");

        let Some(path) = Self::uri_to_path(&params.text_document.uri) else {
            return Ok(None);
        };

        let Some(snap) = self.navigation_snapshot(&path) else {
            return Ok(None);
        };

        let idx = LineIndex::new(&snap.source);
        let range_start = convert::to_text_size(params.range.start, &idx);
        let range_end = convert::to_text_size(params.range.end, &idx);
        let request_range = rowan::TextRange::new(range_start, range_end);

        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(&path) else {
            return Ok(None);
        };
        let Some(parse) = db.parse(file_id) else {
            return Ok(None);
        };
        let root = parse.tree();

        // The LSP has no host-value push channel (#174) — static value labels
        // still resolve from the manifest; `host`-source labels need none.
        // TM-5 (#621): `db` stays locked through this call — inlay hints now
        // also read `db.infer_body` for unannotated `temp` decls.
        let domain_hints = brink_ide::inlay_hints::inlay_hints(
            root.syntax(),
            &snap.analysis,
            &db,
            file_id,
            request_range,
            None,
        );
        drop(db);

        if domain_hints.is_empty() {
            return Ok(None);
        }

        let hints = domain_hints
            .into_iter()
            .map(|h| {
                let (line, col) = idx.line_col(h.offset);
                LspInlayHint {
                    position: Position::new(line, col),
                    label: InlayHintLabel::String(h.label),
                    kind: Some(lsp_inlay_hint_kind(&h.kind)),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right: Some(h.padding_right),
                    data: None,
                }
            })
            .collect();

        Ok(Some(hints))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        tracing::debug!(uri = %params.text_document.uri, "code_lens");
        Ok(None)
    }

    async fn code_lens_resolve(&self, lens: CodeLens) -> Result<CodeLens> {
        tracing::debug!("code_lens_resolve");
        Ok(lens)
    }
}

// ── Background analysis loop ────────────────────────────────────────

/// Test-visible signal that a background [`analysis_loop`] pass has
/// completed and its diagnostics (if any) have been published.
///
/// Fired unconditionally at the end of every pass — unlike
/// `textDocument/publishDiagnostics`, which [`publish_if_changed`] suppresses
/// when the new diagnostics are empty and nothing was previously published,
/// so a test asserting the *absence* of a diagnostic would never observe a
/// notification at all. Integration tests await this instead of polling a
/// fixed wall-clock deadline (#695).
///
/// Uses the `$/` LSP method prefix reserved for protocol-implementation-
/// dependent messages: per spec, a client that doesn't understand it should
/// silently ignore it, so this is safe to send unconditionally in the real
/// server, not just under test.
enum BackgroundAnalysisComplete {}

impl tower_lsp::lsp_types::notification::Notification for BackgroundAnalysisComplete {
    type Params = BackgroundAnalysisCompleteParams;

    const METHOD: &'static str = "$/brink/backgroundAnalysisComplete";
}

/// Params for [`BackgroundAnalysisComplete`].
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct BackgroundAnalysisCompleteParams {
    /// Number of files included in this pass's db snapshot. A test that
    /// just opened exactly one file can wait for `file_count >= 1` to know
    /// the pass reflects that file, without racing on wall-clock timing
    /// against an earlier (e.g. startup-triggered) pass that ran before the
    /// file was opened.
    file_count: usize,
}

/// Background task that runs per-project cross-file analysis outside the db lock.
///
/// Woken by `trigger.notify_one()` whenever a file changes. Uses `yield_now()`
/// to coalesce rapid edits, then snapshots analysis inputs under the lock,
/// runs per-project analysis without holding the lock, and publishes diagnostics
/// for all files whose diagnostic set changed.
///
/// `language` is the same [`LanguageOptions`] `Backend` holds (#599, #660) —
/// `initialize`'s `initializationOptions.dialect`/`.types` handlers write
/// into its shared `Arc<Mutex<_>>`s, and this loop re-reads both every
/// iteration so a client that (re-)declares either gets diagnostics
/// analyzed under the current values on the very next background pass, with
/// no separate propagation step needed.
pub async fn analysis_loop(
    db: Arc<Mutex<brink_db::ProjectDb>>,
    generation: Arc<AtomicU64>,
    trigger: Arc<Notify>,
    tx: watch::Sender<Option<Arc<ProjectAnalyses>>>,
    client: Client,
    publisher: DiagnosticsPublisher,
    language: LanguageOptions,
) {
    loop {
        trigger.notified().await;
        // Coalesce rapid edits — yield so any queued notifications collapse
        tokio::task::yield_now().await;

        // Re-read the declared dialect + types + lints policy each iteration
        // (poisoned-lock-safe, mirrors `Backend::dialect()`) so a client that
        // changes any of them mid-session is picked up on the next pass.
        let opts = AnalysisOptions {
            dialect: language
                .dialect
                .lock()
                .map_or_else(|_| Dialect::default(), |g| *g),
            types: language.types.lock().map_or_else(|_| None, |g| *g),
            lints: language
                .lints
                .lock()
                .map_or_else(|_| LintPolicy::default(), |g| g.clone()),
            ..AnalysisOptions::default()
        };

        // Snapshot inputs under lock, reading the generation in the same locked
        // block so `(content, generation)` is a consistent pair: it reflects
        // exactly the content-revision folded into this pass. For any edit this
        // pass includes, that is `>=` the generation the edit's per-file publish
        // carried (both read the revision under the db lock; the analysis reads
        // at-or-after the write). Tagged `Analysis`, this therefore wins the
        // `DiagnosticsPublisher` anti-downgrade rule against that per-file set.
        let (
            generation,
            projects,
            modules,
            module_diags,
            file_meta,
            per_file_diags,
            file_suppressions,
        ) = {
            let mut db = lock_db(&db);
            // Push the same options this pass analyzes under into the db as a
            // salsa input, *before* reading anything derived from it (issue
            // #1562, folding in the #1553 bug class in its second db holder).
            //
            // The published diagnostics come from the off-db
            // `analyze_with_modules` pass below, which honors `opts` — but
            // several request handlers read the db's own queries directly:
            // hover (`db.effects`/`db.signature`/`db.inferred_signature`/
            // `db.infer_body`), inlay hints, code actions, and rename's UFCS
            // resolution. `Backend` never wrote this input, so every one of
            // them ran under `AnalysisOptions::default()` — `Dialect::StrictInk`
            // no matter what the client declared, which (among other things)
            // gates off M-2d cross-declared-module coexistence in
            // `symbol_index_query`. Every native `.brink` file has a declared
            // module, so two native modules with a same-named flow lost one of
            // them from the db's index, and every db-backed hover row for it
            // silently vanished.
            //
            // Guarded against unchanged values exactly as `IdeSession::
            // sync_db_options` is: salsa stamps the current revision on every
            // write, so an unguarded call here — once per analysis pass, i.e.
            // once per keystroke — would invalidate every direct reader on
            // every edit.
            if db.analysis_options() != &opts {
                db.set_analysis_options(opts.clone());
            }
            let generation = generation.load(Ordering::Relaxed);
            let project_defs = db.compute_projects();
            // `is_native` per root (issue #1562 review finding), captured
            // under the same lock as `project_defs`: the off-db pass below
            // has no `Language` classification of its own, so without this
            // M-2d cross-declared-module coexistence stayed gated on a
            // client having declared `dialect: "brink"` — a native project
            // has no dialect to be wrong about.
            let project_inputs: Vec<_> = project_defs
                .iter()
                .map(|(root, m)| (*root, db.analysis_inputs_for(m), db.is_native(*root)))
                .collect();
            // The project's resolved modules (#1526), cloned out under the
            // same lock as the inputs they qualify. Module identity needs
            // file paths, which the analysis inputs don't carry — without it
            // this pass mints `DefinitionId`s that don't match the db's, so
            // every native `.brink` symbol misses in `db.effects`/
            // `db.signature`/`db.infer_body`. Keyed by `FileId`, so the
            // whole-workspace map is a harmless superset for each project.
            let modules = db.module_map().clone();
            // The map's diagnostics half (`E085` stem collisions, #1553).
            // `analyze_with_modules` below is handed the finished map, so it
            // cannot re-derive them; without folding them back in per project
            // a collision a db-driven compile catches never reaches the editor.
            let module_diags = db.module_map_diagnostics().to_vec();
            let meta = db.file_metadata();
            let diags: Vec<_> = meta
                .iter()
                .filter_map(|(fid, _, _)| Some((*fid, db.file_diagnostics(*fid)?.to_vec())))
                .collect();
            let suppressions: HashMap<brink_ir::FileId, brink_ir::suppressions::Suppressions> =
                meta.iter()
                    .filter_map(|(fid, _, _)| Some((*fid, db.suppressions(*fid)?.clone())))
                    .collect();
            (
                generation,
                project_inputs,
                modules,
                module_diags,
                meta,
                diags,
                suppressions,
            )
        };

        // Run per-project analysis OUTSIDE the lock
        let mut by_root = HashMap::new();
        let mut file_to_roots: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>> = HashMap::new();
        let mut project_members = HashMap::new();

        for (root, inputs, is_native) in &projects {
            let file_refs: Vec<_> = inputs.iter().map(|(id, hir, m)| (*id, hir, m)).collect();
            let mut result =
                brink_analyzer::analyze_with_modules(&file_refs, &modules, &opts, *is_native);
            let members: Vec<_> = inputs.iter().map(|(id, _, _)| *id).collect();
            fold_module_diagnostics(&mut result, &module_diags, &members);
            by_root.insert(*root, Arc::new(result));

            for &member in &members {
                file_to_roots.entry(member).or_default().push(*root);
            }
            project_members.insert(*root, members);
        }

        // Sort the root lists for deterministic primary-project selection
        for roots in file_to_roots.values_mut() {
            roots.sort_by_key(|id| id.0);
        }

        let result = Arc::new(ProjectAnalyses {
            by_root,
            file_to_roots,
            project_members,
        });

        // Publish to watch channel
        let _ = tx.send(Some(Arc::clone(&result)));

        // Publish diagnostics for all affected files
        let file_count = file_meta.len();
        publish_all_diagnostics(
            &publisher,
            &result,
            &file_meta,
            &per_file_diags,
            &file_suppressions,
            generation,
            &opts,
        )
        .await;

        // Test-visible completion signal (#695) — see `BackgroundAnalysisComplete`.
        client
            .send_notification::<BackgroundAnalysisComplete>(BackgroundAnalysisCompleteParams {
                file_count,
            })
            .await;
    }
}

/// Fold the module map's own diagnostics (`E085` stem collisions) into one
/// project's analysis result (issue #1553).
///
/// `brink_analyzer::analyze_with_modules` is handed the *finished* map, so it
/// cannot re-derive them; without this a collision a db-driven compile catches
/// never reaches the editor. `module_diags` is whole-workspace, so it is
/// filtered to `members` — a collision in an unrelated project must not be
/// attributed to this one.
fn fold_module_diagnostics(
    result: &mut AnalysisResult,
    module_diags: &[brink_ir::Diagnostic],
    members: &[brink_ir::FileId],
) {
    result.diagnostics.extend(
        module_diags
            .iter()
            .filter(|d| members.contains(&d.file))
            .cloned(),
    );
}

/// Build a `DiagnosticRelatedInformation` pointing to a project root file.
fn make_project_annotation(
    root_path: &str,
) -> Option<tower_lsp::lsp_types::DiagnosticRelatedInformation> {
    let root_uri = Url::from_file_path(root_path).ok()?;
    Some(tower_lsp::lsp_types::DiagnosticRelatedInformation {
        location: Location {
            uri: root_uri,
            range: Range::default(),
        },
        message: format!("in project: {root_path}"),
    })
}

/// Collect multi-project analysis diagnostics for a file, deduplicating and
/// annotating with project-root related information.
fn collect_multiproject_diags(
    file_id: brink_ir::FileId,
    analyses: &[&Arc<AnalysisResult>],
    roots: &[brink_ir::FileId],
    file_path_map: &HashMap<brink_ir::FileId, &str>,
    idx: &LineIndex,
    lsp_diags: &mut Vec<tower_lsp::lsp_types::Diagnostic>,
    opts: &AnalysisOptions,
) {
    let mut seen: HashMap<(u32, u32, String, String), usize> = HashMap::new();

    for (analysis, root) in analyses.iter().zip(roots) {
        for d in &analysis.diagnostics {
            if d.file != file_id {
                continue;
            }
            let key = (
                d.range.start().into(),
                d.range.end().into(),
                format!("{:?}", d.code),
                d.message.clone(),
            );
            if let Some(&existing_idx) = seen.get(&key) {
                if let Some(ref mut related) = lsp_diags[existing_idx].related_information
                    && let Some(root_path) = file_path_map.get(root)
                    && let Some(annotation) = make_project_annotation(root_path)
                {
                    related.push(annotation);
                }
            } else {
                let mut lsp_diag =
                    convert::diagnostic_to_lsp(d, idx, opts.type_policy(), &opts.lints);
                if let Some(root_path) = file_path_map.get(root)
                    && let Some(annotation) = make_project_annotation(root_path)
                {
                    lsp_diag.related_information = Some(vec![annotation]);
                }
                let diag_idx = lsp_diags.len();
                seen.insert(key, diag_idx);
                lsp_diags.push(lsp_diag);
            }
        }
    }

    // Remove annotations from diagnostics that appear in ALL projects (universal)
    let num_projects = analyses.len();
    for &diag_idx in seen.values() {
        if let Some(ref related) = lsp_diags[diag_idx].related_information
            && related.len() >= num_projects
        {
            lsp_diags[diag_idx].related_information = None;
        }
    }
}

/// Compute the full diagnostic set for each file and hand it to the
/// [`DiagnosticsPublisher`] tagged `Analysis` at `generation`.
///
/// Unions analysis diagnostics from all projects containing a file.
/// Applies suppression directives before publishing.
async fn publish_all_diagnostics(
    publisher: &DiagnosticsPublisher,
    projects: &ProjectAnalyses,
    file_meta: &[(brink_ir::FileId, String, String)],
    per_file_diags: &[(brink_ir::FileId, Vec<brink_ir::Diagnostic>)],
    file_suppressions: &HashMap<brink_ir::FileId, brink_ir::suppressions::Suppressions>,
    generation: u64,
    opts: &AnalysisOptions,
) {
    let lowering_diags: HashMap<brink_ir::FileId, &[brink_ir::Diagnostic]> = per_file_diags
        .iter()
        .map(|(fid, diags)| (*fid, diags.as_slice()))
        .collect();

    let file_path_map: HashMap<brink_ir::FileId, &str> = file_meta
        .iter()
        .map(|(fid, path, _)| (*fid, path.as_str()))
        .collect();

    // Build set of files whose project root has disable_all
    let disable_all_files: std::collections::HashSet<brink_ir::FileId> = projects
        .project_members
        .iter()
        .filter(|(root, _)| file_suppressions.get(root).is_some_and(|s| s.disable_all))
        .flat_map(|(_, members)| members.iter().copied())
        .collect();

    for (file_id, path, source) in file_meta {
        let idx = LineIndex::new(source);

        // Collect raw IR diagnostics (lowering + analysis) for this file
        let mut raw_diags: Vec<brink_ir::Diagnostic> = lowering_diags
            .get(file_id)
            .copied()
            .unwrap_or_default()
            .to_vec();

        let analyses = projects.all_for_file(*file_id);
        if !disable_all_files.contains(file_id) {
            if analyses.len() <= 1 {
                if let Some(analysis) = analyses.first() {
                    for d in &analysis.diagnostics {
                        if d.file == *file_id {
                            raw_diags.push(d.clone());
                        }
                    }
                }
            } else {
                // Multi-project: collect analysis diags, then convert to LSP
                // (multi-project annotation needs LSP-level conversion)
                let sup = file_suppressions.get(file_id);
                let filtered_lowering = if let Some(sup) = sup {
                    brink_ir::suppressions::apply_suppressions(*file_id, source, raw_diags, sup)
                } else {
                    raw_diags
                };

                let mut lsp_diags: Vec<tower_lsp::lsp_types::Diagnostic> = filtered_lowering
                    .iter()
                    .map(|d| convert::diagnostic_to_lsp(d, &idx, opts.type_policy(), &opts.lints))
                    .collect();

                let roots = projects
                    .file_to_roots
                    .get(file_id)
                    .map_or(&[][..], Vec::as_slice);
                collect_multiproject_diags(
                    *file_id,
                    &analyses,
                    roots,
                    &file_path_map,
                    &idx,
                    &mut lsp_diags,
                    opts,
                );

                publisher
                    .publish(
                        *file_id,
                        path,
                        lsp_diags,
                        generation,
                        PublishTier::Analysis,
                        None,
                    )
                    .await;
                continue;
            }
        }

        // Apply suppressions to the combined diagnostic list
        let sup = file_suppressions.get(file_id);
        let filtered = if let Some(sup) = sup {
            brink_ir::suppressions::apply_suppressions(*file_id, source, raw_diags, sup)
        } else {
            raw_diags
        };

        let lsp_diags: Vec<tower_lsp::lsp_types::Diagnostic> = filtered
            .iter()
            .map(|d| convert::diagnostic_to_lsp(d, &idx, opts.type_policy(), &opts.lints))
            .collect();

        publisher
            .publish(
                *file_id,
                path,
                lsp_diags,
                generation,
                PublishTier::Analysis,
                None,
            )
            .await;
    }
}

/// Convert a `CodeActionData` variant to JSON for LSP code action data.
fn code_action_data_to_json(
    data: &brink_ide::code_actions::CodeActionData,
    uri: &str,
) -> serde_json::Value {
    match data {
        brink_ide::code_actions::CodeActionData::SortKnots => {
            serde_json::json!({ "kind": "sort_knots", "uri": uri })
        }
        brink_ide::code_actions::CodeActionData::SortStitches { knot } => {
            serde_json::json!({ "kind": "sort_stitches", "uri": uri, "knot": knot })
        }
        brink_ide::code_actions::CodeActionData::FormatKnot { knot } => {
            serde_json::json!({ "kind": "format_knot", "uri": uri, "knot": knot })
        }
        brink_ide::code_actions::CodeActionData::FormatStitch { knot, stitch } => {
            serde_json::json!({ "kind": "format_stitch", "uri": uri, "knot": knot, "stitch": stitch })
        }
        // Structural move actions — not yet wired in LSP resolve.
        // Surfaced as code actions so editors show them; resolve is a no-op
        // until the LSP gains workspace/applyEdit support for these.
        brink_ide::code_actions::CodeActionData::ReorderStitch {
            knot,
            stitch,
            direction,
        } => {
            let dir = match direction {
                brink_ide::structural_move::Direction::Up => -1,
                brink_ide::structural_move::Direction::Down => 1,
            };
            serde_json::json!({
                "kind": "reorder_stitch", "uri": uri,
                "knot": knot, "stitch": stitch, "direction": dir,
            })
        }
        brink_ide::code_actions::CodeActionData::MoveStitch {
            src_knot,
            stitch,
            dest_knot,
        } => serde_json::json!({
            "kind": "move_stitch", "uri": uri,
            "src_knot": src_knot, "stitch": stitch, "dest_knot": dest_knot,
        }),
        brink_ide::code_actions::CodeActionData::PromoteStitch { knot, stitch } => {
            serde_json::json!({
                "kind": "promote_stitch", "uri": uri, "knot": knot, "stitch": stitch,
            })
        }
        brink_ide::code_actions::CodeActionData::DemoteKnot { knot, dest_knot } => {
            serde_json::json!({
                "kind": "demote_knot", "uri": uri, "knot": knot, "dest_knot": dest_knot,
            })
        }
        brink_ide::code_actions::CodeActionData::AddImport { module, name } => {
            serde_json::json!({
                "kind": "add_import", "uri": uri, "module": module, "name": name,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::Diagnostic;

    use super::{
        ConfigLoadOutcome, ConfigOverrides, PublishDecision, PublishRecord, PublishTier,
        collect_source_files, config_error_diagnostic, native_source_root, path_under_ignored_dir,
        publish_decision, resolve_language_options,
    };

    /// A unique per-test scratch directory under the OS temp dir, mirroring
    /// `brink-driver`'s own `temp_dir` test helper
    /// (`crates/internal/brink-driver/src/source_tree.rs`) — each test gets
    /// an isolated directory so parallel test runs never collide.
    fn temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "brink-lsp-walk-test-{label}-{}-{n}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// #1402 regression, mirroring #1381's `real_fs_list_skips_ignored_dirs`
    /// shape (`crates/internal/brink-driver/src/source_tree.rs`):
    /// `collect_source_files` — the walk `Backend::walk_and_load` delegates to
    /// — must never descend into `target/`, `.git/`, or `node_modules/`.
    /// The fixture plants an unparseable file directly under `target/`
    /// (garbage ink syntax, not merely a stray file) to prove the walk is
    /// *pruned*, not just filtered after the fact: before this fix, that
    /// file would have been enumerated and handed to `load_file_from_disk`,
    /// which parses it — an unparseable file under an ignored directory
    /// must not break the load.
    #[test]
    fn collect_source_files_skips_ignored_dirs() {
        let root = temp_dir("ignored-dirs");

        std::fs::write(root.join("main.ink"), "Hello.\n-> DONE\n").expect("write main.ink");
        std::fs::create_dir_all(root.join("target/debug")).expect("mkdir target/debug");
        std::fs::write(root.join("target/stray.ink"), "-- stray --").expect("write target/stray");
        std::fs::write(
            root.join("target/debug/build.ink"),
            "this is not valid ink syntax {{{ ???",
        )
        .expect("write target/debug/build.ink");
        std::fs::create_dir_all(root.join(".git/objects")).expect("mkdir .git/objects");
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write .git/HEAD");
        std::fs::write(root.join(".git/objects/pack.ink"), "-- pack --")
            .expect("write .git/objects/pack.ink");
        std::fs::create_dir_all(root.join("node_modules/some-pkg")).expect("mkdir node_modules");
        std::fs::write(root.join("node_modules/some-pkg/index.ink"), "-- pkg --")
            .expect("write node_modules/some-pkg/index.ink");

        let mut files = collect_source_files(&root);
        files.sort();

        assert_eq!(
            files,
            vec![root.join("main.ink")],
            "target/, .git/, and node_modules/ must be pruned entirely"
        );

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// Issue #1562: the workspace scan must enumerate native `.brink`
    /// modules as well as `.ink`. Before this, only a `didOpen` ever put a
    /// `.brink` file in the db, so a native workspace's sibling modules were
    /// invisible to go-to-definition, find-references, and completion until
    /// the user opened each one by hand. Non-source files stay out, and the
    /// ignored-dir prune applies to `.brink` exactly as it does to `.ink`.
    #[test]
    fn collect_source_files_admits_native_brink_modules() {
        let root = temp_dir("native-modules");

        std::fs::write(root.join("main.brink"), "flow start() {\n  Hi.\n}\n")
            .expect("write main.brink");
        std::fs::create_dir_all(root.join("market")).expect("mkdir market");
        std::fs::write(
            root.join("market/barter.brink"),
            "flow haggle() {\n  Trade.\n}\n",
        )
        .expect("write market/barter.brink");
        std::fs::write(root.join("legacy.ink"), "Hello.\n-> DONE\n").expect("write legacy.ink");
        std::fs::write(root.join("README.md"), "# not source\n").expect("write README.md");
        std::fs::create_dir_all(root.join("target")).expect("mkdir target");
        std::fs::write(root.join("target/stray.brink"), "flow stray() {\n}\n")
            .expect("write target/stray.brink");

        let mut files = collect_source_files(&root);
        files.sort();

        assert_eq!(
            files,
            vec![
                root.join("legacy.ink"),
                root.join("main.brink"),
                root.join("market/barter.brink"),
            ],
            "both frontends' sources are admitted, non-source files are not, \
             and target/ is still pruned for .brink too"
        );

        std::fs::remove_dir_all(&root).expect("cleanup temp dir");
    }

    /// Issue #1424: a workspace legitimately *rooted* inside an
    /// ignored-dir-named directory (e.g. a `node_modules/vendor-ink` package
    /// opened directly as its own workspace folder) must still have its own
    /// `.ink` files admitted — `is_ignored_dir` is only ever checked against
    /// entries found *while descending from* the walk's starting `dir`
    /// (`collect_source_files`'s `Walk`), never against `dir` itself, so the
    /// root's own name never disqualifies it. A genuinely nested ignored directory
    /// further below that same root must still be pruned, exactly as if the
    /// root weren't ignored-named at all.
    #[test]
    fn collect_source_files_admits_workspace_root_itself_under_ignored_dir() {
        let root = temp_dir("root-under-node-modules").join("node_modules/vendor-ink");
        std::fs::create_dir_all(&root).expect("create root under node_modules");

        std::fs::write(root.join("main.ink"), "Hello.\n-> DONE\n").expect("write main.ink");
        std::fs::create_dir_all(root.join("target/debug")).expect("mkdir target/debug");
        std::fs::write(root.join("target/debug/build.ink"), "-- build --")
            .expect("write target/debug/build.ink");

        let mut files = collect_source_files(&root);
        files.sort();

        assert_eq!(
            files,
            vec![root.join("main.ink")],
            "the root's own node_modules/ ancestry must not block admission of its own \
             files, but a genuinely nested target/ below it must still be pruned"
        );

        std::fs::remove_dir_all(root.ancestors().nth(2).expect("has grandparent"))
            .expect("cleanup temp dir");
    }

    /// #1415 regression: `path_under_ignored_dir` — the guard
    /// `did_change_watched_files` applies to whole file-watcher paths,
    /// since it never walks a directory tree entry-by-entry the way
    /// `collect_source_files` does — must flag a path whose *any*
    /// component is `target/`, `.git/`, or `node_modules/`, not just a leaf
    /// directory name, and must leave ordinary paths alone.
    #[test]
    fn path_under_ignored_dir_matches_any_component() {
        assert!(path_under_ignored_dir("/repo/target/debug/build.ink", &[]));
        assert!(path_under_ignored_dir("/repo/.git/objects/pack.ink", &[]));
        assert!(path_under_ignored_dir(
            "/repo/node_modules/some-pkg/index.ink",
            &[]
        ));
        assert!(!path_under_ignored_dir("/repo/src/main.ink", &[]));
        assert!(!path_under_ignored_dir("/repo/targets/main.ink", &[]));
    }

    /// #1415 review finding: path-scope divergence. `collect_source_files`
    /// only ever tests components *below* whichever workspace root it
    /// started walking from, so a workspace root that itself lives inside
    /// `node_modules/` (vendored ink content opened directly as a folder)
    /// still has its own files admitted at load time. `path_under_ignored_dir`
    /// must agree once scoped to that root — otherwise every subsequent
    /// watcher event for such a workspace would be silently dropped forever,
    /// even though the initial load admitted the same files fine.
    #[test]
    fn path_under_ignored_dir_scopes_below_workspace_root() {
        let root = std::path::PathBuf::from("/repo/node_modules/vendor-ink");

        // The root's own ancestry passes through node_modules/, but a file
        // directly inside the root must not be flagged just because of that.
        assert!(!path_under_ignored_dir(
            "/repo/node_modules/vendor-ink/main.ink",
            std::slice::from_ref(&root)
        ));

        // A genuine descendant ignored dir under the root must still be
        // flagged.
        assert!(path_under_ignored_dir(
            "/repo/node_modules/vendor-ink/target/debug/build.ink",
            std::slice::from_ref(&root)
        ));

        // With no matching root, the check falls back to every component of
        // the full path (matches the pre-#1415-fix, whole-path behavior).
        assert!(path_under_ignored_dir(
            "/repo/node_modules/vendor-ink/main.ink",
            &[]
        ));
    }

    /// #1163 regression: `config_error_diagnostic` must always report
    /// `ERROR` — every `ConfigError` variant (malformed TOML, unreadable
    /// file, a recognized key with the wrong shape) is a genuine load
    /// failure, and the type carries no `DiagnosticCode`/warning tier to
    /// route through `convert::severity_to_lsp` differently. This locks in
    /// that the #1163 fix (routing the literal through
    /// `convert::severity_to_lsp` instead of naming the `tower_lsp` variant
    /// directly) didn't change the resulting severity.
    #[test]
    fn config_error_diagnostic_is_always_error() {
        let err = brink_project_config::ConfigError::NotATable {
            path: "brink.toml".to_owned(),
            key: "types".to_owned(),
            found: "string",
        };
        let diag = config_error_diagnostic(&err, None);
        assert_eq!(
            diag.severity,
            Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR)
        );
    }

    /// #1384: `resolve_language_options`'s published diagnostic for a
    /// malformed `brink.toml` must name the file in its message text, not
    /// just via the diagnostic's implicit file association — a bare
    /// `ConfigError::Display` pre-#1384 (`error.to_string()` with no path
    /// prefix) had nothing identifying which file failed once the message
    /// left the editor's per-file diagnostic list (e.g. in a client's
    /// aggregated "Problems" pane).
    #[test]
    fn resolve_language_options_diagnostic_names_its_path_on_malformed_toml() {
        let root = temp_dir("config-diagnostic-path");
        std::fs::write(
            root.join("brink.toml"),
            "[project]\ndialect = \"sideways\"\n",
        )
        .expect("write brink.toml");

        let (_, outcome) =
            resolve_language_options(&ConfigOverrides::default(), std::slice::from_ref(&root));

        let diag = outcome
            .diagnostic
            .expect("malformed brink.toml must publish a diagnostic");
        let expected_path = root.join("brink.toml").display().to_string();
        assert!(
            diag.message.contains(&expected_path),
            "diagnostic message must name the file, got: {}",
            diag.message
        );
    }

    /// #1572: `brink_project_config::find_config` only ever walks *up* from
    /// the workspace root (stopping at a `.git` boundary), so the discovered
    /// config's directory can never be a subdirectory of the workspace
    /// folder — the only real shape where the config directory differs from
    /// the workspace-root fallback is the reverse: opening a *subfolder* of
    /// a project whose `brink.toml` lives at an ancestor. The fixture puts
    /// `brink.toml` at `root/` with the workspace folder at `root/game`, and
    /// obtains the outcome via a real `resolve_language_options` call (the
    /// only real producer of a `ConfigLoadOutcome`) rather than
    /// hand-constructing one, so the test proves both the wired path and the
    /// branch — it would fail if either the config discovery or the
    /// config-directory branch were dropped.
    #[test]
    fn native_source_root_prefers_the_discovered_config_directory() {
        let root = temp_dir("native-root-config");
        let game = root.join("game");
        std::fs::create_dir_all(&game).expect("create game dir");
        std::fs::write(
            root.join(brink_project_config::CONFIG_FILE_NAME),
            "[project]\ndialect = \"brink\"\n",
        )
        .expect("write brink.toml");

        let (_, outcome) =
            resolve_language_options(&ConfigOverrides::default(), std::slice::from_ref(&game));

        assert_eq!(
            native_source_root(std::slice::from_ref(&game), &outcome),
            Some(root.clone()),
            "the governing brink.toml's directory is the native source root"
        );

        std::fs::remove_dir_all(&root).expect("clean up");
    }

    /// #1572: with no `brink.toml` anywhere, the first workspace folder is the
    /// native source root — and with neither (a client that opened no folder
    /// at all), there is nothing to anchor to, so identity is left exactly as
    /// registered.
    #[test]
    fn native_source_root_falls_back_to_the_first_workspace_root_then_nothing() {
        let workspace = temp_dir("native-root-fallback");
        let other = temp_dir("native-root-fallback-second");

        assert_eq!(
            native_source_root(
                &[workspace.clone(), other.clone()],
                &ConfigLoadOutcome::default()
            ),
            Some(workspace.clone()),
            "no config: the FIRST workspace folder anchors identity"
        );
        assert_eq!(
            native_source_root(&[], &ConfigLoadOutcome::default()),
            None,
            "no config and no workspace folder: nothing to anchor to"
        );

        std::fs::remove_dir_all(&workspace).expect("clean up");
        std::fs::remove_dir_all(&other).expect("clean up");
    }

    /// A distinct diagnostic set identified by its message (content equality is
    /// all `publish_decision` inspects).
    fn diags(msg: &str) -> Vec<Diagnostic> {
        vec![Diagnostic {
            message: msg.to_owned(),
            ..Diagnostic::default()
        }]
    }

    fn record(generation: u64, tier: PublishTier, msg: &str) -> PublishRecord {
        PublishRecord {
            generation,
            tier,
            diags: diags(msg),
        }
    }

    const SEND_AND_RECORD: PublishDecision = PublishDecision {
        send: true,
        record: true,
    };
    const DROP: PublishDecision = PublishDecision {
        send: false,
        record: false,
    };
    const RECORD_ONLY: PublishDecision = PublishDecision {
        send: false,
        record: true,
    };

    #[test]
    fn fresh_file_empty_set_is_dropped() {
        // Never-published clean file: no spurious empty publish.
        assert_eq!(publish_decision(None, 0, PublishTier::PerFile, &[]), DROP,);
    }

    #[test]
    fn fresh_file_nonempty_set_is_sent() {
        assert_eq!(
            publish_decision(None, 0, PublishTier::PerFile, &diags("e1")),
            SEND_AND_RECORD,
        );
    }

    #[test]
    fn analysis_upgrades_perfile_within_a_generation() {
        // PerFile@G already shown; the fuller Analysis@G (same edit) wins.
        let prev = record(1, PublishTier::PerFile, "parse-only");
        assert_eq!(
            publish_decision(
                Some(&prev),
                1,
                PublishTier::Analysis,
                &diags("parse+analysis")
            ),
            SEND_AND_RECORD,
        );
    }

    #[test]
    fn perfile_never_downgrades_a_same_generation_analysis() {
        // THE BUG (#615): a delayed PerFile@G landing after Analysis@G for the
        // same content must be dropped whole — not sent, not recorded — so the
        // client keeps the full set.
        let prev = record(1, PublishTier::Analysis, "parse+analysis");
        assert_eq!(
            publish_decision(Some(&prev), 1, PublishTier::PerFile, &diags("parse-only")),
            DROP,
        );
    }

    #[test]
    fn newer_generation_perfile_replaces_older_analysis() {
        // A fresh edit's PerFile (gen G+1) legitimately supersedes the stale
        // full set computed for the previous content (gen G).
        let prev = record(1, PublishTier::Analysis, "old-content-analysis");
        assert_eq!(
            publish_decision(
                Some(&prev),
                2,
                PublishTier::PerFile,
                &diags("new-content-parse")
            ),
            SEND_AND_RECORD,
        );
    }

    #[test]
    fn stale_older_generation_analysis_is_dropped() {
        // An analysis pass for superseded content must not overwrite a newer set.
        let prev = record(2, PublishTier::Analysis, "current");
        assert_eq!(
            publish_decision(Some(&prev), 1, PublishTier::Analysis, &diags("stale")),
            DROP,
        );
    }

    #[test]
    fn identical_upgrade_records_without_sending() {
        // Analysis@G with the same content as the shown PerFile@G: no need to
        // re-send, but record the tier bump so a later PerFile@G is correctly
        // rejected as a downgrade.
        let prev = record(1, PublishTier::PerFile, "same");
        assert_eq!(
            publish_decision(Some(&prev), 1, PublishTier::Analysis, &diags("same")),
            RECORD_ONLY,
        );
    }

    #[test]
    fn tier_bump_from_identical_upgrade_then_blocks_late_perfile() {
        // Chains the previous case: after the record-only Analysis@G upgrade,
        // a late PerFile@G is a downgrade and is dropped.
        let after_upgrade = record(1, PublishTier::Analysis, "same");
        assert_eq!(
            publish_decision(
                Some(&after_upgrade),
                1,
                PublishTier::PerFile,
                &diags("same")
            ),
            DROP,
        );
    }
}
