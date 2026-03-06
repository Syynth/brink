//! Cross-file semantic analysis for inkle's ink narrative scripting language.
//!
//! The analyzer merges per-file `SymbolManifest`s from `brink-ir` into a
//! unified `SymbolIndex`, then runs validation passes (name resolution,
//! duplicate detection, type checking). Both `brink-compiler` and `brink-lsp`
//! consume the analysis result.

mod manifest;
mod resolve;

pub use brink_ir::FileId;
pub use resolve::ResolutionMap;

use brink_ir::{Diagnostic, HirFile, SymbolIndex, SymbolManifest};

/// The output of cross-file semantic analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// The unified symbol index.
    pub index: SymbolIndex,
    /// Resolved references: maps source range → definition id.
    pub resolutions: ResolutionMap,
    /// Diagnostics produced during analysis (duplicate definitions, unresolved refs, etc.).
    pub diagnostics: Vec<Diagnostic>,
}

/// Run cross-file semantic analysis on a set of lowered files.
///
/// Each entry is a `(FileId, HirFile, SymbolManifest)` tuple produced by
/// per-file HIR lowering. Returns the unified symbol index, resolution map,
/// and any diagnostics.
pub fn analyze(files: Vec<(FileId, HirFile, SymbolManifest)>) -> AnalysisResult {
    let manifest_inputs: Vec<(FileId, SymbolManifest)> = files
        .into_iter()
        .map(|(id, _hir, manifest)| (id, manifest))
        .collect();

    let (index, mut diagnostics) = manifest::merge_manifests(&manifest_inputs);
    let (resolutions, resolve_diags) = resolve::resolve_refs(&index, &manifest_inputs);
    diagnostics.extend(resolve_diags);

    AnalysisResult {
        index,
        resolutions,
        diagnostics,
    }
}
