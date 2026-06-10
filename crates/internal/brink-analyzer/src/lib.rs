//! Cross-file semantic analysis for inkle's ink narrative scripting language.
//!
//! The analyzer merges per-file `SymbolManifest`s from `brink-ir` into a
//! unified `SymbolIndex`, then runs validation passes (name resolution,
//! duplicate detection, type checking). Both `brink-compiler` and `brink-lsp`
//! consume the analysis result.

mod external_check;
mod manifest;
mod resolve;
mod validate;

use std::collections::BTreeMap;

pub use brink_ir::FileId;
pub use brink_ir::ResolutionMap;
pub use external_check::{
    ExternalCheckSeverity, InferredType, ResolvedParam, ResolvedType, SymbolMeta, ValueMeta,
};

use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DocBlock, HirFile, HostManifest, ManifestExternal, SemanticTypeDef, SymbolIndex,
    SymbolKind, SymbolManifest,
};

/// Tooling options for analysis: the registered host manifest and the
/// severity policy for its external checks. Defaults to no manifest.
#[derive(Debug, Clone, Default)]
pub struct AnalysisOptions {
    /// The registered host-capability manifest, if any.
    pub host_manifest: Option<HostManifest>,
    /// Severity policy for manifest-driven external diagnostics.
    pub external_check: ExternalCheckSeverity,
}

/// The output of cross-file semantic analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// The unified symbol index.
    pub index: SymbolIndex,
    /// Resolved references: maps source range → definition id.
    pub resolutions: ResolutionMap,
    /// Diagnostics produced during analysis (duplicate definitions, unresolved refs, etc.).
    pub diagnostics: Vec<Diagnostic>,
    /// Per-symbol metadata enrichment (docs, resolved types, initializer
    /// values), keyed by `DefinitionId`. Empty when no host manifest is
    /// registered and no inline `///` docs are present.
    pub symbol_meta: BTreeMap<DefinitionId, SymbolMeta>,
}

/// Run cross-file semantic analysis with default options (no host manifest).
///
/// Each entry is a `(FileId, HirFile, SymbolManifest)` tuple produced by
/// per-file HIR lowering.
pub fn analyze(files: &[(FileId, &HirFile, &SymbolManifest)]) -> AnalysisResult {
    analyze_with_options(files, &AnalysisOptions::default())
}

/// Run cross-file semantic analysis with explicit tooling options, including
/// an optional host-capability manifest and its external-check severity.
pub fn analyze_with_options(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    opts: &AnalysisOptions,
) -> AnalysisResult {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|&(id, _hir, manifest)| (id, manifest))
        .collect();

    let hir_inputs: Vec<(FileId, &HirFile)> = files.iter().map(|&(id, hir, _)| (id, hir)).collect();

    let (index, mut diagnostics) = manifest::merge_manifests(&manifest_inputs);
    let (resolutions, resolve_diags) = resolve::resolve_refs(&index, &manifest_inputs);
    diagnostics.extend(resolve_diags);
    diagnostics.extend(validate::validate(&hir_inputs));

    // Host-manifest enrichment + checks (tooling/author-time only).
    let inline_docs = collect_inline_docs(&manifest_inputs);
    let (types, registered) = manifest_maps(opts.host_manifest.as_ref());
    let (mut symbol_meta, ext_diags) = external_check::analyze_externals(
        &index,
        &inline_docs,
        &types,
        &registered,
        opts.external_check,
    );
    diagnostics.extend(ext_diags);

    // Knot/stitch doc enrichment (presentational; shares the semantic-type
    // vocabulary, so unknown types still diagnose).
    let (callable_meta, callable_diags) =
        external_check::enrich_callables(&index, &inline_docs, &types, opts.external_check);
    diagnostics.extend(callable_diags);
    symbol_meta.extend(callable_meta);

    // VAR/CONST initializer info + LIST docs (presentational, no diagnostics).
    symbol_meta.extend(external_check::infer_value_meta(
        &hir_inputs,
        &index,
        &inline_docs,
    ));

    // Call-site literal checks (type mismatch, closed domain) over the HIR.
    // Externals only — knot/stitch metadata is presentational, not binding.
    if opts.external_check != ExternalCheckSeverity::Off {
        let name_to_meta: BTreeMap<&str, &SymbolMeta> = symbol_meta
            .iter()
            .filter_map(|(id, meta)| {
                index.symbols.get(id).and_then(|s| {
                    (s.kind == SymbolKind::External).then_some((s.name.as_str(), meta))
                })
            })
            .collect();
        diagnostics.extend(external_check::check_call_sites(&hir_inputs, &name_to_meta));
    }

    AnalysisResult {
        index,
        resolutions,
        diagnostics,
        symbol_meta,
    }
}

/// Collect inline `///` docs across all files, keyed by `(kind, declared name)`.
fn collect_inline_docs(
    files: &[(FileId, &SymbolManifest)],
) -> BTreeMap<(SymbolKind, String), DocBlock> {
    let mut out = BTreeMap::new();
    for &(_id, manifest) in files {
        for (key, doc) in &manifest.docs {
            out.insert(key.clone(), doc.clone());
        }
    }
    out
}

/// Build lookup maps from the registered manifest: semantic types by name and
/// registered externals by name.
fn manifest_maps(
    manifest: Option<&HostManifest>,
) -> (
    BTreeMap<String, SemanticTypeDef>,
    BTreeMap<String, &ManifestExternal>,
) {
    let mut types = BTreeMap::new();
    let mut registered = BTreeMap::new();
    if let Some(manifest) = manifest {
        for ty in &manifest.types {
            types.insert(ty.name.clone(), ty.clone());
        }
        for ext in &manifest.externals {
            registered.insert(ext.name.clone(), ext);
        }
    }
    (types, registered)
}
