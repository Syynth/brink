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
// - [`whole_project_diagnostics_query`] — the passes that genuinely need
//   cross-file state (external_check against the host manifest, and under
//   `types = strict`, `strict::check`, which needs a whole-project
//   `InferenceResult`) — reading the narrow [`resolutions_index_query`]
//   projection and the already-FG-2/FG-2.1-narrowed `type_inference_query`,
//   never the diagnostics-laden bundle.
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

use brink_analyzer::AnalysisResult;
use brink_format::DefinitionId;
use brink_ir::{Diagnostic, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolManifest};

use super::{
    ProjectInput, SourceFile, lowered_query, resolution_index_query, resolve_query,
    symbol_index_query, type_inference_query,
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
/// it doesn't reintroduce whole-project churn). Never another file's HIR: a
/// body edit in file Y leaves file X's memo fully validated (same
/// `Arc`/pointer), not re-executed. `Arc`-wrapped for the same pointer-
/// identity reason as [`ResolvedProject`].
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
    let dialect = project.analysis_options(db).dialect;
    Arc::new(brink_analyzer::per_file_diagnostics(
        file_id,
        hir,
        file_resolutions,
        index,
        dialect,
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

/// Whole-project diagnostics + `symbol_meta` (issue #632 / FG-3 design doc
/// §1 — [`brink_analyzer::whole_project_diagnostics`]): the passes that
/// genuinely need cross-file state — host-manifest enrichment/checks
/// (`external_check`) and, under `types = strict`, the strict typed-mode
/// checks. Reads [`resolutions_index_query`] for index/resolutions (never
/// the diagnostics-laden `analysis_query`) and, under `types = strict` +
/// `dialect = brink`, the already-memoized, FG-2/FG-2.1-narrowed
/// `type_inference_query` — the same TM-3 wiring `analysis_query` used to
/// own directly, moved here unchanged (issue #632's fence: no strict-mode
/// *behavior* change, only its query wiring). Still walks every file's HIR
/// (`full_refs`) — `external_check` and strict checking both genuinely need
/// the whole project, per the design doc's exemption list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WholeProjectDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub symbol_meta: BTreeMap<DefinitionId, brink_analyzer::SymbolMeta>,
}

#[salsa::tracked(returns(ref))]
pub(crate) fn whole_project_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> WholeProjectDiagnostics {
    let files = project.files(db);
    let resolved = resolutions_index_query(db, project);
    let full_refs: Vec<(FileId, &HirFile, &SymbolManifest)> = files
        .iter()
        .map(|f| {
            let lowered = lowered_query(db, *f);
            (f.file_id(db), &lowered.hir, &lowered.manifest)
        })
        .collect();

    let opts = project.analysis_options(db);
    // TM-3 (docs/typed-mode-spec.md §9-step-3): under `types = strict` +
    // `dialect = brink`, reuse the already-memoized, FG-narrowed
    // `type_inference_query` instead of letting `whole_project_diagnostics`
    // recompute inference from scratch via `infer_project` — this is the
    // "inference finally has a consumer" seam the per-def/per-SCC
    // decomposition (FG-2, FG-2.1) exists for: a warm re-analyze after an
    // edit only re-solves the SCC(s) that edit actually touched, not the
    // whole project.
    let strict_inference = (opts.dialect == brink_analyzer::Dialect::Brink
        && opts.types == brink_analyzer::TypePolicy::Strict)
        .then(|| type_inference_query(db, project).as_ref());

    let (diagnostics, symbol_meta) = brink_analyzer::whole_project_diagnostics(
        &full_refs,
        &resolved.index,
        &resolved.resolutions,
        opts,
        strict_inference,
    );
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
