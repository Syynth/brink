//! The `analysis_query` family (issue #632 / FG-3), extracted out of
//! `queries.rs` per issue #662: pure code movement, no semantic change, no
//! signature change. See the parent module's doc comment for how these fit
//! into the overall query-shaped pipeline.

// ─── FG-3 (issue #632): decomposed analysis_query ─────────────────────
//
// `analysis_query`'s only cutoff used to be `PartialEq` over the whole
// `AnalysisResult` — index, resolutions, diagnostics (range-laden), and
// symbol_meta bundled into one struct — so it almost never backdated, and
// every file's validate/dialect_gate/annotation-content checks re-ran on
// nearly any edit, since they were three whole-project passes each looping
// every file. This section splits that into:
//
// - [`resolutions_index_query`] — index + resolutions, no diagnostics: the
//   RESOLUTIONS/INDEX half.
// - [`per_file_diagnostics_query`] / [`contributor_diagnostics_query`] — the
//   genuinely per-file diagnostic contributors (validate, dialect_gate,
//   annotation content checks) behind a thin whole-project aggregator, so a
//   body edit re-runs only the edited file's own contributor.
// - [`whole_project_diagnostics_query`] — now a thin aggregator (issue
//   #750, FG-3 completion): the external-check family is decomposed into
//   [`inline_docs_query`] / [`external_meta_query`] /
//   [`call_site_metas_query`] and the per-file [`value_meta_query`] /
//   [`call_site_diagnostics_query`]; only the M-2 modules pass and the
//   strict typed-mode pass remain genuinely whole-project — reading the
//   narrow [`resolutions_index_query`] projection and the
//   already-FG-2/FG-2.1-narrowed `type_inference_query`, never the
//   diagnostics-laden bundle.
// - [`analysis_diagnostics_query`] — the DIAGNOSTICS half: every diagnostic
//   source merged, in the same order `finish_analysis` produces them, so
//   `db.analysis()` stays output-identical to the monolithic, module-aware
//   `brink_analyzer::analyze_with_modules` path (pinned by
//   `query_equivalence.rs`) — only equal to the module-*blind*
//   `analyze_with_options` for ink projects without a declared `#@module`,
//   see `ProjectDb::module_map`'s doc (issue #1526).
// - [`analysis_query`] — kept as a thin assembler over the above three for
//   `db.analysis()`'s existing LSP/IDE/CLI-facing `AnalysisResult` shape.
//   [`diagnostics_query`] and [`lir_query`] read
//   [`analysis_diagnostics_query`]/[`resolutions_index_query`] directly
//   instead of through this bundle, so a diagnostics-only edit never forces
//   a resolutions-only reader to recompute and vice versa.

use std::collections::BTreeMap;
use std::sync::Arc;

use brink_analyzer::{
    AnalysisResult, ExternalCheckSeverity, SymbolMeta, TypePolicy, register_intrinsic_diagnostics,
};
use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DocBlock, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolKind, SymbolManifest,
};

use crate::determinism::LookupSet;

use super::{
    DefKey, ProjectInput, SourceFile, effects_query, inference_index_query, lowered_query,
    module_map_query, resolution_index_query, resolve_query, symbol_index_query,
    type_inference_query,
};

/// Index + resolutions, aggregated across every file's [`resolve_query`]
/// (issue #632 / FG-3) — deliberately without diagnostics, so this struct's
/// `PartialEq` never touches a diagnostic's range. Neither
/// [`symbol_index_query`] nor any file's [`resolve_query`] reads
/// `project.analysis_options`, so an `AnalysisOptions` edit that only
/// changes diagnostics (e.g. raising `semantic_type_check` to `Error`) never
/// even triggers salsa to re-run this query's closure — not just a
/// backdate, a full skip (pinned by `fg3_dependency_edges.rs`).
///
/// `Arc`-wrapped (design doc §2 Fork 2's "Arc<plain>" ruling, applied here):
/// pointer identity is the observable a re-execution-vs-cutoff test needs —
/// see `fg3_dependency_edges.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProject {
    pub index: Arc<SymbolIndex>,
    pub resolutions: ResolutionMap,
}

#[salsa::tracked]
pub(crate) fn resolutions_index_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<ResolvedProject> {
    let (index, _diags) = symbol_index_query(db, project).clone();
    let mut resolutions = ResolutionMap::new();
    for file in project.files(db) {
        let (file_map, _file_diags) = resolve_query(db, project, *file);
        resolutions.extend(file_map.iter().cloned());
    }
    Arc::new(ResolvedProject { index, resolutions })
}

