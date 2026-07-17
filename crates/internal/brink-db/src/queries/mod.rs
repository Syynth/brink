//! Salsa inputs and tracked queries — the query-shaped compiler pipeline
//! (scripting-substrate spec §4, phase 0 slice B).
//!
//! Layer 0 (inputs): [`SourceFile`] (path + text) and [`ProjectInput`] (the
//! file set, the entry point, and the analysis options). Editor overlays are
//! plain input writes — there is no separate overlay pathway.
//!
//! Layer 1 (per file): [`parse_query`], [`lowered_query`] (HIR + manifest +
//! lowering diagnostics, exactly the composition the old `set_file` cached),
//! [`suppressions_query`], and the project-wide [`include_graph_query`].
//!
//! Layer 2 (project-wide names): [`symbol_index_query`],
//! [`resolution_index_query`] (the early-cutoff seam — see below),
//! [`resolve_query`], [`signature_query`], and [`analysis_query`] — the
//! latter now a thin assembler (issue #632 / FG-3) over
//! [`resolutions_index_query`] (index + resolutions, no diagnostics),
//! [`per_file_diagnostics_query`]/[`contributor_diagnostics_query`] (the
//! per-file validate/dialect_gate/annotation-content split),
//! [`whole_project_diagnostics_query`] (now a thin aggregator — issue #750
//! decomposed the external-check family into [`inline_docs_query`] /
//! [`external_meta_query`] / [`call_site_metas_query`] and the per-file
//! [`value_meta_query`] / [`call_site_diagnostics_query`]; only the M-2
//! modules pass and the strict typed-mode pass remain genuinely
//! whole-project), and
//! [`analysis_diagnostics_query`] (every diagnostic source, merged). See
//! the "FG-3" section below for the full rationale.
//!
//! Layer 3 (lowering/codegen, whole-project in this slice): [`lir_query`],
//! [`story_data_query`], and the per-file [`diagnostics_query`] — both now
//! read the decomposed FG-3 queries directly rather than through the
//! bundled [`analysis_query`].
//!
//! # The `resolution_index` cutoff seam (slice-A findings 1+2, tightened by #517)
//!
//! The full [`SymbolIndex`] carries a `TextRange` per symbol, so nearly any
//! edit shifts ranges and defeats `Eq`-cutoff on the index — dependents of
//! `symbol_index` would re-run on every keystroke. [`resolution_index_query`]
//! sits between the index and reference resolution: it is the full index with
//! locals (`Param`/`Temp`) dropped and ranges zeroed for every remaining
//! (declaration) symbol.
//!
//! Locals were originally kept in the projection (with real ranges) because
//! `lookup_local_in_scope`'s closest-preceding pick was the one place
//! resolution read symbol ranges. That left a gap (finding 1): a body edit
//! that adds/removes a `~ temp` anywhere in the project changes a local's
//! *identity*, not just its range, so `resolution_index_query`'s own output
//! still differed and every file's `resolve` memo still re-ran. #517 closes
//! the gap by having `resolve_query` read the declaring file's own
//! `manifest.locals` instead of the merged index for local lookups (a knot's
//! body lives in exactly one file, so this was always sufficient — see
//! `brink_analyzer::resolve::lookup_local_in_scope`), which also fixes the
//! finding-4 cross-file duplicate-`DefinitionId` aliasing: resolution no
//! longer merges locals from different files, so it can no longer pick the
//! wrong file's declaration. With locals gone, dropping the rest is
//! behavior-neutral by construction (locked by the `query_equivalence` tests
//! and the oracle gate).
//!
//! # Memory bounding (FG-5, issue #647, decision log "FG-5 memory bounding")
//!
//! The per-file query families (`parse`, `lowered`, `suppressions`,
//! `resolve`, `per_file_diagnostics`, `value_meta`, `call_site_diagnostics`,
//! `diagnostics`) and the per-def families keyed by [`DefKey`]
//! (`signature`, `def_body`,
//! `referenced_globals`, `call_edges`, `solve_scc`, `inferred_signature`,
//! `infer_body`) each carry a salsa `lru` capacity — a **runaway guard**,
//! not a working-set trim. Issue #537's measurement (large synthetic
//! projects, 2,000-edit sessions) showed every one of these families scales
//! with live project size and shows zero session-length growth, so a tight
//! LRU would only evict live working-set entries and buy recompute churn on
//! big projects, never save memory. The ceilings are sized far above
//! realistic project scale on purpose (≈30× the measured 132-file scale for
//! per-file families, ≈10× the measured 1,549-def scale for per-def
//! families) so they never evict in steady state, and exist only to cap the
//! pathological/runaway case. **No eviction *policy* design happened here**
//! — that was explicitly ruled out by the data; see the decision log entry
//! for the full ruling. `def_body`, `solve_scc`, `signature`, `infer_body`,
//! and `lowered` additionally specify a `heap_size` estimator
//! ([`heap_size`], issue #538) — the families #537 flagged as the dominant
//! Arc-hidden payloads, so `crate::memory::snapshot`'s `heap_bytes` column
//! reads `Some(_)` for them instead of the honest-`None` every query
//! reported before this pass.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, CallGraph, InferenceResult, SccGraph, Sig, TypePolicy};
use brink_format::{
    CallAtom, CapabilityParam, DefinitionId, DirectEffects, EffectRowEntry, NameId, StoryData,
};
use brink_ir::suppressions::{Suppressions, apply_suppressions, parse_suppressions};
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolutionMap, Severity, SymbolIndex, SymbolKind,
    SymbolManifest, lower, lower_single_knot, lower_top_level,
};
use brink_syntax::Parse;

use crate::db::resolve_include_path;
use crate::determinism::{LookupMap, LookupSet};
use crate::include_graph::IncludeGraph;

mod analysis;
mod heap_size;

pub use analysis::ResolvedProject;
pub(crate) use analysis::{
    analysis_diagnostics_query, analysis_query, call_site_diagnostics_query, call_site_metas_query,
    contributor_diagnostics_query, diagnostics_query, external_meta_query, has_errors_query,
    inline_docs_query, per_file_diagnostics_query, resolutions_index_query, value_meta_query,
    whole_project_diagnostics_query,
};

// ─── Database ────────────────────────────────────────────────────────

