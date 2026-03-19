//! Stateful IDE session wrapping `ProjectDb` + cached analysis.
//!
//! `IdeSession` is the single entry point for IDE queries in the wasm
//! bridge. It owns the project database and caches analysis results,
//! avoiding redundant reparsing on every query call.

use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_ir::{FileId, HirFile, SymbolManifest};

/// A snapshot of analysis inputs, cloned out of the db for background analysis.
pub struct IdeSnapshot {
    inputs: Vec<(FileId, HirFile, SymbolManifest)>,
}

impl IdeSnapshot {
    /// Run cross-file analysis on the snapshot.
    pub fn analyze(&self) -> AnalysisResult {
        let refs: Vec<(FileId, &HirFile, &SymbolManifest)> = self
            .inputs
            .iter()
            .map(|(id, hir, manifest)| (*id, hir, manifest))
            .collect();
        brink_analyzer::analyze(&refs)
    }
}

/// Stateful IDE session — owns `ProjectDb` + cached `AnalysisResult`.
pub struct IdeSession {
    db: ProjectDb,
    analysis: Option<AnalysisResult>,
}

impl IdeSession {
    /// Create an empty session.
    pub fn new() -> Self {
        Self {
            db: ProjectDb::new(),
            analysis: None,
        }
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
