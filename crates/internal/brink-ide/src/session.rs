//! Stateful IDE session wrapping `ProjectDb` + cached analysis.
//!
//! `IdeSession` is the single entry point for IDE queries in the wasm
//! bridge. It owns the project database and caches analysis results,
//! avoiding redundant reparsing on every query call.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use brink_analyzer::{
    AnalysisOptions, AnalysisResult, Dialect, ExternalCheckSeverity, LintPolicy, ModuleMap,
    SemanticTypeDiagnosticSeverity, TypePolicy,
};
use brink_db::{CompileProduct, ProjectDb};
use brink_ir::{FileId, HirFile, HostManifest, ResolvedDialect, SymbolManifest};

use crate::hir_projection::{Projection, project_hir, project_hir_structural};

/// A snapshot of analysis inputs, cloned out of the db for background analysis.
pub struct IdeSnapshot {
    inputs: Vec<(FileId, HirFile, SymbolManifest)>,
    /// The project's resolved modules, cloned out of the db alongside the
    /// inputs (issue #1526). Module identity is a db-layer fact (it needs
    /// file *paths*, which analysis inputs don't carry), and it qualifies
    /// `DefinitionId`s — so without it this snapshot's ids would not match
    /// the ones the db's per-def queries are keyed by, and every native
    /// `.brink` symbol would miss.
    modules: ModuleMap,
    /// The stem-collision diagnostics (`E085`) the db computed alongside
    /// `modules` (issue #1553). `analyze_with_modules` is handed the
    /// finished map, so it cannot re-derive them; without folding them back
    /// in here a collision a db-driven compile catches never reaches the
    /// editor.
    module_diagnostics: Vec<brink_ir::Diagnostic>,
    /// Whether every file in this snapshot is native (`.brink`) source
    /// (issue #1358), read off the db in [`IdeSession::snapshot`]. The
    /// analyzer has no file paths, so this classification has to travel with
    /// the inputs — without it the editor's off-db analysis runs the *ink*
    /// arm over native source: the ink-only T1b dialect gate (`E051`) and
    /// `types = strict` config error (`E064`) fire spuriously, and the B0.9
    /// strict-only gate (`E137`) never fires at all.
    is_native: bool,
    host_manifest: Option<HostManifest>,
    external_check: ExternalCheckSeverity,
    semantic_type_check: SemanticTypeDiagnosticSeverity,
    dialect: Dialect,
    types: Option<TypePolicy>,
    lints: LintPolicy,
}

impl IdeSnapshot {
    /// Run cross-file analysis on the snapshot, including any registered
    /// host-capability manifest.
    pub fn analyze(&self) -> AnalysisResult {
        let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = self
            .inputs
            .iter()
            .map(|(id, hir, manifest)| (*id, hir, manifest))
            .collect();
        let opts = AnalysisOptions {
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
            semantic_type_check: self.semantic_type_check,
            dialect: self.dialect,
            types: self.types,
            // `[lints]`-resolution input (issue #1160/#1366): the policy
            // `IdeSession::set_lint_policy` resolved (via
            // `AnalysisOptions::apply_project_config`, from the served
            // `brink.toml`) and carried into this snapshot by
            // `IdeSession::snapshot`. Spelled out explicitly (not
            // `..Default::default()`) so the next `AnalysisOptions` field
            // added has to be considered here rather than silently
            // defaulting — exactly the "a mount silently doesn't resolve
            // this policy" failure mode #1160's scope note flagged.
            lints: self.lints.clone(),
        };
        // The snapshot's own native classification (issue #1358) — see
        // `is_native`'s field doc. `brink-lsp`'s `analysis_loop` passes the
        // same thing per project root; this is the editor's equivalent.
        let mut result =
            brink_analyzer::analyze_with_modules(&refs, &self.modules, &opts, self.is_native);
        // The db-only half of the module map (issue #1553) — see
        // `module_diagnostics`. Scoped to this snapshot's own files so a
        // partial snapshot never reports a collision it doesn't contain.
        result.diagnostics.extend(
            self.module_diagnostics
                .iter()
                .filter(|d| self.inputs.iter().any(|(id, _, _)| *id == d.file))
                .cloned(),
        );
        result
    }
}

/// Why [`IdeSession::compile`] could not produce an artifact for the requested
/// entry point. Diagnostics that merely *prevent* a successful compile (parse,
/// lowering, analysis errors) are carried in [`CompileProduct::errors`], not
/// here — this is only the "there is nothing to compile" precondition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileEntryError {
    /// The requested entry path is not a file loaded in this session.
    #[error("entry file not found in session: {0}")]
    EntryNotFound(String),
}