/// The salsa database behind [`crate::ProjectDb`].
///
/// Ingredients are registered explicitly (salsa's `inventory` feature is
/// off): link-time collection via life-before-main is exactly the kind of
/// platform magic that breaks on wasm, and the explicit list keeps the query
/// surface reviewable. A query missing from the list panics loudly on first
/// use — any test exercising it catches that immediately.
#[salsa::db]
#[derive(Clone)]
pub(crate) struct BrinkDatabase {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for BrinkDatabase {}

impl Default for BrinkDatabase {
    fn default() -> Self {
        Self {
            storage: salsa::Storage::builder()
                // Inputs + interned keys.
                .ingredient::<SourceFile>()
                .ingredient::<ProjectInput>()
                .ingredient::<DefKey<'_>>()
                // Layer 1.
                .ingredient::<parse_query>()
                .ingredient::<lowered_query>()
                .ingredient::<suppressions_query>()
                .ingredient::<include_graph_query>()
                // Layer 2.
                .ingredient::<module_map_query>()
                .ingredient::<symbol_index_query>()
                .ingredient::<resolution_index_query>()
                .ingredient::<resolve_query>()
                .ingredient::<signature_query>()
                // FG-3 (issue #632): analysis_query decomposed into narrow
                // cutoff-friendly projections. resolutions_index_query
                // (index+resolutions, no diagnostics) and
                // analysis_diagnostics_query (every diagnostic source,
                // assembled from per-file contributors +
                // whole_project_diagnostics_query) are independent queries
                // now, so a diagnostics-only edit never invalidates a
                // resolutions-only reader and vice versa.
                // per_file_diagnostics_query/contributor_diagnostics_query
                // are the per-file validate/dialect_gate/annotation-content
                // split — a body edit in file Y leaves file X's contributor
                // memo untouched. analysis_query itself survives as a thin
                // assembler over these for `db.analysis()`'s existing
                // LSP/IDE/CLI-facing shape.
                .ingredient::<resolutions_index_query>()
                .ingredient::<per_file_diagnostics_query>()
                .ingredient::<contributor_diagnostics_query>()
                // FG-3 completion (issue #750): the external-check family,
                // decomposed. inline_docs_query (project doc merge, Eq
                // cutoff) + external_meta_query (index-driven E039/E040 +
                // enrichment, no HIR) + call_site_metas_query (the
                // range-free name→meta cutoff seam) feed the per-file
                // value_meta_query / call_site_diagnostics_query, so a body
                // edit in file Y re-runs only Y's own value-meta and
                // call-site walks; whole_project_diagnostics_query is now a
                // thin aggregator (plus the genuinely whole-project M-2
                // modules pass and strict pass).
                .ingredient::<inline_docs_query>()
                .ingredient::<external_meta_query>()
                .ingredient::<call_site_metas_query>()
                .ingredient::<value_meta_query>()
                .ingredient::<call_site_diagnostics_query>()
                .ingredient::<whole_project_diagnostics_query>()
                .ingredient::<analysis_diagnostics_query>()
                .ingredient::<analysis_query>()
                .ingredient::<diagnostics_query>()
                // FG-4a (issue #791): the `has_errors` boolean projection
                // (PR #753's seam finding #3) and the LIR-lowering split it
                // gates — see `lir_query`'s doc comment. type_policy_query
                // (issue #806) is the matching narrow `.types` projection so
                // an unrelated AnalysisOptions edit can't re-execute the
                // `no_eq` lowering memo.
                .ingredient::<has_errors_query>()
                .ingredient::<type_policy_query>()
                // FG-4d (issue #830): per-knot LIR chunk memos + the
                // cutoff-friendly struct-shape projection they read;
                // `lir_lowering_query` is now the link phase assembling them.
                .ingredient::<struct_shape_data_query>()
                .ingredient::<normalized_stamped_query>()
                // FG-4e (issue #839): decl_hir_query is the per-file
                // backdating projection lir_prelude_decls_query reads
                // instead of raw HIR, so a knot body edit doesn't force the
                // whole-project declaration collection to re-execute.
                .ingredient::<decl_hir_query>()
                .ingredient::<lir_prelude_decls_query>()
                .ingredient::<KnotChunkKey<'_>>()
                .ingredient::<lir_knot_chunk_query>()
                .ingredient::<lir_lowering_query>()
                // Layer 2/3: type inference (TM-1, advisory-only).
                // Per-def/per-SCC decomposition (FG-2, issue #631):
                // call_edges(def) -> call_graph() -> scc_membership() ->
                // solve_scc(SccId) -> inferred_signature(def)/infer_body(def).
                // Lazy per-reference globals + full dependency narrowing
                // (FG-2.1, issue #638): inferable_defs_query/def_body_query/
                // referenced_globals_query are the new per-def projections
                // call_edges_query/solve_scc_query read instead of every
                // project file's HIR.
                .ingredient::<inference_index_query>()
                .ingredient::<inferable_defs_query>()
                .ingredient::<def_body_query>()
                .ingredient::<referenced_globals_query>()
                .ingredient::<call_edges_query>()
                .ingredient::<call_graph_query>()
                .ingredient::<scc_membership_query>()
                .ingredient::<solve_scc_query>()
                .ingredient::<inferred_signature_query>()
                .ingredient::<type_inference_query>()
                .ingredient::<infer_body_query>()
                .ingredient::<type_diagnostics_query>()
                // T2-1 (issue #860): advisory effect-row inference, sited
                // beside inferred_signature. def_effect_atoms_query is the
                // per-def atom harvest (same body walk referenced_globals/
                // call_edges drive); effects_scc_query lifts solve_scc's
                // per-SCC fixpoint to the effect lattice; effects_query is the
                // per-def view.
                .ingredient::<def_effect_atoms_query>()
                .ingredient::<effects_scc_query>()
                .ingredient::<effects_query>()
                // Layer 3.
                .ingredient::<lir_query>()
                .ingredient::<story_data_query>()
                .build(),
        }
    }
}

// ─── Layer 0: inputs ─────────────────────────────────────────────────

/// One source file: identity (stable [`FileId`] + project-relative path) and
/// its current text. The text is the only mutable input — editor overlays and
/// disk loads both go through `set_text`.
#[salsa::input]
pub(crate) struct SourceFile {
    pub file_id: FileId,
    #[returns(ref)]
    pub path: String,
    #[returns(ref)]
    pub text: String,
}

/// The project-level input: the file set (sorted by [`FileId`]), the compile
/// entry point, and the analysis options (host manifest + external-check
/// severity).
#[salsa::input]
pub(crate) struct ProjectInput {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
    pub entry: Option<FileId>,
    #[returns(ref)]
    pub analysis_options: AnalysisOptions,
}

// ─── Layer 1: per-file queries ───────────────────────────────────────

/// Parse one file's text into a lossless CST.
///
/// `lru = 4096`: a per-file runaway-guard ceiling (issue #647, decision log
/// "FG-5 memory bounding"), not a working-set trim — see this module's doc
/// comment's "Memory bounding" section.
#[salsa::tracked(returns(ref), lru = 4096)]
pub(crate) fn parse_query(db: &dyn salsa::Database, file: SourceFile) -> Parse {
    brink_syntax::parse(file.text(db))
}

/// Per-file lowering output: assembled HIR, symbol manifest, and lowering +
/// syntax diagnostics — the exact product the retired `FileState` cached.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoweredFile {
    pub hir: HirFile,
    pub manifest: SymbolManifest,
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower one file to HIR. Salsa's dependency tracking on the `parse` input
/// replaces the retired per-knot green-node/byte-offset cache (`knot_cache`):
/// the composition below is byte-identical to what `set_file` produced.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647). `heap_size`:
/// one of the five #538/#647 estimators — #537 flagged this family's
/// per-def analogues (`def_body`/`solve_scc`) as the dominant Arc-hidden
/// payload; this is the per-file sibling.
#[salsa::tracked(returns(ref), lru = 4096, heap_size = heap_size::lowered_file_heap_size)]
pub(crate) fn lowered_query(db: &dyn salsa::Database, file: SourceFile) -> LoweredFile {
    lower_file(file.file_id(db), parse_query(db, file))
}

/// Parsed suppression/expectation directives for one file.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(returns(ref), lru = 4096)]
pub(crate) fn suppressions_query(db: &dyn salsa::Database, file: SourceFile) -> Suppressions {
    parse_suppressions(file.text(db))
}

/// The `INCLUDE` graph over the whole project. Always complete — edges are
/// derived from every file's HIR against the full path set, so the old
/// "rebuild after batch load" step no longer exists.
#[salsa::tracked(returns(ref))]
pub(crate) fn include_graph_query(db: &dyn salsa::Database, project: ProjectInput) -> IncludeGraph {
    let files = project.files(db);
    let path_to_id: LookupMap<&str, FileId> = files
        .iter()
        .map(|f| (f.path(db).as_str(), f.file_id(db)))
        .collect();

    let mut graph = IncludeGraph::new();
    for file in files {
        let hir = &lowered_query(db, *file).hir;
        let include_ids: Vec<FileId> = hir
            .includes
            .iter()
            .filter_map(|inc| {
                let resolved = resolve_include_path(file.path(db), &inc.file_path);
                path_to_id.get(resolved.as_str()).copied()
            })
            .collect();
        graph.update(file.file_id(db), include_ids);
    }
    graph
}

// ─── Layer 2: project-wide names ─────────────────────────────────────

/// Every file's resolved module (M-1, docs/modules-spec.md §1/§5) plus the
/// stem-collision diagnostics (`E085`). Extracted as its own memoized query
/// (issue #790) so both [`symbol_index_query`] — which qualifies identity by
/// declared module — and [`resolve_query`] — which needs each referring
/// file's module + imports to scope resolution — share one computation.
///
/// Undeclared stem-modules (the entire pre-modules corpus) resolve to
/// non-qualifying entries, so their `DefinitionId`s stay byte-identical.
#[salsa::tracked(returns(ref))]
pub(crate) fn module_map_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> (brink_analyzer::ModuleMap, Vec<Diagnostic>) {
    let files = project.files(db);
    let module_inputs: Vec<crate::modules::FileModuleInput> = files
        .iter()
        .map(|f| {
            let hir_module = lowered_query(db, *f).hir.module.as_ref();
            crate::modules::FileModuleInput {
                file: f.file_id(db),
                stem: crate::modules::file_stem(f.path(db)).to_string(),
                declared: hir_module.map(|m| m.name.clone()),
                was: hir_module.and_then(|m| m.was.as_ref().map(|(old, _)| old.clone())),
            }
        })
        .collect();
    crate::modules::resolve_modules(&module_inputs, include_graph_query(db, project))
}

/// The merged project-wide symbol index plus indexing diagnostics
/// (duplicates, built-in shadowing). Thin wrapper over
/// [`brink_analyzer::symbol_index`].
#[salsa::tracked(returns(ref))]
pub(crate) fn symbol_index_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> (Arc<SymbolIndex>, Vec<Diagnostic>) {
    let files = project.files(db);
    let manifest_refs: Vec<(FileId, &SymbolManifest)> = files
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).manifest))
        .collect();

    let (module_map, module_diags) = module_map_query(db, project);

    // M-2c/M-2d (issues #784/#790): the cross-declared-module duplicate
    // handling (E096 stopgap → coexistence) is dialect-gated (brink only)
    // inside `symbol_index_with_modules` itself, so the project's configured
    // dialect must reach it here.
    let dialect = project.analysis_options(db).dialect;
    let (index, mut diagnostics) =
        brink_analyzer::symbol_index_with_modules(&manifest_refs, module_map, dialect);
    diagnostics.extend(module_diags.clone());
    (index, diagnostics)
}

/// The early-cutoff projection of the symbol index used by resolution:
/// declarations only (locals dropped entirely — issue #517), ranges zeroed
/// for every remaining symbol (see module docs). Neither a body edit that
/// shifts a global declaration's
/// range nor one that adds/removes a `~ temp`/param anywhere in the project
/// changes this output, so every file's `resolve` memo survives untouched.
///
/// Locals are dropped rather than range-zeroed like the rest: a `Param`/
/// `Temp` entry's *identity* (not just its range) changes when a body edit
/// adds or removes a local, so zeroing its range alone would not have
/// stopped the churn (finding 1). Resolution never needs locals from this
/// projection — [`resolve_query`] feeds `lookup_local_in_scope` the
/// declaring file's own per-file `manifest.locals` instead (a knot's body
/// lives in exactly one file, so cross-file local lookup was never
/// semantically required — see `brink_analyzer::resolve::lookup_local_in_scope`).
#[salsa::tracked(returns(ref))]
pub(crate) fn resolution_index_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<SymbolIndex> {
    let (index, _diags) = symbol_index_query(db, project);
    let mut stripped: SymbolIndex = (**index).clone();
    stripped
        .symbols
        .retain(|_, info| !matches!(info.kind, SymbolKind::Param | SymbolKind::Temp));
    let live_ids: LookupSet<DefinitionId> = stripped.symbols.keys().copied().collect();
    stripped.by_name.retain(|_, ids| {
        ids.retain(|id| live_ids.contains(id));
        !ids.is_empty()
    });
    for info in stripped.symbols.values_mut() {
        info.range = rowan::TextRange::default();
    }
    Arc::new(stripped)
}

/// Resolve one file's references against the project-wide names. Thin
/// wrapper over [`brink_analyzer::resolve`], fed the decls-only cutoff
/// projection for globals and this file's own `manifest.locals` for
/// param/temp lookups — the per-file dependency edge that lets a `~ temp`
/// edit in file Y leave file X's memo untouched (issue #517).
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(returns(ref), lru = 4096)]
pub(crate) fn resolve_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> (Arc<ResolutionMap>, Vec<Diagnostic>) {
    let index = resolution_index_query(db, project);
    let lowered = lowered_query(db, file);

    // Import-scoped resolution (M-2d, docs/modules-spec.md §2; issue #790):
    // feed the resolver this file's own **declared** module and its `IMPORT`
    // list so a bare reference with same-name candidates across declared
    // modules binds to the one this file imported. The scope is inert for
    // the pre-modules / single-module world (no declared module qualifies
    // identity, so every candidate carries `module: None` and the resolver's
    // fast path is byte-identical). `file_module` comes from the shared
    // module map (declared modules only — INCLUDE inheritance already
    // applied), matching how `symbol_index_query` qualified identity.
    let (module_map, _module_diags) = module_map_query(db, project);
    let file_module = module_map
        .get(&file.file_id(db))
        .filter(|m| m.declared)
        .map(|m| m.name.clone());
    let scope = brink_analyzer::ImportScope::new(file_module, &lowered.hir.imports);

    brink_analyzer::resolve(file.file_id(db), &lowered.manifest, index, &scope)
}

/// Interned key for [`signature_query`]. Keyed on the content-addressed
/// [`DefinitionId`] alone: colliding ids among non-local declarations
/// (duplicate names across files) map to a *single* index entry chosen
/// deterministically by the merge, so the memo cannot diverge from what a
/// non-memoized `signature(def)` call would return for the same id. Local
/// (`Param`/`Temp`) ids no longer collide across files in a way that matters
/// here — [`resolution_index_query`] drops locals entirely (issue #517).
#[salsa::interned]
pub(crate) struct DefKey<'db> {
    pub def: DefinitionId,
}

