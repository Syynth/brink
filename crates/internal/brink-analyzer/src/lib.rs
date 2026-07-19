//! Cross-file semantic analysis for inkle's ink narrative scripting language.
//!
//! The analyzer merges per-file `SymbolManifest`s from `brink-ir` into a
//! unified `SymbolIndex`, then runs validation passes (name resolution,
//! duplicate detection, type checking). Both `brink-compiler` and `brink-lsp`
//! consume the analysis result.

mod annotations;
mod await_purity;
mod comparator_contract;
mod conversions;
mod determinism;
mod dialect_gate;
mod effects_assertions;
mod external_check;
mod fn_values;
mod infer;
mod manifest;
mod map_keys;
mod modules;
mod option_conditions;
mod option_rules;
mod protocols;
mod range_refinement;
mod ref_projection;
mod resolve;
mod signature;
mod strict;
mod structs;
mod type_resolution;
mod validate;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use annotations::{
    check as check_annotations, mismatches as annotation_mismatches, resolve as resolve_annotation,
};
pub use await_purity::{
    check as await_purity_diagnostics, condition_callees as await_condition_callees, hir_has_await,
};
pub use brink_ir::FileId;
pub use brink_ir::ResolutionMap;
pub use comparator_contract::{
    check as comparator_contract_diagnostics, comparator_callees, hir_has_comparator_site,
};
pub use dialect_gate::Dialect;
pub use effects_assertions::{
    assertion_defs as effects_assertion_defs, check as effects_assertion_diagnostics,
};
pub use external_check::{
    ExternalCheckSeverity, InferredType, ResolvedParam, ResolvedType,
    SemanticTypeDiagnosticSeverity, SymbolMeta, ValueMeta,
};
pub use infer::{
    BodyTypes, CallGraph, CoalesceError, Def, EffectAtoms, EffectRow, InferenceResult, InferredSig,
    SccGraph, Ty, ValueCallFact, ValueCallKind, call_edges, coalesce, collect_external_sigs,
    def_body, def_effect_atoms, effects_project, infer_project, inferable_defs,
    inferable_defs_from_index, referenced_globals, scc_graph, solve_scc, solve_scc_effects, unify,
    unify_all,
};
pub use manifest::{ModuleMap, ResolvedModule};
pub use protocols::{
    Protocol, ProtocolImplDecl, check_protocol_impls, check_reserved_names,
    is_reserved_protocol_name, iterate_element_ty,
};
pub use resolve::ImportScope;
pub use signature::{Sig, signature};
pub use strict::{TypePolicy, effective_severity, resolve_type_policy};
pub use structs::{ShapeInfo, declared_shapes};

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
    /// TM-3 typed-mode policy (docs/typed-mode-spec.md §1). `None` means
    /// "the project never said" — the effective policy is then keyed on the
    /// dialect via [`resolve_type_policy`] (issue #1127, ruled 2026-07-19):
    /// `Brink` → `Strict`, `StrictInk` → `Gradual` (forever — the oracle
    /// corpus is anchored to it). `Some(_)` is an explicit choice (CLI flag,
    /// `brink.toml`, editor API) and always wins. Read the effective policy
    /// via [`AnalysisOptions::type_policy`], never this field directly.
    ///
    /// `Strict` requires `dialect = Brink` (a config error otherwise,
    /// `E064`) and turns on `Unknown`/`Conflicted`-escape errors, the
    /// boundary annotation-firewall exemption, and auto-wires `E063`
    /// (annotation-vs-inference mismatch) into production. Authoring-time/
    /// tooling input only — never embedded in `.inkb`, mirroring `dialect`.
    pub types: Option<TypePolicy>,
}