/// Stateful IDE session — owns `ProjectDb` + cached `AnalysisResult`.
pub struct IdeSession {
    db: ProjectDb,
    analysis: Option<AnalysisResult>,
    /// The registered host-capability manifest (tooling/author-time), if any.
    host_manifest: Option<HostManifest>,
    /// Host-pushed values for `host`-source semantic types (Tier 3, #174).
    /// Query-time only — not part of analysis, so a push needs no re-analyze.
    host_values: crate::HostValues,
    /// Severity policy for manifest-driven external checks.
    external_check: ExternalCheckSeverity,
    /// Severity policy for unknown-semantic-type diagnostics (`E040`),
    /// parallel to `external_check` (#532).
    semantic_type_check: SemanticTypeDiagnosticSeverity,
    /// The registered dialogue dialect (#368), pre-compiled for
    /// classification. Tooling-only, query-time state consumed by
    /// `line_contexts` — never part of analysis, so registering one needs no
    /// re-analyze. `None` means no dialect is mounted (plain structural
    /// classification only).
    dialect: Option<ResolvedDialect>,
    /// The T1b compiler dialect (docs/t1b-surface-spec.md §1, #589/#600),
    /// set via `set_language_dialect`. Defaults to `Dialect::StrictInk`,
    /// matching `AnalysisOptions::default()`. Unlike `dialect` above (the
    /// #368 dialogue-dialect, query-time-only tooling state), this one feeds
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection` — it
    /// gates the `E051` "brink extension" diagnostic (#611: previously only
    /// `EditorSession`'s local copy gated completions/signature-help, so a
    /// `brink`-dialect project got permanent spurious `E051` from the
    /// background analysis pass regardless of this setting).
    language_dialect: Dialect,
    /// TM-3 typed-mode policy (docs/typed-mode-spec.md §1), set via
    /// `set_type_policy`. `None` until a caller opts in — the effective
    /// policy is then the dialect-keyed default (issue #1127, ruled
    /// 2026-07-19: `brink` → strict, `strict-ink` → gradual), resolved by
    /// `brink_analyzer::resolve_type_policy` via `AnalysisOptions::
    /// type_policy()`. Mirrors `language_dialect` exactly (#660: PR #656
    /// left this hardcoded to `Gradual` in both `snapshot` and
    /// `analysis_options`, so the IDE/LSP/web surface could not reach
    /// `types = strict` at all — the compiler CLI's `--types strict` was the
    /// only path). Authoring-time/tooling input only, feeding
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection`, same as
    /// `language_dialect`.
    type_policy: Option<TypePolicy>,
    /// Resolved `[lints]` policy (issue #1160), set via `set_lint_policy`.
    /// Defaults to `LintPolicy::default()` (a no-op: every diagnostic keeps
    /// its `DiagnosticCode::severity()` default), matching
    /// `AnalysisOptions::default()`. Unlike `language_dialect`/`type_policy`
    /// there is no explicit-vs-file precedence to track here — `[lints]` has
    /// no CLI-flag/editor-API override source of its own yet (see
    /// `AnalysisOptions::apply_project_config`'s doc comment), so the file is
    /// always the source of truth for the *whole table* (issue #1397: a
    /// fresh `apply_project_config` call replaces the resolved policy
    /// wholesale, not just the codes it mentions). Feeds
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection` exactly
    /// like `language_dialect`/`type_policy` (#1366: previously hardcoded to
    /// `LintPolicy::default()` in both `snapshot` and `analysis_options`, so
    /// a project's `[lints]` never reached the IDE/LSP/web surface).
    lints: LintPolicy,
    /// Per-file HIR projection cache (#480): the canonical structural model
    /// is computed once per edit and shared by every per-line/per-span view
    /// (`line_contexts`, folding, `hir_spans`). The flag records whether the
    /// entry carries the analyzer identity join — a structural-only entry is
    /// upgraded on first identity-needing access once analysis exists.
    /// Invalidated on source updates and on every `apply_analysis` (the
    /// identity join depends on it); the dialect never enters the
    /// projection, so registering one keeps the cache.
    projection_cache: RefCell<HashMap<FileId, (bool, Arc<Projection>)>>,
}