/// Per-declaration signature stub (spec §4 layer 2). Reads the decls-only,
/// range-stripped index projection — [`Sig`] carries no ranges, so this is
/// output-identical to reading the full index for declarations, while
/// backdating across whitespace/body edits. Locals are not addressable here
/// (returns `None` for a `Param`/`Temp` [`DefinitionId`], issue #517):
/// resolving one would require scanning every file's `manifest.locals` to
/// find the declaring file, reintroducing the project-wide invalidation this
/// projection exists to avoid. No consumer calls `signature` with a local id
/// today (phase-0 stub, not yet wired to hover).
///
/// **Declaring-file dependency only (issue #630 / FG-1 §2.1).**
/// `brink_analyzer::signature` reads only the declaring file's HIR (looked
/// up by `SymbolInfo.file`, known from the index) — this query used to build
/// `hir_refs` over *every* project file before calling it, so salsa recorded
/// a read-edge on every file's `lowered_query` regardless, and a body edit
/// in any file re-ran every signature memo. Filtering `project.files(db)`
/// down to the one matching `SourceFile` before calling `lowered_query`
/// means the only per-file dependency recorded is the declaring file's own —
/// a body edit elsewhere in the project no longer invalidates this memo.
///
/// **Manifest dependency (T1d-2b, issue #774, docs/t1d-spec.md §3).** Also
/// reads `project.analysis_options(db).host_manifest` so `handle<K>`
/// annotations resolve to `Ty::Handle(K)` here — the registered manifest is
/// project-wide, host-set config, not derived from any file's edits, so
/// reading it is the same coarse dependency shape `per_file_diagnostics_query`
/// already reads `host_manifest` at, not a reintroduction of whole-project
/// per-file churn.
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647, decision log
/// "FG-5 memory bounding" — #537's data showed this family scales with
/// live project defs, never with session length, so the ceiling is sized
/// far above realistic project scale and never evicts in steady state).
/// `heap_size = heap_size::signature_heap_size`: one of the five #538
/// estimators — #537 named `signature` the widest-fanout per-def memo.
#[salsa::tracked(lru = 16384, heap_size = heap_size::signature_heap_size)]
pub(crate) fn signature_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Option<Arc<Sig>> {
    let index = resolution_index_query(db, project);
    let def_id = def.def(db);
    let declaring_file = index.symbols.get(&def_id)?.file;
    let hir_refs: Vec<(FileId, &HirFile)> = project
        .files(db)
        .iter()
        .filter(|f| f.file_id(db) == declaring_file)
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();
    let opts = project.analysis_options(db);
    brink_analyzer::signature(def_id, index, &hir_refs, opts.host_manifest.as_ref())
}

// ─── Layer 2/3: type inference (TM-1, advisory-only) ──────────────────
//
// The checker substrate (typed-mode-spec §2/§9 step 1): `signature`/
// `infer_body`/`type_diagnostics`. **Advisory-only** — nothing here changes
// compiler output; `type_inference_query` is not read by `lir_query` or
// `story_data_query`, so it is lazy by construction (computed only when a
// consumer calls `infer_body`/`type_diagnostics`, which today is nobody —
// see the PR's warm/cold benchmark report). Whole-project, like
// `analysis_query`/`lir_query` in this same slice (scripting-substrate spec
// §7 defers per-container splitting to slice C); `infer_body_query` and
// `type_diagnostics_query` are thin per-def/per-file views over the one
// project-wide memo, mirroring `signature_query`'s and `diagnostics_query`'s
// own shape.

/// The cutoff projection of the symbol index feeding whole-project type
/// inference (issue #630 / FG-1 §3): every symbol — declarations *and*
/// locals (`Param`/`Temp`) — with ranges zeroed.
///
/// Unlike [`resolution_index_query`] (name *resolution*'s projection, which
/// drops locals entirely — issue #517, because a local's identity, not just
/// its range, changes when a `~ temp` is added/removed elsewhere), inference
/// reads `index.symbols.get(def)` only for already-*resolved* ids
/// (`brink_analyzer::infer::body::ty_of_def`/`observe`/`infer_list_literal`)
/// to recover a local's `kind`/`name` — it never resolves a name against
/// this index, so dropping locals here would silently make every local
/// reference type as `Unknown`. Neither this query nor
/// [`brink_analyzer::signature`] (called for globals via `infer/mod.rs`'s
/// `collect_globals`) ever reads a symbol's range, so zeroing it is safe and
/// backdates this projection across any edit that adds/removes no
/// declaration or local.
#[salsa::tracked(returns(ref))]
pub(crate) fn inference_index_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<SymbolIndex> {
    let (index, _diags) = symbol_index_query(db, project);
    let mut stripped: SymbolIndex = (**index).clone();
    for info in stripped.symbols.values_mut() {
        info.range = rowan::TextRange::default();
    }
    Arc::new(stripped)
}

/// A strongly-connected component's stable identifier (FG-2, issue #631):
/// the component's minimum-valued `DefinitionId` member, exactly
/// [`brink_analyzer::scc_graph`]'s own sort/dedup key. A plain alias, not a
/// fresh newtype — it *is* a real member `DefinitionId`, reused as the
/// component's name, so the existing [`DefKey`] interning already covers it:
/// [`solve_scc_query`]'s key is `DefKey::new(db, scc_id)`, the same
/// interning [`signature_query`]/[`infer_body_query`] use for a definition's
/// own id.
pub(crate) type SccId = DefinitionId;

/// The project's inferable (knot/stitch) def ids, sourced from the index
/// alone (FG-2.1, issue #638, Ruling 2b — `inferable_defs_query`, the
/// `inference_index_query` precedent applied to the "which defs have a
/// body" question). No HIR read: `call_graph_query`'s per-def loop and
/// [`call_edges_query`]/[`referenced_globals_query`]'s `inferable`
/// membership check both read this instead of walking every project file's
/// HIR just to enumerate ids — a body edit that adds/removes no
/// knot/stitch declaration leaves this memo's *dependency edge* untouched
/// (it only reads `inference_index_query`, never `lowered_query`).
#[salsa::tracked(returns(ref))]
pub(crate) fn inferable_defs_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> BTreeSet<DefinitionId> {
    let index = inference_index_query(db, project);
    brink_analyzer::inferable_defs_from_index(index)
}

/// One inferable def's own params + body, read from its declaring file's
/// HIR alone (FG-2.1, issue #638, Ruling 2b — `def_body_query(def)`, the
/// per-def HIR projection `solve_scc_query` reads instead of every
/// project file's `lowered_query`). `Arc<plain>`, `Eq`-derived so an
/// edit to a *different* def in the same declaring file — which still
/// changes that file's `lowered_query` output — backdates here as long as
/// this specific def's own params/body come out byte-identical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DefBody {
    pub file: FileId,
    pub params: Vec<brink_ir::Param>,
    /// The knot's `): type ===` return annotation, if any (T1c — feeds the
    /// annotation-firewall overlay in `infer_def_body`).
    pub return_annotation: Option<brink_ir::TypeExpr>,
    pub body: brink_ir::Block,
}

/// `lru = 16384`: per-def runaway-guard ceiling (issue #647). `heap_size =
/// heap_size::def_body_heap_size`: one of the five #538 estimators — #537
/// named `def_body` (holds a full HIR `Block` clone per def) one of the
/// two dominant Arc-hidden-payload families.
#[salsa::tracked(lru = 16384, heap_size = heap_size::def_body_heap_size)]
pub(crate) fn def_body_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Option<Arc<DefBody>> {
    let index = inference_index_query(db, project);
    let def_id = def.def(db);
    let declaring_file = index.symbols.get(&def_id)?.file;
    let file = project
        .files(db)
        .iter()
        .find(|f| f.file_id(db) == declaring_file)?;
    let hir = &lowered_query(db, *file).hir;
    let (params, return_annotation, body) =
        brink_analyzer::def_body(def_id, &[(declaring_file, hir)], index)?;
    Some(Arc::new(DefBody {
        file: declaring_file,
        params,
        return_annotation,
        body,
    }))
}

/// The VAR/CONST global ids one def's body references (FG-2.1, issue #638,
/// Ruling 1 — `referenced_globals_query(def)`, the pre-scan `solve_scc_query`
/// resolves into a narrow `BodyCtx.globals` map via `signature_query`,
/// exactly the same declaring-file-only dependency edge
/// [`def_body_query`] uses). Also the per-def global *read set* a future T2
/// effect row needs — see `brink_analyzer::referenced_globals`'s docs.
///
/// Passes `None` for `brink_analyzer::referenced_globals`'s manifest
/// parameter (T1d-2b, issue #774) deliberately: this pass discards every
/// computed type (only the referenced-def-id *set* survives), so a
/// registered manifest can never change its output — reading
/// `project.analysis_options(db)` here would only add a needless
/// project-wide invalidation edge to the FG-2.1 narrow per-def dependency
/// this query exists to keep narrow, for zero behavioral benefit.
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 16384)]
pub(crate) fn referenced_globals_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Arc<BTreeSet<DefinitionId>> {
    let index = inference_index_query(db, project);
    let def_id = def.def(db);
    let Some(declaring_file) = index.symbols.get(&def_id).map(|info| info.file) else {
        return Arc::new(BTreeSet::new());
    };
    let Some(file) = project
        .files(db)
        .iter()
        .find(|f| f.file_id(db) == declaring_file)
    else {
        return Arc::new(BTreeSet::new());
    };
    let hir = &lowered_query(db, *file).hir;
    let (resolutions, _diags) = resolve_query(db, project, *file);
    Arc::new(brink_analyzer::referenced_globals(
        def_id,
        &[(declaring_file, hir)],
        index,
        resolutions,
        None,
    ))
}

/// Pass 1, per-def (FG-2, issue #631 — `call_edges(def)`). Thin salsa
/// wrapper over [`brink_analyzer::call_edges`]; the per-def key gives Eq
/// cutoff on this def's own edge set (`BTreeSet<DefinitionId>`, no ranges,
/// derived `Eq`) — see the design doc §2 table's explicit allowance to keep
/// reusing `infer_def_body` and discard types, as `infer_project` already
/// did, for this pass's computation.
///
/// **Narrowed inputs (FG-2.1, issue #638, Ruling 2a).** Reads only `def`'s
/// own declaring file's `lowered_query`/`resolve_query` — never every
/// project file's — plus the index-sourced [`inferable_defs_query`]. No
/// globals map at all (pass 1 discards every computed type; see
/// `brink_analyzer::call_edges`'s docs).
///
/// Passes `None` for `brink_analyzer::call_edges`'s manifest parameter
/// (T1d-2b, issue #774) — same rationale as [`referenced_globals_query`]:
/// pass 1 discards every computed type, so the manifest can never change
/// this query's output, and reading `project.analysis_options(db)` here
/// would only widen this per-def query's dependency edge for no benefit.
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 16384)]
pub(crate) fn call_edges_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Arc<BTreeSet<DefinitionId>> {
    let index = inference_index_query(db, project);
    let def_id = def.def(db);
    let Some(declaring_file) = index.symbols.get(&def_id).map(|info| info.file) else {
        return Arc::new(BTreeSet::new());
    };
    let Some(file) = project
        .files(db)
        .iter()
        .find(|f| f.file_id(db) == declaring_file)
    else {
        return Arc::new(BTreeSet::new());
    };
    let hir = &lowered_query(db, *file).hir;
    let (resolutions, _diags) = resolve_query(db, project, *file);
    let inferable = inferable_defs_query(db, project);
    Arc::new(brink_analyzer::call_edges(
        def_id,
        &[(declaring_file, hir)],
        index,
        resolutions,
        inferable,
        None,
    ))
}

