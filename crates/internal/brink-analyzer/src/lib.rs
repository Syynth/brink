//! Cross-file semantic analysis for inkle's ink narrative scripting language.
//!
//! The analyzer merges per-file `SymbolManifest`s from `brink-ir` into a
//! unified `SymbolIndex`, then runs validation passes (name resolution,
//! duplicate detection, type checking). Both `brink-compiler` and `brink-lsp`
//! consume the analysis result.

mod dialect_gate;
mod external_check;
mod infer;
mod manifest;
mod resolve;
mod signature;
mod validate;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use brink_ir::FileId;
pub use brink_ir::ResolutionMap;
pub use dialect_gate::Dialect;
pub use external_check::{
    ExternalCheckSeverity, InferredType, ResolvedParam, ResolvedType,
    SemanticTypeDiagnosticSeverity, SymbolMeta, ValueMeta,
};
pub use infer::{BodyTypes, InferenceResult, InferredSig, Ty, infer_project, unify, unify_all};
pub use signature::{Sig, signature};

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
    /// Severity policy for unknown-semantic-type diagnostics (`E040`).
    /// Defaults to `Tolerant` (the #339/#527 default-tolerant path); raise to
    /// `Error` to re-enable strict checking with no manifest registered
    /// (#532).
    pub semantic_type_check: SemanticTypeDiagnosticSeverity,
    /// T1b compiler dialect: gates brink-extension syntax (blocks, sigil
    /// literals, indexing). Defaults to `StrictInk` — an authoring-time/
    /// tooling input only, mount-time (CLI flag) in T1b-1; project-file
    /// config is out of scope (docs/t1b-surface-spec.md §1, #368 precedent).
    pub dialect: Dialect,
}

/// The output of cross-file semantic analysis.
///
/// `PartialEq` supports early-cutoff backdating when the result is produced
/// by the salsa `analysis` query in `brink-db`.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisResult {
    /// The unified symbol index.
    pub index: Arc<SymbolIndex>,
    /// Resolved references: maps source range → definition id.
    pub resolutions: ResolutionMap,
    /// Diagnostics produced during analysis (duplicate definitions, unresolved refs, etc.).
    pub diagnostics: Vec<Diagnostic>,
    /// Per-symbol metadata enrichment (docs, resolved types, initializer
    /// values), keyed by `DefinitionId`. Empty when no host manifest is
    /// registered and no inline `///` docs are present.
    pub symbol_meta: BTreeMap<DefinitionId, SymbolMeta>,
}

/// Build the project-wide declaration index from per-file symbol manifests.
///
/// Query-shaped seam for the scripting substrate (spec §4, layer 2 —
/// `symbol_index()`): a pure function of the per-file manifests, returning
/// the merged index plus indexing diagnostics (duplicate definitions,
/// built-in shadowing). Declarations only — no body analysis happens here,
/// though the index does include body-declared locals (params/temps), which
/// hierarchical resolution needs.
#[must_use]
pub fn symbol_index(files: &[(FileId, &SymbolManifest)]) -> (Arc<SymbolIndex>, Vec<Diagnostic>) {
    let (index, diagnostics) = manifest::merge_manifests(files);
    (Arc::new(index), diagnostics)
}

/// Resolve one file's references against the project-wide symbol index.
///
/// Query-shaped seam for the scripting substrate (spec §4, layer 2 —
/// `resolve(FileId)`): a pure function of the symbol index and this file's
/// own manifest. It never reads another file's content, so a body edit in
/// file B can only affect file A's resolutions by way of the shared index.
#[must_use]
pub fn resolve(
    file: FileId,
    manifest: &SymbolManifest,
    index: &SymbolIndex,
) -> (Arc<ResolutionMap>, Vec<Diagnostic>) {
    let (map, diagnostics) = resolve::resolve_file(index, file, manifest);
    (Arc::new(map), diagnostics)
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

    let (index, mut diagnostics) = symbol_index(&manifest_inputs);
    let mut resolutions = ResolutionMap::new();
    for &(file_id, manifest) in &manifest_inputs {
        let (file_map, file_diags) = resolve(file_id, manifest, &index);
        resolutions.extend(Arc::unwrap_or_clone(file_map));
        diagnostics.extend(file_diags);
    }

    finish_analysis(files, index, resolutions, diagnostics, opts)
}