impl IdeSession {
    /// Create an empty session.
    pub fn new() -> Self {
        Self {
            db: ProjectDb::new(),
            analysis: None,
            host_manifest: None,
            host_values: crate::HostValues::new(),
            external_check: ExternalCheckSeverity::default(),
            semantic_type_check: SemanticTypeDiagnosticSeverity::default(),
            dialect: None,
            language_dialect: Dialect::default(),
            type_policy: None,
            lints: LintPolicy::default(),
            projection_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Register (or replace) the host-capability manifest, then re-analyze.
    pub fn set_host_manifest(&mut self, manifest: HostManifest) {
        self.host_manifest = Some(manifest);
        self.reanalyze();
    }

    /// Clear the registered host manifest, then re-analyze.
    pub fn clear_host_manifest(&mut self) {
        self.host_manifest = None;
        self.reanalyze();
    }

    /// Replace the host-pushed value cache (Tier 3, #174). No re-analyze — these
    /// values are consumed only by the argument picker + value-label inlay
    /// hints at query time, not by analysis.
    pub fn set_host_values(&mut self, values: crate::HostValues) {
        self.host_values = values;
    }

    /// Clear the host-pushed value cache.
    pub fn clear_host_values(&mut self) {
        self.host_values.clear();
    }

    /// The host-pushed value cache (empty when no host is attached).
    #[must_use]
    pub fn host_values(&self) -> &crate::HostValues {
        &self.host_values
    }

    /// Register (or replace) the dialogue dialect (#368). Compiles the
    /// dialect's patterns once, up front, so `line_contexts` never
    /// re-compiles on a hot path. Tooling-only — consumed at query time by
    /// `line_contexts`, never by analysis, so no re-analyze.
    pub fn set_dialect(&mut self, dialect: ResolvedDialect) {
        self.dialect = Some(dialect);
    }

    /// Clear the registered dialect. `line_contexts` reverts to plain
    /// structural classification (no `dialect` facet on any line).
    pub fn clear_dialect(&mut self) {
        self.dialect = None;
    }

    /// The registered dialect, if any.
    #[must_use]
    pub fn dialect(&self) -> Option<&ResolvedDialect> {
        self.dialect.as_ref()
    }

    /// Set the T1b compiler dialect (docs/t1b-surface-spec.md §1, #589/#600),
    /// then re-analyze — this is the diagnostics-facing counterpart of
    /// completions/signature-help gating (#611). `Dialect::Brink` drops the
    /// `E051` "brink extension" diagnostic for extension syntax that already
    /// lowers and runs under either dialect; `Dialect::StrictInk` (the
    /// default) restores it.
    pub fn set_language_dialect(&mut self, dialect: Dialect) {
        self.language_dialect = dialect;
        self.reanalyze();
    }

    /// The registered T1b compiler dialect (defaults to `Dialect::StrictInk`).
    #[must_use]
    pub fn language_dialect(&self) -> Dialect {
        self.language_dialect
    }

    /// Set the TM-3 typed-mode policy (docs/typed-mode-spec.md §1, #660),
    /// then re-analyze — the diagnostics-facing counterpart of the compiler
    /// CLI's `--types strict`. `TypePolicy::Strict` requires
    /// `language_dialect() == Dialect::Brink`, or `analyze` reports a single
    /// project-level `E064` config-error diagnostic instead of running the
    /// normal passes (see `brink_analyzer::strict::config_error`) — same
    /// caller responsibility as `set_language_dialect`.
    pub fn set_type_policy(&mut self, types: TypePolicy) {
        self.type_policy = Some(types);
        self.reanalyze();
    }

    /// The *effective* TM-3 typed-mode policy: an explicit
    /// `set_type_policy` value if one was ever registered, else the
    /// dialect-keyed default (issue #1127 — `brink` → `Strict`,
    /// `strict-ink` → `Gradual`), resolved through the one
    /// `brink_analyzer::resolve_type_policy` seam.
    #[must_use]
    pub fn type_policy(&self) -> TypePolicy {
        brink_analyzer::resolve_type_policy(self.language_dialect, self.type_policy)
    }

    /// Set the resolved `[lints]` policy (issue #1160/#1366), then
    /// re-analyze. Callers resolve the policy themselves — typically by
    /// running a parsed `brink.toml`'s `[lints]` table through
    /// `AnalysisOptions::apply_project_config`, which **replaces** the
    /// policy wholesale from the file rather than merging onto whatever was
    /// resolved before (issue #1397), and passing the result's `.lints`
    /// back here. `brink-web`'s
    /// `EditorSession::apply_project_config` and the CLI's
    /// `Project::ide_session()` (`brink-cli/src/ide/project.rs`, issue
    /// #1393) both call this — the CLI resolves `[lints]` once, via
    /// `resolve_analysis_options`, into the `Driver`'s `AnalysisOptions` for
    /// compile/analysis, then `Project::ide_session()` forwards that same
    /// resolved policy here so a `brink ide` structural-op safety gate
    /// (`structural_result::gate`) sees it too instead of
    /// `LintPolicy::default()`.
    pub fn set_lint_policy(&mut self, lints: LintPolicy) {
        self.lints = lints;
        self.reanalyze();
    }

    /// The current resolved `[lints]` policy (defaults to
    /// `LintPolicy::default()`, a no-op).
    #[must_use]
    pub fn lint_policy(&self) -> &LintPolicy {
        &self.lints
    }

    /// Set the severity policy for manifest-driven external checks, then
    /// re-analyze.
    pub fn set_external_check(&mut self, severity: ExternalCheckSeverity) {
        self.external_check = severity;
        self.reanalyze();
    }

    /// Set the severity policy for unknown-semantic-type diagnostics
    /// (`E040`), then re-analyze. Parallel to [`Self::set_external_check`]
    /// (#532) — raise to `Error` to re-enable strict checking (catching
    /// typo'd host semantic-type tags) even with no manifest registered.
    pub fn set_semantic_type_check(&mut self, severity: SemanticTypeDiagnosticSeverity) {
        self.semantic_type_check = severity;
        self.reanalyze();
    }

    /// Re-run analysis on the current inputs (e.g. after a manifest change).
    ///
    /// Pushes the session's options into the db first (see
    /// [`sync_db_options`](Self::sync_db_options)) — every option setter
    /// funnels through here, so that one call keeps the db's
    /// `AnalysisOptions` input in step with the session's own state.
    fn reanalyze(&mut self) {
        self.sync_db_options();
        let result = self.snapshot().analyze();
        self.apply_analysis(result);
    }

    /// Write the session's current [`analysis_options`](Self::analysis_options)
    /// into its own [`ProjectDb`] as a salsa input (issue #1553).
    ///
    /// The editor-facing analysis runs *off* the db
    /// ([`snapshot`](Self::snapshot) → [`IdeSnapshot::analyze`]), but many IDE
    /// features read db queries directly — `per_file_diagnostics`,
    /// `symbol_index`, `diagnostics`, `effects`, `infer_body` — and those are
    /// gated on this input. Before #1553 only [`compile`](Self::compile) ever
    /// wrote it, so a session that never compiled read every one of those
    /// queries under `AnalysisOptions::default()`: M-2d cross-module
    /// duplicate coexistence (`brink`-only in `symbol_index_query`) and the
    /// B0.9 native strict-only check (`E137`, which needs an explicit
    /// `types = gradual`) were silently gated off, among others.
    ///
    /// Guarded against unchanged values: salsa's `set_analysis_options`
    /// stamps the current revision unconditionally on every write, so an
    /// unguarded call would invalidate every direct reader
    /// (`per_file_diagnostics_query`, `symbol_index_query`, `resolve_query`,
    /// `lir_query`/`story_data`) even when the value didn't actually change.
    /// [`compile`](Self::compile) still writes its own (possibly overriding)
    /// options unconditionally — the next option change re-establishes the
    /// session's.
    fn sync_db_options(&mut self) {
        let options = self.analysis_options();
        if self.db.analysis_options() != &options {
            self.db.set_analysis_options(options);
        }
    }

    /// Add or update a source file in the database.
    pub fn update_source(&mut self, path: &str, source: String) -> FileId {
        let file_id = self.db.update_file(path, source);
        self.projection_cache.borrow_mut().remove(&file_id);
        file_id
    }

    /// Remove a file from the project. Clears cached analysis.
    pub fn remove_file(&mut self, path: &str) {
        self.db.remove_file(path);
        self.analysis = None;
        self.projection_cache.borrow_mut().clear();
    }

    /// Create a snapshot of current analysis inputs.
    pub fn snapshot(&self) -> IdeSnapshot {
        IdeSnapshot {
            inputs: self.db.analysis_inputs(),
            modules: self.db.module_map().clone(),
            module_diagnostics: self.db.module_map_diagnostics().to_vec(),
            is_native: self.db.is_all_native(),
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
            semantic_type_check: self.semantic_type_check,
            dialect: self.language_dialect,
            types: self.type_policy,
            lints: self.lints.clone(),
        }
    }

    /// Store a computed analysis result. Clears the projection cache: the
    /// range-keyed identity join is derived from analysis.
    pub fn apply_analysis(&mut self, result: AnalysisResult) {
        self.analysis = Some(result);
        self.projection_cache.borrow_mut().clear();
    }

    /// The file's HIR projection — computed once per source/analysis
    /// generation and shared by every structural view (#480). Carries the
    /// analyzer identity join when analysis is available (a superset the
    /// structural views simply ignore); a structural-only cached entry is
    /// recomputed with identity the first time analysis-bearing access needs
    /// it.
    pub fn projection(&self, file: FileId) -> Option<Arc<Projection>> {
        let want_identity = self.analysis.is_some();
        if let Some((has_identity, p)) = self.projection_cache.borrow().get(&file)
            && (*has_identity || !want_identity)
        {
            return Some(Arc::clone(p));
        }
        let hir = self.db.hir(file)?;
        let source = self.db.source(file)?;
        let projection = Arc::new(match self.analysis.as_ref() {
            Some(analysis) => project_hir(hir, source, analysis, file),
            None => project_hir_structural(hir, source),
        });
        self.projection_cache
            .borrow_mut()
            .insert(file, (want_identity, Arc::clone(&projection)));
        Some(projection)
    }

    /// Convenience: update source, snapshot, analyze, and store the result.
    pub fn update_and_analyze(&mut self, path: &str, source: String) -> FileId {
        let file_id = self.update_source(path, source);
        let snap = self.snapshot();
        let result = snap.analyze();
        self.apply_analysis(result);
        file_id
    }

    /// Get the underlying project database (for queries that need it).
    pub fn db(&self) -> &ProjectDb {
        &self.db
    }

    /// Get the cached analysis result.
    pub fn analysis(&self) -> Option<&AnalysisResult> {
        self.analysis.as_ref()
    }

    /// Re-analyze the project with `overlay` (project-relative path → source)
    /// replacing the on-disk content of matching files, **without mutating this
    /// session**. Files absent from the overlay keep their current source.
    ///
    /// Returns the fresh analysis paired with the throwaway `ProjectDb` it was
    /// run against — the db reassigns `FileId`s, so callers must resolve any
    /// `FileId` in the result (paths, sources) through the returned db, not
    /// through this session.
    ///
    /// Used by safe-rename (and other hypothetical refactors) to gate on the
    /// diagnostics an edit *would* introduce before applying it. This re-lowers
    /// every file, so it is for one-shot author actions — never a hot path.
    #[must_use]
    pub fn analyze_overlay(
        &self,
        overlay: &std::collections::BTreeMap<String, String>,
    ) -> (AnalysisResult, ProjectDb) {
        let mut db = ProjectDb::new();
        for id in self.db.file_ids() {
            let Some(path) = self.db.file_path(id) else {
                continue;
            };
            let source = overlay
                .get(path)
                .cloned()
                .or_else(|| self.db.source(id).map(str::to_owned))
                .unwrap_or_default();
            db.update_file(path, source);
        }
        // The gate db's own options input (#1553), so any query a caller
        // reads back off the returned db is judged under the same policy as
        // the session's — not `AnalysisOptions::default()`.
        db.set_analysis_options(self.analysis_options());
        let result = {
            let inputs = db.analysis_inputs();
            let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
                .iter()
                .map(|(id, hir, manifest)| (*id, hir, manifest))
                .collect();
            // The throwaway db's own module map (#1526) — the overlay keeps
            // every file at its current path, but a native file's identity
            // is path-derived, so analyzing module-blind here would make the
            // gate's ids disagree with the returned db's.
            let modules = db.module_map().clone();
            // The gate db's own native classification (issue #1358): the
            // overlay keeps every file at its current path, so this matches
            // the session — but reading it off the gate db keeps the flag
            // and the file set that it describes from ever disagreeing.
            let mut result = brink_analyzer::analyze_with_modules(
                &refs,
                &modules,
                &self.analysis_options(),
                db.is_all_native(),
            );
            // The map's db-only diagnostics half (#1553) — the whole point of
            // the gate is to report the diagnostics an edit *would* introduce,
            // and a stem collision is one of them.
            result
                .diagnostics
                .extend(db.module_map_diagnostics().iter().cloned());
            result
        };
        (result, db)
    }

    /// Analyze a *complete* project projection — an explicit `path → source`
    /// map that stands in for the entire file set, **without mutating this
    /// session**. Unlike [`analyze_overlay`](Self::analyze_overlay) (which keeps
    /// the current paths and only substitutes sources), this replaces the whole
    /// project, so files may appear at *different* paths than they do now. Used
    /// by directory rename/move (#314), where the gate must model files that
    /// have relocated to new keys.
    ///
    /// Returns the fresh analysis paired with the throwaway `ProjectDb` it ran
    /// against — the db owns the new `FileId`s, so callers must resolve any
    /// `FileId` in the result (paths, sources) through the returned db.
    #[must_use]
    pub fn analyze_projection(
        &self,
        projection: &std::collections::BTreeMap<String, String>,
    ) -> (AnalysisResult, ProjectDb) {
        let mut db = ProjectDb::new();
        for (path, source) in projection {
            db.update_file(path, source.clone());
        }
        // Same as `analyze_overlay` (#1553): the gate db is judged under the
        // session's options, not the defaults.
        db.set_analysis_options(self.analysis_options());
        let result = {
            let inputs = db.analysis_inputs();
            let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
                .iter()
                .map(|(id, hir, manifest)| (*id, hir, manifest))
                .collect();
            // The projected db's own module map (#1526). This path *moves*
            // files to new keys, and a native file's module is its path, so
            // the map has to come from the projected db — the whole point of
            // the gate is to model identity after the move.
            let modules = db.module_map().clone();
            // The projected db's own native classification (issue #1358).
            // This path *moves* files to new keys, and `Language` is
            // extension-derived, so — exactly as for the module map above —
            // the flag has to come from the projected db to model the file
            // set after the move.
            let mut result = brink_analyzer::analyze_with_modules(
                &refs,
                &modules,
                &self.analysis_options(),
                db.is_all_native(),
            );
            // A move is exactly the edit that can *introduce* a stem
            // collision, so the gate has to see the map's diagnostics half
            // (#1553).
            result
                .diagnostics
                .extend(db.module_map_diagnostics().iter().cloned());
            result
        };
        (result, db)
    }

    /// The current analysis options (registered host manifest +
    /// external-check / semantic-type-check severities + T1b compiler
    /// dialect + TM-3 typed-mode policy + resolved `[lints]` policy), for
    /// callers that run their own analysis/compile pass.
    /// `analyze_overlay`/`analyze_projection` use this, so the declared
    /// dialect and types policy carry through to their
    /// gate-check passes too (#611, #660).
    pub fn analysis_options(&self) -> AnalysisOptions {
        AnalysisOptions {
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
            semantic_type_check: self.semantic_type_check,
            dialect: self.language_dialect,
            types: self.type_policy,
            // See the matching note on `IdeSnapshot::analyze` (#1160/#1366):
            // the policy `set_lint_policy` resolved. Spelled out explicitly,
            // not `..Default::default()` — see that note for why.
            lints: self.lints.clone(),
        }
    }

    /// Compile the project rooted at `entry` on **this session's own
    /// `ProjectDb`** — the same db the analysis path reads (#1032). Sets the
    /// db's compile entry and analysis options as salsa inputs, then pulls the
    /// memoized `story_data` artifact query. Because compile and analysis now
    /// share one db (one file set, one lowering, one options input), a compile
    /// can never diverge from analysis on manifest/dialect/policy/file-state:
    /// the divergence that produced #1004 (manifest missing from compile) and
    /// its siblings is structurally unrepresentable rather than closed by
    /// wiring each input into a second, throwaway driver.
    ///
    /// `options` are the analysis options to compile under — pass
    /// [`analysis_options`](Self::analysis_options) for the session default, or
    /// an override for a per-call variation (e.g. compile-for-export). They are
    /// written to the db as an input; an unchanged value is a salsa no-op (no
    /// memo invalidation), so repeated compiles under the same options reuse the
    /// warm db's incremental results.
    ///
    /// **Does not perturb editor diagnostic state.** The editor's cached
    /// analysis (`self.analysis`) and projection cache are computed off-db (via
    /// [`snapshot`](Self::snapshot)/[`analyze`](IdeSnapshot::analyze)) and are
    /// left untouched; the db-level `analysis`/`lir`/`story_data` salsa queries
    /// this sets inputs for are read only here, on the compile path.
    ///
    /// The returned [`CompileProduct`]'s diagnostics are keyed by [`FileId`]
    /// into **this** db, so a caller resolves each to a path/source through the
    /// same session (`file_path`/`source`) — no throwaway-driver id remapping.
    ///
    /// ## Ruling: this stays on the imperative `set_file`/`set_entry` path (#1385)
    ///
    /// #1361 migrated `brink-web`'s one-shot `compile()`/`compile_fragment()`
    /// onto the #1306 producer (`brink_environment::Project::load` →
    /// `brink_environment::compile(&env)`), but deliberately scope-fenced
    /// `EditorSession::compile_project` — which delegates to this method —
    /// because it drives a different, incremental, live-editing db shared with
    /// `brink-lsp`. #1385 is the owner issue for resolving that fence, and the
    /// deliberate decision is: **do not migrate.** `IdeSession::compile` keeps
    /// pushing salsa inputs directly onto its own long-lived [`ProjectDb`].
    ///
    /// This ruling is scoped to **today's `compile(&Environment)` free
    /// function** (`crates/internal/brink-environment/src/lib.rs`), not to
    /// `Environment` as a value type in the abstract — see the last bullet
    /// below for the alternative that scoping deliberately excludes.
    /// Reasoning:
    ///
    /// - **`compile(&Environment)` is intentionally non-incremental.** Per its
    ///   own doc, it "seeds a **fresh** salsa `ProjectDb`" from a frozen,
    ///   point-in-time value on every call — "no ambient reads, no walk-up, no
    ///   I/O." That is exactly right for a one-shot mount handed a full
    ///   document snapshot per call (the CLI, `brink-web`'s stateless
    ///   `compile()`/`compile_fragment()`). It is exactly wrong for
    ///   `IdeSession`, whose entire reason to exist (see the module doc) is to
    ///   hold **one** persistent `ProjectDb` across many edits and queries so
    ///   an unrelated file's parse/HIR/analysis memos survive a single-file
    ///   keystroke edit. Routing every `compile_project` call through
    ///   *today's* `Project::load`/`compile(&env)` would re-walk the tree,
    ///   re-hash every source, and reseed a brand-new `Driver::new()` db on
    ///   each call (that function's own body does exactly that:
    ///   `set_analysis_options` + `set_file` per key + `set_entry` onto a
    ///   fresh driver) — discarding the incremental state this session exists
    ///   to keep warm, with no compensating benefit.
    /// - **As it stands, it would resurrect the #1004 divergence class.**
    ///   #1032 made compile and analysis share this one db specifically so
    ///   they can never diverge on manifest/dialect/policy/file-state (see
    ///   this method's opening paragraph). *Today's* `Project::load` +
    ///   `compile(&env)` builds an `Environment` from a fresh `SourceTree`
    ///   walk and compiles it on its own freshly-minted `ProjectDb` (via
    ///   `Driver::new()`). Wiring `compile_project` through that entry point
    ///   unmodified would put a *second* db back in the picture alongside
    ///   `IdeSession`'s own. This is a property of `compile`'s current body
    ///   (it always mints a throwaway driver), not an inherent property of
    ///   the `Environment` value type — see below.
    /// - **The live alternative, named and deferred, not dismissed.** A
    ///   `compile_into(&mut Driver, &Environment)` variant — same
    ///   `set_analysis_options`/`set_file`/`set_entry` push, applied to the
    ///   *session's own* `ProjectDb` instead of a fresh one — would preserve
    ///   incrementality outright (salsa's `set_file` is a no-op for unchanged
    ///   content, so unrelated files' memos survive) while keeping exactly
    ///   one db, sidestepping the #1004 divergence concern above by
    ///   construction. That is a real, live alternative, not a hypothetical
    ///   future-salsa escape hatch — it is deliberately **out of scope for
    ///   this ruling** because it requires designing and landing a new
    ///   `brink-environment` entry point, which #1385 did not ask this PR to
    ///   do. It is the natural next step if/when `IdeSession` is revisited to
    ///   compile against `Environment`-shaped input; bring it back as its own
    ///   proposal rather than treating this doc comment as having settled it.
    /// - **The adjacent design question is still genuinely open.** #1347
    ///   (`needs-design`, unresolved) asks whether `IdeSession`'s live-typing
    ///   diagnostics should route through `ProjectDb`'s own salsa-level
    ///   `analysis_query`/`per_file_diagnostics_query` surface at all. Forcing
    ///   `compile_project` onto a *different* producer now would prejudge that
    ///   still-open call rather than wait for it. Still unresolved, but now
    ///   measured: `docs/live-typing-diagnostics-divergence.md` inventories
    ///   what the two surfaces actually disagree on (native files only, in
    ///   both directions), pinned by `tests/live_typing_db_divergence.rs`.
    ///
    /// **Relationship to the #1306 decision-log entry**
    /// (`docs/decision-log.md`, "Compilation environment as a deterministic,
    /// serializable input" — "producer vs. pure input"): that entry names
    /// `set_file`/`set_entry`/`set_analysis_options` as exactly the imperative
    /// push `Environment`/`compile(&env)` exists to reify, and folds "the LSP
    /// mount (#1131)" into the producer's scope — i.e. it anticipated
    /// `IdeSession`-shaped mounts eventually feeding `Environment` too. This
    /// ruling is a **narrower, present-tense carve-out**: it does not
    /// contradict #1306's long-run direction, it says today's
    /// `compile(&Environment)` shape (one throwaway db per call) is not yet
    /// the right fit for `IdeSession`'s incremental db, pending the
    /// `compile_into`-style seam above.
    ///
    /// # Errors
    /// Returns [`CompileEntryError::EntryNotFound`] if `entry` is not a file
    /// loaded in this session.
    pub fn compile(
        &mut self,
        entry: &str,
        options: &AnalysisOptions,
    ) -> Result<CompileProduct, CompileEntryError> {
        if self.db.set_entry(entry).is_none() {
            return Err(CompileEntryError::EntryNotFound(entry.to_owned()));
        }
        self.db.set_analysis_options(options.clone());
        // `story_data()` is `Some` whenever an entry is set (just done above);
        // clone the memoized product out so the borrow on the db ends here.
        Ok(self.db.story_data().cloned().unwrap_or_default())
    }

    /// Look up a file's ID by path.
    pub fn file_id(&self, path: &str) -> Option<FileId> {
        self.db.file_id(path)
    }

    /// Get the HIR for a file.
    pub fn hir(&self, id: FileId) -> Option<&HirFile> {
        self.db.hir(id)
    }

    /// Get the symbol manifest for a file.
    pub fn manifest(&self, id: FileId) -> Option<&SymbolManifest> {
        self.db.manifest(id)
    }

    /// Get the source text for a file.
    pub fn source(&self, id: FileId) -> Option<&str> {
        self.db.source(id)
    }

    /// Get the path for a file.
    pub fn file_path(&self, id: FileId) -> Option<&str> {
        self.db.file_path(id)
    }

    /// Get the parse tree root for a file.
    pub fn syntax_root(&self, id: FileId) -> Option<brink_syntax::SyntaxNode> {
        self.db.parse(id).map(brink_syntax::Parse::syntax)
    }
}

impl Default for IdeSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use brink_ir::{
        BaseType, Constraint, DiagnosticCode, ExternalKind, HostManifest, ManifestExternal,
        ManifestParam, SemanticTypeDef, TypeRef,
    };

    use super::{
        AnalysisOptions, Dialect, ExternalCheckSeverity, IdeSession,
        SemanticTypeDiagnosticSeverity, TypePolicy,
    };

    fn color_manifest() -> HostManifest {
        HostManifest {
            externals: vec![ManifestExternal {
                name: "tint".into(),
                params: vec![ManifestParam {
                    name: "c".into(),
                    ty: TypeRef("color".into()),
                }],
                returns: TypeRef::default(),
                kind: ExternalKind::Presentation,
                doc: None,

                widgets: vec![],
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "color".into(),
                base: BaseType::String,
                constraint: Some(Constraint::Enum {
                    values: vec!["#FF0000".into()],
                }),
                values: None,
                widget: None,
            }],
        }
    }

    const SRC: &str = "EXTERNAL tint(c)\n~ tint(\"nope\")\n-> END\n";

    #[test]
    fn registered_manifest_drives_checks_and_enrichment() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", SRC.to_string());
        session.set_host_manifest(color_manifest());
        let analysis = session.analysis().expect("analysis");

        // Enrichment is surfaced.
        assert!(
            analysis
                .symbol_meta
                .values()
                .any(|m| m.kind == ExternalKind::Presentation),
            "symbol_meta should carry the registered kind"
        );
        // The closed-domain (enum) violation on the literal "nope" is flagged.
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E042),
            "expected E042, got {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn external_check_off_suppresses_diagnostics() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", SRC.to_string());
        session.set_external_check(ExternalCheckSeverity::Off);
        session.set_host_manifest(color_manifest());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E042),
            "Off should suppress manifest diagnostics"
        );
        // ...but enrichment is still built.
        assert!(!analysis.symbol_meta.is_empty(), "meta built even when Off");
    }

    /// #532: ink with a host semantic type param and no registered manifest.
    const HOST_TYPE_SRC: &str = "\
/// @param who {actor_id}
EXTERNAL add_state(who)
";

    #[test]
    fn semantic_type_check_defaults_to_tolerant_with_no_manifest() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", HOST_TYPE_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E040),
            "default (Tolerant) with no manifest: no E040: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn semantic_type_check_error_diagnoses_with_no_manifest() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", HOST_TYPE_SRC.to_string());
        session.set_semantic_type_check(SemanticTypeDiagnosticSeverity::Error);
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E040),
            "Error with no manifest: E040 fires: {:?}",
            analysis.diagnostics
        );
    }

    /// #611: a `~ { … }` multi-line logic block is brink-extension syntax
    /// (docs/t1b-surface-spec.md §1) — flagged `E051` under the default
    /// `StrictInk` dialect, silent under `Brink`.
    const BRINK_EXT_SRC: &str = "~ {\n    temp x = 0\n}\n-> END\n";

    #[test]
    fn strict_ink_default_flags_e051_on_extension_syntax() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", BRINK_EXT_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "StrictInk default: E051 stands: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_brink_suppresses_e051_on_analyze() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", BRINK_EXT_SRC.to_string());
        session.set_language_dialect(Dialect::Brink);
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "brink dialect: no E051 on valid extension syntax: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn set_language_dialect_reanalyzes_files_added_afterward() {
        // The dialect is set before the file is even loaded — `reanalyze`
        // must be picked up by the subsequent `update_and_analyze` (which
        // re-snapshots and re-reads `language_dialect`), not just by files
        // present at `set_language_dialect` time.
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.update_and_analyze("t.ink", BRINK_EXT_SRC.to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "brink dialect set before load: no E051: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn analyze_overlay_and_analyze_projection_respect_declared_dialect() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", "-> END\n".to_string());
        session.set_language_dialect(Dialect::Brink);

        let mut overlay = std::collections::BTreeMap::new();
        overlay.insert("t.ink".to_string(), BRINK_EXT_SRC.to_string());
        let (overlay_result, _db) = session.analyze_overlay(&overlay);
        assert!(
            !overlay_result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "analyze_overlay: brink dialect carries through: {:?}",
            overlay_result.diagnostics
        );

        let mut projection = std::collections::BTreeMap::new();
        projection.insert("t.ink".to_string(), BRINK_EXT_SRC.to_string());
        let (projection_result, _db) = session.analyze_projection(&projection);
        assert!(
            !projection_result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E051),
            "analyze_projection: brink dialect carries through: {:?}",
            projection_result.diagnostics
        );
    }

    /// #660/#1127: with no explicit `set_type_policy`, the effective policy
    /// is the dialect-keyed default (2026-07-19 ruling) — `Gradual` under
    /// the default `StrictInk` dialect (byte-identical to pre-#619
    /// behavior), `Strict` once the dialect is `Brink`.
    #[test]
    fn type_policy_default_is_dialect_keyed() {
        let mut session = IdeSession::new();
        assert_eq!(session.type_policy(), TypePolicy::Gradual);
        assert_eq!(
            session.analysis_options().type_policy(),
            TypePolicy::Gradual,
            "analysis_options must mirror the resolved default"
        );

        session.set_language_dialect(Dialect::Brink);
        assert_eq!(session.type_policy(), TypePolicy::Strict);
        assert_eq!(
            session.analysis_options().type_policy(),
            TypePolicy::Strict,
            "brink dialect with no explicit types defaults strict (#1127)"
        );
    }

    /// #660 (TM-3 basic reachability): `types = strict` under the default
    /// `StrictInk` dialect is a project-level config error (`E064`) — the
    /// IDE surface must reach this the same way the compiler CLI's
    /// `--types strict --dialect strict-ink` does.
    #[test]
    fn set_type_policy_strict_with_strict_ink_dialect_is_config_error() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", "-> END\n".to_string());
        session.set_type_policy(TypePolicy::Strict);
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E064),
            "types=strict + dialect=strict-ink (default): expected E064: {:?}",
            analysis.diagnostics
        );
    }

    /// #660: with `dialect = brink`, `types = strict` turns on the
    /// Unknown-escape check (`E065`) on `analyze` — proving the setter
    /// reaches the real strict-mode checks, not just the config-error path.
    #[test]
    fn set_type_policy_strict_with_brink_dialect_flags_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.set_type_policy(TypePolicy::Strict);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "types=strict + dialect=brink: expected E065 on unused param `x`: {:?}",
            analysis.diagnostics
        );
    }

    /// B0.9 native strict-only enforcement (issue #1342): the IDE/editor
    /// surface (`brink-web`'s `EditorSession::compile_project`, which calls
    /// `IdeSession::compile` exactly as here — see that method's doc)
    /// reaches `E137` the same way `brink_compiler::compile_path_with_options`
    /// does — proving the salsa `story_data()` compile path (not just the
    /// pure `analyze_with_options` diagnostics path `set_type_policy`'s
    /// sibling tests above exercise) also hits the gate.
    #[test]
    fn compile_native_file_with_explicit_gradual_types_reports_e137() {
        let mut session = IdeSession::new();
        session.update_and_analyze(
            "main.brink",
            "flow main() {\n  Hello. -> END\n}\n".to_string(),
        );
        let options = AnalysisOptions {
            types: Some(TypePolicy::Gradual),
            ..Default::default()
        };
        let product = session
            .compile("main.brink", &options)
            .expect("entry file is loaded");

        assert!(
            product
                .errors
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E137),
            "types=gradual native compile: expected E137: {:?}",
            product.errors
        );
        assert!(
            product.story.is_none(),
            "a hard error must not emit a story"
        );
    }

    /// The paired positive case: `types = strict` on the same native entry
    /// compiles cleanly (no `E137`).
    #[test]
    fn compile_native_file_with_explicit_strict_types_has_no_e137() {
        let mut session = IdeSession::new();
        session.update_and_analyze(
            "main.brink",
            "flow main() {\n  Hello. -> END\n}\n".to_string(),
        );
        let options = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..Default::default()
        };
        let product = session
            .compile("main.brink", &options)
            .expect("entry file is loaded");

        assert!(
            !product
                .errors
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E137),
            "types=strict native compile: expected no E137: {:?}",
            product.errors
        );
        assert!(product.story.is_some(), "expected a compiled story");
    }

    /// #1127 (default flip): under `dialect = brink` with NO explicit
    /// `set_type_policy`, the effective policy is `Strict`, so the
    /// Unknown-escape check fires — the strict default reaches the analysis
    /// path without any opt-in call.
    #[test]
    fn brink_dialect_default_flags_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink-dialect default is strict (#1127): expected E065: {:?}",
            analysis.diagnostics
        );
    }

    /// #1127: `set_type_policy(Gradual)` remains the explicit opt-out knob
    /// under `dialect = brink` — the same construct is silent again.
    #[test]
    fn explicit_gradual_opt_out_suppresses_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.set_type_policy(TypePolicy::Gradual);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "explicit gradual opt-out must not flag E065: {:?}",
            analysis.diagnostics
        );
    }

    /// #660: `analyze_overlay`/`analyze_projection` must carry the declared
    /// types policy through to their gate-check passes too, mirroring the
    /// existing dialect coverage.
    #[test]
    fn analyze_overlay_and_analyze_projection_respect_declared_type_policy() {
        let mut session = IdeSession::new();
        session.update_and_analyze("t.ink", "-> END\n".to_string());
        session.set_language_dialect(Dialect::Brink);
        session.set_type_policy(TypePolicy::Strict);

        let mut overlay = std::collections::BTreeMap::new();
        overlay.insert(
            "t.ink".to_string(),
            "=== noop(x) ===\nHello.\n-> DONE\n".to_string(),
        );
        let (overlay_result, _db) = session.analyze_overlay(&overlay);
        assert!(
            overlay_result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "analyze_overlay: strict types policy carries through: {:?}",
            overlay_result.diagnostics
        );

        let mut projection = std::collections::BTreeMap::new();
        projection.insert(
            "t.ink".to_string(),
            "=== noop(x) ===\nHello.\n-> DONE\n".to_string(),
        );
        let (projection_result, _db) = session.analyze_projection(&projection);
        assert!(
            projection_result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "analyze_projection: strict types policy carries through: {:?}",
            projection_result.diagnostics
        );
    }

    #[test]
    fn projection_cache_shares_and_invalidates() {
        let mut s = IdeSession::new();
        let file = s.update_and_analyze("main.ink", "=== a ===\n-> DONE\n".to_owned());

        let p1 = s.projection(file).expect("projection");
        let p2 = s.projection(file).expect("projection");
        assert!(Arc::ptr_eq(&p1, &p2), "same generation → shared Arc");

        // A source update invalidates: new Arc, new content.
        let file = s.update_and_analyze("main.ink", "=== a ===\n=== b ===\n-> DONE\n".to_owned());
        let p3 = s.projection(file).expect("projection");
        assert!(!Arc::ptr_eq(&p1, &p3), "update → fresh projection");

        // With analysis present the cached flavor carries identity.
        assert!(
            p3.spans.iter().any(|sp| sp.def_id.is_some()),
            "identity-joined flavor cached when analysis exists"
        );
    }
}