/// The whole-project call graph, merged from every inferable def's
/// [`call_edges_query`] (FG-2, issue #631 — the derived `call_graph()` the
/// design doc's §2 table names). [`CallGraph`]'s `Eq` (added for this slice)
/// is the cutoff [`scc_membership_query`] backdates on.
///
/// Inherently project-wide (FG-2.1, issue #638, Ruling 2c: `call_graph_query`
/// is one of the two queries that "genuinely need project shape") — it must
/// enumerate every inferable def to build the graph. What FG-2.1 narrows is
/// each *individual* read inside the loop: [`inferable_defs_query`] is
/// index-only (no HIR), and each [`call_edges_query`] call is validated
/// without re-executing unless *that specific def's* declaring file changed
/// — so an edit in file X only pays for X's own defs, even though this
/// query's own closure still walks the whole project's def list every time
/// it *does* run.
#[salsa::tracked(returns(ref))]
pub(crate) fn call_graph_query(db: &dyn salsa::Database, project: ProjectInput) -> CallGraph {
    let defs = inferable_defs_query(db, project);
    let mut graph = CallGraph::new();
    for &def in defs {
        graph.add_node(def);
        let edges = call_edges_query(db, project, DefKey::new(db, def));
        for &callee in edges.iter() {
            graph.add_edge(def, callee);
        }
    }
    graph
}

/// SCC partition + condensation DAG over the whole project's call graph
/// (FG-2, issue #631 — `scc_membership()` (+ topo order)). Thin salsa
/// wrapper over [`brink_analyzer::scc_graph`]. The other query Ruling 2c
/// keeps project-wide — SCC membership is inherently a global graph
/// property, not narrowable per-def.
#[salsa::tracked(returns(ref))]
pub(crate) fn scc_membership_query(db: &dyn salsa::Database, project: ProjectInput) -> SccGraph {
    let graph = call_graph_query(db, project);
    brink_analyzer::scc_graph(graph)
}

/// One SCC's finalized inference result: signatures for the SCC's own
/// members plus their full body-type pictures. `Arc<plain>`, `Eq`-derived —
/// the per-SCC cutoff [`inferred_signature_query`]/[`infer_body_query`]
/// backdate on (Arc<plain> ruling, design doc §2 Fork 2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SolvedScc {
    pub signatures: BTreeMap<DefinitionId, brink_analyzer::InferredSig>,
    pub bodies: BTreeMap<DefinitionId, brink_analyzer::BodyTypes>,
}

/// Pass 2, per-SCC (FG-2, issue #631 — `solve_scc(SccId)`). Reads
/// `solve_scc_query` for every condensation predecessor first (recursion is
/// acyclic by construction — the condensation is a DAG, Fork 1 ruling — so
/// salsa never sees a cycle here), merges their finalized signatures into
/// `known_sigs`, then runs [`brink_analyzer::solve_scc`]'s bounded fixpoint
/// (plain Rust inside this one query execution) for exactly this
/// component's own members. Returns an empty [`SolvedScc`] for an id that
/// isn't any component's minimum member (defensive — never panics on a
/// stale/unknown key).
///
/// **Full narrowing (FG-2.1, issue #638, Ruling 2b + Ruling 1).** HIR:
/// [`def_body_query`] per member — only this batch's own declaring files,
/// never every project file's. Resolutions: only those same declaring
/// files' `resolve_query` results (sufficient to resolve every `Path` range
/// inside a member's own body, cross-file targets included — see
/// `signature_query`'s docs on why resolution reads the *source* file's map
/// only). Globals: the narrow map built from every member's
/// [`referenced_globals_query`] pre-scan, each id resolved through the
/// existing per-declaring-file [`signature_query`] (never a whole-project
/// globals scan). `inferable`: the same index-sourced
/// [`inferable_defs_query`] `call_edges_query` uses.
///
/// **Manifest dependency (T1d-2b, issue #774, docs/t1d-spec.md §3).** Also
/// reads `project.analysis_options(db).host_manifest`, threaded to
/// [`brink_analyzer::solve_scc`] so a `handle<K>` param/return/temp
/// annotation resolves to `Ty::Handle(K)` during the per-SCC body-uses
/// solve — the seam that makes strict-mode handle-kind rejection reachable
/// end-to-end (the #767 acceptance criterion): once two locals of
/// different declared handle kinds are unified together, the #627 lattice
/// folds them to `Ty::Conflicted`, and `strict::check`'s pre-existing
/// `E066` classification reports it — this query is what was missing to
/// let a genuine `Ty::Handle` ever reach that lattice from body-usage
/// inference through the salsa-memoized pipeline. Same coarse project-wide
/// dependency shape [`signature_query`]/`per_file_diagnostics_query`
/// already read `host_manifest` at — unlike [`call_edges_query`]/
/// [`referenced_globals_query`] (whose *outputs* the manifest can never
/// change), this query's output genuinely depends on it.
///
/// **`inline_docs` dependency (issue #805).** Also reads [`inline_docs_query`]
/// — the same project-wide merged `///` doc-comment memo [`external_meta_query`]
/// already reads — and threads it to [`brink_analyzer::solve_scc`] so an
/// `EXTERNAL` documented purely inline (no matching registered
/// `ManifestExternal`) now seeds a `known_sigs` entry too, and so a
/// registered/inline param or return type naming a *scalar* semantic type
/// (not just a `handle<K>` kind) resolves to its own base `Ty`. Range-free
/// (`DocBlock` carries no source spans), so this doesn't reintroduce the
/// whole-project-HIR churn FG-2/FG-2.1 eliminated — a doc-content-preserving
/// edit backdates through `inline_docs_query`'s own `Eq` cutoff exactly like
/// every other reader of that memo.
///
/// `lru = 16384`: per-def (per-SCC) runaway-guard ceiling (issue #647).
/// `heap_size = heap_size::solve_scc_heap_size`: one of the five #538
/// estimators — #537 named `solve_scc` (holds signatures+bodies per SCC)
/// the other dominant Arc-hidden-payload family alongside `def_body`.
#[salsa::tracked(lru = 16384, heap_size = heap_size::solve_scc_heap_size)]
pub(crate) fn solve_scc_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    scc: DefKey<'db>,
) -> Arc<SolvedScc> {
    let scc_id: SccId = scc.def(db);
    let membership = scc_membership_query(db, project);
    let Some(batch) = membership
        .order
        .iter()
        .find(|comp| comp.iter().next().copied() == Some(scc_id))
    else {
        return Arc::new(SolvedScc::default());
    };

    let mut known_sigs: BTreeMap<DefinitionId, brink_analyzer::InferredSig> = BTreeMap::new();
    if let Some(deps) = membership.depends_on.get(&scc_id) {
        for &dep in deps {
            let solved = solve_scc_query(db, project, DefKey::new(db, dep));
            known_sigs.extend(solved.signatures.iter().map(|(k, v)| (*k, v.clone())));
        }
    }

    let index = inference_index_query(db, project);
    let inferable = inferable_defs_query(db, project);

    // Per-def HIR projection (Ruling 2b): only this batch's own members'
    // bodies. `member_bodies` keeps the owned `Arc<DefBody>`s alive for the
    // `Def` borrows built from them below.
    let member_bodies: BTreeMap<DefinitionId, Arc<DefBody>> = batch
        .iter()
        .filter_map(|&id| def_body_query(db, project, DefKey::new(db, id)).map(|b| (id, b)))
        .collect();
    let defs: Vec<brink_analyzer::Def<'_>> = member_bodies
        .iter()
        .map(|(&id, b)| brink_analyzer::Def {
            id,
            file: b.file,
            params: &b.params,
            body: &b.body,
            return_annotation: b.return_annotation.as_ref(),
        })
        .collect();

    // Pre-scan + narrow map (Ruling 1): union of every member's
    // referenced_globals, each resolved through the existing
    // per-declaring-file `signature_query`.
    let mut global_ids: BTreeSet<DefinitionId> = BTreeSet::new();
    for &member in batch {
        global_ids.extend(referenced_globals_query(db, project, DefKey::new(db, member)).iter());
    }
    // `value_type` covers the scalar/list/divert domain; `fn_type` (T1c
    // follow-up, issue #712) covers `Ty::Fn` separately, since
    // `InferredType` has no `Fn` form (`brink_analyzer::Sig::fn_type`'s
    // doc) — mirrors `brink_analyzer::infer::collect_globals`'s own
    // fallback exactly, so this narrowed path stays composed-equals-
    // monolithic with it.
    let mut globals: BTreeMap<DefinitionId, brink_analyzer::Ty> = BTreeMap::new();
    for gid in global_ids {
        if let Some(sig) = signature_query(db, project, DefKey::new(db, gid)) {
            if let Some(vt) = sig.value_type {
                globals.insert(gid, brink_analyzer::Ty::from(vt));
            } else if let Some(ft) = sig.fn_type.clone() {
                globals.insert(gid, ft);
            }
        }
    }

    // Narrowed resolutions (Ruling 2b): only this batch's own declaring
    // files' `resolve_query` results, deduplicated by file.
    let mut resolutions = ResolutionMap::new();
    let member_files: BTreeSet<FileId> = member_bodies.values().map(|b| b.file).collect();
    for file_id in member_files {
        if let Some(file) = project.files(db).iter().find(|f| f.file_id(db) == file_id) {
            let (file_map, _diags) = resolve_query(db, project, *file);
            resolutions.extend(file_map.iter().cloned());
        }
    }

    let opts = project.analysis_options(db);
    let inline_docs = inline_docs_query(db, project);
    let (signatures, bodies) = brink_analyzer::solve_scc(
        batch,
        &defs,
        index,
        &resolutions,
        &globals,
        inferable,
        known_sigs,
        opts.host_manifest.as_ref(),
        inline_docs,
    );
    Arc::new(SolvedScc { signatures, bodies })
}

/// Per-def inferred signature (`inferred_signature(def)`, FG-2 issue #631 —
/// the missing per-def API TM-2's firewall consumer needs most). `None` for
/// a def with no inferable body (not a knot/stitch, or an unknown id) — same
/// `None` contract as [`signature_query`]/[`infer_body_query`].
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 16384)]
pub(crate) fn inferred_signature_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Option<Arc<brink_analyzer::InferredSig>> {
    let def_id = def.def(db);
    let membership = scc_membership_query(db, project);
    let scc_id = *membership.member_of.get(&def_id)?;
    let solved = solve_scc_query(db, project, DefKey::new(db, scc_id));
    solved.signatures.get(&def_id).cloned().map(Arc::new)
}

