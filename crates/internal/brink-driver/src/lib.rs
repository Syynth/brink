//! Pipeline orchestration for the brink ink compiler.
//!
//! `Driver` wraps a `ProjectDb` and provides higher-level operations:
//! file discovery, analysis orchestration, diagnostic collection, and
//! LIR input preparation. Both the compiler (one-shot) and LSP (long-lived)
//! use `Driver` as their entry point.

mod diagnostics;
mod discover;
mod discover_native;
mod source_tree;

use std::collections::HashMap;
use std::io;

pub use brink_analyzer::{
    AnalysisOptions, AnalysisResult, Dialect, LintLevel, LintPolicy, TypePolicy,
};
pub use brink_db::{CompileProduct, LirProduct, ProjectDb, SourceTree};
pub use brink_ir::FileId;
pub use diagnostics::DiagnosticReport;
pub use discover::DiscoverError;
pub use source_tree::{GitRev, RealFs, is_native, native_source_root, relative_key};

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

    /// Discover all files reachable via INCLUDEs from the entry point.
    pub fn discover<F>(&mut self, entry: &str, read_file: F) -> Result<(), DiscoverError>
    where
        F: FnMut(&str) -> Result<String, io::Error>,
    {
        discover::discover(&mut self.db, entry, &mut { read_file })
    }

    /// Discover a native `.brink` project: enumerate `tree` (sorted,
    /// root-relative keys, scoped to `tree`'s own constructor-held root —
    /// issue #1371) and load every file — no `INCLUDE` BFS, since native has
    /// no `INCLUDE`s. `tree` must be constructed with the project's source
    /// root (`RealFs::new`/`GitRev::new`) — see [`native_source_root`] to
    /// derive it from an entry path.
    pub fn discover_native(&mut self, tree: &dyn SourceTree) -> Result<(), DiscoverError> {
        discover_native::discover_native(&mut self.db, tree)
    }

    // ── Analysis ─────────────────────────────────────────────────────

    /// Run cross-file analysis on all files (memoized by the db's `analysis`
    /// query — an unchanged project returns the cached result).
    pub fn analyze(&mut self) -> &AnalysisResult {
        self.db.analysis()
    }

    /// Run analysis on a specific subset of files (one project). Not cached.
    ///
    /// Module-aware and options-honoring (issue #1553): the pass runs with
    /// the db's own [`ProjectDb::module_map`] and registered
    /// [`AnalysisOptions`], so the `DefinitionId`s it mints key this db's
    /// per-def queries and the declared dialect/types/lints reach it — the
    /// same contract [`analyze`](Self::analyze) has. A bare
    /// `brink_analyzer::analyze`/`analyze_with_options` here was
    /// module-*blind* and dropped the options entirely, which for a native
    /// `.brink` project (whose module is its path, always declared) mints a
    /// different identity space than the db's — see
    /// [`ProjectDb::module_map`]'s doc.
    ///
    /// Stem-collision diagnostics (`E085`) are folded in from
    /// [`ProjectDb::module_map_diagnostics`], scoped to `file_ids`, for the
    /// same reason: the analyzer is handed the finished map and cannot
    /// re-derive them.
    pub fn analyze_project(&self, file_ids: &[FileId]) -> AnalysisResult {
        let inputs = self.db.analysis_inputs_for(file_ids);
        let file_refs: Vec<_> = inputs
            .iter()
            .map(|(id, hir, manifest)| (*id, hir, manifest))
            .collect();
        let mut result = brink_analyzer::analyze_with_modules(
            &file_refs,
            self.db.module_map(),
            self.db.analysis_options(),
        );
        result.diagnostics.extend(
            self.db
                .module_map_diagnostics()
                .iter()
                .filter(|d| file_ids.contains(&d.file))
                .cloned(),
        );
        result
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

#[cfg(test)]
mod tests {
    use super::*;

    /// [`analyze_project`](Driver::analyze_project) must be module-aware: for
    /// a native `.brink` project the `DefinitionId`s it mints have to key the
    /// same db per-def queries ([`ProjectDb::effects`],
    /// [`ProjectDb::signature`]) the db itself is queried by elsewhere —
    /// otherwise every `analyze_project`-derived id misses on those queries
    /// (issue #1553). A bare `brink_analyzer::analyze`/`analyze_with_options`
    /// over the same inputs is module-*blind*: a native file's module is
    /// path-derived and always declared, so it mints a different identity
    /// space than the db's own module-aware queries, and this test fails
    /// against that old code path.
    #[test]
    fn analyze_project_ids_key_the_db_per_def_queries_for_native_files() {
        let mut driver = Driver::new();
        driver.db_mut().update_file(
            "market/barter.brink",
            "flow haggle() {\n  You haggle over the price.\n}\n".to_owned(),
        );
        let main = driver.db_mut().update_file(
            "main.brink",
            "use story::market::barter::haggle;\n\nflow start() {\n  The market is busy.\n  -> haggle\n}\n"
                .to_owned(),
        );

        let ids: Vec<FileId> = driver.db().file_ids().collect();
        let result = driver.analyze_project(&ids);

        let haggle_ids = result
            .index
            .by_name
            .get("haggle")
            .expect("`haggle` is declared");
        assert_eq!(haggle_ids.len(), 1, "exactly one `haggle`");
        let id = haggle_ids[0];

        assert!(
            driver.db().effects(id).is_some(),
            "`db.effects` missed for `haggle` ({id}) — analyze_project minted \
             an id the db's own queries don't recognize"
        );
        assert!(
            driver.db().signature(id).is_some(),
            "`db.signature` missed for `haggle` ({id}) — analyze_project minted \
             an id the db's own queries don't recognize"
        );

        // Sanity: the other file's flow is reachable too.
        assert!(result.index.by_name.contains_key("start"));
        let _ = main;
    }

    /// [`analyze_project`](Driver::analyze_project) folds in the module map's
    /// stem-collision diagnostics (`E085`), scoped to the requested
    /// `file_ids` — not the whole db. `head.ink` declares module `alpha`;
    /// the separate, undeclared `alpha.ink` has stem `alpha`, which is the
    /// forbidden footgun the diagnostic exists for. The diagnostic is
    /// attributed to the undeclared file (`alpha.ink`), so scoping
    /// `file_ids` to exclude it must also exclude the diagnostic.
    #[test]
    fn analyze_project_folds_e085_scoped_to_file_ids() {
        let mut driver = Driver::new();
        // `head.ink` declares module `alpha`; `alpha.ink` is a separate,
        // undeclared file whose *stem* is also `alpha` — the collision.
        let head = driver
            .db_mut()
            .update_file("head.ink", "#@module(alpha)\n== a_knot ==\nHi\n".to_owned());
        let collider = driver
            .db_mut()
            .update_file("alpha.ink", "== other ==\nHi\n".to_owned());

        // Scoped to both files: the collision is folded in.
        let result_scoped = driver.analyze_project(&[head, collider]);
        assert!(
            result_scoped
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E085),
            "expected E085 stem collision when both files are in scope, got {:?}",
            result_scoped
                .diagnostics
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>()
        );

        // Scoped to just `head.ink`: the colliding file is out of scope, so
        // the diagnostic (which is attributed to `alpha.ink`) must not
        // appear — pinning the `file_ids.contains(&d.file)` filter.
        let result_unscoped = driver.analyze_project(&[head]);
        assert!(
            !result_unscoped
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E085),
            "E085 must not appear when the colliding file is excluded from file_ids, got {:?}",
            result_unscoped
                .diagnostics
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>()
        );
    }
}
