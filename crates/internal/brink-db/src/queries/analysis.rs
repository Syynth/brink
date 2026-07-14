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
//   `db.analysis()` stays output-identical to the monolithic
//   `brink_analyzer::analyze_with_options` path (pinned by
//   `query_equivalence.rs`).
// - [`analysis_query`] — kept as a thin assembler over the above three for
//   `db.analysis()`'s existing LSP/IDE/CLI-facing `AnalysisResult` shape.
//   [`diagnostics_query`] and [`lir_query`] read
//   [`analysis_diagnostics_query`]/[`resolutions_index_query`] directly
//   instead of through this bundle, so a diagnostics-only edit never forces
//   a resolutions-only reader to recompute and vice versa.

use std::collections::BTreeMap;
use std::sync::Arc;

use brink_analyzer::{AnalysisResult, ExternalCheckSeverity, SymbolMeta, TypePolicy};
use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DocBlock, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolKind, SymbolManifest,
};

use super::{
    ProjectInput, SourceFile, inference_index_query, lowered_query, resolution_index_query,
    resolve_query, symbol_index_query, type_inference_query,
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
/// manifest (T1d-2, docs/t1d-spec.md §3 — `handle<K>` annotation content
/// checks' declared-handle-kind lookup). The manifest is project-wide,
/// host-set config, not derived from any file's edits — reading it here is
/// the same coarse, range-free dependency shape as `dialect`, already read
/// two lines below, so it doesn't reintroduce the whole-project churn FG-3
/// eliminated. Never another file's HIR: a body edit in file Y leaves file
/// X's memo fully validated (same `Arc`/pointer), not re-executed.
/// `Arc`-wrapped for the same pointer-identity reason as [`ResolvedProject`].
#[salsa::tracked]
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
    Arc::new(brink_analyzer::per_file_diagnostics(
        file_id,
        hir,
        file_resolutions,
        index,
        opts.dialect,
        opts.host_manifest.as_ref(),
    ))
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
#[salsa::tracked]
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
#[salsa::tracked]
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
    if opts.types == TypePolicy::Strict {
        let strict_inference = (opts.dialect == brink_analyzer::Dialect::Brink)
            .then(|| type_inference_query(db, project).as_ref());
        diagnostics.extend(brink_analyzer::strict_diagnostics(
            &hir_refs,
            &resolved.index,
            &resolved.resolutions,
            opts,
            strict_inference,
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

    WholeProjectDiagnostics {
        diagnostics,
        symbol_meta,
    }
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
/// result. Output-identical to the pre-FG-3 query and to the monolithic
/// `analyze_with_options` path (pinned by `query_equivalence.rs`); the
/// decomposition changes *dependency edges*, not values. Narrower consumers
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
#[salsa::tracked(returns(ref))]
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
