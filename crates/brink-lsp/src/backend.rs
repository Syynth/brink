use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use brink_analyzer::{AnalysisOptions, AnalysisResult, Dialect, TypePolicy};
use brink_syntax::ast::AstNode;
use tokio::sync::{Notify, watch};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CodeLens, CodeLensOptions, CodeLensParams, CompletionItem,
    CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentFormattingParams,
    DocumentRangeFormattingParams, DocumentSymbolParams, DocumentSymbolResponse, FileChangeType,
    FileSystemWatcher, FoldingRange, FoldingRangeKind, FoldingRangeParams,
    FoldingRangeProviderCapability, GlobPattern, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, InlayHint as LspInlayHint, InlayHintLabel, InlayHintParams, Location,
    MarkupContent, MarkupKind, OneOf, ParameterInformation, ParameterLabel, Position,
    PrepareRenameResponse, Range, ReferenceParams, Registration, RenameOptions, RenameParams,
    SaveOptions, SemanticTokens, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    SignatureHelp, SignatureHelpOptions, SignatureHelpParams, SignatureInformation,
    SymbolInformation, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url,
    WorkDoneProgressOptions, WorkspaceEdit, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer};

use brink_ide::{
    CompletionContext, cursor_scope, detect_completion_context, is_visible_in_context,
};

use crate::convert::{self, LineIndex};
use crate::semantic_tokens;

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
    /// `"strict"` or `"gradual"`; defaults to `Gradual`, matching
    /// `AnalysisOptions::default()`. Mirrors `dialect` exactly (#660: PR
    /// #656 left this reachable only via the compiler CLI's `--types
    /// strict`, never via the IDE/LSP surface) — feeds `analysis_loop` so
    /// its diagnostics analyze under the client-declared types policy too.
    types: Arc<Mutex<TypePolicy>>,
}

