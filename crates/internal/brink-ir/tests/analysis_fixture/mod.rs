//! Shared test fixture: the analyzer piece-function composition
//! (index → per-file scope/resolve → finish), for suites that exercise
//! resolution/import-scope semantics against HAND-CRAFTED module maps.
//!
//! This existed as the production `brink_analyzer::analyze_with_modules`
//! monolith until option A total (ruled 2026-08-24) deleted it — production
//! analysis is `brink-db`'s salsa composition (`analysis_query` /
//! `subset_analysis_query`), which derives module identity from file
//! *paths* and therefore cannot be handed a synthetic map. These suites'
//! whole point is synthetic maps (pinning resolution behavior for exact
//! module shapes without arranging a filesystem layout to induce them), so
//! they compose the same public piece functions the salsa queries call —
//! a test fixture over the one engine, not a second engine.

use std::collections::BTreeMap;
use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, AnalysisResult, ImportScope, ModuleMap};
use brink_ir::{FileId, HirFile, SymbolManifest};

/// Compose index → resolve → finish over `files` with a hand-crafted
/// `modules` map — the retired monolith's exact composition (including the
/// map-authoritative declared-module scoping and per-file scope capture).
pub fn analyze_with_map(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    modules: &ModuleMap,
    opts: &AnalysisOptions,
    is_native: bool,
) -> AnalysisResult {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> =
        files.iter().map(|&(id, _hir, m)| (id, m)).collect();
    let (index, mut diagnostics) = brink_analyzer::symbol_index_with_modules(
        &manifest_inputs,
        modules,
        opts.dialect,
        is_native,
    );

    let mut resolutions = brink_ir::ResolutionMap::new();
    let mut scopes: BTreeMap<FileId, ImportScope> = BTreeMap::new();
    for &(file_id, hir, manifest) in files {
        let declared_module = match modules.get(&file_id) {
            Some(resolved) => resolved.declared.then(|| resolved.name.clone()),
            None => hir.module.as_ref().map(|m| m.name.clone()),
        };
        let scope = ImportScope::new(declared_module, &hir.imports);
        let (file_map, file_diags) = brink_analyzer::resolve(file_id, manifest, &index, &scope);
        resolutions.extend(Arc::unwrap_or_clone(file_map));
        diagnostics.extend(file_diags);
        scopes.insert(file_id, scope);
    }

    let hir_files: Vec<(FileId, &HirFile)> = files.iter().map(|&(id, hir, _)| (id, hir)).collect();
    diagnostics.extend(brink_analyzer::conventions_confinement_diagnostics(
        &hir_files,
        modules,
        opts.conventions.as_deref(),
    ));

    brink_analyzer::finish_analysis(
        files,
        index,
        resolutions,
        diagnostics,
        opts,
        is_native,
        None,
        &scopes,
    )
}
