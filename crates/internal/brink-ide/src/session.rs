//! Stateful IDE session wrapping `ProjectDb` + cached analysis.
//!
//! `IdeSession` is the single entry point for IDE queries in the wasm
//! bridge. It owns the project database and caches analysis results,
//! avoiding redundant reparsing on every query call.

use brink_analyzer::{AnalysisOptions, AnalysisResult, ExternalCheckSeverity};
use brink_db::ProjectDb;
use brink_ir::{FileId, HirFile, HostManifest, SymbolManifest};

/// A snapshot of analysis inputs, cloned out of the db for background analysis.
pub struct IdeSnapshot {
    inputs: Vec<(FileId, HirFile, SymbolManifest)>,
    host_manifest: Option<HostManifest>,
    external_check: ExternalCheckSeverity,
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
    /// Severity policy for manifest-driven external checks.
    external_check: ExternalCheckSeverity,
}

impl IdeSession {
    /// Create an empty session.
    pub fn new() -> Self {
        Self {
            db: ProjectDb::new(),
            analysis: None,
            host_manifest: None,
            external_check: ExternalCheckSeverity::default(),
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

    /// Set the severity policy for manifest-driven external checks, then
    /// re-analyze.
    pub fn set_external_check(&mut self, severity: ExternalCheckSeverity) {
        self.external_check = severity;
        self.reanalyze();
    }

    /// Re-run analysis on the current inputs (e.g. after a manifest change).
    fn reanalyze(&mut self) {
        let result = self.snapshot().analyze();
        self.apply_analysis(result);
    }

    /// Add or update a source file in the database.
    pub fn update_source(&mut self, path: &str, source: String) -> FileId {
        self.db.update_file(path, source)
    }

    /// Remove a file from the project. Clears cached analysis.
    pub fn remove_file(&mut self, path: &str) {
        self.db.remove_file(path);
        self.analysis = None;
    }

    /// Create a snapshot of current analysis inputs.
    pub fn snapshot(&self) -> IdeSnapshot {
        IdeSnapshot {
            inputs: self.db.analysis_inputs(),
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
        }
    }

    /// Store a computed analysis result.
    pub fn apply_analysis(&mut self, result: AnalysisResult) {
        self.analysis = Some(result);
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

    /// The current analysis options (registered host manifest + external-check
    /// severity), for callers that run their own analysis/compile pass.
    pub fn analysis_options(&self) -> AnalysisOptions {
        AnalysisOptions {
            host_manifest: self.host_manifest.clone(),
            external_check: self.external_check,
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
    use brink_ir::{
        BaseType, Constraint, DiagnosticCode, ExternalKind, HostManifest, ManifestExternal,
        ManifestParam, SemanticTypeDef, TypeRef,
    };

    use super::{ExternalCheckSeverity, IdeSession};

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
            }],
            types: vec![SemanticTypeDef {
                name: "color".into(),
                base: BaseType::String,
                constraint: Some(Constraint::Enum {
                    values: vec!["#FF0000".into()],
                }),
                values: None,
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
}