/// One def's raw effect atoms (T2-1, docs/effects-spec.md §2/§4, issue #860 —
/// `def_effect_atoms(def)`). The per-def read/write/call-kind atom bundle the
/// effect-row fixpoint closes over, harvested by the exact same body walk
/// [`referenced_globals_query`]/[`call_edges_query`] already drive — same
/// declaring-file-only HIR + resolution dependency edges, same index-sourced
/// [`inferable_defs_query`] for classifying a call target as an edge vs. a
/// terminal external. Passes `None` for the manifest for the same reason
/// [`call_edges_query`] does: this pass discards every computed type, so the
/// manifest can never change the *structural* atom sets it keeps.
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 16384)]
pub(crate) fn def_effect_atoms_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Arc<brink_analyzer::EffectAtoms> {
    let index = inference_index_query(db, project);
    let def_id = def.def(db);
    let Some(declaring_file) = index.symbols.get(&def_id).map(|info| info.file) else {
        return Arc::new(brink_analyzer::EffectAtoms::default());
    };
    let Some(file) = project
        .files(db)
        .iter()
        .find(|f| f.file_id(db) == declaring_file)
    else {
        return Arc::new(brink_analyzer::EffectAtoms::default());
    };
    let hir = &lowered_query(db, *file).hir;
    let (resolutions, _diags) = resolve_query(db, project, *file);
    let inferable = inferable_defs_query(db, project);
    Arc::new(brink_analyzer::def_effect_atoms(
        def_id,
        &[(declaring_file, hir)],
        index,
        resolutions,
        inferable,
        None,
    ))
}

/// One SCC's finalized effect rows (T2-1, docs/effects-spec.md §4, issue #860
/// — the per-SCC effect fixpoint, lifting [`solve_scc_query`]'s exact shape to
/// the effect lattice). Reads every condensation-predecessor SCC's
/// `effects_scc_query` first for `known_rows` (recursion is acyclic — the
/// condensation is a DAG, same Fork 1 ruling [`solve_scc_query`] relies on, so
/// salsa never sees a cycle), collects every member's
/// [`def_effect_atoms_query`], then runs [`brink_analyzer::solve_scc_effects`]
/// for this component's own members. Returns an empty map for an id that isn't
/// any component's minimum member (defensive — never panics on a stale key).
///
/// `lru = 16384`: per-def (per-SCC) runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 16384)]
pub(crate) fn effects_scc_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    scc: DefKey<'db>,
) -> Arc<BTreeMap<DefinitionId, brink_analyzer::EffectRow>> {
    let scc_id: SccId = scc.def(db);
    let membership = scc_membership_query(db, project);
    let Some(batch) = membership
        .order
        .iter()
        .find(|comp| comp.iter().next().copied() == Some(scc_id))
    else {
        return Arc::new(BTreeMap::new());
    };

    let mut known_rows: BTreeMap<DefinitionId, brink_analyzer::EffectRow> = BTreeMap::new();
    if let Some(deps) = membership.depends_on.get(&scc_id) {
        for &dep in deps {
            let solved = effects_scc_query(db, project, DefKey::new(db, dep));
            known_rows.extend(solved.iter().map(|(k, v)| (*k, v.clone())));
        }
    }

    let atoms: BTreeMap<DefinitionId, brink_analyzer::EffectAtoms> = batch
        .iter()
        .map(|&id| {
            (
                id,
                (*def_effect_atoms_query(db, project, DefKey::new(db, id))).clone(),
            )
        })
        .collect();

    Arc::new(brink_analyzer::solve_scc_effects(
        batch,
        &atoms,
        &known_rows,
    ))
}

/// Per-def effect row (T2-1, docs/effects-spec.md §4, issue #860 —
/// `effects(def)`, the advisory row query sited beside
/// [`inferred_signature_query`]). Routes through the def's SCC exactly as
/// [`inferred_signature_query`] routes through [`solve_scc_query`]. `None` for
/// a def with no inferable body (not a knot/stitch, or an unknown id) — same
/// contract as [`inferred_signature_query`]/[`infer_body_query`].
///
/// **Consumed by `story_data` since T2-3** (#862): `populate_effect_rows`
/// reads this for every inferable def to emit the `EffectRows` section. The
/// row is still additive metadata the *runtime* does not consume (the linker
/// never reads `effect_rows`), so the oracle stays byte-identical — but the row
/// now ships in the `.inkb`, so this is no longer advisory-only. `lir_product`
/// and `diagnostics` still do not read it.
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647).
#[salsa::tracked(lru = 16384)]
pub(crate) fn effects_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Option<Arc<brink_analyzer::EffectRow>> {
    let def_id = def.def(db);
    let membership = scc_membership_query(db, project);
    let scc_id = *membership.member_of.get(&def_id)?;
    let solved = effects_scc_query(db, project, DefKey::new(db, scc_id));
    solved.get(&def_id).cloned().map(Arc::new)
}

/// Whole-project type inference — now an aggregation over
/// [`scc_membership_query`] + [`solve_scc_query`] (FG-2, issue #631; was a
/// single monolithic [`brink_analyzer::infer_project`] call
/// pre-decomposition). Still re-sourced off `inference_index`/`resolve`,
/// never `analysis_query` (FG-1 §3) — every query this reads
/// (`scc_membership_query` -> `call_graph_query` -> `call_edges_query` ->
/// `inference_inputs`) traces back to the same two roots, so the
/// pointer-identity guarantee `fg1_dependency_edges.rs` pins (a
/// diagnostics-only edit leaves this memo fully validated, never
/// re-executed) still holds after this refactor.
#[salsa::tracked(returns(ref))]
pub(crate) fn type_inference_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<InferenceResult> {
    let membership = scc_membership_query(db, project);
    let mut signatures = BTreeMap::new();
    let mut bodies = BTreeMap::new();
    let mut seen: BTreeSet<SccId> = BTreeSet::new();
    for comp in &membership.order {
        let Some(scc_id) = comp.iter().next().copied() else {
            continue;
        };
        if !seen.insert(scc_id) {
            continue;
        }
        let solved = solve_scc_query(db, project, DefKey::new(db, scc_id));
        signatures.extend(solved.signatures.iter().map(|(k, v)| (*k, v.clone())));
        bodies.extend(solved.bodies.iter().map(|(k, v)| (*k, v.clone())));
    }
    Arc::new(InferenceResult { signatures, bodies })
}

/// Per-def inferred body types (`infer_body(def)`). Re-pointed at
/// `solve_scc(scc_of(def))` (FG-2, issue #631) — was a view over the
/// whole-project `type_inference_query` memo pre-decomposition. `None` for a
/// def with no inferable body (not a knot/stitch, or an unknown id) — same
/// `None` contract as [`signature_query`].
///
/// `lru = 16384`: per-def runaway-guard ceiling (issue #647). `heap_size =
/// heap_size::infer_body_heap_size`: one of the five #538 estimators.
#[salsa::tracked(lru = 16384, heap_size = heap_size::infer_body_heap_size)]
pub(crate) fn infer_body_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Option<Arc<brink_analyzer::BodyTypes>> {
    let def_id = def.def(db);
    let membership = scc_membership_query(db, project);
    let scc_id = *membership.member_of.get(&def_id)?;
    let solved = solve_scc_query(db, project, DefKey::new(db, scc_id));
    solved.bodies.get(&def_id).cloned().map(Arc::new)
}

/// Per-file type diagnostics (`type_diagnostics(FileId)`). **Advisory-only
/// in this slice**: TM-1 produces inference *results* (`infer_body`,
/// `signature`), not new user-facing diagnostics (typed-mode-spec §9 step 1:
/// "essentially no new user-facing diagnostics") — this always returns
/// empty today. The query exists now, correctly shaped, so TM-3 (strict
/// mode's `Unknown`-escape errors) only has to fill the body in rather than
/// threading a new query through every consumer.
#[salsa::tracked(returns(ref))]
pub(crate) fn type_diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Vec<Diagnostic> {
    let _ = (db, project, file);
    Vec::new()
}

// ─── Layer 3: lowering / codegen (whole-project in slice B) ──────────

/// Outcome of the pipeline through LIR lowering, mirroring
/// `brink-compiler`'s `compile_lir` stage sequence exactly.
///
/// `program` is `None` when errors (or a missing entry point) prevented
/// lowering; `errors`/`warnings` are the suppression-filtered, partitioned
/// diagnostics (plus LIR lowering warnings on success).
#[derive(Clone, Default)]
pub struct LirProduct {
    /// The lowered LIR program, if diagnostics allowed lowering to run.
    pub program: Option<Arc<brink_ir::lir::Program>>,
    /// Error-severity diagnostics (compilation failed if non-empty).
    pub errors: Vec<Diagnostic>,
    /// Warning-severity diagnostics (including LIR warnings on success).
    pub warnings: Vec<Diagnostic>,
}

/// `lir::Program` has no `PartialEq`, so program identity (`Arc::ptr_eq`) is
/// the only cheap proxy. This impl exists solely to satisfy salsa's update
/// fallback — backdating is disabled via `no_eq` on [`lir_query`], so it is
/// never used to claim two independently-computed programs equal.
impl PartialEq for LirProduct {
    fn eq(&self, other: &Self) -> bool {
        let program_eq = match (&self.program, &other.program) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        program_eq && self.errors == other.errors && self.warnings == other.warnings
    }
}

/// The LIR-lowering half of [`lir_query`] split out on its own (issue #791 /
/// FG-4a — PR #753's seam finding #3), gated on [`has_errors_query`]'s
/// narrow boolean instead of the full [`analysis_diagnostics_query`] vector.
/// Reads only [`resolutions_index_query`], every file's [`lowered_query`]
/// HIR, [`include_graph_query`], and the narrow [`type_policy_query`]
/// projection — never the raw analysis diagnostics, and never the raw
/// `AnalysisOptions` input field (issue #806) — so a diagnostics edit that
/// changes *content* without flipping [`has_errors_query`]'s verdict, and an
/// options edit that doesn't change the `types` policy, both leave this memo
/// (and its `Arc<Program>` pointer) fully validated, not re-executed
/// (`fg4a_dependency_edges.rs`).
/// `no_eq`: `lir::Program` has no `PartialEq`, same reasoning as
/// [`LirProduct`]'s own impl below.
#[derive(Clone, Default)]
pub(crate) struct LirLowering {
    /// The lowered LIR program, if lowering succeeded with no Error-severity
    /// lowering diagnostic.
    pub program: Option<Arc<brink_ir::lir::Program>>,
    /// Error-severity diagnostics raised *during* LIR lowering (never from
    /// `analysis_diagnostics_query` — those are [`lir_query`]'s own concern).
    pub errors: Vec<Diagnostic>,
    /// Warning-severity diagnostics raised during LIR lowering.
    pub warnings: Vec<Diagnostic>,
}

impl PartialEq for LirLowering {
    fn eq(&self, other: &Self) -> bool {
        let program_eq = match (&self.program, &other.program) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        program_eq && self.errors == other.errors && self.warnings == other.warnings
    }
}

