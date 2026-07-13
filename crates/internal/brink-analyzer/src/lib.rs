//! Cross-file semantic analysis for inkle's ink narrative scripting language.
//!
//! The analyzer merges per-file `SymbolManifest`s from `brink-ir` into a
//! unified `SymbolIndex`, then runs validation passes (name resolution,
//! duplicate detection, type checking). Both `brink-compiler` and `brink-lsp`
//! consume the analysis result.

mod annotations;
mod conversions;
mod dialect_gate;
mod external_check;
mod infer;
mod manifest;
mod resolve;
mod signature;
mod strict;
mod structs;
mod validate;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use annotations::{
    check as check_annotations, mismatches as annotation_mismatches, resolve as resolve_annotation,
};
pub use brink_ir::FileId;
pub use brink_ir::ResolutionMap;
pub use dialect_gate::Dialect;
pub use external_check::{
    ExternalCheckSeverity, InferredType, ResolvedParam, ResolvedType,
    SemanticTypeDiagnosticSeverity, SymbolMeta, ValueMeta,
};
pub use infer::{
    BodyTypes, CallGraph, Def, InferenceResult, InferredSig, SccGraph, Ty, call_edges, def_body,
    infer_project, inferable_defs, inferable_defs_from_index, referenced_globals, scc_graph,
    solve_scc, unify, unify_all,
};
pub use signature::{Sig, signature};
pub use strict::{TypePolicy, effective_severity};

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
    /// TM-3 typed-mode policy (docs/typed-mode-spec.md §1): `Gradual` (the
    /// default) is today's behavior, byte-identical forever. `Strict`
    /// requires `dialect = Brink` (a config error otherwise, `E064`) and
    /// turns on `Unknown`/`Conflicted`-escape errors, the boundary
    /// annotation-firewall exemption, and auto-wires `E063`
    /// (annotation-vs-inference mismatch) into production. Authoring-time/
    /// tooling input only — never embedded in `.inkb`, mirroring `dialect`.
    pub types: TypePolicy,
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

    finish_analysis(files, index, resolutions, diagnostics, opts, None)
}

/// Per-file diagnostic contributors (issue #632 / FG-3,
/// `docs/fine-grained-salsa-proposal.md` §1 item 4): structural validation,
/// the dialect gate, and (brink dialect only) annotation-*content* checks —
/// the three passes `finish_analysis` used to run as whole-project loops
/// (`validate::validate`/`dialect_gate::check`/`annotations::check`, each
/// internally iterating every file) even though none of them actually reads
/// another file's state:
///
/// - [`validate::validate`] never reads cross-file state at all.
/// - [`dialect_gate::check`]'s only cross-file-shaped input, the resolution
///   map, is queried only for `(this file, range)` pairs — a reference's
///   resolution record always carries the file the reference itself lives
///   in, never another file's — so `file_resolutions` need only be this
///   file's own slice.
/// - [`annotations::check`]'s only cross-file input is the project's
///   declared `LIST` names, itself derivable from a range-free index
///   projection (`declared_list_names` reads no symbol's range).
///
/// This is the query-shaped seam `brink-db`'s `per_file_diagnostics_query`
/// wraps: a body edit in file Y leaves file X's per-file contributor memo
/// untouched (pinned by `fg3_dependency_edges.rs`).
#[must_use]
pub fn per_file_diagnostics(
    file: FileId,
    hir: &HirFile,
    file_resolutions: &ResolutionMap,
    index: &SymbolIndex,
    dialect: Dialect,
) -> Vec<Diagnostic> {
    let files = [(file, hir)];
    let mut out = validate::validate(&files);
    out.extend(dialect_gate::check(&files, file_resolutions, dialect));
    // Annotation *content* checks (E061/E062) run only under the brink
    // dialect: under `strict-ink` the annotation is already rejected whole
    // by `dialect_gate` (E051), and critiquing the inside of rejected
    // syntax is noise (maintainer ruling 2026-07-13).
    if dialect == Dialect::Brink {
        out.extend(annotations::check(&files, index));
    }
    out
}

