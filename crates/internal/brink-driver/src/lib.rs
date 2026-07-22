//! Pipeline orchestration for the brink ink compiler.
//!
//! `Driver` wraps a `ProjectDb` and provides higher-level operations:
//! file discovery, analysis orchestration, diagnostic collection, and
//! LIR input preparation. Both the compiler (one-shot) and LSP (long-lived)
//! use `Driver` as their entry point.

mod diagnostics;
mod discover;
mod native_discover;

use std::collections::HashMap;
use std::io;

pub use brink_analyzer::{AnalysisOptions, AnalysisResult, Dialect, TypePolicy};
pub use brink_db::{CompileProduct, LirProduct, ProjectDb};
pub use brink_ir::FileId;
pub use diagnostics::DiagnosticReport;
pub use discover::DiscoverError;
pub use native_discover::discover_native;

/// Whether an entry path feeds the native `.brink` frontend (B0.10b) — the
/// same extension test the db's `lowered_query` dispatches on, lifted here so
/// [`Driver::discover`] can branch its whole discovery *model* (a native
/// filesystem walk vs the ink `INCLUDE` BFS) on it.
fn entry_is_native(entry: &str) -> bool {
    std::path::Path::new(entry)
        .extension()
        .is_some_and(|ext| ext == "brink")
}

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

    /// Set the analysis options (e.g. a registered host manifest + external
    /// check severity) used by [`analyze`](Self::analyze). An input write —
    /// dependent queries recompute on next read.
    pub fn set_analysis_options(&mut self, options: AnalysisOptions) {
        self.db.set_analysis_options(options);
    }

    /// Borrow the underlying database.
    pub fn db(&self) -> &ProjectDb {
        &self.db
    }

    /// Mutably borrow the underlying database.
    ///
    /// Salsa's dependency tracking invalidates derived queries on input
    /// writes, so no manual cache invalidation happens here.
    pub fn db_mut(&mut self) -> &mut ProjectDb {
        &mut self.db
    }

    /// Consume the driver and return the underlying database.
    pub fn into_db(self) -> ProjectDb {
        self.db
    }

    // ── Discovery ────────────────────────────────────────────────────

    /// Discover a project's files from an entry point.
    ///
    /// Branches on the entry's frontend (B0.10b): a native `.brink` entry runs
    /// the [filesystem walk](Self::discover_native) over the declared source
    /// root (the `read_file` closure is unused — native reads the disk
    /// directly and the tree, not an `INCLUDE` closure, is the compilation
    /// universe); an ink entry runs the `INCLUDE` BFS through `read_file`.
    pub fn discover<F>(&mut self, entry: &str, read_file: F) -> Result<(), DiscoverError>
    where
        F: FnMut(&str) -> Result<String, io::Error>,
    {
        if entry_is_native(entry) {
            self.discover_native(entry).map(|_| ())
        } else {
            discover::discover(&mut self.db, entry, &mut { read_file })
        }
    }

    /// Discover every `.brink` file in the native project rooted at `entry`'s
    /// declared source root, loading them into the db in deterministic
    /// sorted-by-relative-path order (B0.10b). Returns the loaded relative
    /// keys in `FileId` order. Reads the real filesystem — see
    /// [`native_discover::discover_native`].
    pub fn discover_native(&mut self, entry: &str) -> Result<Vec<String>, DiscoverError> {
        native_discover::discover_native(&mut self.db, entry)
    }

    // ── Analysis ─────────────────────────────────────────────────────

    /// Run cross-file analysis on all files (memoized by the db's `analysis`
    /// query — an unchanged project returns the cached result).
    pub fn analyze(&mut self) -> &AnalysisResult {
        self.db.analysis()
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