// ─── FG-4d: per-container LIR chunk memos + link ─────────────────────
//
// `lir_lowering_query` below is now the **link phase**
// (`docs/fine-grained-salsa-proposal.md` §5 + the three-resolution-moments
// appendix): it reads a per-knot chunk memo per definition, assembles them
// (plus the whole-root chunk) into a `Program`, and gates on
// `has_errors_query`. The per-knot memos (`lir_knot_chunk_query`) are the
// FG-4d win — an edit that leaves a knot's declaring file, the project
// resolutions, and the struct-shape projection unchanged leaves that knot's
// chunk `Arc` pointer-identical (non-re-execution), and the whole-project
// link re-runs but backdates on the `StoryData` `Eq` firebreak
// (`story_data_query`). Byte-identity with the monolithic path is by
// construction: the chunk lowering and assembly are the *same* `brink-ir`
// functions `lower_to_program_with_type_mode` composes, fed the same inputs
// in the same interleaved walk order.
//
// Input-breadth limit (issue #830, #815): the per-knot memo depends on the
// whole-project `resolutions_index_query` and `struct_shape_data_query`, so
// non-re-execution holds for edits those two backdate across (a
// diagnostics-only / `AnalysisOptions` edit, and — for a knot in an
// *unedited* file — any edit whose resolutions/struct-shapes are unchanged).
// `topological_order` is now narrowed to `entry`'s transitive `INCLUDE`
// closure (issue #815, landed separately) rather than falling back to all
// project files, so `struct_shape_data_query` and the link's own inputs are
// scoped the same way.

/// The cutoff-friendly struct-shape projection (FG-4d): the
/// `NameId`-free, `Eq`-able [`StructShapeData`] the per-knot chunk memo reads
/// instead of every file's HIR. Backdates when no `STRUCT` declaration (or
/// struct-typed global annotation) changed, so an unrelated edit leaves the
/// knot chunks that read it pointer-identical. Reads the same
/// `resolutions_index_query` index the monolithic path's `build_prelude`
/// does, so its ids/offsets are byte-identical.
#[salsa::tracked(returns(ref))]
pub(crate) fn struct_shape_data_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> brink_ir::lir::StructShapeData {
    let Some(entry) = project.entry(db) else {
        return brink_ir::lir::StructShapeData::default();
    };
    let files = project.files(db);
    let graph = include_graph_query(db, project);
    let by_id: LookupMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let topo = graph.topological_order(entry);
    let hir_refs: Vec<(FileId, &HirFile)> = topo
        .iter()
        .filter_map(|id| by_id.get(id).map(|f| (*id, &lowered_query(db, *f).hir)))
        .collect();
    let resolved = resolutions_index_query(db, project);
    brink_ir::lir::build_struct_shape_data(&hir_refs, &resolved.index)
}

/// One file's declaration-only HIR projection (issue #839 / FG-4e):
/// `constants`/`variables`/`lists`/`structs`/`externals` kept, `root_content`
/// and every knot's body dropped to `Block::default()`/`Vec::new()`. Backed
/// by [`HirFile`]'s derived `PartialEq`, so a body-only edit — one that
/// leaves every declaration untouched — backdates this memo across the edit,
/// same as [`normalized_stamped_query`] backdates the file's syntax tree.
///
/// This is the per-file dependency edge [`lir_prelude_decls_query`] reads
/// instead of the raw, body-carrying [`lowered_query`]: `brink_ir::lir`'s
/// declaration-collection passes ([`collect_globals`], [`collect_lists`],
/// [`collect_externals`], [`build_shape_table`], [`build_global_shape_map`])
/// never read `root_content`/`knots` (see `PreludeDecls`'s doc in
/// `brink-ir`), so stripping them here is behavior-neutral for every reader
/// and turns "any reachable file changed" into "a reachable file's
/// declarations changed" as the trigger for re-interning the project name
/// table.
///
/// [`collect_globals`]: brink_ir::lir::build_prelude_decls
/// [`collect_lists`]: brink_ir::lir::build_prelude_decls
/// [`collect_externals`]: brink_ir::lir::build_prelude_decls
/// [`build_shape_table`]: brink_ir::lir::build_prelude_decls
/// [`build_global_shape_map`]: brink_ir::lir::build_prelude_decls
#[salsa::tracked(returns(ref))]
pub(crate) fn decl_hir_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> HirFile {
    let _ = project;
    let hir = &lowered_query(db, file).hir;
    HirFile {
        root_content: brink_ir::hir::Block::default(),
        knots: Vec::new(),
        ..hir.clone()
    }
}

/// One file's HIR after the pre-LIR normalize + container-id stamp passes,
/// memoized per file (FG-4d). Without this, each of a K-knot file's per-knot
/// chunk memos would repeat the file's normalize+stamp, turning a cold
/// compile into O(K²) work per file; sharing it here keeps the per-def split
/// from regressing cold compile. Both passes are per-file independent, so
/// this is byte-identical to the file's slice of the whole-project prelude.
/// Reads the whole-project index only for label-container stamping (the same
/// index the monolithic path stamps with), and `HirFile`'s value `Eq`
/// backdates it across edits that leave this file's normalized shape
/// unchanged.
#[salsa::tracked(returns(ref))]
pub(crate) fn normalized_stamped_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Arc<HirFile> {
    let resolved = resolutions_index_query(db, project);
    let mut hir = lowered_query(db, file).hir.clone();
    brink_ir::normalize_file(&mut hir);
    let mut slice = [(file.file_id(db), hir)];
    brink_ir::stamp_container_ids(&mut slice, &resolved.index);
    let [(_, stamped)] = slice;
    Arc::new(stamped)
}

/// [`brink_ir::lir::PreludeDecls`] wrapped so [`lir_prelude_decls_query`]
/// (`no_eq`: the wrapped `ShapeTable`/`GlobalShapeMap` carry `NameId`s valid
/// only within this specific prelude's numbering, so they cannot be `Eq`
/// without a NameId-free relocation redesign — same reasoning as
/// [`LirLowering`]'s `program`) can still satisfy salsa's `Update` bound.
/// `Arc`-wrapped so a validated (non-re-executed) memo hands back the *same*
/// allocation — `Arc::ptr_eq` is the non-re-execution proof, same pattern as
/// [`LoweredChunk`].
#[derive(Clone)]
pub(crate) struct PreludeDeclsResult {
    pub decls: Arc<brink_ir::lir::PreludeDecls>,
}

impl PartialEq for PreludeDeclsResult {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.decls, &other.decls)
    }
}

/// The whole-project declaration-level prelude (issue #839 / FG-4e —
/// `docs/fine-grained-salsa-proposal.md`'s pattern, applied past structs to
/// the rest of `build_prelude`): [`brink_ir::lir::build_prelude_decls`] over
/// every entry-reachable file's [`decl_hir_query`] projection instead of the
/// monolithic link's inline `build_prelude` call over raw, body-carrying
/// HIR. Because [`decl_hir_query`] itself backdates across a body-only edit,
/// a knot body edit anywhere in the project leaves *this* query's recorded
/// dependencies unchanged — [`lir_lowering_query`] no longer pays
/// `collect_globals`/`collect_lists`/`collect_externals`/`build_shape_table`/
/// `build_global_shape_map`'s full re-interning cost on every recompile, only
/// when some reachable file's actual declarations change (`fg4e_prelude_
/// decls.rs`).
///
/// Same input-breadth limit as [`struct_shape_data_query`] (issue #815):
/// scoped to `entry`'s transitive `INCLUDE` closure, not full backdating
/// across *any* unrelated project file's declarations (that would need a
/// per-file decl memo feeding the interning step directly, which the
/// project-wide, order-sensitive `NameTable` this produces does not allow
/// without the same NameId-free-projection-plus-relocation redesign
/// `StructShapeData` did for structs alone — out of this slice's scope, see
/// the PR description).
#[salsa::tracked(no_eq)]
pub(crate) fn lir_prelude_decls_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> PreludeDeclsResult {
    let type_mode = match type_policy_query(db, project) {
        TypePolicy::Strict => brink_ir::lir::TypeMode::Strict,
        TypePolicy::Gradual => brink_ir::lir::TypeMode::Gradual,
    };
    let Some(entry) = project.entry(db) else {
        return PreludeDeclsResult {
            decls: Arc::new(brink_ir::lir::PreludeDecls::empty(type_mode)),
        };
    };
    let files = project.files(db);
    let by_id: LookupMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let graph = include_graph_query(db, project);
    let topo = graph.topological_order(entry);
    let decl_refs: Vec<(FileId, &HirFile)> = topo
        .iter()
        .filter_map(|id| {
            by_id
                .get(id)
                .map(|f| (*id, decl_hir_query(db, project, *f)))
        })
        .collect();
    let resolved = resolutions_index_query(db, project);
    let decls = brink_ir::lir::build_prelude_decls(
        &decl_refs,
        &resolved.index,
        &resolved.resolutions,
        type_mode,
    );
    PreludeDeclsResult {
        decls: Arc::new(decls),
    }
}

/// Interned key for [`lir_knot_chunk_query`]: a knot identified by its
/// declaring file and its index within that file's knot list. Keyed on
/// `(FileId, knot_index)` rather than the knot's `DefinitionId` so two knots
/// that would hash to the same address (e.g. same-named file-local knots)
/// never collapse onto one memo — the byte-identity hazard `DefinitionId`
/// keying would carry.
#[salsa::interned]
pub(crate) struct KnotChunkKey<'db> {
    pub file: FileId,
    pub knot_index: u32,
}

/// One knot's lowered LIR chunk plus its lowering diagnostics — the value a
/// per-knot memo stores. `chunk` is `Arc`-wrapped so a validated (non-re-
/// executed) memo hands back the *same* allocation, which
/// `Arc::ptr_eq` detects (the non-re-execution proof — same reasoning as
/// `LirLowering`'s `program`). The `PartialEq` exists solely to satisfy
/// salsa's `Update` bound; `no_eq` disables backdating, so it is never used
/// to claim two independently-lowered chunks equal.
#[derive(Clone, Default)]
pub(crate) struct LoweredChunk {
    pub chunk: Arc<brink_ir::lir::ScopeChunk>,
    pub diagnostics: Vec<Diagnostic>,
}

impl PartialEq for LoweredChunk {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.chunk, &other.chunk) && self.diagnostics == other.diagnostics
    }
}

