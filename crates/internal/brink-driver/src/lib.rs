//! Pipeline orchestration for the brink ink compiler.
//!
//! `Driver` wraps a `ProjectDb` and provides higher-level operations:
//! file discovery, analysis orchestration, diagnostic collection, and
//! LIR input preparation. Both the compiler (one-shot) and LSP (long-lived)
//! use `Driver` as their entry point.

mod diagnostics;
mod discover;

use std::collections::HashMap;
use std::io;

pub use brink_analyzer::AnalysisResult;
pub use brink_db::ProjectDb;
pub use brink_ir::FileId;
pub use diagnostics::DiagnosticReport;
pub use discover::DiscoverError;

/// Pipeline orchestration wrapper around `ProjectDb`.
pub struct Driver {
    db: ProjectDb,
}

impl Driver {
    /// Create a new driver with an empty database.
    pub fn new() -> Self {
        Self {
            db: ProjectDb::new(),
        }
    }

    /// Create a driver from an existing database.
    pub fn from_db(db: ProjectDb) -> Self {
        Self { db }
    }

    /// Borrow the underlying database.
    pub fn db(&self) -> &ProjectDb {
        &self.db
    }

    /// Mutably borrow the underlying database.
    pub fn db_mut(&mut self) -> &mut ProjectDb {
        &mut self.db
    }

    /// Consume the driver and return the underlying database.
    pub fn into_db(self) -> ProjectDb {
        self.db
    }

    // ── Discovery ────────────────────────────────────────────────────

    /// Discover all files reachable via INCLUDEs from the entry point.
    pub fn discover<F>(&mut self, entry: &str, read_file: F) -> Result<(), DiscoverError>
    where
        F: FnMut(&str) -> Result<String, io::Error>,
    {
        discover::discover(&mut self.db, entry, &mut { read_file })
    }

    // ── Analysis ─────────────────────────────────────────────────────

    /// Run cross-file analysis on all files (or return cached result).
    pub fn analyze(&mut self) -> &AnalysisResult {
        self.db.analyze()
    }

    /// Run analysis on a specific subset of files (one project). Not cached.
    pub fn analyze_project(&self, file_ids: &[FileId]) -> AnalysisResult {
        let inputs = self.db.analysis_inputs_for(file_ids);
        let file_refs: Vec<_> = inputs
            .iter()
            .map(|(id, hir, manifest)| (*id, hir, manifest))
            .collect();
        brink_analyzer::analyze(&file_refs)
    }

    /// Snapshot analysis inputs for a subset of files.
    pub fn analysis_inputs_for(
        &self,
        file_ids: &[FileId],
    ) -> Vec<(FileId, brink_ir::HirFile, brink_ir::SymbolManifest)> {
        self.db.analysis_inputs_for(file_ids)
    }

    /// Snapshot all analysis inputs.
    pub fn analysis_inputs(&self) -> Vec<(FileId, brink_ir::HirFile, brink_ir::SymbolManifest)> {
        self.db.analysis_inputs()
    }

    // ── Project graph ────────────────────────────────────────────────

    /// Compute independent projects from include relationships.
    pub fn compute_projects(&self) -> Vec<(FileId, Vec<FileId>)> {
        self.db.compute_projects()
    }

    /// Return file IDs in topological include order.
    pub fn file_ids_topo(&self, entry: FileId) -> Vec<FileId> {
        self.db.file_ids_topo(entry)
    }

    /// Snapshot file metadata for diagnostic publishing.
    pub fn file_metadata(&self) -> Vec<(FileId, String, String)> {
        self.db.file_metadata()
    }

    // ── Diagnostics ──────────────────────────────────────────────────

    /// Collect all diagnostics (lowering + analysis), apply suppressions, partition.
    pub fn collect_diagnostics(
        &self,
        analysis: &AnalysisResult,
        entry: Option<FileId>,
    ) -> DiagnosticReport {
        diagnostics::collect_diagnostics(&self.db, analysis, entry)
    }

    // ── LIR preparation ─────────────────────────────────────────────

    /// Prepare inputs for LIR lowering.
    ///
    /// Returns HIR files in topological order and a path map for diagnostics.
    pub fn lir_inputs(
        &self,
        entry: FileId,
    ) -> (Vec<(FileId, &brink_ir::HirFile)>, HashMap<FileId, String>) {
        let ids = self.file_ids_topo(entry);
        let files: Vec<_> = ids
            .into_iter()
            .filter_map(|id| self.db.hir(id).map(|hir| (id, hir)))
            .collect();
        let paths: HashMap<_, _> = files
            .iter()
            .filter_map(|(id, _)| self.db.file_path(*id).map(|p| (*id, p.to_string())))
            .collect();
        (files, paths)
    }
}

impl Default for Driver {
    fn default() -> Self {
        Self::new()
    }
}