/// Assemble the final [`AnalysisResult`] from the already-computed layer-2
/// pieces (index + per-file resolutions), running the remaining monolithic
/// passes: validation, host-manifest enrichment/checks, and value-meta
/// inference.
///
/// Query-shaped seam for the scripting substrate: `brink-db`'s salsa
/// `analysis` query composes [`symbol_index`] and per-file [`resolve`]
/// queries and then calls this — the same back half [`analyze_with_options`]
/// runs — so the query-composed result is identical to the monolithic one by
/// construction.
pub fn finish_analysis(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    index: Arc<SymbolIndex>,
    resolutions: ResolutionMap,
    mut diagnostics: Vec<Diagnostic>,
    opts: &AnalysisOptions,
) -> AnalysisResult {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|&(id, _hir, manifest)| (id, manifest))
        .collect();

    let hir_inputs: Vec<(FileId, &HirFile)> = files.iter().map(|&(id, hir, _)| (id, hir)).collect();

    diagnostics.extend(validate::validate(&hir_inputs));
    diagnostics.extend(dialect_gate::check(&hir_inputs, &resolutions, opts.dialect));

    // Host-manifest enrichment + checks (tooling/author-time only).
    let inline_docs = collect_inline_docs(&manifest_inputs);
    let (types, registered) = manifest_maps(opts.host_manifest.as_ref());
    let has_manifest = opts.host_manifest.is_some();
    // Unknown-semantic-type checking (`E040`) is on when a manifest is
    // registered, or when the severity lever is explicitly raised to `Error`
    // (#532) — a host can opt back into strict checking with no manifest.
    let check_unknown_types =
        has_manifest || opts.semantic_type_check == SemanticTypeDiagnosticSeverity::Error;
    let (mut symbol_meta, ext_diags) = external_check::analyze_externals(
        &index,
        &inline_docs,
        &types,
        &registered,
        opts.external_check,
        check_unknown_types,
    );
    diagnostics.extend(ext_diags);

    // Knot/stitch doc enrichment (presentational; shares the semantic-type
    // vocabulary, so unknown types still diagnose — but only once a manifest
    // is registered, or the severity lever is raised (#339/#532); see
    // `resolve_type`).
    let (callable_meta, callable_diags) = external_check::enrich_callables(
        &index,
        &inline_docs,
        &types,
        opts.external_check,
        check_unknown_types,
    );
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

#[cfg(test)]
mod tests {
    //! End-to-end coverage for #339: host semantic types (`///` `@param`
    //! tags referencing host vocabulary, e.g. `actor_id`) must not block
    //! compilation when no `HostManifest` is registered, while a registered
    //! manifest keeps full checking (a genuinely unknown type still errors).

    use brink_ir::{BaseType, HostManifest, SemanticTypeDef};

    use super::{
        AnalysisOptions, FileId, SemanticTypeDiagnosticSeverity, analyze, analyze_with_options,
    };

    /// ink with an `EXTERNAL` whose param is typed with a host semantic type
    /// (`actor_id`) — exactly the `host.ink`-generated shape from the issue.
    const SRC: &str = "\
/// @param who {actor_id}
EXTERNAL add_state(who)
";

    fn lower(src: &str) -> (brink_ir::hir::HirFile, brink_ir::SymbolManifest) {
        let parsed = brink_syntax::parse(src);
        let tree = parsed.tree();
        let (hir, manifest, diags) = brink_ir::hir::lower(FileId(0), &tree);
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        (hir, manifest)
    }

    #[test]
    fn host_semantic_type_compiles_host_free_with_no_manifest() {
        let (hir, manifest) = lower(SRC);
        // `analyze()` uses `AnalysisOptions::default()` — no host manifest —
        // matching the real "no HostManifest registered" consumer path
        // (`compileProject()` with no `setHostManifest` call).
        let result = analyze(&[(FileId(0), &hir, &manifest)]);
        assert!(
            result.diagnostics.is_empty(),
            "host-free compile must not error on unknown semantic types: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn host_semantic_type_still_checked_once_manifest_registered() {
        let (hir, manifest) = lower(SRC);
        let host_manifest = HostManifest {
            externals: Vec::new(),
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            }],
        };
        let opts = AnalysisOptions {
            host_manifest: Some(host_manifest),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "known semantic type resolves cleanly: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn genuinely_unknown_type_still_errors_when_manifest_registered() {
        // Same shape, but the registered manifest does NOT define `actor_id`
        // — a manifest being present makes checking fully binding again.
        let (hir, manifest) = lower(SRC);
        let opts = AnalysisOptions {
            host_manifest: Some(HostManifest::default()),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.code == brink_ir::DiagnosticCode::E040)
                .count(),
            1,
            "manifest registered but type unknown: E040 still fires: {:?}",
            result.diagnostics
        );
    }

    /// #532: `semantic_type_check` defaults to `Tolerant`, matching the
    /// #339/#527 default-tolerant behavior — an explicit `Tolerant` opt-in
    /// behaves identically to the unset default.
    #[test]
    fn semantic_type_check_default_is_tolerant() {
        let (hir, manifest) = lower(SRC);
        let opts = AnalysisOptions {
            semantic_type_check: SemanticTypeDiagnosticSeverity::Tolerant,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "Tolerant (default) with no manifest: no E040: {:?}",
            result.diagnostics
        );
    }

    /// #532: raising `semantic_type_check` to `Error` re-enables strict
    /// checking even with no manifest registered — a host can catch typo'd
    /// semantic-type tags before wiring up a full manifest.
    #[test]
    fn semantic_type_check_error_diagnoses_with_no_manifest() {
        let (hir, manifest) = lower(SRC);
        let opts = AnalysisOptions {
            semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.code == brink_ir::DiagnosticCode::E040)
                .count(),
            1,
            "Error with no manifest: E040 still fires: {:?}",
            result.diagnostics
        );
    }

    /// #532: the lever composes with a registered manifest that defines the
    /// type — a known type never diagnoses regardless of severity.
    #[test]
    fn semantic_type_check_error_with_known_type_in_manifest_is_clean() {
        let (hir, manifest) = lower(SRC);
        let host_manifest = HostManifest {
            externals: Vec::new(),
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            }],
        };
        let opts = AnalysisOptions {
            host_manifest: Some(host_manifest),
            semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "known type resolves cleanly regardless of severity: {:?}",
            result.diagnostics
        );
    }
}