impl AnalysisOptions {
    /// The effective `types` policy for this options set — the one
    /// resolution seam (issue #1127): an explicit [`Self::types`] wins;
    /// otherwise the dialect-keyed default from [`resolve_type_policy`].
    #[must_use]
    pub fn type_policy(&self) -> TypePolicy {
        resolve_type_policy(self.dialect, self.types)
    }
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

/// The merged symbol index with `DefinitionId`s qualified by each file's
/// **declared** module (M-1, docs/modules-spec.md §5).
///
/// Identical to [`symbol_index`] for undeclared stem-modules (the entire
/// pre-modules corpus) — byte-identical `DefinitionId`s — and qualifies
/// names by module only for files carrying `#@module`. `brink-db`'s
/// `symbol_index_query` builds the [`ModuleMap`] from file stems,
/// `#@module` declarations, and the INCLUDE graph, then calls this.
///
/// `dialect` gates the M-2c cross-declared-module duplicate escalation
/// (issue #784): see [`manifest::merge_manifests_with_modules`].
#[must_use]
pub fn symbol_index_with_modules(
    files: &[(FileId, &SymbolManifest)],
    modules: &ModuleMap,
    dialect: Dialect,
) -> (Arc<SymbolIndex>, Vec<Diagnostic>) {
    let (index, diagnostics) = manifest::merge_manifests_with_modules(files, modules, dialect);
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
    scope: &ImportScope,
) -> (Arc<ResolutionMap>, Vec<Diagnostic>) {
    let (map, diagnostics) = resolve::resolve_file(index, scope, file, manifest);
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
    for &(file_id, hir, manifest) in files {
        // Import-scoped resolution (M-2d, issue #790). This whole-project
        // convenience path uses the non-module-qualified `symbol_index`, so
        // every symbol carries `module: None` and the scope is inert (flat
        // resolution) — but building it from the file's own HIR keeps this
        // path honest and mirrors the real `brink-db` pipeline, which feeds
        // the INCLUDE-resolved module.
        let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
        let (file_map, file_diags) = resolve(file_id, manifest, &index, &scope);
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
/// - [`annotations::check`]'s cross-file inputs are the project's declared
///   `LIST`/`STRUCT` names (derivable from a range-free index projection —
///   `declared_list_names`/`declared_struct_names` read no symbol's range)
///   and, for `handle<K>` (T1d-2, docs/t1d-spec.md §3), the registered host
///   manifest — project-wide, host-set config, not file-edit-derived, so
///   reading it here is the same coarse dependency shape `dialect` already
///   is, not a reintroduction of whole-project churn.
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
    host_manifest: Option<&HostManifest>,
) -> Vec<Diagnostic> {
    let files = [(file, hir)];
    let mut out = validate::validate(&files);
    out.extend(dialect_gate::check(&files, file_resolutions, dialect));
    // NS-A1 E107 (bare-`none`-needs-context, docs/stdlib-spec.md §1.4) —
    // dialect-INDEPENDENT, unlike the brink-only block below: the rule is
    // part of the Option package itself, and under `strict-ink` (where
    // `VAR`/`CONST` initializers aren't in the gate's block-tree walk) it
    // is also what keeps `VAR x = none` an error at all. Same per-file
    // argument as `dialect_gate`: the resolution records consulted always
    // carry this file's own id.
    out.extend(option_rules::check(&files, file_resolutions));
    // Annotation *content* checks (E061) run only under the brink
    // dialect: under `strict-ink` the annotation is already rejected whole
    // by `dialect_gate` (E051), and critiquing the inside of rejected
    // syntax is noise (maintainer ruling 2026-07-13).
    if dialect == Dialect::Brink {
        out.extend(annotations::check(&files, index, host_manifest));
        // T1c `#fn` creation-site checks (E079/E080/E081) follow the same
        // brink-only rule: under `strict-ink` the literal is already
        // rejected whole (E051). Per-file by the same argument as
        // `dialect_gate`: the resolution records consulted always carry
        // this file's own id.
        out.extend(fn_values::check(&files, file_resolutions, index));
        // T1e-1 `ref lvalue-path` creation-site checks (E080 durable root,
        // E097 standalone position, docs/t1e-spec.md §2/§6, issue #831) —
        // same brink-only rule, same per-file argument as `fn_values`'s own
        // comment just above (a reference's resolution record always
        // carries the file the reference itself lives in).
        out.extend(ref_projection::check(&files, file_resolutions, index));
        // Struct construction-literal duplicate-field check (E084, issue
        // #675) — same brink-only rule, and unlike `structs::check`'s
        // missing/extra/mistyped trio this runs under *both* `types`
        // policies (see `structs`' module doc): a repeated field name is a
        // structural mistake detectable from the literal alone, with no
        // shape resolution or whole-project inference needed.
        out.extend(structs::check_duplicates(&files));
        // Map-literal key-domain warning (E106, issue #598,
        // docs/t1b-surface-spec.md §3) — same brink-only rule and the same
        // policy-independence `structs::check_duplicates` documents: a
        // statically-visible non-key-domain literal key is a structural
        // authoring mistake detectable from the literal alone, no shape
        // resolution or whole-project inference needed.
        out.extend(map_keys::check(&files));
        // NS-A3 protocol-registry name reservation (E113, F6 ruled
        // 2026-07-19, docs/stdlib-spec.md §9.6): `display`/`compare`/`next`
        // are reserved method names under the brink dialect — an author
        // declaration is a hard error, not an E035 warning. Brink-only:
        // under strict-ink there is no protocol registry and vanilla ink
        // identifiers stay untouched (the oracle corpus is out of reach by
        // construction).
        out.extend(protocols::check_reserved_names(&files));
    }
    out
}

/// Collect inline `///` docs across all files, keyed by `(kind, declared
/// name)` — the project-wide doc merge feeding the external/callable/value
/// enrichment passes. Exposed as its own seam (issue #750 / FG-3
/// completion) so `brink-db` can memoize it behind an `Eq`-cutoff query:
/// [`DocBlock`] carries no ranges, so any edit that leaves every `///`
/// block's parsed content intact backdates the memo even though the pass
/// reads every file's manifest.
#[must_use]
pub fn project_inline_docs(
    files: &[(FileId, &SymbolManifest)],
) -> BTreeMap<(SymbolKind, String), DocBlock> {
    collect_inline_docs(files)
}

/// The index-driven half of the external-check family (issue #750 / FG-3
/// completion): host-manifest enrichment + checks for `EXTERNAL`s
/// ([`external_check::analyze_externals`] — arity `E039`, unknown semantic
/// types `E040`) followed by knot/stitch doc enrichment
/// ([`external_check::enrich_callables`], same `E040` vocabulary), in
/// exactly that order for both the diagnostics and the `symbol_meta`
/// merge. Reads the index and the merged inline docs only — never any
/// file's HIR — which is what lets `brink-db` memoize it separately from
/// the per-file HIR walks ([`file_value_meta`] /
/// [`file_call_site_diagnostics`]).
#[must_use]
pub fn external_meta_diagnostics(
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
    opts: &AnalysisOptions,
) -> (BTreeMap<DefinitionId, SymbolMeta>, Vec<Diagnostic>) {
    let (types, registered) = manifest_maps(opts.host_manifest.as_ref());
    let has_manifest = opts.host_manifest.is_some();
    // Unknown-semantic-type checking (`E040`) is on when a manifest is
    // registered, or when the severity lever is explicitly raised to `Error`
    // (#532) — a host can opt back into strict checking with no manifest.
    let check_unknown_types =
        has_manifest || opts.semantic_type_check == SemanticTypeDiagnosticSeverity::Error;
    let (mut symbol_meta, mut diagnostics) = external_check::analyze_externals(
        index,
        inline_docs,
        &types,
        &registered,
        opts.external_check,
        check_unknown_types,
    );

    // Knot/stitch doc enrichment (presentational; shares the semantic-type
    // vocabulary, so unknown types still diagnose — but only once a manifest
    // is registered, or the severity lever is raised (#339/#532); see
    // `resolve_type`).
    let (callable_meta, callable_diags) = external_check::enrich_callables(
        index,
        inline_docs,
        &types,
        opts.external_check,
        check_unknown_types,
    );
    diagnostics.extend(callable_diags);
    symbol_meta.extend(callable_meta);

    (symbol_meta, diagnostics)
}

/// Project the external-kind entries of an enrichment map to a name-keyed
/// map for the call-site checks (issue #750 / FG-3 completion). Range-free
/// by construction ([`SymbolMeta`] carries no spans), so `brink-db` can put
/// an `Eq`-cutoff memo between the (often-invalidated, full-ranged-index-
/// reading) enrichment pass and every file's call-site walk — the
/// `resolution_index` playbook.
///
/// Fed [`external_meta_diagnostics`]'s output, this is identical to the
/// pre-split filter over the *fully merged* `symbol_meta`: the callable
/// ([`external_check::enrich_callables`]) and value
/// ([`external_check::infer_value_meta`]) passes only ever key
/// `Knot`/`Stitch` and `Variable`/`Constant`/`List` ids respectively, so no
/// entry they add can pass the `SymbolKind::External` filter here.
/// Same-name duplicates resolve identically too: iteration is in
/// `DefinitionId` order in both shapes, later entries overwriting.
#[must_use]
pub fn call_site_metas(
    index: &SymbolIndex,
    metas: &BTreeMap<DefinitionId, SymbolMeta>,
) -> BTreeMap<String, SymbolMeta> {
    metas
        .iter()
        .filter_map(|(id, meta)| {
            index.symbols.get(id).and_then(|s| {
                (s.kind == SymbolKind::External).then(|| (s.name.clone(), meta.clone()))
            })
        })
        .collect()
}

/// One file's VAR/CONST/LIST initializer/doc enrichment (issue #750 / FG-3
/// completion — the per-file slice of [`external_check::infer_value_meta`],
/// which `whole_project_diagnostics` used to run as one loop over every
/// file's HIR). Purely presentational — never produces diagnostics. A
/// declaration's initializer lives in exactly one file, so the per-file
/// split is behavior-neutral: the whole-project result is the file-order
/// merge of the per-file maps (later files overwrite on the — deliberately
/// deterministic — duplicate-name id collision, exactly as the single loop
/// did). Reads no symbol ranges from `index` (only `by_name` + `kind`), so
/// a range-zeroed index projection serves it.
#[must_use]
pub fn file_value_meta(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> BTreeMap<DefinitionId, SymbolMeta> {
    external_check::infer_value_meta(&[(file, hir)], index, inline_docs)
}

/// One file's external call-site literal checks (`E041` type mismatch,
/// `E042` closed domain) — the per-file slice of
/// [`external_check::check_call_sites`] (issue #750 / FG-3 completion).
/// The checker only ever reads the file it is visiting plus the name-keyed
/// external metas, so the per-file split is behavior-neutral; the caller
/// owns both the [`ExternalCheckSeverity`] gate and the file-order
/// concatenation the single whole-project walk produced.
#[must_use]
pub fn file_call_site_diagnostics(
    file: FileId,
    hir: &HirFile,
    metas: &BTreeMap<String, SymbolMeta>,
) -> Vec<Diagnostic> {
    let name_to_meta: BTreeMap<&str, &SymbolMeta> = metas
        .iter()
        .map(|(name, meta)| (name.as_str(), meta))
        .collect();
    external_check::check_call_sites(&[(file, hir)], &name_to_meta)
}

/// The M-2 module import + visibility checks (docs/modules-spec.md
/// §2/§4/§7): import well-formedness and cross-module `#@private`
/// reference enforcement. Purely additive — every trigger needs an
/// `IMPORT`/`#@private`/`#@public` construct absent from the pre-modules
/// world, so the oracle/tier1 corpus is untouched. Genuinely whole-project
/// (reads every file's HIR plus the project-wide resolutions to walk
/// cross-module references), so it stays a whole-project pass in
/// `brink-db`'s decomposed `whole_project_diagnostics_query` rather than
/// gaining a per-file split here (issue #750 / FG-3 completion rebase note;
/// a per-file slice is possible FG-4-era work if module churn is ever hot).
#[must_use]
pub fn module_diagnostics(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    modules::check(files, index, resolutions)
}

/// The strict typed-mode pass (docs/typed-mode-spec.md §1/§9-step-3),
/// extracted from `whole_project_diagnostics`'s body (issue #750 / FG-3
/// completion) so `brink-db` can run it without also paying for the
/// external-check family's inputs. Returns empty under `types = gradual` —
/// byte-identical, forever.
///
/// `types = strict` requires `dialect = brink` — a config error (`E064`)
/// otherwise, reported alone (nothing else strict-specific runs against a
/// project whose dialect already rejects the annotation syntax strict mode
/// needs). Under `dialect = brink`, runs inference (reusing
/// `strict_inference` when the caller already computed one — see
/// [`whole_project_diagnostics`]'s doc) and wires in Unknown/Conflicted-
/// escape (`E065`/`E066`) plus `E063` mismatches.
///
/// `inline_docs` (issue #805): forwarded to [`infer::infer_project`]'s own
/// `EXTERNAL`-signature seeding when `strict_inference` isn't already
/// supplied — the pure/self-contained fallback path only; `brink-db`'s
/// production seam always supplies `strict_inference` (its FG-narrowed
/// `type_inference_query`, which reads `inline_docs_query` itself through
/// `solve_scc_query`), so this parameter is inert there. Kept required
/// (rather than defaulted away) so the pure path stays composed-equals-
/// monolithic with the salsa one for every caller, not just the memoized
/// production one.
#[must_use]
pub fn strict_diagnostics(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    opts: &AnalysisOptions,
    strict_inference: Option<&InferenceResult>,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if opts.type_policy() == TypePolicy::Strict {
        if let Some(diag) = strict::config_error(opts.dialect, files.first().map(|&(f, _)| f)) {
            diagnostics.push(diag);
        } else {
            let owned_inference;
            let inference = if let Some(inf) = strict_inference {
                inf
            } else {
                owned_inference = infer::infer_project(
                    files,
                    index,
                    resolutions,
                    opts.host_manifest.as_ref(),
                    inline_docs,
                );
                &owned_inference
            };
            diagnostics.extend(strict::check(
                files,
                index,
                inference,
                resolutions,
                opts.host_manifest.as_ref(),
            ));
            // Issue #1004: escape-check each registered `EXTERNAL`
            // declaration's own param types against the manifest/inline-doc
            // signatures. `strict::check` above only walks `hir.knots`, so a
            // manifest-typed external param would otherwise never be verified
            // — resolved types stay clean, an unresolvable `ManifestParam.ty`
            // reports `E065` at the external's own declaration span. Seeded
            // from the same `collect_external_sigs` resolution that feeds
            // call-site argument checking; runs on this shared
            // `strict_diagnostics` seam so the pure `analyze_with_options`
            // path and `brink-db`'s query path get byte-identical output.
            let external_sigs =
                infer::collect_external_sigs(index, opts.host_manifest.as_ref(), inline_docs);
            diagnostics.extend(strict::check_external_escapes(index, &external_sigs));
        }
    }
    diagnostics
}

/// Whole-project diagnostic contributors that genuinely need cross-file
/// state (issue #632 / FG-3 design doc §1), now composed of the same
/// per-pass seams `brink-db`'s decomposed queries wrap (issue #750 / FG-3
/// completion) — [`module_diagnostics`], [`strict_diagnostics`],
/// [`external_meta_diagnostics`], per-file [`file_value_meta`], and per-file
/// [`file_call_site_diagnostics`] behind [`call_site_metas`] — in exactly
/// the pre-split order, so the query-composed result is identical to this
/// monolithic one by construction (pinned by `query_equivalence.rs`).
///
/// `strict_inference`: TM-3's strict pass needs a whole-project
/// [`InferenceResult`] (docs/typed-mode-spec.md §9-step-3 — E063 auto-wiring
/// "must run inference anyway"). Pass `None` to have this function compute
/// its own via [`infer_project`] (the self-contained default —
/// [`analyze_with_options`]'s pure, non-salsa path). Pass `Some` to reuse an
/// already-computed result instead — `brink-db` supplies its FG-narrowed,
/// per-SCC-memoized `type_inference_query` here so strict mode's
/// warm-reanalyze cost is the incremental one the FG spine exists for, not a
/// from-scratch whole-project solve on every keystroke. Ignored entirely
/// under `types = gradual` or when the dialect makes strict mode a config
/// error. The `types = strict` + wrong-dialect config error (`E064`) is
/// computed exactly once, inside [`strict_diagnostics`] (issue #632's
/// TM-3-interaction fence).
#[must_use]
pub fn whole_project_diagnostics(
    files: &[(FileId, &HirFile, &SymbolManifest)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    opts: &AnalysisOptions,
    strict_inference: Option<&InferenceResult>,
) -> (Vec<Diagnostic>, BTreeMap<DefinitionId, SymbolMeta>) {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|&(id, _hir, manifest)| (id, manifest))
        .collect();
    let hir_inputs: Vec<(FileId, &HirFile)> = files.iter().map(|&(id, hir, _)| (id, hir)).collect();

    // Computed once, up front (moved ahead of `strict_diagnostics`, issue
    // #805): both the TM-3 strict pass's `EXTERNAL`-signature seeding and
    // the host-manifest enrichment pass below need the project-wide merged
    // `///` doc map.
    let inline_docs = collect_inline_docs(&manifest_inputs);

    // M-2 module import + visibility checks (docs/modules-spec.md
    // §2/§4/§7), first in diagnostic order.
    let mut diagnostics = module_diagnostics(&hir_inputs, index, resolutions);

    // TM-3 strict typed-mode policy. Gradual mode returns empty here —
    // byte-identical, forever.
    diagnostics.extend(strict_diagnostics(
        &hir_inputs,
        index,
        resolutions,
        opts,
        strict_inference,
        &inline_docs,
    ));

    // Host-manifest enrichment + checks (tooling/author-time only) — the
    // index-driven half: externals (E039/E040), then callables.
    let (mut symbol_meta, ext_diags) = external_meta_diagnostics(index, &inline_docs, opts);
    diagnostics.extend(ext_diags);

    // Name-keyed external metas for the call-site checks — built before the
    // value-meta merge, which is identical to the pre-split post-merge
    // filter (see `call_site_metas`'s doc for the argument).
    let cs_metas = call_site_metas(index, &symbol_meta);

    // VAR/CONST initializer info + LIST docs (presentational, no
    // diagnostics), merged in file order.
    for &(file_id, hir, _) in files {
        symbol_meta.extend(file_value_meta(file_id, hir, index, &inline_docs));
    }

    // Call-site literal checks (type mismatch, closed domain) over the HIR,
    // in file order. Externals only — knot/stitch metadata is
    // presentational, not binding.
    if opts.external_check != ExternalCheckSeverity::Off {
        for &(file_id, hir, _) in files {
            diagnostics.extend(file_call_site_diagnostics(file_id, hir, &cs_metas));
        }
    }

    // T2-2 `#@effects(…)` exceedance check (docs/effects-spec.md §10, issue
    // #861) — brink-only, same TM-2 "content checks skip strict-ink"
    // precedent `per_file_diagnostics` documents (the directive is already
    // rejected whole by `dialect_gate`'s `E051` under strict-ink). Only pays
    // for `effects_project`'s whole-project inference when at least one
    // assertion actually exists anywhere in the project — an unannotated
    // project stays effects-inference-free, matching T2-1's advisory-only
    // posture.
    //
    // The FS-2 `await`-condition purity gate (E105,
    // docs/flow-suspension-spec.md §3/§5, issue #928) rides the same
    // whole-project effect table and the same brink-only + laziness posture:
    // it needs `effects_project`'s rows to judge a condition's transitive
    // effect, so both passes share one inference when *either* an `#@effects`
    // assertion or an `await` appears anywhere in the project.
    // The NS-A4 comparator-contract gate (E119, docs/stdlib-spec.md §4b,
    // issue #1110) rides the same whole-project effect table with the same
    // brink-only + laziness posture: a project with no `sort_by`/
    // `sorted_by`-with-inline-`#fn` site never triggers effect inference
    // for it.
    let needs_effects = hir_inputs.iter().any(|&(_, hir)| {
        hir_has_effects_assertion(hir)
            || await_purity::hir_has_await(hir)
            || comparator_contract::hir_has_comparator_site(hir)
    });
    if opts.dialect == Dialect::Brink && needs_effects {
        let rows =
            infer::effects_project(&hir_inputs, index, resolutions, opts.host_manifest.as_ref());
        for &(file_id, hir) in &hir_inputs {
            // Import-scoped resolution (issue #881, the T2 follow-up to
            // M-2d/#790): the assertion's own `reads`/`writes`/`calls` clause
            // names must resolve through this file's own declared module +
            // imports, exactly like every other reference does — see
            // `effects_assertions::check`'s doc.
            let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
            diagnostics.extend(effects_assertions::check(
                file_id, hir, index, &scope, &rows,
            ));
            // The `await` purity gate resolves each condition's calls through
            // this file's own resolution records (`resolutions` carries file
            // provenance, filtered inside `await_purity::check`).
            diagnostics.extend(await_purity::check(file_id, hir, index, resolutions, &rows));
            // The NS-A4 comparator-contract gate (E119) — same resolution
            // discipline, judging inline `#fn(target)` comparators of
            // `sort_by`/`sorted_by` against their target's row.
            diagnostics.extend(comparator_contract::check(
                file_id,
                hir,
                index,
                resolutions,
                &rows,
            ));
        }
    }

    (diagnostics, symbol_meta)
}

/// Cheap structural scan: does any knot/stitch in `hir` carry a
/// `#@effects(…)` assertion? The laziness gate for
/// [`whole_project_diagnostics`]'s exceedance pass — avoids running
/// [`infer::effects_project`] at all for a project that never uses the
/// directive.
fn hir_has_effects_assertion(hir: &HirFile) -> bool {
    hir.knots.iter().any(|k| {
        k.effects_assertion.is_some() || k.stitches.iter().any(|s| s.effects_assertion.is_some())
    })
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
            opts.host_manifest.as_ref(),
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

    /// The dialect-keyed default (issue #1127, ruled 2026-07-19). Under
    /// `strict-ink`, an unset `types` resolves gradual FOREVER — the same
    /// source, `types` never set, must produce results identical to a build
    /// that predates TM-3 entirely: no `E064`/`E065`/`E066`, and `E063`
    /// stays un-auto-invoked (the #618/PR#640 ruling, untouched — the
    /// oracle corpus is anchored to this). Under `brink`, the same unset
    /// `types` now resolves strict, so the Unknown-escape check fires;
    /// explicit `Gradual` remains the opt-out knob and restores silence.
    #[test]
    fn types_default_is_dialect_keyed() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);

        // strict-ink + unset types: gradual, byte-identical forever.
        let opts = AnalysisOptions {
            dialect: Dialect::StrictInk,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "strict-ink (default types = gradual) must stay silent: {:?}",
            result.diagnostics
        );

        // brink + unset types: strict — the Unknown-escape check fires.
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == brink_ir::DiagnosticCode::E065),
            "brink (default types = strict) must flag the Unknown escape: {:?}",
            result.diagnostics
        );

        // brink + explicit gradual: the opt-out knob restores silence.
        let opts = AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Gradual),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            result.diagnostics.is_empty(),
            "brink + explicit gradual opt-out must stay silent: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn strict_with_strict_ink_dialect_is_a_config_error_and_nothing_else_runs() {
        let src = "=== noop(x) ===\nHello.\n-> DONE\n";
        let (hir, manifest) = lower_one(src);
        let opts = AnalysisOptions {
            dialect: Dialect::StrictInk,
            types: Some(TypePolicy::Strict),
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
            types: Some(TypePolicy::Strict),
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
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        };
        let result = analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }
}