/// One file's per-file diagnostic contributors (issue #632 / FG-3 design doc
/// §1 item 4 — [`brink_analyzer::per_file_diagnostics`]): structural
/// validation, the dialect gate, and (brink dialect only) annotation-content
/// checks. Reads only this file's own `lowered_query`/`resolve_query`, plus
/// the narrow, cutoff-friendly [`resolution_index_query`] projection (for
/// annotation content checks' declared-`LIST`-name lookup — range-free, so
/// it doesn't reintroduce whole-project churn) and the registered host
/// manifest (T1d-2, docs/t1d-spec.md §3 — `Handle<K>` annotation content
/// checks' declared-handle-kind lookup). The manifest is project-wide,
/// host-set config, not derived from any file's edits — reading it here is
/// the same coarse, range-free dependency shape as `dialect`, already read
/// two lines below, so it doesn't reintroduce the whole-project churn FG-3
/// eliminated. Never another file's HIR: a body edit in file Y leaves file
/// X's memo fully validated (same `Arc`/pointer), not re-executed.
/// `Arc`-wrapped for the same pointer-identity reason as [`ResolvedProject`].
///
/// Also the B0.9 native strict-only enforcement point
/// ([`brink_analyzer::native_strict_only_error`], issue #1342): this is the
/// narrowest seam that has both a file's own [`super::Language`]
/// classification (`super::file_language`) and `AnalysisOptions` access —
/// `super::lower_native_file` has neither (issue #1179's finding), so the
/// check cannot live there. Reading `opts.types` here doesn't widen this
/// query's dependency edge: `opts` (the whole `AnalysisOptions`) is already
/// read for `dialect`/`host_manifest` above.
///
/// Same seam decouples the T1b dialect gate from native files (issue #1348):
/// `dialect` is an ink-only axis (docs/t1b-surface-spec.md §1), orthogonal to
/// this file's [`super::Language`] classification, so
/// `brink_analyzer::per_file_diagnostics`'s `is_native` flag — computed here,
/// once, and reused for both calls below — skips the gate for a native file
/// exactly the way `native_strict_only_error` above is native-conditional.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 4096)]
pub(crate) fn per_file_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    let file_id = file.file_id(db);
    let hir = &lowered_query(db, file).hir;
    let (file_resolutions, _diags) = resolve_query(db, project, file);
    let index = resolution_index_query(db, project);
    let opts = project.analysis_options(db);
    let is_native = super::file_language(file.path(db)) == super::Language::Native;
    let mut diagnostics = brink_analyzer::per_file_diagnostics(
        file_id,
        hir,
        file_resolutions,
        index,
        opts.dialect,
        is_native,
        opts.host_manifest.as_ref(),
    );
    if is_native {
        diagnostics.extend(brink_analyzer::native_strict_only_error(
            file_id, opts.types,
        ));
    }
    Arc::new(diagnostics)
}

/// Aggregated per-file diagnostic contributors across the whole project
/// (issue #632 / FG-3 — "a thin aggregator" per the design doc). The loop
/// itself is cheap: each iteration is a salsa memo lookup, not a HIR walk —
/// [`per_file_diagnostics_query`]'s actual `validate`/`dialect_gate`/
/// annotation-content work only re-runs for the file(s) whose own
/// dependencies changed.
#[salsa::tracked(returns(ref))]
pub(crate) fn contributor_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for file in project.files(db) {
        out.extend(
            per_file_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }
    out
}

/// The project-wide inline `///` doc merge (issue #750 / FG-3 completion —
/// [`brink_analyzer::project_inline_docs`]), keyed by `(kind, declared
/// name)`. Reads every file's manifest, but the output is range-free
/// ([`DocBlock`] carries parsed doc content only), so any edit that leaves
/// every `///` block intact backdates this memo — the `Eq`-cutoff seam
/// between per-file manifest churn and the doc-consuming enrichment passes
/// ([`external_meta_query`], [`value_meta_query`]).
#[salsa::tracked(returns(ref))]
pub(crate) fn inline_docs_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<BTreeMap<(SymbolKind, String), DocBlock>> {
    let manifest_inputs: Vec<(FileId, &SymbolManifest)> = project
        .files(db)
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).manifest))
        .collect();
    Arc::new(brink_analyzer::project_inline_docs(&manifest_inputs))
}

/// The index-driven half of the external-check family (issue #750 / FG-3
/// completion — [`brink_analyzer::external_meta_diagnostics`]): host-
/// manifest enrichment + checks for externals (`E039`/`E040`) plus
/// knot/stitch doc enrichment. Reads the *full ranged* [`symbol_index_query`]
/// (diagnostic spans need real ranges) and [`inline_docs_query`] — never any
/// file's HIR, which is the decomposition's point: the pre-#750 shape ran
/// this inside a query that also walked every file's HIR, so any body edit
/// re-ran the whole family. Cheap to re-execute (proportional to
/// externals/callables, no HIR walk); its range-free `symbol_meta` half
/// backdates dependents via [`call_site_metas_query`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExternalMeta {
    pub symbol_meta: BTreeMap<DefinitionId, SymbolMeta>,
    pub diagnostics: Vec<Diagnostic>,
}

#[salsa::tracked(returns(ref))]
pub(crate) fn external_meta_query(db: &dyn salsa::Database, project: ProjectInput) -> ExternalMeta {
    let (index, _diags) = symbol_index_query(db, project);
    let inline_docs = inline_docs_query(db, project);
    let opts = project.analysis_options(db);
    let (symbol_meta, diagnostics) =
        brink_analyzer::external_meta_diagnostics(index, inline_docs, opts);
    ExternalMeta {
        symbol_meta,
        diagnostics,
    }
}

/// Name-keyed external metas for the call-site checks (issue #750 / FG-3
/// completion — [`brink_analyzer::call_site_metas`]): the range-free
/// projection of [`external_meta_query`]'s enrichment map, filtered to
/// `SymbolKind::External`. This is the cutoff seam guarding every file's
/// [`call_site_diagnostics_query`] memo (the `resolution_index` playbook):
/// a body edit shifts declaration ranges → the full index changes →
/// [`external_meta_query`] re-executes — but as long as no external's
/// *content* (docs/manifest/params) changed, this projection comes out
/// `Eq`, and every other file's call-site memo stays fully validated
/// without re-executing. `Arc`-wrapped for the same pointer-identity
/// reason as [`per_file_diagnostics_query`].
#[salsa::tracked]
pub(crate) fn call_site_metas_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<BTreeMap<String, SymbolMeta>> {
    let (index, _diags) = symbol_index_query(db, project);
    let ext = external_meta_query(db, project);
    Arc::new(brink_analyzer::call_site_metas(index, &ext.symbol_meta))
}

/// One file's VAR/CONST/LIST initializer/doc enrichment (issue #750 / FG-3
/// completion — [`brink_analyzer::file_value_meta`]): purely presentational
/// `symbol_meta` entries, no diagnostics. Reads only this file's own
/// `lowered_query`, the range-zeroed [`inference_index_query`] projection
/// (the pass reads `by_name` + `kind`, never a symbol's range — see the
/// analyzer seam's doc), and [`inline_docs_query`] — so a body edit in file
/// Y leaves file X's memo fully validated (same `Arc`), not re-executed.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 4096)]
pub(crate) fn value_meta_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<BTreeMap<DefinitionId, SymbolMeta>> {
    let hir = &lowered_query(db, file).hir;
    let index = inference_index_query(db, project);
    let inline_docs = inline_docs_query(db, project);
    Arc::new(brink_analyzer::file_value_meta(
        file.file_id(db),
        hir,
        index,
        inline_docs,
    ))
}

