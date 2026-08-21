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
use crate::backend::projects::{AnalysisSnapshot, NativeProjects};
use crate::convert::{self, LineIndex};
use crate::semantic_tokens;

mod adapters;
pub(crate) mod projects;

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
    /// `brink.toml`'s `[project] conventions` pointer (issue #1844; renamed
    /// from `elements` by #2180). `resolve_language_options` already
    /// resolves this correctly (it runs the file straight through
    /// `AnalysisOptions::apply_project_config`), but until issue #1880
    /// nothing carried the resolved value past `store` into
    /// `analysis_loop`'s own `AnalysisOptions` — every background pass fed
    /// `analysis_inputs_for`'s `lowered_query` a hardcoded `None`
    /// regardless of what `brink.toml` configured, so a native project's
    /// `@[convention]` handlers never claimed prose across files in the
    /// background pass, and unclaimed scene headings hit `lower_native`'s
    /// `E129` arm and dropped their scene bodies from the LSP's view. Since
    /// issue #2335, `analyze_with_modules` (which this loop calls directly,
    /// with no `ProjectDb` in between) also runs the confinement/
    /// unconfigured `E169` check off this same field — previously that gate
    /// was reachable only through a `brink-db` query this loop never calls.
    /// Mirrors `dialect`/`types`/`lints` exactly.
    conventions: Arc<Mutex<Option<String>>>,
}

impl LanguageOptions {
    pub fn new() -> Self {
        Self {
            dialect: Arc::new(Mutex::new(Dialect::default())),
            types: Arc::new(Mutex::new(None)),
            lints: Arc::new(Mutex::new(LintPolicy::default())),
            conventions: Arc::new(Mutex::new(None)),
        }
    }

    /// Write a freshly `resolve_language_options`-resolved dialect/types/
    /// lints/conventions into the shared session state (poisoned-lock-safe,
    /// mirrors [`Backend::dialect`]) — the common tail of `initialize` and
    /// [`Backend::reload_brink_toml`], both of which compute a fresh
    /// resolution and must publish it identically. Takes `resolved` by value
    /// since neither caller reads it again afterward.
    ///
    /// `resolved` is destructured exhaustively (no `..`), the same
    /// "spelled-out, not `Default`" pattern `IdeSnapshot::analyze`/
    /// `IdeSession::analysis_options`/`IdeSession::apply_analysis_options`
    /// use — issue #2334's shared-seam fix applied to this crate's own
    /// producer, which has no `IdeSession` to route through (`LanguageOptions`
    /// carries no `host_manifest`/`external_check`/`semantic_type_check`
    /// fields at all — brink-lsp has no host-manifest/external-check surface
    /// today). Adding a new `AnalysisOptions` field breaks this match until
    /// it's given an explicit `_` (deliberately unsupported here) or a real
    /// field to carry it in, rather than silently vanishing at this exact
    /// point the way `conventions` did three times running (#1880/#2317).
    fn store(&self, resolved: AnalysisOptions) {
        let AnalysisOptions {
            host_manifest: _,
            external_check: _,
            semantic_type_check: _,
            dialect,
            types,
            lints,
            conventions,
        } = resolved;
        if let Ok(mut guard) = self.dialect.lock() {
            *guard = dialect;
        }
        if let Ok(mut guard) = self.types.lock() {
            *guard = types;
        }
        if let Ok(mut guard) = self.lints.lock() {
            *guard = lints;
        }
        if let Ok(mut guard) = self.conventions.lock() {
            *guard = conventions;
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
    db: Arc<Mutex<NativeProjects>>,
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
    /// The undeclared-rename-detection baseline per file (issue #1672 part
    /// 2, docs/modules-spec.md §5): both `publish_perfile_diagnostics` and
    /// the background [`analysis_loop`]'s `publish_all_diagnostics` diff a
    /// file's current [`brink_ir::SymbolManifest`] against the entry
    /// recorded here (via [`rename_suspicion_diags`]) to surface an
    /// undeclared-rename hint. Shared with `analysis_loop` (constructed once
    /// in `main`, cloned into both) so the two publishers read the exact
    /// same baseline.
    ///
    /// Review finding on #1672 part 2 (blocking + should-fix): the baseline
    /// is advanced *only* by [`Self::checkpoint_manifest_baseline`], called
    /// from `did_open`/`did_save` — never from a per-file publish, and never
    /// on every `did_change` keystroke. Advancing it on every publish (the
    /// original shape) had two failures: (1) the background pass's own
    /// same-generation publish diffed against a baseline the per-file
    /// publish had *already* overwritten with the new content, so it always
    /// recomputed an empty suspicion set and its same-or-newer `Analysis`-
    /// tier publish silently replaced the client's hint with nothing —
    /// flashing for milliseconds then gone forever; (2) anchoring at every
    /// keystroke meant the suggested "old name" was usually an intermediate
    /// typing state (`hub` -> `plaz` -> `plaza`) rather than anything ever
    /// saved. Anchoring only at `did_open`/`did_save` fixes both: the
    /// baseline is stable across a burst of `did_change`s (so the fast and
    /// background publishes for the same edit agree), and it never records
    /// a name that only existed mid-keystroke.
    ///
    /// Absent for a file never published before (first `did_open`), which is
    /// exactly right — there's nothing to diff against yet.
    previous_manifests: Arc<Mutex<HashMap<brink_ir::FileId, brink_ir::SymbolManifest>>>,
}

/// Compute the undeclared-rename-suspicion diagnostics for `file_id`'s
/// current manifest (issue #1672 part 2), diffed read-only against the
/// checkpoint recorded in `previous_manifests` — see
/// [`Backend::previous_manifests`] for why this never writes back.
///
/// Gated on `dialect == Dialect::Brink`: `#@was` — what accepting the
/// suggestion ultimately writes — is itself a brink-extension directive
/// (`dialect_gate.rs`: "M-3 … `#@was` is brink-only"), so under the default
/// `Dialect::StrictInk` the suggestion would point the author at a directive
/// that immediately produces a fresh `E051` (review finding on #1672 part
/// 2, blocking: `rename()` itself gates on this same condition, but this
/// wiring site — and the equivalent one in `publish_all_diagnostics` — had
/// no dialect check at all).
fn rename_suspicion_diags(
    previous_manifests: &Mutex<HashMap<brink_ir::FileId, brink_ir::SymbolManifest>>,
    dialect: Dialect,
    file_id: brink_ir::FileId,
    new_manifest: &brink_ir::SymbolManifest,
    idx: &LineIndex,
) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    if dialect != Dialect::Brink {
        return Vec::new();
    }
    let previous = previous_manifests
        .lock()
        .map_or_else(|_| None, |guard| guard.get(&file_id).cloned());
    previous.as_ref().map_or_else(Vec::new, |previous| {
        brink_ide::rename_detection::detect_undeclared_renames(previous, new_manifest)
            .iter()
            .map(|s| convert::rename_suspicion_to_lsp(s, idx))
            .collect()
    })
}

impl Backend {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        client: Client,
        db: Arc<Mutex<NativeProjects>>,
        analysis_rx: watch::Receiver<Option<Arc<ProjectAnalyses>>>,
        analysis_trigger: Arc<Notify>,
        generation: Arc<AtomicU64>,
        publisher: DiagnosticsPublisher,
        language: LanguageOptions,
        previous_manifests: Arc<Mutex<HashMap<brink_ir::FileId, brink_ir::SymbolManifest>>>,
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
            previous_manifests,
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

