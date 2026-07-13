//! Stateful IDE session wrapping `ProjectDb` + cached analysis.
//!
//! `IdeSession` is the single entry point for IDE queries in the wasm
//! bridge. It owns the project database and caches analysis results,
//! avoiding redundant reparsing on every query call.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use brink_analyzer::{
    AnalysisOptions, AnalysisResult, Dialect, ExternalCheckSeverity,
    SemanticTypeDiagnosticSeverity, TypePolicy,
};
use brink_db::ProjectDb;
use brink_ir::{FileId, HirFile, HostManifest, ResolvedDialect, SymbolManifest};

use crate::hir_projection::{Projection, project_hir, project_hir_structural};

/// A snapshot of analysis inputs, cloned out of the db for background analysis.
pub struct IdeSnapshot {
    inputs: Vec<(FileId, HirFile, SymbolManifest)>,
    host_manifest: Option<HostManifest>,
    external_check: ExternalCheckSeverity,
    semantic_type_check: SemanticTypeDiagnosticSeverity,
    dialect: Dialect,
    types: TypePolicy,
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
        };
        brink_analyzer::analyze_with_options(&refs, &opts)
    }
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
    /// `set_type_policy`. Defaults to `TypePolicy::Gradual`, matching
    /// `AnalysisOptions::default()` — byte-identical to pre-#619/#660
    /// behavior until a caller opts in. Mirrors `language_dialect` exactly
    /// (#660: PR #656 left this hardcoded to `Gradual` in both `snapshot`
    /// and `analysis_options`, so the IDE/LSP/web surface could not reach
    /// `types = strict` at all — the compiler CLI's `--types strict` was the
    /// only path). Authoring-time/tooling input only, feeding
    /// `analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection`, same as
    /// `language_dialect`.
    type_policy: TypePolicy,
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
            type_policy: TypePolicy::default(),
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
        self.type_policy = types;
        self.reanalyze();
    }

    /// The registered TM-3 typed-mode policy (defaults to
    /// `TypePolicy::Gradual`).
    #[must_use]
    pub fn type_policy(&self) -> TypePolicy {
        self.type_policy
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
    fn reanalyze(&mut self) {
        let result = self.snapshot().analyze();
        self.apply_analysis(result);
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
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
            semantic_type_check: self.semantic_type_check,
            dialect: self.language_dialect,
            types: self.type_policy,
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
        let result = {
            let inputs = db.analysis_inputs();
            let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
                .iter()
                .map(|(id, hir, manifest)| (*id, hir, manifest))
                .collect();
            brink_analyzer::analyze_with_options(&refs, &self.analysis_options())
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
        let result = {
            let inputs = db.analysis_inputs();
            let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = inputs
                .iter()
                .map(|(id, hir, manifest)| (*id, hir, manifest))
                .collect();
            brink_analyzer::analyze_with_options(&refs, &self.analysis_options())
        };
        (result, db)
    }

    /// The current analysis options (registered host manifest +
    /// external-check / semantic-type-check severities + T1b compiler
    /// dialect + TM-3 typed-mode policy), for callers that run their own
    /// analysis/compile pass. `analyze_overlay`/`analyze_projection` use
    /// this, so the declared dialect and types policy carry through to their
    /// gate-check passes too (#611, #660).
    pub fn analysis_options(&self) -> AnalysisOptions {
        AnalysisOptions {
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
            semantic_type_check: self.semantic_type_check,
            dialect: self.language_dialect,
            types: self.type_policy,
        }
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
        Dialect, ExternalCheckSeverity, IdeSession, SemanticTypeDiagnosticSeverity, TypePolicy,
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

    /// #660: `IdeSession` defaults to `TypePolicy::Gradual` (byte-identical
    /// to pre-#619 behavior) until a caller opts in via `set_type_policy`.
    #[test]
    fn type_policy_defaults_to_gradual() {
        let session = IdeSession::new();
        assert_eq!(session.type_policy(), TypePolicy::Gradual);
        assert_eq!(
            session.analysis_options().types,
            TypePolicy::Gradual,
            "analysis_options must mirror the default, not a hardcoded literal"
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

    /// #660: the default `TypePolicy::Gradual` must NOT flag the same
    /// unused-param construct as `E065` — the setter must not blanket-enable
    /// strict checks, only thread through the caller's actual choice.
    #[test]
    fn gradual_default_does_not_flag_unknown_escape() {
        let mut session = IdeSession::new();
        session.set_language_dialect(Dialect::Brink);
        session.update_and_analyze("t.ink", "=== noop(x) ===\nHello.\n-> DONE\n".to_string());
        let analysis = session.analysis().expect("analysis");

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "gradual (default) must not flag E065: {:?}",
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