/// One file's external call-site literal checks (`E041`/`E042`) — issue
/// #750 / FG-3 completion, [`brink_analyzer::file_call_site_diagnostics`].
/// Reads only this file's own `lowered_query` plus the range-free
/// [`call_site_metas_query`] projection, so a body edit in file Y leaves
/// file X's memo fully validated (same `Arc`), not re-executed — the last
/// per-file HIR walk `finish_analysis` still ran project-wide. Empty when
/// the `external_check` severity is `Off` (the same gate the monolithic
/// path applies before walking any file).
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 4096)]
pub(crate) fn call_site_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    if project.analysis_options(db).external_check == ExternalCheckSeverity::Off {
        return Arc::new(Vec::new());
    }
    let metas = call_site_metas_query(db, project);
    let hir = &lowered_query(db, file).hir;
    Arc::new(brink_analyzer::file_call_site_diagnostics(
        file.file_id(db),
        hir,
        &metas,
    ))
}

/// One file's `#@effects(…)` exceedance diagnostics (T2-2,
/// docs/effects-spec.md §10, issue #861). Brink-only, same TM-2
/// content-check precedent [`per_file_diagnostics_query`]'s doc cites: under
/// `strict-ink` the directive is already rejected whole by `dialect_gate`'s
/// `E051`, so checking its declared names here would be noise.
///
/// Reads only the def ids [`brink_analyzer::effects_assertion_defs`] finds
/// in *this file's* HIR (a structural scan — no inference triggered by the
/// scan itself) and, for exactly those defs, the salsa-memoized per-def
/// [`effects_query`]. A file with no `#@effects` directive at all never
/// calls `effects_query`, so an unannotated project stays effect-inference-
/// free — T2-1's advisory/lazy posture, preserved.
///
/// The assertion's `reads`/`writes`/`calls` clause names are resolved
/// through this file's own [`brink_analyzer::ImportScope`] (issue #881, the
/// T2 follow-up to M-2d/#790), built from [`module_map_query`] + this file's
/// own `IMPORT`s exactly like [`resolve_query`] builds it — so the checker
/// can never attribute a clause to a different declared module's same-name
/// cell than the one this file's own resolution binds.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 4096)]
pub(crate) fn effects_assertion_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    if project.analysis_options(db).dialect != brink_analyzer::Dialect::Brink {
        return Arc::new(Vec::new());
    }
    let file_id = file.file_id(db);
    let hir = &lowered_query(db, file).hir;
    let index = resolution_index_query(db, project);
    let def_ids = brink_analyzer::effects_assertion_defs(hir, index, file_id);
    if def_ids.is_empty() {
        return Arc::new(Vec::new());
    }
    let mut rows = BTreeMap::new();
    for id in def_ids {
        if let Some(row) = effects_query(db, project, DefKey::new(db, id)) {
            rows.insert(id, (*row).clone());
        }
    }
    let (module_map, _module_diags) = module_map_query(db, project);
    let file_module = module_map
        .get(&file_id)
        .filter(|m| m.declared)
        .map(|m| m.name.clone());
    let scope = brink_analyzer::ImportScope::new(file_module, &hir.imports);
    Arc::new(brink_analyzer::effects_assertion_diagnostics(
        file_id, hir, index, &scope, &rows,
    ))
}

/// One file's FS-2 `await`-condition purity diagnostics (E105,
/// docs/flow-suspension-spec.md §3/§5, issue #928). Brink-only + lazy, the
/// same posture as [`effects_assertion_diagnostics_query`]: a file with no
/// `await` never fetches a single per-def effect row, so an await-free project
/// stays effect-inference-free.
///
/// Unlike the assertion query (which knows its target defs up front), the
/// callees a condition names are discovered by resolving the condition's
/// calls ([`brink_analyzer::await_condition_callees`]); each is judged by its
/// salsa-memoized per-def [`effects_query`] row — the incremental analogue of
/// the monolithic path's whole-project `effects_project` table.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 4096)]
pub(crate) fn await_purity_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    if project.analysis_options(db).dialect != brink_analyzer::Dialect::Brink {
        return Arc::new(Vec::new());
    }
    let file_id = file.file_id(db);
    let hir = &lowered_query(db, file).hir;
    if !brink_analyzer::hir_has_await(hir) {
        return Arc::new(Vec::new());
    }
    let (file_resolutions, _diags) = resolve_query(db, project, file);
    let index = resolution_index_query(db, project);
    let callee_defs = brink_analyzer::await_condition_callees(file_id, hir, file_resolutions);
    let mut rows = BTreeMap::new();
    for id in callee_defs {
        if let Some(row) = effects_query(db, project, DefKey::new(db, id)) {
            rows.insert(id, (*row).clone());
        }
    }
    Arc::new(brink_analyzer::await_purity_diagnostics(
        file_id,
        hir,
        index,
        file_resolutions,
        &rows,
    ))
}