/// Lower one knot into a self-contained LIR chunk — the per-`DefinitionId`
/// unit of FG-4d. Reads only the declaring file's `lowered_query` HIR
/// (per-file edge), the whole-project `resolutions_index_query`
/// (backdates across body/diagnostics edits), and
/// `struct_shape_data_query` (backdates unless a struct declaration
/// changed) — so a knot in an unedited file whose resolutions and struct
/// shapes are unchanged keeps its chunk `Arc` across the edit. `no_eq`:
/// `ScopeChunk` has no `PartialEq` (holds `lir::Container`), so this never
/// backdates — the link re-runs and re-anchors on `StoryData`'s `Eq`.
#[salsa::tracked(no_eq)]
pub(crate) fn lir_knot_chunk_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    key: KnotChunkKey<'_>,
) -> LoweredChunk {
    let file_id = key.file(db);
    let knot_index = key.knot_index(db) as usize;
    let Some(source) = project
        .files(db)
        .iter()
        .copied()
        .find(|f| f.file_id(db) == file_id)
    else {
        return LoweredChunk::default();
    };

    let resolved = resolutions_index_query(db, project);
    let shape_data = struct_shape_data_query(db, project);
    // Narrow `.types` projection (issue #806/#809) — not the raw
    // `AnalysisOptions` field — so an unrelated options edit doesn't
    // re-execute this chunk memo.
    let type_mode = match type_policy_query(db, project) {
        TypePolicy::Strict => brink_ir::lir::TypeMode::Strict,
        TypePolicy::Gradual => brink_ir::lir::TypeMode::Gradual,
    };
    let file_paths: LookupMap<FileId, String> = project
        .files(db)
        .iter()
        .map(|f| (f.file_id(db), f.path(db).clone()))
        .collect();

    // The file's normalized+stamped HIR, shared across all its knots'
    // memos (so a K-knot file normalizes once, not K times).
    let hir_file = normalized_stamped_query(db, project, source);
    let Some(knot) = hir_file.knots.get(knot_index) else {
        return LoweredChunk::default();
    };

    let (chunk, diagnostics) = brink_ir::lir::lower_knot_chunk_incremental(
        hir_file,
        knot,
        &resolved.index,
        &resolved.resolutions,
        &file_paths,
        shape_data,
        type_mode,
        file_id,
    );
    LoweredChunk {
        chunk: Arc::new(chunk),
        diagnostics,
    }
}

/// The project's TM-3 `types` policy as its own narrow projection query
/// (issue #806 / PR #809, mirroring [`has_errors_query`]'s pattern): a raw
/// `project.analysis_options(db).types` field read inside
/// [`lir_lowering_query`] would register a dependency on the *whole*
/// `AnalysisOptions` input field, so any options edit — registering a host
/// manifest, toggling `semantic_type_check`, even re-setting the identical
/// value — would force the `no_eq` lowering memo to fully re-execute and
/// allocate a fresh `Arc<Program>`. `TypePolicy`'s derived `Eq` is the
/// cheapest possible cutoff: an options edit that doesn't change `.types`
/// re-executes only this trivial projection, backdates it, and leaves
/// [`lir_lowering_query`] (and its `Arc<Program>` pointer) fully validated —
/// see `fg4a_dependency_edges.rs`. Behavior-neutral by construction: the
/// same field, read one query-hop later.
///
/// FG-4d (issue #830) also routes the per-knot chunk memos' `.types` read
/// through this projection, so an `AnalysisOptions` edit that leaves `.types`
/// unchanged keeps every knot chunk `Arc` pointer-identical.
#[salsa::tracked]
pub(crate) fn type_policy_query(db: &dyn salsa::Database, project: ProjectInput) -> TypePolicy {
    project.analysis_options(db).types
}

/// FG-4d **link phase**: assemble the per-knot chunk memos and the whole-root
/// chunk into a `Program`. Gated on `has_errors_query` (unchanged). See the
/// section comment above for the byte-identity and non-re-execution
/// arguments.
#[salsa::tracked(no_eq)]
pub(crate) fn lir_lowering_query(db: &dyn salsa::Database, project: ProjectInput) -> LirLowering {
    let Some(entry) = project.entry(db) else {
        return LirLowering::default();
    };
    if has_errors_query(db, project) {
        return LirLowering::default();
    }

    let files = project.files(db);
    let resolved = resolutions_index_query(db, project);

    // LIR inputs in topological include order (paste-before semantics),
    // mirroring `Driver::lir_inputs`. Narrowed to `entry`'s transitive
    // `INCLUDE` closure (issue #815) — files outside it never lower here;
    // their diagnostics still run independently via
    // `analysis_diagnostics_query`/`diagnostics_query` below and in
    // `super::diagnostics_query`, which iterate `project.files(db)`
    // directly rather than through this topo order.
    let graph = include_graph_query(db, project);
    let by_id: LookupMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let topo = graph.topological_order(entry);
    let paths: LookupMap<FileId, String> = topo
        .iter()
        .filter_map(|id| by_id.get(id).map(|f| (*id, f.path(db).clone())))
        .collect();

    // TM-4c (`docs/typed-mode-spec.md` §6): the project's `types` policy
    // gates static-offset record field ops, and also partitions lowering
    // diagnostics by effective severity below. Read through the narrow
    // [`type_policy_query`] projection, never the raw `AnalysisOptions`
    // input field — see its doc comment (issue #806).
    //
    // [`brink-ir`'s local `TypeMode` mirror](brink_ir::lir::TypeMode) is now
    // decided once, inside [`lir_prelude_decls_query`]'s own
    // `type_policy_query` read — this function no longer needs its own copy
    // (issue #839 / FG-4e removed the direct `build_prelude` call that used
    // to consume it here).
    let types = type_policy_query(db, project);

    // Whole-project prelude (issue #839 / FG-4e): declarations + struct
    // shapes + the seeded name table come from [`lir_prelude_decls_query`]
    // (its own memo, cutoff-friendly across body-only edits — see its doc),
    // and the normalized+stamped HIR reuses the already-memoized per-file
    // [`normalized_stamped_query`] instead of `build_prelude`'s inline
    // normalize+stamp recompute. `assemble_prelude` is pure composition —
    // byte-identical to the monolithic `build_prelude` by construction (see
    // `PreludeDecls`'s doc in `brink-ir`).
    let prelude_decls = lir_prelude_decls_query(db, project);
    let normalized: Vec<(FileId, HirFile)> = topo
        .iter()
        .filter_map(|id| {
            by_id
                .get(id)
                .map(|f| (*id, (**normalized_stamped_query(db, project, *f)).clone()))
        })
        .collect();
    let prelude = brink_ir::lir::assemble_prelude((*prelude_decls.decls).clone(), normalized);
    let (root_chunks, root_temp_slots) = brink_ir::lir::lower_root_content_for_prelude(
        &prelude,
        &resolved.index,
        &resolved.resolutions,
        &paths,
    );

    // Interleave in walk order (per file: root content, then that file's
    // knots) — the order the assembler dedups names against. Knot chunks come
    // from the per-knot memos; declaration diagnostics lead, matching the
    // monolithic diagnostic order exactly.
    let mut lir_diagnostics = prelude.decl_diagnostics.clone();
    let mut ordered_chunks: Vec<brink_ir::lir::ScopeChunk> = Vec::new();
    let prelude_files = prelude.files();
    let mut root_iter = root_chunks.into_iter();
    for (file_id, hir_file) in &prelude_files {
        if let Some((chunk, diags)) = root_iter.next() {
            ordered_chunks.push(chunk);
            lir_diagnostics.extend(diags);
        }
        for knot_index in 0..hir_file.knots.len() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a file won't declare anywhere near u32::MAX knots"
            )]
            let key = KnotChunkKey::new(db, *file_id, knot_index as u32);
            let lowered = lir_knot_chunk_query(db, project, key);
            ordered_chunks.push((*lowered.chunk).clone());
            lir_diagnostics.extend(lowered.diagnostics.clone());
        }
    }

    let program =
        brink_ir::lir::assemble_program(&prelude, ordered_chunks, root_temp_slots, &resolved.index);

    // LIR lowering itself is total (T1b-2: every construct lowers to a
    // program regardless of dialect). Error-severity lowering diagnostics
    // (T1b-3's E055/E056) still gate `program: None` exactly like an
    // analysis-phase error would.
    let (lir_errors, lir_warnings): (Vec<Diagnostic>, Vec<Diagnostic>) = lir_diagnostics
        .into_iter()
        .partition(|d| brink_analyzer::effective_severity(d.code, types) == Severity::Error);

    if lir_errors.is_empty() {
        LirLowering {
            program: Some(Arc::new(program)),
            errors: lir_errors,
            warnings: lir_warnings,
        }
    } else {
        // A lowering-phase Error-severity diagnostic (E055/E056) blocks
        // compilation: surface it, never hand back a diagnostically-invalid
        // program.
        LirLowering {
            program: None,
            errors: lir_errors,
            warnings: lir_warnings,
        }
    }
}

/// Whole-project LIR lowering (slice B: one project query; slice C splits it
/// per container). `no_eq`: `lir::Program` has no `PartialEq`, so this memo
/// never backdates — [`story_data_query`] backdates on `StoryData` instead.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn lir_query(db: &dyn salsa::Database, project: ProjectInput) -> LirProduct {
    let files = project.files(db);
    let Some(entry) = project.entry(db) else {
        return LirProduct::default();
    };

    // FG-3 (issue #632): read the assembled diagnostics directly, not
    // through the bundled `analysis_query` — a resolutions-only change (no
    // diagnostic anywhere differs) leaves this half's dependency fully
    // validated.
    let diagnostics = analysis_diagnostics_query(db, project);

    // Diagnostic gate — the same report `compile_lir` builds.
    let disable_all = files
        .iter()
        .find(|f| f.file_id(db) == entry)
        .is_some_and(|f| suppressions_query(db, *f).disable_all);
    let inputs: Vec<FileDiagnostics<'_>> = files
        .iter()
        .map(|f| FileDiagnostics {
            file: f.file_id(db),
            source: f.text(db),
            suppressions: suppressions_query(db, *f),
            lowering: &lowered_query(db, *f).diagnostics,
        })
        .collect();
    let types = project.analysis_options(db).types;
    let (mut errors, mut warnings) =
        partition_diagnostics(&inputs, diagnostics, disable_all, types);

    // FG-4a (issue #791, PR #753 seam finding #3): the gate deciding
    // whether to attempt (potentially expensive) LIR lowering reads the
    // narrow `has_errors_query` boolean projection instead of
    // `errors.is_empty()` directly. `errors`/`warnings` above are still
    // needed for *this* query's own return value — diagnostic content, not
    // just presence — but the lowering itself is fully delegated to
    // `lir_lowering_query`, which reads `has_errors_query` only (never the
    // raw diagnostics vector), so its `Arc<Program>` survives a diagnostics
    // edit that doesn't flip the error verdict. `has_errors_query` computes
    // this exact `errors.is_empty()` value from the same inputs (see its
    // doc comment), so this is behavior-neutral by construction.
    if has_errors_query(db, project) {
        return LirProduct {
            program: None,
            errors,
            warnings,
        };
    }

    let lowering = lir_lowering_query(db, project);
    errors.extend(lowering.errors);
    warnings.extend(lowering.warnings);

    LirProduct {
        program: lowering.program,
        errors,
        warnings,
    }
}

/// Outcome of the full pipeline: compiled [`StoryData`] or the diagnostics
/// that prevented it. Batch compile = pull this one query (spec §5).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompileProduct {
    /// The compiled story, if compilation succeeded.
    pub story: Option<Arc<StoryData>>,
    /// Error-severity diagnostics (compilation failed if non-empty).
    pub errors: Vec<Diagnostic>,
    /// Warning-severity diagnostics.
    pub warnings: Vec<Diagnostic>,
}