impl LanguageOptions {
    pub fn new() -> Self {
        Self {
            dialect: Arc::new(Mutex::new(Dialect::default())),
            types: Arc::new(Mutex::new(TypePolicy::default())),
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

    fn uri_to_path(uri: &Url) -> Option<String> {
        uri.to_file_path()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
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
                .map(|d| convert::diagnostic_to_lsp(d, &idx))
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

    /// Scan workspace directories for .ink files and load them all.
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

    /// Recursively walk a directory, loading all .ink files.
    fn walk_and_load(&self, dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.walk_and_load(&path);
            } else if path.extension().is_some_and(|ext| ext == "ink") {
                let path_str = path.to_string_lossy().into_owned();
                self.load_file_from_disk(&path_str);
            }
        }
    }
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

/// Read `initializationOptions.<key>` as a string and, if present, write the
/// mapped value into `slot` (poisoned-lock-safe — a poisoned mutex silently
/// keeps its prior value rather than panicking). Shared by `initialize`'s
/// `dialect` and `types` (#660) handlers, which differ only in the key name
/// and the string→enum mapping.
fn apply_initialization_option<T: Copy>(
    params: &InitializeParams,
    key: &str,
    slot: &Mutex<T>,
    map: impl FnOnce(&str) -> T,
) {
    if let Some(requested) = params
        .initialization_options
        .as_ref()
        .and_then(|opts| opts.get(key))
        .and_then(|v| v.as_str())
        && let Ok(mut guard) = slot.lock()
    {
        *guard = map(requested);
    }
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
        if let Ok(mut ws) = self.workspace_roots.lock() {
            *ws = roots;
        }

        // T1b compiler dialect (docs/t1b-surface-spec.md §1, #589): an
        // authoring-time/tooling input, read once from
        // `initializationOptions.dialect` ("brink" or "strict-ink"; any
        // other value, or absence, keeps the `StrictInk` default). Gates
        // stdlib slice 1 completion/signature help only — see the `dialect`
        // field's doc comment.
        apply_initialization_option(&params, "dialect", &self.language.dialect, |v| match v {
            "brink" => Dialect::Brink,
            _ => Dialect::StrictInk,
        });

        // TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660): read
        // once from `initializationOptions.types` ("strict" or "gradual";
        // any other value, or absence, keeps the `Gradual` default), mirroring
        // the `dialect` handling directly above. `Strict` requires
        // `dialect = brink` (a config-error diagnostic otherwise, `E064`) —
        // the client's responsibility, same as the compiler CLI.
        apply_initialization_option(&params, "types", &self.language.types, |v| match v {
            "strict" => TypePolicy::Strict,
            _ => TypePolicy::Gradual,
        });

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

        // Register file watcher for **/*.ink (fire-and-forget — some test
        // clients don't respond to server-initiated requests)
        let client = self.client.clone();
        tokio::spawn(async move {
            let watcher = FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.ink".to_owned()),
                kind: None,
            };
            let registration = Registration {
                id: "ink-file-watcher".to_owned(),
                method: "workspace/didChangeWatchedFiles".to_owned(),
                register_options: serde_json::to_value(
                    tower_lsp::lsp_types::DidChangeWatchedFilesRegistrationOptions {
                        watchers: vec![watcher],
                    },
                )
                .ok(),
            };
            if let Err(e) = client.register_capability(vec![registration]).await {
                tracing::warn!("failed to register file watcher: {e}");
            }
        });

        // Scan workspace directories for .ink files
        self.load_workspace_files();
        self.trigger_analysis();
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        tracing::debug!(count = params.changes.len(), "did_change_watched_files");

        let mut changed = false;
        for change in &params.changes {
            let Some(path) = Self::uri_to_path(&change.uri) else {
                continue;
            };

            match change.typ {
                FileChangeType::CREATED | FileChangeType::CHANGED => {
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

        let Some(loc) =
            brink_ide::navigation::goto_definition(&snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };

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

        let refs = brink_ide::navigation::find_references(
            &snap.analysis,
            snap.file_id,
            offset,
            params.context.include_declaration,
        );

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

        for info in snap.analysis.index.symbols.values() {
            if !is_visible_in_context(&ctx, info, &cursor_scope) {
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

        let Some(range) = brink_ide::rename::prepare_rename(&snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };

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

        let Some(result) =
            brink_ide::rename::rename(&snap.analysis, snap.file_id, offset, &params.new_name)
        else {
            return Ok(None);
        };

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

// ─── LSP adapter helpers ────────────────────────────────────────────

/// Convert a domain `DocumentSymbol` to an LSP `DocumentSymbol`.
#[expect(deprecated, reason = "DocumentSymbol requires deprecated fields")]
fn domain_symbol_to_lsp(
    sym: brink_ide::document::DocumentSymbol,
    idx: &LineIndex,
) -> tower_lsp::lsp_types::DocumentSymbol {
    let children: Vec<_> = sym
        .children
        .into_iter()
        .map(|c| domain_symbol_to_lsp(c, idx))
        .collect();

    tower_lsp::lsp_types::DocumentSymbol {
        name: sym.name,
        detail: sym.detail,
        kind: convert::symbol_kind_to_lsp(sym.kind),
        tags: None,
        deprecated: None,
        range: convert::to_lsp_range(sym.full_range, idx),
        selection_range: convert::to_lsp_range(sym.range, idx),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

/// Build a `CompletionItem` from a `SymbolInfo`.
fn make_completion_item(
    info: &brink_ir::SymbolInfo,
    label_override: Option<String>,
) -> CompletionItem {
    let kind = match info.kind {
        brink_ir::SymbolKind::Knot => CompletionItemKind::MODULE,
        brink_ir::SymbolKind::Stitch | brink_ir::SymbolKind::External => {
            CompletionItemKind::FUNCTION
        }
        brink_ir::SymbolKind::Variable
        | brink_ir::SymbolKind::Constant
        | brink_ir::SymbolKind::Param
        | brink_ir::SymbolKind::Temp => CompletionItemKind::VARIABLE,
        brink_ir::SymbolKind::List => CompletionItemKind::ENUM,
        brink_ir::SymbolKind::ListItem => CompletionItemKind::ENUM_MEMBER,
        brink_ir::SymbolKind::Label => CompletionItemKind::REFERENCE,
        brink_ir::SymbolKind::Struct => CompletionItemKind::STRUCT,
    };

    let detail = match info.kind {
        brink_ir::SymbolKind::Knot if info.detail.as_deref() == Some("function") => {
            Some("function knot".to_string())
        }
        _ if !info.params.is_empty() => {
            let params: Vec<_> = info.params.iter().map(|p| p.name.as_str()).collect();
            Some(format!("({})", params.join(", ")))
        }
        _ => None,
    };

    CompletionItem {
        label: label_override.unwrap_or_else(|| info.name.clone()),
        kind: Some(kind),
        detail,
        ..Default::default()
    }
}

/// Build a `CompletionItem` for a T1b stdlib slice 1 function
/// (docs/t1b-surface-spec.md §5, #589) — signature as `detail` (the
/// lvalue-mutator rule renders right there, e.g. `push(a: lvalue, v)`), the
/// one-line semantics as markdown documentation.
fn make_stdlib_completion_item(f: &brink_ide::stdlib::StdlibFn) -> CompletionItem {
    CompletionItem {
        label: f.name.to_owned(),
        kind: Some(CompletionItemKind::FUNCTION),
        detail: Some(f.signature_label()),
        documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
            MarkupContent {
                kind: MarkupKind::Markdown,
                value: f.doc.to_owned(),
            },
        )),
        ..Default::default()
    }
}

fn format_config_from_options(
    _options: &tower_lsp::lsp_types::FormattingOptions,
) -> brink_fmt::FormatConfig {
    brink_fmt::FormatConfig::default()
}

/// Convert `brink_ide::diff_to_edits` output to LSP `TextEdit`s.
fn diff_to_lsp_edits(old: &str, new: &str) -> Vec<TextEdit> {
    let idx = LineIndex::new(old);
    brink_ide::diff_to_edits(old, new)
        .into_iter()
        .map(|(range, new_text)| TextEdit {
            range: convert::to_lsp_range(range, &idx),
            new_text,
        })
        .collect()
}

/// Check whether two LSP ranges overlap.
fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
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

        // Re-read the declared dialect + types policy each iteration
        // (poisoned-lock-safe, mirrors `Backend::dialect()`) so a client that
        // changes either mid-session is picked up on the next pass.
        let opts = AnalysisOptions {
            dialect: language
                .dialect
                .lock()
                .map_or_else(|_| Dialect::default(), |g| *g),
            types: language
                .types
                .lock()
                .map_or_else(|_| TypePolicy::default(), |g| *g),
            ..AnalysisOptions::default()
        };

        // Snapshot inputs under lock, reading the generation in the same locked
        // block so `(content, generation)` is a consistent pair: it reflects
        // exactly the content-revision folded into this pass. For any edit this
        // pass includes, that is `>=` the generation the edit's per-file publish
        // carried (both read the revision under the db lock; the analysis reads
        // at-or-after the write). Tagged `Analysis`, this therefore wins the
        // `DiagnosticsPublisher` anti-downgrade rule against that per-file set.
        let (generation, projects, file_meta, per_file_diags, file_suppressions) = {
            let db = lock_db(&db);
            let generation = generation.load(Ordering::Relaxed);
            let project_defs = db.compute_projects();
            let project_inputs: Vec<_> = project_defs
                .iter()
                .map(|(root, members)| (*root, db.analysis_inputs_for(members)))
                .collect();
            let meta = db.file_metadata();
            let diags: Vec<_> = meta
                .iter()
                .filter_map(|(fid, _, _)| Some((*fid, db.file_diagnostics(*fid)?.to_vec())))
                .collect();
            let suppressions: HashMap<brink_ir::FileId, brink_ir::suppressions::Suppressions> =
                meta.iter()
                    .filter_map(|(fid, _, _)| Some((*fid, db.suppressions(*fid)?.clone())))
                    .collect();
            (generation, project_inputs, meta, diags, suppressions)
        };

        // Run per-project analysis OUTSIDE the lock
        let mut by_root = HashMap::new();
        let mut file_to_roots: HashMap<brink_ir::FileId, Vec<brink_ir::FileId>> = HashMap::new();
        let mut project_members = HashMap::new();

        for (root, inputs) in &projects {
            let file_refs: Vec<_> = inputs
                .iter()
                .map(|(id, hir, manifest)| (*id, hir, manifest))
                .collect();
            let result = brink_analyzer::analyze_with_options(&file_refs, &opts);
            by_root.insert(*root, Arc::new(result));

            let members: Vec<_> = inputs.iter().map(|(id, _, _)| *id).collect();
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
                let mut lsp_diag = convert::diagnostic_to_lsp(d, idx);
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
                    .map(|d| convert::diagnostic_to_lsp(d, &idx))
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
            .map(|d| convert::diagnostic_to_lsp(d, &idx))
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

    use super::{PublishDecision, PublishRecord, PublishTier, publish_decision};

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