/// One file's conventions-module confinement diagnostics (`E169`, issue
/// #1844 — the MODULE half of the 2026-07-31 §9.1 ruling's item (4); #1838/
/// #1847 cover the *placement* half, `E112`). A pattern-claiming
/// `@[convention(claims = "…", order = N)]` handler is legal only in the project's
/// configured conventions module (`brink.toml`'s `[project] elements`);
/// this is the one seam that has both a file's real module identity
/// ([`module_map_query`]'s native branch, `crate::modules::
/// native_module_path`) and the resolved `AnalysisOptions` the pointer
/// travels on — `brink_analyzer::analyze_with_modules` is not it, since
/// `brink-db` never calls that path (issue #1863's own finding).
///
/// Lazy in the same shape as [`await_purity_diagnostics_query`]/
/// [`comparator_contract_diagnostics_query`]: a file with no declared claim
/// handler never even reads [`module_map_query`]. Three more cases are
/// intentionally silent, not merely lazy — see
/// `brink_analyzer::conventions_module_diagnostics`'s own module doc for
/// why: an unset `elements` key (nothing configured to confine against
/// yet), a bare preset name (`elements = "screenplay"`, which names a
/// `std::conventions::*` module rather than a project file — no path in
/// the tree to compare against without a preset registry this slice
/// doesn't build), and a path-shaped pointer that resolves to no file that
/// actually exists in `project.files(db)` (a typo, a moved/deleted target,
/// an `.ink`-suffixed path) — that last case is checked HERE, against
/// [`module_map_query`]'s real module set, before any file is compared
/// against it; otherwise every claiming handler in the project would be
/// flagged for not living in a file that was never there to begin with.
/// Reported via `tracing::warn!` (the same "warn, never silently drop"
/// channel `resolve_options` uses for `ConfigWarning`s) rather than
/// silently dropped.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647), matching
/// every other per-file diagnostic query in this module.
#[salsa::tracked(lru = 4096)]
pub(crate) fn conventions_confinement_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    let hir = &lowered_query(db, file).hir;
    if hir.claim_handlers.is_empty() {
        return Arc::new(Vec::new());
    }
    let opts = project.analysis_options(db);
    let Some(pointer) = opts.elements.as_deref() else {
        return Arc::new(Vec::new());
    };
    if !brink_analyzer::is_path_shaped_elements_pointer(pointer) {
        return Arc::new(Vec::new());
    }
    let file_id = file.file_id(db);
    let (module_map, _module_diags) = module_map_query(db, project);
    let Some(this_module) = module_map.get(&file_id).map(|m| m.name.as_str()) else {
        return Arc::new(Vec::new());
    };
    let native_root = project.native_root(db).as_deref();
    let expected_module = crate::modules::native_module_path(&crate::modules::root_relative_key(
        native_root,
        pointer,
    ));
    // The pointer must resolve against a REAL file in the project before it
    // can confine anything. A typo'd `elements` value, a moved/deleted
    // target, or an `.ink`-suffixed path all produce an `expected_module`
    // no file actually has — without this check, every claiming handler in
    // the project (including the one in the real intended conventions
    // module) would be flagged at `E169`, telling the author to move it
    // into a file that does not exist, with no signal that the config
    // itself is at fault. `module_map`'s iteration order can't affect this
    // check: `any` only asks whether *some* file matches, never which one.
    if !module_map.values().any(|m| m.name == expected_module) {
        // Same "warn, never silently drop" channel `resolve_options` uses
        // for `ConfigWarning`s (house rule) — the pointer problem is
        // surfaced, just not as an `E169` storm against files that were
        // never the ones at fault.
        tracing::warn!(
            "[project] elements = \"{pointer}\" does not match any file in the project \
             (expected module `{expected_module}`) — conventions-module confinement (E169) \
             is skipped until this is fixed"
        );
        return Arc::new(Vec::new());
    }
    let is_conventions_module = this_module == expected_module;
    Arc::new(brink_analyzer::conventions_module_diagnostics(
        file_id,
        hir,
        is_conventions_module,
        pointer,
    ))
}

/// One file's `register`-intrinsic confinement diagnostics (`E175`, issue
/// #1840 Q5 — the *legality* half of the 2026-08-02 "`register` is a
/// comptime-only intrinsic" ruling). `register` is legal only inside the
/// project's configured conventions module's `fn conventions()`.
///
/// Lazy the same way [`conventions_confinement_diagnostics_query`] is: a
/// file with no unresolved `register(...)` call anywhere never reads
/// [`module_map_query`]. **Deliberately NOT the same early-outs as that
/// query for the "unconfigured" cases** — see
/// `brink_analyzer::register_intrinsic_diagnostics`'s own module doc for
/// why: `register`'s legality is a language-level restriction, not a
/// project-configuration-dependent one, so "no conventions module
/// configured at all" must resolve to `is_conventions_module: false` (every
/// call in the project is then illegal), never to "skip this file
/// entirely". This intentionally duplicates
/// [`conventions_confinement_diagnostics_query`]'s ~10-line `is_conventions_
/// module` resolution rather than sharing it, to keep the two queries'
/// differing "unconfigured" postures from ever being tempted to drift back
/// together by a well-meaning refactor.
///
/// Threads this file's own [`resolve_query`] output through to
/// `register_intrinsic_diagnostics` — a call whose range resolved to a real
/// symbol (same-file or cross-file declaration, or a local temp/param of
/// the same name) is a shadow, never the intrinsic, exactly the same
/// resolution-map-backed check `dialect_gate::check` already does for
/// every other T1b stdlib name.
///
/// `lru = 4096`: same per-file runaway-guard ceiling as every other
/// per-file diagnostic query in this module.
#[salsa::tracked(lru = 4096)]
pub(crate) fn register_intrinsic_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    let hir = &lowered_query(db, file).hir;
    let file_id = file.file_id(db);
    let (file_resolutions, _diags) = resolve_query(db, project, file);

    // Cheap pre-check (mirrors `conventions_confinement_diagnostics_query`'s
    // own laziness): skip `module_map_query` entirely when this file can
    // never produce a diagnostic no matter how `is_conventions_module`
    // resolves. `is_conventions_module: false` is the maximal/superset case
    // — every structurally-found, not-already-resolved-to-a-real-symbol
    // call gets flagged — so an empty result here means `true` would be
    // empty too, and it's safe to skip project resolution entirely.
    if register_intrinsic_diagnostics(file_id, hir, false, file_resolutions.as_ref()).is_empty() {
        return Arc::new(Vec::new());
    }

    let opts = project.analysis_options(db);
    let is_conventions_module = 'resolved: {
        let Some(pointer) = opts.elements.as_deref() else {
            // No conventions module configured at all: there is no
            // possible legal placement for `register` anywhere in the
            // project.
            break 'resolved false;
        };
        if !brink_analyzer::is_path_shaped_elements_pointer(pointer) {
            // A bare preset name (`elements = "screenplay"`) names no
            // project file — same conclusion.
            break 'resolved false;
        }
        let (module_map, _module_diags) = module_map_query(db, project);
        let Some(this_module) = module_map.get(&file_id).map(|m| m.name.as_str()) else {
            break 'resolved false;
        };
        let native_root = project.native_root(db).as_deref();
        let expected_module = crate::modules::native_module_path(
            &crate::modules::root_relative_key(native_root, pointer),
        );
        if !module_map.values().any(|m| m.name == expected_module) {
            // A path-shaped pointer resolving to no real file. E169's own
            // query already warns for this (`tracing::warn!`) whenever a
            // claiming handler exists anywhere in the project — not
            // duplicated here to avoid two warnings for one misconfigured
            // pointer.
            break 'resolved false;
        }
        this_module == expected_module
    };

    Arc::new(register_intrinsic_diagnostics(
        file_id,
        hir,
        is_conventions_module,
        file_resolutions.as_ref(),
    ))
}