/// Whole-project codegen: LIR → [`StoryData`] via `brink-codegen-inkb`.
///
/// `brink_codegen_inkb::emit` only fails on a `Program` that violates an
/// invariant an earlier, non-suppressible LIR-lowering diagnostic (E057) is
/// supposed to guarantee — see `CodegenError`'s doc comment and #586. That
/// can't happen via this query today (`lir.program` is only `Some` when
/// `lir.errors` is already empty, which requires E057 to not have fired),
/// but codegen has no way to prove that structurally, so this still handles
/// the `Err` case for real rather than assuming it away: surfaced as an
/// `E060` compile error (no meaningful source span survives into codegen
/// for this class of defect, so it's anchored at the project entry file
/// with an empty range) rather than silently downgrading to `story: None`
/// with an empty `errors` — which would look like "nothing to compile" to
/// every caller instead of "codegen refused to compile this."
/// Populate the T2-3 `EffectRows` table (#862, `docs/effects-spec.md` §11):
/// one factored row per inferable definition (every knot/stitch — the host's
/// resume-scheduling estimate, §12.1). Rows are read straight off the advisory
/// [`effects_query`] fixpoint and lowered to wire vocabulary:
///
/// - `reads`/`writes` cells ride through as [`DefinitionId`]s (already sorted —
///   they come from `BTreeSet`s).
/// - each call-kind name is interned into the story's `name_table`
///   (find-or-append, in the row's sorted call order for determinism) and
///   emitted as a [`CallAtom`] with the capability-parameter slot populated
///   `Any` (component-granular, the v1 value) and the reserved
///   handle-parameter slot left `None`.
/// - the per-dispatch entry list is empty in v1 (call-through-value is inferred
///   as opaque, folded into the direct part) — but the row structure ships the
///   slot so §7 narrowing is not structurally foreclosed.
///
/// Appending call names to `name_table` is safe for inertness: existing
/// `NameId` indices are unchanged, and the only references to the appended
/// names are from `effect_rows`, which the runtime does not read.
#[expect(clippy::cast_possible_truncation)]
fn populate_effect_rows(db: &dyn salsa::Database, project: ProjectInput, story: &mut StoryData) {
    let inferable = inferable_defs_query(db, project);
    if inferable.is_empty() {
        return;
    }

    // Owned name→id lookup so we can both read and append to `name_table`.
    let mut name_lookup: BTreeMap<String, u16> = story
        .name_table
        .iter()
        .enumerate()
        .map(|(i, s)| (s.clone(), i as u16))
        .collect();

    let mut rows: Vec<EffectRowEntry> = Vec::with_capacity(inferable.len());
    for &def in inferable {
        let Some(row) = effects_query(db, project, DefKey::new(db, def)) else {
            continue;
        };
        let mut calls: Vec<CallAtom> = Vec::with_capacity(row.calls.len());
        for name in &row.calls {
            let id = if let Some(&id) = name_lookup.get(name) {
                id
            } else {
                let id = story.name_table.len() as u16;
                story.name_table.push(name.clone());
                name_lookup.insert(name.clone(), id);
                id
            };
            calls.push(CallAtom {
                name: NameId(id),
                capability: CapabilityParam::Any,
                handle_param: None,
            });
        }
        rows.push(EffectRowEntry {
            def,
            direct: DirectEffects {
                reads: row.reads.iter().copied().collect(),
                writes: row.writes.iter().copied().collect(),
                calls,
                opaque: row.opaque,
            },
            dispatches: Vec::new(),
        });
    }
    // `inferable` is a `BTreeSet`, so `rows` is already sorted by `def`.
    story.effect_rows = rows;
}

#[salsa::tracked(returns(ref))]
pub(crate) fn story_data_query(db: &dyn salsa::Database, project: ProjectInput) -> CompileProduct {
    let lir = lir_query(db, project);
    let Some(program) = lir.program.as_ref() else {
        return CompileProduct {
            story: None,
            errors: lir.errors.clone(),
            warnings: lir.warnings.clone(),
        };
    };
    match brink_codegen_inkb::emit(program) {
        Ok(mut story) => {
            // T2-3 (#862, `docs/effects-spec.md` §11): first real emission into
            // the `EffectRows` section. Codegen has no analyzer access, so the
            // rows are attached here — this query is the one canonical codegen
            // site (FG-6), so there is exactly one emission point. The rows are
            // additive metadata the runtime does not consume yet, so episodes
            // stay byte-identical (the linker never reads `effect_rows`).
            populate_effect_rows(db, project, &mut story);
            CompileProduct {
                story: Some(Arc::new(story)),
                errors: lir.errors.clone(),
                warnings: lir.warnings.clone(),
            }
        }
        Err(err) => {
            let mut errors = lir.errors.clone();
            errors.push(Diagnostic {
                file: project.entry(db).unwrap_or(FileId(0)),
                range: rowan::TextRange::default(),
                message: format!("{}: {err}", DiagnosticCode::E060.title()),
                code: DiagnosticCode::E060,
            });
            CompileProduct {
                story: None,
                errors,
                warnings: lir.warnings.clone(),
            }
        }
    }
}

// ─── Shared diagnostic partitioning ──────────────────────────────────

/// Per-file inputs to [`partition_diagnostics`].
pub struct FileDiagnostics<'a> {
    /// The file these diagnostics belong to.
    pub file: FileId,
    /// The file's source text (for line-directive matching).
    pub source: &'a str,
    /// The file's parsed suppression directives.
    pub suppressions: &'a Suppressions,
    /// The file's lowering + syntax diagnostics.
    pub lowering: &'a [Diagnostic],
}

/// Default (empty) suppressions for analysis diagnostics that reference a
/// file absent from the input set — mirrors the old driver's
/// `unwrap_or_default` behavior.
static NO_SUPPRESSIONS: Suppressions = Suppressions {
    disable_all: false,
    disable_file: false,
    line_directives: std::collections::BTreeMap::new(),
};

/// Collect all diagnostics (lowering + analysis), apply suppressions, and
/// partition into `(errors, warnings)`.
///
/// This is the single implementation behind both `brink-driver`'s
/// `collect_diagnostics` and the [`lir_query`] gate — extracted so the query
/// path and the legacy driver path cannot drift. `files` must be ordered by
/// [`FileId`] (both callers iterate the sorted file set).
///
/// `disable_all`: whether the entry file carries `brink-disable-all`
/// (compiler mode skips analysis diagnostics entirely); pass `false` for LSP
/// mode where analysis diagnostics are always included.
///
/// `types`: the project's TM-3 `types` policy — every diagnostic is
/// partitioned by [`brink_analyzer::effective_severity`], not the raw
/// [`DiagnosticCode::severity`] default, so `E063` (annotation-vs-inference
/// mismatch) partitions as an error under `types = strict` and a warning
/// under `types = gradual` (the #640-round ruling) no matter which of this
/// function's two callers ([`lir_query`] or `brink-driver`'s
/// `collect_diagnostics`) is asking.
#[must_use]
pub fn partition_diagnostics(
    files: &[FileDiagnostics<'_>],
    analysis_diagnostics: &[Diagnostic],
    disable_all: bool,
    types: brink_analyzer::TypePolicy,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let mut partition = |d: Diagnostic| {
        if brink_analyzer::effective_severity(d.code, types) == Severity::Error {
            errors.push(d);
        } else {
            warnings.push(d);
        }
    };

    // Per-file lowering diagnostics.
    for input in files {
        let filtered = apply_suppressions(
            input.file,
            input.source,
            input.lowering.to_vec(),
            input.suppressions,
        );
        for d in filtered {
            partition(d);
        }
    }

    // Analysis diagnostics (unless disable_all).
    if !disable_all {
        let mut by_file: LookupMap<FileId, Vec<Diagnostic>> = LookupMap::new();
        for d in analysis_diagnostics {
            by_file.entry(d.file).or_default().push(d.clone());
        }
        // Sort by FileId for determinism.
        let mut file_ids: Vec<_> = by_file.keys().copied().collect();
        file_ids.sort_by_key(|id| id.0);
        for fid in file_ids {
            let diags = by_file.remove(&fid).unwrap_or_default();
            let (source, suppressions) = files
                .iter()
                .find(|input| input.file == fid)
                .map_or(("", &NO_SUPPRESSIONS), |input| {
                    (input.source, input.suppressions)
                });
            let filtered = apply_suppressions(fid, source, diags, suppressions);
            for d in filtered {
                partition(d);
            }
        }
    }

    (errors, warnings)
}

// ─── Per-file lowering (ported from the retired `set_file` path) ─────

/// Lower one parsed file to HIR + manifest + diagnostics.
///
/// This is the exact composition the pre-salsa `set_file` performed
/// (per-knot lowering + top-level lowering + assembly + syntax errors), kept
/// intact rather than collapsed onto `brink_ir::lower` so the output is
/// byte-identical to the previous pipeline.
fn lower_file(file_id: FileId, parse: &Parse) -> LoweredFile {
    let tree = parse.tree();

    // Per-knot lowering (document order).
    let knot_entries: Vec<_> = tree
        .knots()
        .map(|knot_ast| lower_single_knot(file_id, &knot_ast))
        .collect();

    // Top-level lowering (everything outside knots).
    let (root_content, top_level_knots, top_manifest, top_diagnostics) =
        lower_top_level(file_id, &tree);

    // Assemble a complete `HirFile`: use `lower()` for the declarations
    // (variables, constants, lists, externals, includes), then replace knots
    // and root content with the per-knot/top-level products above.
    let (mut hir, _full_manifest, _full_diag) = lower(file_id, &tree);
    hir.knots = knot_entries
        .iter()
        .filter_map(|(knot, _, _)| knot.clone())
        .collect();
    hir.knots.extend(top_level_knots);
    hir.root_content = root_content;

    // Merge manifests: top-level + all knots.
    let mut manifest = top_manifest;
    for (_, knot_manifest, _) in &knot_entries {
        merge_manifest_into(&mut manifest, knot_manifest);
    }

    // Merge diagnostics, then surface parser/syntax errors as compile
    // diagnostics (`E037`) so malformed source fails the compile.
    let mut diagnostics = top_diagnostics;
    for (_, _, knot_diags) in &knot_entries {
        diagnostics.extend(knot_diags.iter().cloned());
    }
    diagnostics.extend(parse.errors().iter().map(|e| Diagnostic {
        file: file_id,
        range: e.range,
        message: e.message.clone(),
        code: DiagnosticCode::E037,
    }));

    LoweredFile {
        hir,
        manifest,
        diagnostics,
    }
}

/// Merge `src` manifest fields into `dst`.
fn merge_manifest_into(dst: &mut SymbolManifest, src: &SymbolManifest) {
    dst.knots.extend(src.knots.iter().cloned());
    dst.stitches.extend(src.stitches.iter().cloned());
    dst.variables.extend(src.variables.iter().cloned());
    dst.constants.extend(src.constants.iter().cloned());
    dst.lists.extend(src.lists.iter().cloned());
    dst.externals.extend(src.externals.iter().cloned());
    dst.labels.extend(src.labels.iter().cloned());
    dst.list_items.extend(src.list_items.iter().cloned());
    dst.locals.extend(src.locals.iter().cloned());
    dst.unresolved.extend(src.unresolved.iter().cloned());
    dst.docs
        .extend(src.docs.iter().map(|(k, v)| (k.clone(), v.clone())));
}