    /// Re-sync every native project's own root with `ProjectDb` (issue
    /// #1580), so the identity the editor mints for a `.brink` file's
    /// module equals what a real compile of *its own governing* `brink.toml`
    /// mints.
    ///
    /// A workspace no longer has one native source root — [`NativeProjects`]
    /// discovers every governing `brink.toml` (`NativeRootsContext`,
    /// `projects.rs`) and re-syncs each project's own `native_root`
    /// independently. Only [`NativeProjectKey::Default`](projects::NativeProjectKey)
    /// — the legacy single project every `.ink` file still lives in — also
    /// gets `set_ink_root` called on it: ink's project extent is
    /// INCLUDE-reachability from that one root, out of #1580's scope, so
    /// every other (`Root`/`Orphan`) project's `ink_root` is left unset and
    /// `native_root`/`ink_root` now *diverge by design* on any workspace with
    /// more than one governing `brink.toml` (unlike `brink-compiler`'s
    /// `prepare_driver`, which computes a *per-entry* root from the entry's
    /// own directory, since a one-shot compile has no broader "workspace" to
    /// anchor to).
    ///
    /// Called from `initialize` and from every later
    /// [`reload_brink_toml`](Self::reload_brink_toml). The filesystem walk
    /// for sibling `brink.toml`s happens before the [`mutate_db`](Self::mutate_db)
    /// call so it never runs under the `NativeProjects` lock (issue #1580
    /// review finding); `mutate_db` itself still advances the content
    /// generation, since changing a root changes every native module name in
    /// that project, a real input change the background pass must
    /// re-analyze against.
    fn register_native_root(&self, roots: &[PathBuf], outcome: &ConfigLoadOutcome) {
        // Issue #1580: discover every governing `brink.toml` in the
        // workspace, not just the one `native_source_root` finds by walking
        // up from the first root. `compute_roots_context` performs a full
        // recursive filesystem walk (`discover_other_native_roots`), so it
        // runs here, BEFORE the lock — otherwise a batch of `brink.toml`
        // watched-file events (`did_change_watched_files` calls this once
        // per changed config) would each block every other LSP request on a
        // full workspace walk.
        let ctx = NativeProjects::compute_roots_context(roots, outcome);
        self.mutate_db(|projects| projects.apply_roots_context(ctx));
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

            let mut lsp_diags: Vec<_> = filtered
                .iter()
                .map(|d| convert::diagnostic_to_lsp(d, &idx, types, &lints))
                .collect();

            // Undeclared-rename detection (issue #1672 part 2,
            // docs/modules-spec.md §5): diff this file's manifest against
            // the checkpoint recorded for it (see
            // [`Self::checkpoint_manifest_baseline`]) and fold any
            // suspicion hint into the *same* publish —
            // `publishDiagnostics` replaces a URI's whole diagnostic set
            // per call, so this can't be sent separately without one
            // clobbering the other. Read-only: the baseline is advanced
            // only at `did_open`/`did_save`, never here (review finding on
            // #1672 part 2 — see [`rename_suspicion_diags`]).
            if let Some(new_manifest) = db.manifest(file_id) {
                let dialect = db
                    .analysis_options_for(file_id)
                    .map_or_else(Dialect::default, |o| o.dialect);
                lsp_diags.extend(rename_suspicion_diags(
                    &self.previous_manifests,
                    dialect,
                    file_id,
                    new_manifest,
                    &idx,
                ));
            }

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

    /// Advance the undeclared-rename-detection baseline (issue #1672 part 2)
    /// for `path` to its just-loaded manifest — see
    /// [`Self::previous_manifests`] for why this is the *only* place the
    /// baseline moves. Called from `did_open` and `did_save`, always after
    /// that handler's own [`Self::publish_perfile_diagnostics`] call, so the
    /// event's own diff (if any) still runs against the *old* checkpoint
    /// before this replaces it.
    fn checkpoint_manifest_baseline(&self, path: &str) {
        let db = lock_db(&self.db);
        let Some(file_id) = db.file_id(path) else {
            return;
        };
        let Some(manifest) = db.manifest(file_id) else {
            return;
        };
        if let Ok(mut guard) = self.previous_manifests.lock() {
            guard.insert(file_id, manifest.clone());
        }
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
    fn mutate_db<R>(&self, f: impl FnOnce(&mut NativeProjects) -> R) -> R {
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
        db.rebuild_include_graphs();
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
///
/// Delegates to [`brink_db::has_recognized_source_extension`] (issue #2368)
/// rather than a local, case-sensitive `ext == "ink" || ext == "brink"`
/// check — a real ink/native file spelled `story.INK`/`main.BRINK` (reachable
/// on a case-insensitive filesystem, macOS/Windows default) must classify as
/// source the same as its lowercase spelling.
fn is_source_path(path: &std::path::Path) -> bool {
    path.to_str()
        .is_some_and(brink_db::has_recognized_source_extension)
}

/// Whether `path` names a native `.brink` file — the only frontend whose
/// grammar has cue (`@NAME`) syntax at all
/// (`cue_names_are_never_harvested_from_the_ink_frontend`, `brink-analyzer`).
/// Used by [`Backend::completion`]'s cue-completion gate (review finding on
/// #2134, minor) to tell "an ink prose line that happens to start with `@`"
/// apart from a real native cue position.
///
/// Delegates to [`brink_db::is_native_source_path`] (issue #2368) — the
/// shared, case-insensitive seam `brink-db`'s own `file_language` already
/// implements correctly, rather than a local, case-sensitive `ext ==
/// "brink"` copy that silently misclassified `.BRINK` as non-native.
fn is_native_path(path: &str) -> bool {
    brink_db::is_native_source_path(path)
}

fn lock_db(db: &Arc<Mutex<NativeProjects>>) -> std::sync::MutexGuard<'_, NativeProjects> {
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
/// boolean, `None` for a missing key. A key that *is* present but holds a
/// non-bool value is skipped with a `tracing::warn!` — the same "warn, never
/// silently drop" channel [`explicit_initialization_lints`] uses for a
/// present-but-malformed `lints` value — rather than being treated as
/// silently unset.
fn explicit_initialization_bool(params: &InitializeParams, key: &str) -> Option<bool> {
    let value = params
        .initialization_options
        .as_ref()
        .and_then(|opts| opts.get(key))?;
    if let Some(b) = value.as_bool() {
        Some(b)
    } else {
        tracing::warn!("initializationOptions.{key}: expected a boolean, got {value}, ignored");
        None
    }
}

/// Read `initializationOptions.lints` — a client-declared per-code
/// lint-level override map (issue #1417), the LSP's counterpart of the
/// CLI's repeatable `--deny`/`--warn`/`--allow <CODE>` flags (#1373) and
/// `BrinkPlugin::with_config`'s `ProjectConfig.lints` (#1394). Accepts a
/// JSON object `{ "<CODE>": "deny" | "warn" | "allow" | "info" | "hint" }`
/// — the same five strings a `brink.toml` `[lints]` table accepts
/// (`brink_project_config::parse_lint_level`; `"info"`/`"hint"` added by
/// issue #1162). A missing key resolves to no overrides at all (an empty
/// map, the same as never setting the field). A present but non-object
/// value, or a per-code value that isn't one of the five recognized
/// strings, is skipped with a `tracing::warn!` — the same "warn, never
/// silently drop" channel [`resolve_language_options`] already uses for a
/// `brink.toml`'s own unknown keys — rather than resolving to a hard
/// `initialize` failure; the real code/overridability validation still
/// happens once, downstream, in
/// `AnalysisOptions::apply_lint_overrides` (#1160's `validate_lint_code`
/// gate).
fn explicit_initialization_lints(params: &InitializeParams) -> BTreeMap<String, LintLevel> {
    let mut lints = BTreeMap::new();
    let Some(value) = params
        .initialization_options
        .as_ref()
        .and_then(|opts| opts.get("lints"))
    else {
        return lints;
    };
    let Some(obj) = value.as_object() else {
        tracing::warn!("initializationOptions.lints: expected an object, got {value}, ignored");
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
            Some("info") => {
                lints.insert(code.clone(), LintLevel::Info);
            }
            Some("hint") => {
                lints.insert(code.clone(), LintLevel::Hint);
            }
            _ => {
                tracing::warn!(
                    "initializationOptions.lints.{code}: expected \"allow\" | \"warn\" | \"deny\" | \"info\" | \"hint\", ignored"
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
/// `brink-project-config`'s [`brink_project_config::find_config_with_warnings`]
/// walk directly (rather than [`brink_project_config::discover_from_entry`],
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
/// it. A `brink.toml` the bounded walk stepped over (a workspace/git
/// boundary, or the ancestor-depth cap for a VCS-less workspace, #1435) gets
/// the same `tracing::warn!` treatment — logged, not silently dropped,
/// though (like the unknown-key case) it never earns its own
/// `ConfigLoadOutcome::diagnostic`.
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

    if let Some(root) = roots.first() {
        let (path, discovery_warnings) = brink_project_config::find_config_with_warnings(root);
        // A config the bounded walk stepped over (#1435) — never used below,
        // only logged, on the same "warn, never silently drop" channel every
        // other warning in this function uses.
        for warning in &discovery_warnings {
            tracing::warn!("{warning}");
        }
        if let Some(path) = path {
            outcome.path = Some(path.clone());
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    match brink_project_config::parse_str_at(path.display().to_string(), &text) {
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
                    }
                }
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
/// divergence from the prune this helper claims to mirror). A path under
/// **non-empty** `roots`, none of which is a prefix of it, falls back to
/// checking every component, same as before — see below for what happens
/// when `roots` itself is empty.
///
/// `roots` itself can be empty — single-file mode (no workspace folders and
/// no legacy `root_uri`), or a watcher event that lands before `initialize`
/// has populated `self.workspace_roots` at all. There, house rule 19a
/// applies exactly as it did for `native_root` in #1576: with nothing to
/// strip, the path is returned untouched rather than mangled — this
/// function declines to prune rather than falling back to matching
/// components of the raw, unscoped absolute path. That whole-path fallback
/// is exactly the pre-#1415 behavior, and it treats any directory name that
/// merely *appears* somewhere in the user's absolute path (a project
/// checked out under `~/code/node_modules-backup/…`, say) as an ignored
/// directory, silently rejecting a real file it has no business rejecting
/// (#1434). No workspace root means there is no root-relative frame to
/// scope the check against, so the check is skipped rather than guessed.
fn path_under_ignored_dir(path: &str, roots: &[std::path::PathBuf]) -> bool {
    if roots.is_empty() {
        return false;
    }
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
                    // `@` (issue #2134 review finding, blocking): the cue-
                    // name completion position (`CompletionContext::CueName`)
                    // sits right after `@` at the start of a line — without
                    // it as a trigger character, a client that only asks the
                    // server for completions on a registered trigger (rather
                    // than on every keystroke) never requests them at that
                    // position, contradicting the "a real user hits this by
                    // typing `@`" reachability claim.
                    trigger_characters: Some(vec!["->".into(), ".".into(), "@".into()]),
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
        // Seed the undeclared-rename baseline from this open (issue #1672
        // part 2) — after the publish above, so a freshly opened file with
        // no prior checkpoint still correctly diffs against nothing.
        self.checkpoint_manifest_baseline(&path);
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
        // Re-anchor the undeclared-rename baseline at this save (issue
        // #1672 part 2, review finding: the "stable checkpoint" — anchoring
        // on every `did_change` instead recorded intermediate typing states
        // as the suggested old name). After the publish above, so this
        // save's own diff still runs against the *previous* checkpoint.
        self.checkpoint_manifest_baseline(&path);
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
            // Same reasoning for the undeclared-rename manifest baseline
            // (issue #1672 part 2): a reopen has no "previous compile" of
            // its own yet, so a stale entry from before the close must not
            // survive to be diffed against the reopened file's first edit.
            if let Ok(mut guard) = self.previous_manifests.lock() {
                guard.remove(&fid);
            }
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
        let projects = lock_db(&self.db);
        let Some(db) = projects.project_for_path(&path) else {
            return Ok(None);
        };
        let Some(loc) =
            brink_ide::navigation::goto_definition(db, &snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };
        drop(projects);

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
        let projects = lock_db(&self.db);
        let Some(db) = projects.project_for_path(&path) else {
            return Ok(None);
        };
        let refs = brink_ide::navigation::find_references(
            db,
            &snap.analysis,
            snap.file_id,
            offset,
            params.context.include_declaration,
        );
        drop(projects);

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
        let projects = lock_db(&self.db);
        let Some(db) = projects.project_for_path(&path) else {
            return Ok(None);
        };
        let Some(info) = brink_ide::hover::hover(
            &snap.analysis,
            db,
            snap.file_id,
            &snap.source,
            offset,
            &snap.project_files,
        ) else {
            return Ok(None);
        };
        drop(projects);

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

        let mut ctx = detect_completion_context(&snap.source, byte_offset);
        let cursor_scope = cursor_scope(&snap.source, byte_offset);

        let mut items: Vec<CompletionItem> = Vec::new();

        // Cue-name completion (issue #2134, `docs/prose-dialect-spec.md`
        // §5): every `@NAME` cue harvested anywhere in the project — not
        // just this file — completes here, "harvest by default" regardless
        // of whether any conventions handler claims it. Reads the range-free
        // completion projection (`harvest_completion_names`), not the raw
        // `harvest_index`, for the same Eq-cutoff reason
        // `resolution_index_query` exists for the symbol index (see that
        // query's own doc).
        //
        // `detect_completion_context` is dialect-agnostic (review finding on
        // #2134, minor): it classifies purely from source text, so a plain
        // ink prose line that happens to start with `@` (e.g. `@midnight the
        // clock…`) is misread as the same `CueName` position, even though
        // ink's grammar has no cue syntax at all
        // (`cue_names_are_never_harvested_from_the_ink_frontend`,
        // `brink-analyzer`) — an ink file's harvest contribution is always
        // empty, project-wide harvest from *other* native files
        // notwithstanding. Gate on the file's own language (native `.brink`
        // vs ink), not on whether the harvest happens to be empty right now:
        // a native file with zero declared cues anywhere in the project is
        // still a genuine (if currently empty) cue position and must keep
        // returning no items rather than falling back to ordinary symbols —
        // exactly what `cue_name_completion_offers_nothing_but_harvested_cues`
        // pins. Only an ink file — which can never mean a cue, regardless of
        // harvest state — downgrades `ctx` to `General` here.
        if matches!(ctx, CompletionContext::CueName) {
            if is_native_path(&path) {
                let projects = lock_db(&self.db);
                let names = projects
                    .project_for_path(&path)
                    .map(brink_db::ProjectDb::harvest_completion_names);
                drop(projects);
                if let Some(names) = names {
                    for cue in &names.cues {
                        items.push(CompletionItem {
                            label: cue.clone(),
                            kind: Some(CompletionItemKind::CONSTANT),
                            detail: Some("cue".to_owned()),
                            ..Default::default()
                        });
                    }
                }
                return Ok(Some(CompletionResponse::Array(items)));
            }
            ctx = CompletionContext::General;
        }

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

        let all_files = lock_db(&self.db).all_file_metadata();

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

        // #1350: a `.brink` document must be classified from the *native*
        // CST (`db.parse_native`/`semantic_tokens_native`), not ink's —
        // `db.parse` always runs the ink frontend regardless of extension
        // (the dispatch on `db.is_native` lives only in `lowered_query`),
        // so calling it unconditionally here reproduced #2280's bug one
        // layer up, for every real editor talking to this server over LSP.
        let data = {
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
            if db.is_native(file_id) {
                let Some(parse) = db.parse_native(file_id) else {
                    return Ok(None);
                };
                let root = parse.syntax();
                semantic_tokens::compute_semantic_tokens_native(&source, &root, &analysis, file_id)
            } else {
                let Some(parse) = db.parse(file_id) else {
                    return Ok(None);
                };
                let root = parse.syntax();
                semantic_tokens::compute_semantic_tokens(&source, &root, &analysis, file_id)
            }
        };

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

        let range = params.range;

        // #1350: same native-CST routing as `semantic_tokens_full` above.
        let data = {
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
            if db.is_native(file_id) {
                let Some(parse) = db.parse_native(file_id) else {
                    return Ok(None);
                };
                let root = parse.syntax();
                semantic_tokens::compute_semantic_tokens_range_native(
                    &source,
                    &root,
                    &analysis,
                    file_id,
                    range.start.line,
                    range.end.line,
                )
            } else {
                let Some(parse) = db.parse(file_id) else {
                    return Ok(None);
                };
                let root = parse.syntax();
                semantic_tokens::compute_semantic_tokens_range(
                    &source,
                    &root,
                    &analysis,
                    file_id,
                    range.start.line,
                    range.end.line,
                )
            }
        };

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
        let projects = lock_db(&self.db);
        let Some(db) = projects.project_for_path(&path) else {
            return Ok(None);
        };
        let Some(range) =
            brink_ide::rename::prepare_rename(db, &snap.analysis, snap.file_id, offset)
        else {
            return Ok(None);
        };
        drop(projects);

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
        let projects = lock_db(&self.db);
        let Some(db) = projects.project_for_path(&path) else {
            return Ok(None);
        };
        let Some(result) =
            brink_ide::rename::rename(db, &snap.analysis, snap.file_id, offset, &params.new_name)
        else {
            return Ok(None);
        };
        drop(projects);

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

        let (source, is_native, import_actions, fn_value_actions, value_call_actions) = {
            let projects = lock_db(&self.db);
            let Some(db) = projects.project_for_path(&path) else {
                return Ok(Some(vec![]));
            };
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
            let import_actions = brink_ide::import_fix::import_actions(db, file_id, offset);
            // T1c creation-site + call()/bind() strict quick-fixes (issue
            // #744): same session-aware merge posture.
            let fn_value_actions =
                brink_ide::creation_site_fix::fn_value_actions(db, file_id, offset);
            let value_call_actions =
                brink_ide::value_call_fix::value_call_actions(db, file_id, offset);
            (
                source,
                db.is_native(file_id),
                import_actions,
                fn_value_actions,
                value_call_actions,
            )
        };

        let idx = LineIndex::new(&source);
        let cursor_offset: usize = idx
            .offset(params.range.start.line, params.range.start.character)
            .into();

        // #2360: `brink_ide::code_actions::code_actions` unconditionally
        // parses `source` with `brink_syntax::parse` and offers actions over
        // ink-only structure (`tree.knots()`/`tree.stitches()` — sort/format
        // knot/stitch) that has no native analog at all: a `.brink` file has
        // no `=== knot ===`/stitch headers for the ink parser to ever match,
        // so this is a coincidental no-op today rather than a guaranteed
        // one — the exact "gate explicitly, don't rely on the coincidence"
        // lesson PRs #2286/#2358 already applied to `sort_knots_in_source`
        // and `convert_element` in `crates/brink-web`'s `EditorSession`.
        let mut domain_actions = if is_native {
            Vec::new()
        } else {
            brink_ide::code_actions::code_actions(&source, cursor_offset)
        };
        domain_actions.extend(import_actions);
        domain_actions.extend(fn_value_actions);
        domain_actions.extend(value_call_actions);

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

        let Some(action_data) = code_action_data_from_json(kind, &data) else {
            return Ok(action);
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
            // #2360: `brink_fmt::format` is ink-only (it unconditionally
            // ink-parses), so formatting a native document would rewrite it
            // from a misparse. Decline until a native formatter path exists.
            if db.is_native(file_id) {
                return Ok(None);
            }
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
            // #2360: same native gate as `formatting` above.
            if db.is_native(file_id) {
                return Ok(None);
            }
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

        let projects = lock_db(&self.db);
        let Some(db) = projects.project_for_path(&path) else {
            return Ok(None);
        };
        let Some(file_id) = db.file_id(&path) else {
            return Ok(None);
        };

        // The LSP has no host-value push channel (#174) — static value labels
        // still resolve from the manifest; `host`-source labels need none.
        // TM-5 (#621): `db` stays locked through this call — inlay hints now
        // also read `db.infer_body` for unannotated `temp` decls.
        //
        // #2360: route a `.brink` document through `inlay_hints_native` off
        // `db.parse_native` — `db.parse` always runs the ink frontend
        // regardless of extension, the same class of bug #1350 fixed for
        // semantic tokens.
        let domain_hints = if db.is_native(file_id) {
            let Some(parse) = db.parse_native(file_id) else {
                return Ok(None);
            };
            let root = parse.syntax();
            brink_ide::inlay_hints::inlay_hints_native(
                &root,
                &snap.analysis,
                db,
                file_id,
                request_range,
                None,
            )
        } else {
            let Some(parse) = db.parse(file_id) else {
                return Ok(None);
            };
            let root = parse.tree();
            brink_ide::inlay_hints::inlay_hints(
                root.syntax(),
                &snap.analysis,
                db,
                file_id,
                request_range,
                None,
            )
        };
        drop(projects);

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
#[expect(clippy::too_many_arguments)]
pub async fn analysis_loop(
    db: Arc<Mutex<NativeProjects>>,
    generation: Arc<AtomicU64>,
    trigger: Arc<Notify>,
    tx: watch::Sender<Option<Arc<ProjectAnalyses>>>,
    client: Client,
    publisher: DiagnosticsPublisher,
    language: LanguageOptions,
    // Shared with `Backend` (issue #1672 part 2, review finding): the
    // background pass diffs the same undeclared-rename baseline
    // `Backend::checkpoint_manifest_baseline` maintains, read-only here
    // too — see [`rename_suspicion_diags`] for why both publishers must
    // agree on this baseline within one generation.
    previous_manifests: Arc<Mutex<HashMap<brink_ir::FileId, brink_ir::SymbolManifest>>>,
) {
    loop {
        trigger.notified().await;
        // Coalesce rapid edits — yield so any queued notifications collapse
        tokio::task::yield_now().await;

        // Re-read the declared dialect + types + lints policy each iteration
        // (poisoned-lock-safe, mirrors `Backend::dialect()`) so a client that
        // changes any of them mid-session is picked up on the next pass.
        //
        // Spelled out field-by-field rather than `..AnalysisOptions::
        // default()` (issue #2334: the same "spelled-out, not `Default`"
        // completeness guard `IdeSnapshot::analyze`/`IdeSession::
        // apply_analysis_options` use) — a `..Default::default()` tail lets
        // a *new* `AnalysisOptions` field silently default here forever,
        // which is exactly how `conventions` almost stayed unreachable from
        // this loop (issue #1880) before it was added by hand. `host_manifest`/
        // `external_check`/`semantic_type_check` genuinely have no
        // brink-lsp-side source today (no `initializationOptions` surface
        // for them, mirroring `LanguageOptions`'s own field set above) —
        // explicit defaults here, not an implicit `..`, so the next field
        // brink-lsp *does* need to forward breaks this construction until
        // it's added.
        let opts = AnalysisOptions {
            host_manifest: None,
            external_check: brink_analyzer::ExternalCheckSeverity::default(),
            semantic_type_check: brink_analyzer::SemanticTypeDiagnosticSeverity::default(),
            dialect: language
                .dialect
                .lock()
                .map_or_else(|_| Dialect::default(), |g| *g),
            types: language.types.lock().map_or_else(|_| None, |g| *g),
            lints: language
                .lints
                .lock()
                .map_or_else(|_| LintPolicy::default(), |g| g.clone()),
            // `[project] conventions` (issue #1880): without this, every
            // background analysis pass fed `snapshot_for_analysis` a
            // hardcoded `None` regardless of what `brink.toml` configured.
            // `analysis_inputs_for`'s `lowered_query` reads this through
            // `external_claim_handlers_query` to inject cross-file claiming
            // — with it hardcoded `None`, a configured conventions module's
            // `@[convention]` handlers claimed nothing outside their own
            // file, so unclaimed scene headings elsewhere fell to
            // `lower_native`'s loud `E129` arm and dropped their scene
            // bodies from the LSP's view. `analyze_with_modules` below also
            // reads this field directly to run the confinement/unconfigured
            // `E169` check itself now (issue #2335).
            conventions: language
                .conventions
                .lock()
                .map_or_else(|_| None, |g| g.clone()),
        };

        // Snapshot inputs under lock, reading the generation in the same locked
        // block so `(content, generation)` is a consistent pair: it reflects
        // exactly the content-revision folded into this pass. For any edit this
        // pass includes, that is `>=` the generation the edit's per-file publish
        // carried (both read the revision under the db lock; the analysis reads
        // at-or-after the write). Tagged `Analysis`, this therefore wins the
        // `DiagnosticsPublisher` anti-downgrade rule against that per-file set.
        // Issue #1580: `Backend` may now hold several independent native
        // projects (one per governing `brink.toml`, plus ink's own
        // default) rather than one shared db. `NativeProjects::
        // snapshot_for_analysis` does per-project exactly what this block
        // used to do to one db — push `opts` as each project's own salsa
        // input (guarded against unchanged values exactly as before, see
        // its own doc), then snapshot `compute_projects`/`analysis_inputs_
        // for`/`is_native`/`module_map`/`module_map_diagnostics`/
        // `file_metadata`/`file_diagnostics`/`suppressions`/`manifest` —
        // and merges every project's output into one set of flat
        // collections. That merge is safe (never conflates two projects'
        // files) because every project's `FileId`s are disjoint ranges.
        let snap = {
            let mut db = lock_db(&db);
            db.snapshot_for_analysis(&opts)
        };
        let generation = generation.load(Ordering::Relaxed);
        let AnalysisSnapshot {
            projects,
            modules,
            module_diags,
            file_meta,
            per_file_diags,
            file_suppressions,
            manifests,
        } = snap;

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
            &manifests,
            &previous_manifests,
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
///
/// `manifests`/`previous_manifests` fold the undeclared-rename-suspicion
/// hint (issue #1672 part 2) into this `Analysis`-tier set too — review
/// finding on #1672 part 2 (blocking): without this, the fast per-file
/// publish was the *only* place that ever computed the hint, and this
/// pass's same-or-newer `Analysis`-tier publish (computing a set that never
/// carried it) won the anti-downgrade exchange and silently replaced it
/// with nothing, the moment background analysis ran for the same edit.
#[expect(clippy::too_many_arguments)]
#[expect(clippy::too_many_lines)]
async fn publish_all_diagnostics(
    publisher: &DiagnosticsPublisher,
    projects: &ProjectAnalyses,
    file_meta: &[(brink_ir::FileId, String, String)],
    per_file_diags: &[(brink_ir::FileId, Vec<brink_ir::Diagnostic>)],
    file_suppressions: &HashMap<brink_ir::FileId, brink_ir::suppressions::Suppressions>,
    generation: u64,
    opts: &AnalysisOptions,
    manifests: &HashMap<brink_ir::FileId, brink_ir::SymbolManifest>,
    previous_manifests: &Mutex<HashMap<brink_ir::FileId, brink_ir::SymbolManifest>>,
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
                if let Some(new_manifest) = manifests.get(file_id) {
                    lsp_diags.extend(rename_suspicion_diags(
                        previous_manifests,
                        opts.dialect,
                        *file_id,
                        new_manifest,
                        &idx,
                    ));
                }

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

        let mut lsp_diags: Vec<tower_lsp::lsp_types::Diagnostic> = filtered
            .iter()
            .map(|d| convert::diagnostic_to_lsp(d, &idx, opts.type_policy(), &opts.lints))
            .collect();
        if let Some(new_manifest) = manifests.get(file_id) {
            lsp_diags.extend(rename_suspicion_diags(
                previous_manifests,
                opts.dialect,
                *file_id,
                new_manifest,
                &idx,
            ));
        }

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
        brink_ide::code_actions::CodeActionData::AddImport {
            module,
            name,
            native,
        } => {
            serde_json::json!({
                "kind": "add_import", "uri": uri, "module": module, "name": name,
                "native": native,
            })
        }
        brink_ide::code_actions::CodeActionData::TrimFnLiteralArgs {
            target,
            occurrence,
            keep,
        } => serde_json::json!({
            "kind": "trim_fn_literal_args", "uri": uri,
            "target": target, "occurrence": occurrence, "keep": keep,
        }),
        brink_ide::code_actions::CodeActionData::BindFnLiteralRefArgs {
            target,
            occurrence,
            vars,
        } => serde_json::json!({
            "kind": "bind_fn_literal_ref_args", "uri": uri,
            "target": target, "occurrence": occurrence, "vars": vars,
        }),
        brink_ide::code_actions::CodeActionData::TrimValueCallArgs {
            verb,
            occurrence,
            keep,
        } => serde_json::json!({
            "kind": "trim_value_call_args", "uri": uri,
            "verb": verb, "occurrence": occurrence, "keep": keep,
        }),
    }
}

/// Read `data[field]` as a JSON u64 and narrow it to `usize`, clamping
/// (never wrapping) on a lossy platform/value combination — this data only
/// ever carries small in-file occurrence/argument counts, but a malformed or
/// tampered `data` payload must not silently truncate into a wrong index.
fn json_u64_as_usize(data: &serde_json::Value, field: &str) -> usize {
    data.get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(usize::MAX)
}

/// Decode a `code_action_resolve` request's `data` payload (the `kind`
/// discriminator plus the rest of the JSON object built by
/// [`code_action_data_to_json`]) back into a [`CodeActionData`]. `None` for
/// an unrecognized `kind` — this also covers `ReorderStitch`/`MoveStitch`/
/// `PromoteStitch`/`DemoteKnot`: [`code_action_data_to_json`] *does* emit a
/// `kind` string for each of them (`"reorder_stitch"`/`"move_stitch"`/
/// `"promote_stitch"`/`"demote_knot"`), they simply have no decode arm below, so
/// resolve is a no-op for them (see that fn's own doc: they are surfaced but
/// not yet resolvable over LSP).
///
/// Split out of [`Backend::code_action_resolve`] to keep that function under
/// the workspace's `too_many_lines` lint budget — the other multi-arm
/// dispatch table in this file, [`code_action_data_to_json`], already lives
/// as its own free function for the same reason (`format_config_from_options`
/// is the same pattern but lives in `backend/adapters.rs`, imported above).
///
/// [`CodeActionData`]: brink_ide::code_actions::CodeActionData
fn code_action_data_from_json(
    kind: Option<&str>,
    data: &serde_json::Value,
) -> Option<brink_ide::code_actions::CodeActionData> {
    let knot_name = data
        .get("knot")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    Some(match kind {
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
            let native = data
                .get("native")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            brink_ide::code_actions::CodeActionData::AddImport {
                module: module.to_owned(),
                name: name.to_owned(),
                native,
            }
        }
        Some("trim_fn_literal_args") => {
            let target = data
                .get("target")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            brink_ide::code_actions::CodeActionData::TrimFnLiteralArgs {
                target: target.to_owned(),
                occurrence: json_u64_as_usize(data, "occurrence"),
                keep: json_u64_as_usize(data, "keep"),
            }
        }
        Some("bind_fn_literal_ref_args") => {
            let target = data
                .get("target")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let vars = data
                .get("vars")
                .and_then(serde_json::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            brink_ide::code_actions::CodeActionData::BindFnLiteralRefArgs {
                target: target.to_owned(),
                occurrence: json_u64_as_usize(data, "occurrence"),
                vars,
            }
        }
        Some("trim_value_call_args") => {
            let verb = data
                .get("verb")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            brink_ide::code_actions::CodeActionData::TrimValueCallArgs {
                verb: verb.to_owned(),
                occurrence: json_u64_as_usize(data, "occurrence"),
                keep: json_u64_as_usize(data, "keep"),
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::Diagnostic;

    use std::collections::HashMap;
    use std::sync::Mutex;

    use brink_analyzer::Dialect;
    use rowan::{TextRange, TextSize};

    use super::{
        ConfigLoadOutcome, ConfigOverrides, LanguageOptions, LineIndex, PublishDecision,
        PublishRecord, PublishTier, collect_source_files, config_error_diagnostic, is_native_path,
        is_source_path, native_source_root, path_under_ignored_dir, publish_decision,
        rename_suspicion_diags, resolve_language_options,
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

    /// Review finding on #2134 (minor): `is_native_path` is the gate
    /// `Backend::completion` uses to tell a real native cue position apart
    /// from an ink prose line that merely starts with `@` — get the
    /// extension boundary right, including the no-extension and
    /// `.brink`-substring-but-not-suffix cases.
    #[test]
    fn is_native_path_matches_only_the_brink_extension() {
        assert!(is_native_path("main.brink"));
        assert!(is_native_path("market/vendor.brink"));
        assert!(!is_native_path("main.ink"));
        assert!(!is_native_path("notes.brink.txt"));
        assert!(!is_native_path("no_extension"));
    }

    /// Issue #2368: `is_native_path`/`is_source_path` must now delegate to
    /// `brink_db`'s shared, case-insensitive predicates instead of a local
    /// `ext == "brink"`/`ext == "ink" || ext == "brink"` copy — a real
    /// `.BRINK`/`.INK` file (reachable on a case-insensitive filesystem,
    /// macOS/Windows default) must classify identically to its lowercase
    /// spelling, not silently fall through as unrecognized.
    #[test]
    fn is_native_path_and_is_source_path_are_case_insensitive() {
        assert!(
            is_native_path("main.BRINK"),
            "uppercase .BRINK must be native"
        );
        assert!(
            is_native_path("Market/Vendor.Brink"),
            "mixed-case .Brink must be native"
        );
        assert!(
            is_source_path(std::path::Path::new("main.BRINK")),
            "uppercase .BRINK must count as a tracked source file"
        );
        assert!(
            is_source_path(std::path::Path::new("story.INK")),
            "uppercase .INK must count as a tracked source file"
        );
        assert!(
            !is_source_path(std::path::Path::new("readme.MD")),
            "an unrecognized extension must still be rejected"
        );
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
    /// directory name, and must leave ordinary paths alone. Scoped to a
    /// matching root (rather than `&[]`, which after #1434 skips the check
    /// entirely — see `path_under_ignored_dir_does_not_prune_without_a_root`
    /// below) so this still exercises the "any component" claim.
    #[test]
    fn path_under_ignored_dir_matches_any_component() {
        let root = std::path::PathBuf::from("/repo");
        let roots = std::slice::from_ref(&root);
        assert!(path_under_ignored_dir(
            "/repo/target/debug/build.ink",
            roots
        ));
        assert!(path_under_ignored_dir("/repo/.git/objects/pack.ink", roots));
        assert!(path_under_ignored_dir(
            "/repo/node_modules/some-pkg/index.ink",
            roots
        ));
        assert!(!path_under_ignored_dir("/repo/src/main.ink", roots));
        assert!(!path_under_ignored_dir("/repo/targets/main.ink", roots));
    }

    /// #1603 review: with a **non-empty** `roots`, none of which is a prefix
    /// of `path`, `path_under_ignored_dir` falls back to checking every
    /// component of the raw, unscoped path (the `.unwrap_or(full)` branch) —
    /// deliberately preserved pre-#1434 behavior, distinct from the
    /// empty-`roots` case which declines to prune entirely (see
    /// `path_under_ignored_dir_does_not_prune_without_a_root`). This was the
    /// only reachable case left with no direct test.
    #[test]
    fn path_under_ignored_dir_matches_any_component_when_no_root_prefixes_it() {
        let root = std::path::PathBuf::from("/repo");
        assert!(path_under_ignored_dir(
            "/elsewhere/node_modules/pkg/main.ink",
            std::slice::from_ref(&root)
        ));
    }

    /// #1434 regression (the issue's own acceptance criterion): with
    /// `workspace_roots` empty — single-file mode, or a watcher event that
    /// arrives before `initialize` has populated it — there is no
    /// root-relative frame to scope the check against. Falling back to the
    /// pre-#1415 whole-path check (as `path_under_ignored_dir` used to,
    /// before this fix) would flag any absolute path that merely *contains*
    /// an ignored-looking component anywhere in its ancestry, even when
    /// that component has nothing to do with the file's actual project
    /// tree. The fix declines to prune rather than guessing.
    #[test]
    fn path_under_ignored_dir_does_not_prune_without_a_root() {
        assert!(!path_under_ignored_dir(
            "/Users/dev/node_modules/my-project/story.ink",
            &[]
        ));
        assert!(!path_under_ignored_dir("/repo/target/debug/build.ink", &[]));
        assert!(!path_under_ignored_dir("/repo/.git/objects/pack.ink", &[]));
        assert!(!path_under_ignored_dir("/repo/src/main.ink", &[]));
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

        // With `roots` empty — no root at all to scope against — #1434
        // changed this from falling back to the pre-#1415-fix whole-path
        // check (which wrongly flagged this file) to declining to prune.
        assert!(!path_under_ignored_dir(
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

    /// Issue #1880: `resolve_language_options` already resolves
    /// `[project] conventions` correctly (it runs the file straight through
    /// `AnalysisOptions::apply_project_config`, the same seam every other
    /// field uses) — the gap was `LanguageOptions::store` silently dropping
    /// the resolved value on the floor instead of writing it into the
    /// shared session state `analysis_loop` reads. This proves the whole
    /// chain end to end: a real `resolve_language_options` call against a
    /// `brink.toml` that sets a path-shaped `conventions` pointer, stored
    /// into a fresh `LanguageOptions`, must be readable back out — a
    /// regression test for this exact fix, not just the pre-existing
    /// resolution.
    #[test]
    fn resolve_language_options_conventions_pointer_survives_store() {
        let root = temp_dir("conventions-store");
        std::fs::write(
            root.join("brink.toml"),
            "[project]\nconventions = \"conventions.brink\"\n",
        )
        .expect("write brink.toml");

        let (resolved, _outcome) =
            resolve_language_options(&ConfigOverrides::default(), std::slice::from_ref(&root));
        assert_eq!(
            resolved.conventions.as_deref(),
            Some("conventions.brink"),
            "resolve_language_options must resolve a path-shaped [project] \
             conventions pointer"
        );

        let language = LanguageOptions::new();
        language.store(resolved);
        let stored = language
            .conventions
            .lock()
            .expect("uncontended lock")
            .clone();
        assert_eq!(
            stored,
            Some("conventions.brink".to_owned()),
            "LanguageOptions::store must carry the resolved conventions \
             pointer into shared session state — before issue #1880's fix \
             this field did not exist and the value was silently dropped, \
             so analysis_loop's background passes always analyzed as if no \
             conventions module were configured"
        );

        std::fs::remove_dir_all(&root).ok();
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

    /// A minimal `DeclaredSymbol` naming a knot, for feeding
    /// [`rename_suspicion_diags`] a manifest without going through a full
    /// `IdeSession`.
    fn knot_decl(name: &str, start: u32) -> brink_ir::DeclaredSymbol {
        let end = start + u32::try_from(name.len()).expect("test name fits u32");
        brink_ir::DeclaredSymbol {
            name: name.to_owned(),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        }
    }

    #[test]
    fn rename_suspicion_diags_is_gated_on_brink_dialect() {
        // Review finding on #1672 part 2 (blocking): `rename()` itself
        // gates `#@was` stamping on `Dialect::Brink` (see
        // `brink_ide::rename::renaming_under_strict_ink_dialect_does_not_stamp_was`),
        // but this wiring site had no dialect check at all — under the
        // default `Dialect::StrictInk`, the suspicion would suggest a
        // directive (`#@was`) that itself produces a fresh `E051`.
        let file_id = brink_ir::FileId(0);
        let mut previous = HashMap::new();
        previous.insert(
            file_id,
            brink_ir::SymbolManifest {
                knots: vec![knot_decl("hub", 4)],
                ..Default::default()
            },
        );
        let previous_manifests = Mutex::new(previous);
        let new_manifest = brink_ir::SymbolManifest {
            knots: vec![knot_decl("plaza", 4)],
            ..Default::default()
        };
        let idx = LineIndex::new("=== plaza ===\nHi.\n-> END\n");

        let strict_ink = rename_suspicion_diags(
            &previous_manifests,
            Dialect::StrictInk,
            file_id,
            &new_manifest,
            &idx,
        );
        assert!(
            strict_ink.is_empty(),
            "no suspicion under Dialect::StrictInk — #@was is brink-only, so suggesting it \
             would point the author at a directive that itself produces E051: {strict_ink:?}"
        );

        let brink = rename_suspicion_diags(
            &previous_manifests,
            Dialect::Brink,
            file_id,
            &new_manifest,
            &idx,
        );
        assert_eq!(
            brink.len(),
            1,
            "the same diff must surface the hint under Dialect::Brink: {brink:?}"
        );
        assert_eq!(brink[0].code, Some(lsp_code("rename-suspicion")));
    }

    /// Build the `NumberOrString::String` code value `diagnostic_to_lsp`/
    /// `rename_suspicion_to_lsp` publish, for asserting against a returned
    /// [`Diagnostic::code`].
    fn lsp_code(code: &str) -> tower_lsp::lsp_types::NumberOrString {
        tower_lsp::lsp_types::NumberOrString::String(code.to_owned())
    }
}