/// One file's NS-A4 comparator-contract diagnostics (E119,
/// docs/stdlib-spec.md §4b, issue #1110 — extended to the fn-value verb
/// trio `map`/`filter`/`fold` by issue #1679, §4): `sort_by`/`sorted_by`/
/// `map`/`filter`/`fold` calls whose callback's row — named either by an
/// inline `#fn(target)` literal (ink/brink) or, since issue #1887, a
/// native bare-name reference — provably exceeds pure·silent. Brink-only
/// + lazy, the exact
/// [`await_purity_diagnostics_query`] shape: a file with no such site never
/// fetches a single per-def effect row, so a callback-free project stays
/// effect-inference-free.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 4096)]
pub(crate) fn comparator_contract_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<Vec<Diagnostic>> {
    if project.analysis_options(db).dialect != brink_analyzer::Dialect::Brink {
        return Arc::new(Vec::new());
    }
    let file_id = file.file_id(db);
    let hir = &lowered_query(db, file).hir;
    if !brink_analyzer::hir_has_comparator_site(hir) {
        return Arc::new(Vec::new());
    }
    let (file_resolutions, _diags) = resolve_query(db, project, file);
    let index = resolution_index_query(db, project);
    let callee_defs = brink_analyzer::comparator_callees(file_id, hir, index, file_resolutions);
    let mut rows = BTreeMap::new();
    for id in callee_defs {
        if let Some(row) = effects_query(db, project, DefKey::new(db, id)) {
            rows.insert(id, (*row).clone());
        }
    }
    Arc::new(brink_analyzer::comparator_contract_diagnostics(
        file_id,
        hir,
        index,
        file_resolutions,
        &rows,
    ))
}

/// Whole-project diagnostics + `symbol_meta` (issue #632 / FG-3 design doc
/// §1), now a thin aggregator (issue #750 / FG-3 completion) over the
/// decomposed external-check family — [`external_meta_query`] + per-file
/// [`value_meta_query`] / [`call_site_diagnostics_query`] — plus the two
/// genuinely whole-project passes left: the M-2 module import/visibility
/// checks ([`brink_analyzer::module_diagnostics`], which need every file's
/// HIR plus the project-wide resolutions) and, under `types = strict`, the
/// strict typed-mode checks ([`brink_analyzer::strict_diagnostics`], which
/// need a whole-project [`InferenceResult`] — the FG-4-era candidate for a
/// per-SCC-reading split, out of #750's scope). The aggregation loops are
/// salsa memo lookups, not HIR walks: a body edit in file Y re-runs only
/// Y's own value-meta/call-site contributors (plus the modules pass, which
/// post-dates #750's decomposition — M-1/M-2 landed while this slice was in
/// flight and is per-file-splittable follow-up work if it ever shows up
/// hot).
///
/// [`InferenceResult`]: brink_analyzer::InferenceResult
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WholeProjectDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub symbol_meta: BTreeMap<DefinitionId, SymbolMeta>,
}