/// Whole-project diagnostic contributors that genuinely need cross-file
/// state (issue #632 / FG-3 design doc §1): host-manifest enrichment/checks
/// (`external_check` — needs the full ranged index for diagnostic spans and
/// every file's HIR to find call sites anywhere in the project) and, under
/// `types = strict`, the strict typed-mode checks (`strict::check` — needs a
/// whole-project [`InferenceResult`]). Also produces `symbol_meta`
/// (doc/type enrichment for hover etc.), which is inherently project-wide
/// the same way.
///
/// `strict_inference`: TM-3's strict pass needs a whole-project
/// [`InferenceResult`] (docs/typed-mode-spec.md §9-step-3 — E063 auto-wiring
/// "must run inference anyway"). Pass `None` to have this function compute
/// its own via [`infer_project`] (the self-contained default —
/// [`analyze_with_options`]'s pure, non-salsa path). Pass `Some` to reuse an
/// already-computed result instead — `brink-db`'s
/// `whole_project_diagnostics_query` supplies its FG-narrowed,
/// per-SCC-memoized `type_inference_query` here so strict mode's
/// warm-reanalyze cost is the incremental one the FG spine exists for, not a
/// from-scratch whole-project solve on every keystroke. Ignored entirely
/// under `types = gradual` or when the dialect makes strict mode a config
/// error. The `types = strict` + wrong-dialect config error (`E064`) is
/// computed exactly once here, guarded by the same top-level `if` as before
/// this split (issue #632's TM-3-interaction fence).
#[must_use]
pub fn whole_project_diagnostics(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    opts: &AnalysisOptions,
    strict_inference: Option<&infer::InferenceResult>,
) -> (Vec<Diagnostic>, BTreeMap<DefinitionId, SymbolMeta>) {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|&(id, _hir, manifest)| (id, manifest))
        .collect();
    let hir_inputs: Vec<(FileId, &HirFile)> = files.iter().map(|&(id, hir, _)| (id, hir)).collect();

    let mut diagnostics = Vec::new();

    // TM-3 strict typed-mode policy (docs/typed-mode-spec.md §1/§9-step-3).
    // `types = strict` requires `dialect = brink` — a config error (`E064`)
    // otherwise, reported alone (nothing else strict-specific runs against a
    // project whose dialect already rejects the annotation syntax strict
    // mode needs). Under `dialect = brink`, run inference (reusing
    // `strict_inference` when the caller already computed one) and wire in
    // Unknown/Conflicted-escape (`E065`/`E066`) plus `E063` mismatches.
    // Gradual mode never reaches this block — byte-identical, forever.
    if opts.types == TypePolicy::Strict {
        if let Some(diag) = strict::config_error(opts.dialect, hir_inputs.first().map(|&(f, _)| f))
        {
            diagnostics.push(diag);
        } else {
            let owned_inference;
            let inference = if let Some(inf) = strict_inference {
                inf
            } else {
                owned_inference = infer::infer_project(&hir_inputs, index, resolutions);
                &owned_inference
            };
            diagnostics.extend(strict::check(&hir_inputs, index, inference, resolutions));
        }
    }

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
        index,
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
        index,
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
        index,
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

    (diagnostics, symbol_meta)
}

/// Assemble the final [`AnalysisResult`] from the already-computed layer-2
/// pieces (index + per-file resolutions), running the remaining passes:
/// per-file diagnostic contributors ([`per_file_diagnostics`]) for every
/// file, then the whole-project contributors
/// ([`whole_project_diagnostics`]).
///
/// Query-shaped seam for the scripting substrate: `brink-db`'s salsa
/// `analysis_query` composes [`symbol_index`] and per-file [`resolve`]
/// queries, then the decomposed per-file/whole-project queries this
/// function's two halves wrap (issue #632 / FG-3) — the same sequence this
/// function runs, in the same order, so the query-composed result is
/// identical to the monolithic one by construction (pinned by
/// `query_equivalence.rs`).
///
/// `strict_inference`: see [`whole_project_diagnostics`]'s doc — forwarded
/// unchanged.
pub fn finish_analysis(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    index: Arc<SymbolIndex>,
    resolutions: ResolutionMap,
    mut diagnostics: Vec<Diagnostic>,
    opts: &AnalysisOptions,
    strict_inference: Option<&infer::InferenceResult>,
) -> AnalysisResult {
    for &(file_id, hir, _manifest) in files {
        let file_resolutions: ResolutionMap = resolutions
            .iter()
            .filter(|r| r.file == file_id)
            .cloned()
            .collect();
        diagnostics.extend(per_file_diagnostics(
            file_id,
            hir,
            &file_resolutions,
            &index,
            opts.dialect,
        ));
    }

    let (whole_diagnostics, symbol_meta) =
        whole_project_diagnostics(files, &index, &resolutions, opts, strict_inference);
    diagnostics.extend(whole_diagnostics);

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
        AnalysisOptions, Dialect, FileId, SemanticTypeDiagnosticSeverity, TypePolicy, analyze,
        analyze_with_options,
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

    // ── TM-3 (#619): strict policy end-to-end through analyze_with_options ──

    fn lower_one(src: &str) -> (brink_ir::hir::HirFile, brink_ir::SymbolManifest) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, diags) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        (hir, manifest)
    }

    /// Gradual is byte-identical forever: the same source, `types` left at
    /// its default (`Gradual`), under either dialect, must produce results
    /// identical to a build that predates TM-3 entirely — no `E064`/`E065`/
    /// `E066`, and `E063` stays un-auto-invoked (matching the #618/PR#640
    /// ruling this issue explicitly does not touch).
    #[test]
    fn gradual_is_byte_identical_regardless_of_dialect() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);
        for dialect in [Dialect::StrictInk, Dialect::Brink] {
            let opts = AnalysisOptions {
                dialect,
                ..AnalysisOptions::default()
            };
            let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
            assert!(
                result.diagnostics.is_empty(),
                "gradual (default types) must stay silent under dialect {dialect:?}: {:?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn strict_with_strict_ink_dialect_is_a_config_error_and_nothing_else_runs() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: TypePolicy::Strict,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        let strict_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| {
                matches!(
                    d.code,
                    brink_ir::DiagnosticCode::E064
                        | brink_ir::DiagnosticCode::E065
                        | brink_ir::DiagnosticCode::E066
                )
            })
            .collect();
        assert_eq!(
            strict_diags.len(),
            1,
            "exactly the one config error, nothing else: {:?}",
            result.diagnostics
        );
        assert_eq!(strict_diags[0].code, brink_ir::DiagnosticCode::E064);
    }

    #[test]
    fn strict_with_brink_dialect_surfaces_unknown_escape_as_a_compile_error() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            types: TypePolicy::Strict,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "{:?}",
            result.diagnostics
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .find(|d| d.code == brink_ir::DiagnosticCode::E065)
                .expect("checked above")
                .code
                .severity(),
            brink_ir::Severity::Error,
            "Unknown-escape is a compile error under strict, not a warning"
        );
    }

    #[test]
    fn strict_clean_project_compiles_with_no_diagnostics() {
        let src =
            "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            types: TypePolicy::Strict,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }
}