#[salsa::tracked(returns(ref))]
pub(crate) fn whole_project_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> WholeProjectDiagnostics {
    let opts = project.analysis_options(db);
    let resolved = resolutions_index_query(db, project);
    let hir_refs: Vec<(FileId, &HirFile)> = project
        .files(db)
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();

    // M-2 module import + visibility checks (docs/modules-spec.md
    // §2/§4/§7), first in diagnostic order (matching
    // `whole_project_diagnostics`'s monolithic composition).
    let mut diagnostics =
        brink_analyzer::module_diagnostics(&hir_refs, &resolved.index, &resolved.resolutions);

    // TM-3 strict typed-mode pass (docs/typed-mode-spec.md §9-step-3),
    // second in diagnostic order. Under `types = strict` + `dialect =
    // brink`, reuse the already-memoized, FG-narrowed
    // `type_inference_query` instead of letting the analyzer recompute
    // inference from scratch via `infer_project` — the "inference finally
    // has a consumer" seam the per-def/per-SCC decomposition (FG-2, FG-2.1)
    // exists for. Gradual mode (the default) skips this block entirely.
    if opts.type_policy() == TypePolicy::Strict {
        let strict_inference = (opts.dialect == brink_analyzer::Dialect::Brink)
            .then(|| type_inference_query(db, project).as_ref());
        // `strict_inference` is always `Some` here whenever `dialect =
        // brink` (the only case `strict_diagnostics`'s own fallback would
        // otherwise run `infer_project`), so `inline_docs_query` is read
        // for uniformity with the pure path's signature (issue #805) —
        // `type_inference_query` -> `solve_scc_query` already reads the
        // same memo for the actual `EXTERNAL`-signature seeding.
        let inline_docs = inline_docs_query(db, project);
        // `is_native` (issue #1348): `dialect` is an ink-only axis — a
        // native project has no dialect to be wrong about, so the ink-only
        // `E064` config error must never fire for one. Same
        // `super::project_is_native` seam `compilation_closure_files` itself
        // uses to decide the frontend for the whole compilation unit.
        diagnostics.extend(brink_analyzer::strict_diagnostics(
            &hir_refs,
            &resolved.index,
            &resolved.resolutions,
            opts,
            super::project_is_native(db, project),
            strict_inference,
            inline_docs,
        ));
    }

    // Externals + callables (index-driven, memoized without HIR deps), then
    // per-file value metas (file order), then per-file call-site checks
    // (file order) — exactly `brink_analyzer::whole_project_diagnostics`'s
    // own composition order.
    let ext = external_meta_query(db, project);
    diagnostics.extend(ext.diagnostics.iter().cloned());
    let mut symbol_meta = ext.symbol_meta.clone();

    for file in project.files(db) {
        symbol_meta.extend(
            value_meta_query(db, project, *file)
                .iter()
                .map(|(k, v)| (*k, v.clone())),
        );
    }
    for file in project.files(db) {
        diagnostics.extend(
            call_site_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }
    // T2-2 `#@effects(…)` exceedance check (docs/effects-spec.md §10, issue
    // #861) — per-file, lazy (see `effects_assertion_diagnostics_query`'s
    // doc): a project with no `#@effects` directive never triggers effect
    // inference here.
    for file in project.files(db) {
        diagnostics.extend(
            effects_assertion_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }
    // FS-2 `await`-condition purity gate (E105,
    // docs/flow-suspension-spec.md §3/§5, issue #928) — per-file, lazy (see
    // `await_purity_diagnostics_query`'s doc): an await-free project never
    // triggers effect inference here.
    for file in project.files(db) {
        diagnostics.extend(
            await_purity_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }
    // NS-A4 comparator-contract gate (E119, docs/stdlib-spec.md §4b, issue
    // #1110 — extended to the fn-value verb trio `map`/`filter`/`fold` by
    // issue #1679, §4) — per-file, lazy (see
    // `comparator_contract_diagnostics_query`'s doc): a project with no
    // inline-`#fn` comparator/callback site never triggers effect
    // inference here.
    for file in project.files(db) {
        diagnostics.extend(
            comparator_contract_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }
    // Conventions-module confinement gate (E169, issue #1844) — per-file,
    // lazy (see `conventions_confinement_diagnostics_query`'s doc): a file
    // with no declared claim handler never even reads `module_map_query`.
    for file in project.files(db) {
        diagnostics.extend(
            conventions_confinement_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }
    // `register`-intrinsic confinement gate (E175, issue #1840 Q5) —
    // per-file, lazy (see `register_intrinsic_diagnostics_query`'s doc): a
    // file with no `register(...)` call never even reads
    // `module_map_query`.
    for file in project.files(db) {
        diagnostics.extend(
            register_intrinsic_diagnostics_query(db, project, *file)
                .iter()
                .cloned(),
        );
    }

    // B3a UFCS resolution (issue #1482, D1–D5 RULED 2026-07-26) — last,
    // matching `brink_analyzer::whole_project_diagnostics`' own composition
    // order. The verdict table itself (issue #1506) is [`ufcs_resolution_
    // query`]'s own memo, shared with LIR lowering — this just takes the
    // diagnostics half.
    diagnostics.extend(
        ufcs_resolution_query(db, project)
            .diagnostics
            .iter()
            .cloned(),
    );

    WholeProjectDiagnostics {
        diagnostics,
        symbol_meta,
    }
}

/// B3a UFCS resolution (issue #1482/#1506): the project's verdict table,
/// translated to `brink-ir`'s own lowering-facing mirror type
/// (`brink_ir::lir::UfcsLookup`), plus the diagnostics the analyzer's `ufcs`
/// pass produced alongside it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UfcsResolution {
    pub table: brink_ir::lir::UfcsLookup,
    pub diagnostics: Vec<Diagnostic>,
}

/// Compute [`UfcsResolution`], translating the analyzer's verdict table to
/// `brink-ir`'s own lowering-facing mirror type at this one seam — see that
/// type's doc for why `brink-ir` can't name `brink_analyzer::UfcsVerdict`
/// directly (it sits below `brink-analyzer` in the crate graph).
///
/// Memoized once per project and read by four call sites —
/// [`whole_project_diagnostics_query`] (the diagnostics half), (issue #1506)
/// `lir_knot_chunk_query`'s per-knot LIR lowering plus `lir_lowering_query`'s
/// own root-content step, and (issue #1507) `ProjectDb::ufcs_verdict`, which
/// `brink-ide`'s hover/go-to-def wiring reads through — so all four see the
/// same table rather than each re-running whole-project inference.
///
/// Lazy on the same argument [`whole_project_diagnostics_query`]'s old
/// inline check used: a project with no dotted-callee call anywhere never
/// triggers inference here (every ink project is in that set by
/// construction — ink's own lowering cannot produce a multi-segment callee
/// path; see `brink-analyzer`'s `ufcs` module doc), and builds (and stays
/// pointer-stable at) the empty table.
#[salsa::tracked(returns(ref))]
pub(crate) fn ufcs_resolution_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> UfcsResolution {
    let resolved = resolutions_index_query(db, project);
    let hir_refs: Vec<(FileId, &HirFile)> = project
        .files(db)
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();

    if !hir_refs
        .iter()
        .any(|&(_, hir)| brink_analyzer::project_has_ufcs_call(hir))
    {
        return UfcsResolution {
            table: brink_ir::lir::UfcsLookup::new(),
            diagnostics: Vec::new(),
        };
    }

    // Reuses the FG-narrowed, per-SCC-memoized `type_inference_query`
    // rather than letting the analyzer recompute inference from scratch —
    // the same seam `whole_project_diagnostics_query`'s strict block above
    // reuses.
    let inference = type_inference_query(db, project);
    let (table, diagnostics) = brink_analyzer::ufcs_resolution(
        &hir_refs,
        &resolved.index,
        &resolved.resolutions,
        inference.as_ref(),
    );

    UfcsResolution {
        // The one shared translation point (issue #1506) — see
        // `brink_analyzer::ufcs_lir_lookup`'s own doc.
        table: brink_analyzer::ufcs_lir_lookup(&table),
        diagnostics,
    }
}

/// B1 `or`-coalescing typing (issue #1492/#1471): the project's recorded
/// per-step chain shapes, translated to `brink-ir`'s own lowering-facing
/// mirror type (`brink_ir::lir::CoalesceLookup`).
///
/// Only the **table** half of `brink_analyzer::coalesce_types` is kept: its
/// `E066` diagnostics are strict-mode-only and already reach
/// [`whole_project_diagnostics_query`] through `strict::check`'s own wiring
/// (see `brink_analyzer::coalesce_types`' doc — surfacing them from here
/// too would emit strict-only diagnostics under `types = gradual`, and
/// duplicate them under strict).
///
/// Deliberately **not** gated on the `types` policy: the recorded shapes are
/// a typing *record*, not a strict-mode check. Native's un-overridden
/// default is gradual (`brink-analyzer::strict::native_strict_only_error`'s
/// own doc), and a gradual chain whose operands *are* statically pinned
/// still deserves the right code shape; only genuinely unpinned steps come
/// back as `CoalesceShape::RuntimeCheck`.
///
/// Memoized once per project and read by the two LIR-lowering call sites
/// (`lir_knot_chunk_query`, `lir_lowering_query`'s root-content step), so
/// both see the same table. Lazy the same way [`ufcs_resolution_query`] is:
/// a project with no `or`-coalescing anywhere (every ink-dialect project,
/// by construction — `InfixOp::Coalesce` is native-lowering-only) never
/// triggers whole-project inference here and stays pointer-stable at the
/// empty table.
#[salsa::tracked(returns(ref))]
pub(crate) fn coalesce_types_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> brink_ir::lir::CoalesceLookup {
    let resolved = resolutions_index_query(db, project);
    let hir_refs: Vec<(FileId, &HirFile)> = project
        .files(db)
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();

    if !hir_refs
        .iter()
        .any(|&(_, hir)| brink_analyzer::project_has_coalesce(hir))
    {
        return brink_ir::lir::CoalesceLookup::new();
    }

    // Reuses the FG-narrowed, per-SCC-memoized `type_inference_query`
    // rather than letting the analyzer recompute inference from scratch —
    // the same seam `ufcs_resolution_query` above reuses.
    let inference = type_inference_query(db, project);
    let (table, _strict_only_diagnostics) = brink_analyzer::coalesce_types(
        &hir_refs,
        &resolved.index,
        inference.as_ref(),
        &resolved.resolutions,
    );
    // The one shared translation point (issue #1471) — see
    // `brink_analyzer::coalesce_lir_lookup`'s own doc.
    brink_analyzer::coalesce_lir_lookup(&table)
}

/// All analysis diagnostics, assembled in the exact order
/// [`brink_analyzer::finish_analysis`] would produce them (issue #632 /
/// FG-3): symbol-index diagnostics, every file's own `resolve_query`
/// diagnostics, the per-file contributors
/// ([`contributor_diagnostics_query`]), then the whole-project contributors
/// ([`whole_project_diagnostics_query`]). [`diagnostics_query`] filters this
/// by file; [`lir_query`] reads it directly for its error gate — neither
/// goes through the bundled [`analysis_query`] anymore.
#[salsa::tracked(returns(ref))]
pub(crate) fn analysis_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Vec<Diagnostic> {
    let (_index, mut diagnostics) = symbol_index_query(db, project).clone();
    for file in project.files(db) {
        let (_file_map, file_diags) = resolve_query(db, project, *file);
        diagnostics.extend(file_diags.iter().cloned());
    }
    diagnostics.extend(contributor_diagnostics_query(db, project).iter().cloned());
    diagnostics.extend(
        whole_project_diagnostics_query(db, project)
            .diagnostics
            .iter()
            .cloned(),
    );
    diagnostics
}

/// Full cross-file analysis (issue #632 / FG-3: now a thin assembler over
/// [`resolutions_index_query`] + [`analysis_diagnostics_query`] +
/// [`whole_project_diagnostics_query`] rather than calling
/// [`brink_analyzer::finish_analysis`] directly) — `db.analysis()`'s public
/// shape, kept for LSP/IDE/CLI consumers that want the whole bundled
/// result. Output-identical to the pre-FG-3 query and to the monolithic,
/// module-aware `analyze_with_modules` path (pinned by
/// `query_equivalence.rs`) — only equal to the module-*blind*
/// `analyze_with_options` for ink projects without a declared `#@module`,
/// see `ProjectDb::module_map`'s doc (issue #1526); the decomposition
/// changes *dependency edges*, not values. Narrower consumers
/// ([`diagnostics_query`], [`lir_query`]) read the three sub-queries
/// directly instead of through this bundle.
#[salsa::tracked(returns(ref))]
pub(crate) fn analysis_query(db: &dyn salsa::Database, project: ProjectInput) -> AnalysisResult {
    let resolved = resolutions_index_query(db, project);
    let diagnostics = analysis_diagnostics_query(db, project).clone();
    let whole = whole_project_diagnostics_query(db, project);
    AnalysisResult {
        index: Arc::clone(&resolved.index),
        resolutions: resolved.resolutions.clone(),
        diagnostics,
        symbol_meta: whole.symbol_meta.clone(),
    }
}

/// Per-file diagnostics (spec §4 layer 3): this file's lowering + syntax
/// diagnostics plus its share of the cross-file analysis diagnostics. Raw —
/// suppression filtering stays a consumer concern (see
/// [`partition_diagnostics`]). Reads [`analysis_diagnostics_query`] directly
/// (issue #632 / FG-3) rather than through the bundled [`analysis_query`],
/// so a resolutions-only change (no diagnostic anywhere differs) leaves this
/// memo's dependency fully validated.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(returns(ref), lru = 4096)]
pub(crate) fn diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Vec<Diagnostic> {
    let file_id = file.file_id(db);
    let mut out = lowered_query(db, file).diagnostics.clone();
    out.extend(
        analysis_diagnostics_query(db, project)
            .iter()
            .filter(|d| d.file == file_id)
            .cloned(),
    );
    out
}

/// Whether the project has at least one Error-severity diagnostic after
/// suppression filtering and [`brink_analyzer::effective_severity`]
/// partitioning (issue #791 / FG-4a — PR #753's seam finding #3: "`lir_query`
/// still reads `analysis_diagnostics_query` wholesale for its error gate;
/// FG-4's per-container chunks will want a 'has any error' boolean
/// projection so chunk memos don't ride the full diagnostic vector's Eq").
///
/// Computes the exact same `errors.is_empty()` verdict [`super::lir_query`]'s
/// gate used to compute inline, from the exact same inputs
/// ([`analysis_diagnostics_query`] plus every file's lowering diagnostics,
/// suppressions, and the entry file's `disable_all` flag) via the same
/// shared [`partition_diagnostics`] — so this is a pure re-expression of the
/// gate as its own query, not a new rule. `bool`'s `PartialEq` is the
/// cheapest possible cutoff: a diagnostics edit that changes *content* (a
/// message, an added warning) without flipping whether any error exists
/// backdates this memo, so any dependent that reads only this boolean (not
/// the full `Vec<Diagnostic>`) stays fully validated across that edit — see
/// `fg4a_dependency_edges.rs`.
///
/// [`partition_diagnostics`]: super::partition_diagnostics
#[salsa::tracked]
pub(crate) fn has_errors_query(db: &dyn salsa::Database, project: ProjectInput) -> bool {
    let files = project.files(db);
    let Some(entry) = project.entry(db) else {
        return false;
    };
    let disable_all = files
        .iter()
        .find(|f| f.file_id(db) == entry)
        .is_some_and(|f| super::suppressions_query(db, *f).disable_all);
    let inputs: Vec<super::FileDiagnostics<'_>> = files
        .iter()
        .map(|f| super::FileDiagnostics {
            file: f.file_id(db),
            source: f.text(db),
            suppressions: super::suppressions_query(db, *f),
            lowering: &lowered_query(db, *f).diagnostics,
        })
        .collect();
    let opts = project.analysis_options(db);
    let types = opts.type_policy();
    let diagnostics = analysis_diagnostics_query(db, project);
    let (errors, _warnings) =
        super::partition_diagnostics(&inputs, diagnostics, disable_all, types, &opts.lints);
    !errors.is_empty()
}

/// The same [`partition_diagnostics`] "does at least one Error-severity
/// diagnostic exist" verdict as [`has_errors_query`], but scoped to the
/// project's **codegen closure** ([`super::compilation_closure_files`]) rather
/// than every file loaded into the project db — the same reachability
/// machinery `struct_shape_data_query`/`lir_prelude_decls_query`/
/// `lir_lowering_query` in `queries/mod.rs` already use. For an ink project
/// that closure is `entry`'s transitive `INCLUDE` closure (issue #815's
/// established narrowing); for a **native** project it is every discovered
/// `.brink` module (issue #1296), so a broken **unreferenced** sibling module
/// still fails this gate — the whole native module tree is the compilation
/// unit (Rust parity).
///
/// [`has_errors_query`] itself is untouched and stays whole-project: it feeds
/// `db.has_errors()`/`db.lir_product()`, IDE-surface reads FG-4a's
/// dependency-edge tests pin on purpose (issue #791) — a broken file
/// genuinely unrelated to any particular entry must still show up as a
/// project-wide error signal there. This narrower query is the *additional*
/// gate the #1032 collapse ruling adds for `compileProject`'s artifact path
/// ([`super::lir_in_closure_query`] / `db.story_data()`): once the editor's
/// session db and analysis db became the same db, a WIP scratch file or a
/// second, `INCLUDE`-unrelated story sharing that db could flip
/// `compileProject(entry)` from `ok:true` to `ok:false` even though codegen
/// only ever lowered `entry`'s own closure (#815) — a false-negative gate,
/// not corrupt output. Scoping the gate to match what codegen actually reads
/// closes that gap: an unrelated file's error still surfaces through
/// `diagnostics_query`/`db.diagnostics(file)` (both still whole-project,
/// unchanged), it just no longer blocks a different entry's build.
#[salsa::tracked]
pub(crate) fn has_errors_in_closure_query(db: &dyn salsa::Database, project: ProjectInput) -> bool {
    let Some(entry) = project.entry(db) else {
        return false;
    };
    let files = project.files(db);
    let closure: LookupSet<FileId> = super::compilation_closure_files(db, project)
        .into_iter()
        .collect();
    let disable_all = files
        .iter()
        .find(|f| f.file_id(db) == entry)
        .is_some_and(|f| super::suppressions_query(db, *f).disable_all);
    let inputs: Vec<super::FileDiagnostics<'_>> = files
        .iter()
        .filter(|f| closure.contains(&f.file_id(db)))
        .map(|f| super::FileDiagnostics {
            file: f.file_id(db),
            source: f.text(db),
            suppressions: super::suppressions_query(db, *f),
            lowering: &lowered_query(db, *f).diagnostics,
        })
        .collect();
    let opts = project.analysis_options(db);
    let types = opts.type_policy();
    let diagnostics: Vec<Diagnostic> = analysis_diagnostics_query(db, project)
        .iter()
        .filter(|d| closure.contains(&d.file))
        .cloned()
        .collect();
    let (errors, _warnings) =
        super::partition_diagnostics(&inputs, &diagnostics, disable_all, types, &opts.lints);
    !errors.is_empty()
}
