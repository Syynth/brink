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
//! [`resolve_query`], [`signature_query`], and [`analysis_query`].
//!
//! Layer 3 (lowering/codegen, whole-project in this slice): [`lir_query`],
//! [`story_data_query`], and the per-file [`diagnostics_query`].
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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, AnalysisResult, CallGraph, InferenceResult, SccGraph, Sig};
use brink_format::{DefinitionId, StoryData};
use brink_ir::suppressions::{Suppressions, apply_suppressions, parse_suppressions};
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolutionMap, Severity, SymbolIndex, SymbolKind,
    SymbolManifest, lower, lower_single_knot, lower_top_level,
};
use brink_syntax::Parse;

use crate::db::resolve_include_path;
use crate::include_graph::IncludeGraph;

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
                .ingredient::<symbol_index_query>()
                .ingredient::<resolution_index_query>()
                .ingredient::<resolve_query>()
                .ingredient::<signature_query>()
                .ingredient::<analysis_query>()
                .ingredient::<diagnostics_query>()
                // Layer 2/3: type inference (TM-1, advisory-only).
                // Per-def/per-SCC decomposition (FG-2, issue #631):
                // call_edges(def) -> call_graph() -> scc_membership() ->
                // solve_scc(SccId) -> inferred_signature(def)/infer_body(def).
                .ingredient::<inference_index_query>()
                .ingredient::<call_edges_query>()
                .ingredient::<call_graph_query>()
                .ingredient::<scc_membership_query>()
                .ingredient::<solve_scc_query>()
                .ingredient::<inferred_signature_query>()
                .ingredient::<type_inference_query>()
                .ingredient::<infer_body_query>()
                .ingredient::<type_diagnostics_query>()
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
#[salsa::tracked(returns(ref))]
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
#[salsa::tracked(returns(ref))]
pub(crate) fn lowered_query(db: &dyn salsa::Database, file: SourceFile) -> LoweredFile {
    lower_file(file.file_id(db), parse_query(db, file))
}

/// Parsed suppression/expectation directives for one file.
#[salsa::tracked(returns(ref))]
pub(crate) fn suppressions_query(db: &dyn salsa::Database, file: SourceFile) -> Suppressions {
    parse_suppressions(file.text(db))
}

/// The `INCLUDE` graph over the whole project. Always complete — edges are
/// derived from every file's HIR against the full path set, so the old
/// "rebuild after batch load" step no longer exists.
#[salsa::tracked(returns(ref))]
pub(crate) fn include_graph_query(db: &dyn salsa::Database, project: ProjectInput) -> IncludeGraph {
    let files = project.files(db);
    let path_to_id: HashMap<&str, FileId> = files
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
    brink_analyzer::symbol_index(&manifest_refs)
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
    let live_ids: std::collections::HashSet<DefinitionId> =
        stripped.symbols.keys().copied().collect();
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
#[salsa::tracked(returns(ref))]
pub(crate) fn resolve_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> (Arc<ResolutionMap>, Vec<Diagnostic>) {
    let index = resolution_index_query(db, project);
    brink_analyzer::resolve(file.file_id(db), &lowered_query(db, file).manifest, index)
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
#[salsa::tracked]
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
    brink_analyzer::signature(def_id, index, &hir_refs)
}

/// Full cross-file analysis, composed from the layer-2 queries plus the
/// analyzer's monolithic back half ([`brink_analyzer::finish_analysis`]) —
/// the same sequence `analyze_with_options` runs, so the result is identical
/// by construction.
#[salsa::tracked(returns(ref))]
pub(crate) fn analysis_query(db: &dyn salsa::Database, project: ProjectInput) -> AnalysisResult {
    let files = project.files(db);

    let (index, mut diagnostics) = symbol_index_query(db, project).clone();
    let mut resolutions = ResolutionMap::new();
    for file in files {
        let (file_map, file_diags) = resolve_query(db, project, *file);
        resolutions.extend(file_map.iter().cloned());
        diagnostics.extend(file_diags.iter().cloned());
    }

    let full_refs: Vec<(FileId, &HirFile, &SymbolManifest)> = files
        .iter()
        .map(|f| {
            let lowered = lowered_query(db, *f);
            (f.file_id(db), &lowered.hir, &lowered.manifest)
        })
        .collect();

    brink_analyzer::finish_analysis(
        &full_refs,
        index,
        resolutions,
        diagnostics,
        project.analysis_options(db),
    )
}

/// Per-file diagnostics (spec §4 layer 3): this file's lowering + syntax
/// diagnostics plus its share of the cross-file analysis diagnostics. Raw —
/// suppression filtering stays a consumer concern (see
/// [`partition_diagnostics`]).
#[salsa::tracked(returns(ref))]
pub(crate) fn diagnostics_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Vec<Diagnostic> {
    let file_id = file.file_id(db);
    let mut out = lowered_query(db, file).diagnostics.clone();
    out.extend(
        analysis_query(db, project)
            .diagnostics
            .iter()
            .filter(|d| d.file == file_id)
            .cloned(),
    );
    out
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

/// Shared input-gathering for every per-def/per-SCC inference query below
/// (FG-2, issue #631): the same three inputs [`brink_analyzer::infer_project`]
/// itself takes — the inference index projection, every file's HIR, and the
/// merged per-file resolutions — assembled exactly the way
/// `type_inference_query` assembled them pre-decomposition (issue #630 /
/// FG-1 §3: re-sourced off `inference_index_query`/`resolve_query`, never
/// `analysis_query`, so the pointer-identity guarantee
/// `fg1_dependency_edges.rs` pins keeps holding after this refactor).
fn inference_inputs(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> (Arc<SymbolIndex>, Vec<(FileId, &HirFile)>, ResolutionMap) {
    let files = project.files(db);
    let index = Arc::clone(inference_index_query(db, project));
    let mut resolutions = ResolutionMap::new();
    for file in files {
        let (file_map, _file_diags) = resolve_query(db, project, *file);
        resolutions.extend(file_map.iter().cloned());
    }
    let hir_refs: Vec<(FileId, &HirFile)> = files
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();
    (index, hir_refs, resolutions)
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

/// Pass 1, per-def (FG-2, issue #631 — `call_edges(def)`). Thin salsa
/// wrapper over [`brink_analyzer::call_edges`]; the per-def key gives Eq
/// cutoff on this def's own edge set (`BTreeSet<DefinitionId>`, no ranges,
/// derived `Eq`) — see the design doc §2 table's explicit allowance to keep
/// reusing `infer_def_body` and discard types, as `infer_project` already
/// did, for this pass's computation.
#[salsa::tracked]
pub(crate) fn call_edges_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Arc<BTreeSet<DefinitionId>> {
    let (index, hir_refs, resolutions) = inference_inputs(db, project);
    Arc::new(brink_analyzer::call_edges(
        def.def(db),
        &hir_refs,
        &index,
        &resolutions,
    ))
}

/// The whole-project call graph, merged from every inferable def's
/// [`call_edges_query`] (FG-2, issue #631 — the derived `call_graph()` the
/// design doc's §2 table names). [`CallGraph`]'s `Eq` (added for this slice)
/// is the cutoff [`scc_membership_query`] backdates on.
#[salsa::tracked(returns(ref))]
pub(crate) fn call_graph_query(db: &dyn salsa::Database, project: ProjectInput) -> CallGraph {
    let (index, hir_refs, _resolutions) = inference_inputs(db, project);
    let defs = brink_analyzer::inferable_defs(&hir_refs, &index);
    let mut graph = CallGraph::new();
    for &def in &defs {
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
/// wrapper over [`brink_analyzer::scc_graph`].
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
#[salsa::tracked]
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

    let (index, hir_refs, resolutions) = inference_inputs(db, project);
    let (signatures, bodies) =
        brink_analyzer::solve_scc(batch, &hir_refs, &index, &resolutions, known_sigs);
    Arc::new(SolvedScc { signatures, bodies })
}

/// Per-def inferred signature (`inferred_signature(def)`, FG-2 issue #631 —
/// the missing per-def API TM-2's firewall consumer needs most). `None` for
/// a def with no inferable body (not a knot/stitch, or an unknown id) — same
/// `None` contract as [`signature_query`]/[`infer_body_query`].
#[salsa::tracked]
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
#[salsa::tracked]
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

/// Whole-project LIR lowering (slice B: one project query; slice C splits it
/// per container). `no_eq`: `lir::Program` has no `PartialEq`, so this memo
/// never backdates — [`story_data_query`] backdates on `StoryData` instead.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn lir_query(db: &dyn salsa::Database, project: ProjectInput) -> LirProduct {
    let files = project.files(db);
    let Some(entry) = project.entry(db) else {
        return LirProduct::default();
    };

    let analysis = analysis_query(db, project);

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
    let (errors, warnings) = partition_diagnostics(&inputs, &analysis.diagnostics, disable_all);

    if !errors.is_empty() {
        return LirProduct {
            program: None,
            errors,
            warnings,
        };
    }

    // LIR inputs in topological include order (paste-before semantics),
    // mirroring `Driver::lir_inputs`.
    let graph = include_graph_query(db, project);
    let all_ids: Vec<FileId> = files.iter().map(|f| f.file_id(db)).collect();
    let by_id: HashMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let topo = graph.topological_order(entry, &all_ids);
    let hir_refs: Vec<(FileId, &HirFile)> = topo
        .iter()
        .filter_map(|id| by_id.get(id).map(|f| (*id, &lowered_query(db, *f).hir)))
        .collect();
    let paths: HashMap<FileId, String> = topo
        .iter()
        .filter_map(|id| by_id.get(id).map(|f| (*id, f.path(db).clone())))
        .collect();

    let (program, lir_diagnostics) =
        brink_ir::lir::lower_to_program(&hir_refs, &analysis.index, &analysis.resolutions, &paths);

    // LIR lowering itself is total (T1b-2: every construct lowers to a
    // program regardless of dialect) — a diagnostic pushed during lowering
    // doesn't stop `lower_to_program` from returning `Some`. It must still
    // be severity-partitioned like every other diagnostic here: an
    // Error-severity one (T1b-3's E055/E056 — a collection mutator's rvalue
    // first argument, or a mutator used in expression position) blocks
    // compilation exactly like an analysis-phase error would, not just a
    // cosmetic warning on an otherwise-successful compile.
    let (mut lir_errors, mut lir_warnings): (Vec<Diagnostic>, Vec<Diagnostic>) = lir_diagnostics
        .into_iter()
        .partition(|d| d.code.severity() == Severity::Error);

    if program.is_some() && lir_errors.is_empty() {
        let mut errors = errors;
        errors.append(&mut lir_errors);
        let mut warnings = warnings;
        warnings.append(&mut lir_warnings);
        LirProduct {
            program: program.map(Arc::new),
            errors,
            warnings,
        }
    } else {
        // Either `lower_to_program` refused to lower (its residual-extension
        // backstop fired — E053, meaning a T1b brink-extension HIR node
        // reached LIR lowering despite the dialect gate, only possible if
        // the gate's analysis diagnostic was suppressed — see #572 review),
        // or lowering succeeded but produced an Error-severity diagnostic
        // (T1b-3's rvalue-mutator/mutator-in-expression-position checks).
        // Either way: surface it as a compile error, never return a corrupt,
        // partial, or diagnostically-invalid program.
        let mut errors = errors;
        errors.append(&mut lir_errors);
        let mut warnings = warnings;
        warnings.append(&mut lir_warnings);
        LirProduct {
            program: None,
            errors,
            warnings,
        }
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
        Ok(story) => CompileProduct {
            story: Some(Arc::new(story)),
            errors: lir.errors.clone(),
            warnings: lir.warnings.clone(),
        },
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
#[must_use]
pub fn partition_diagnostics(
    files: &[FileDiagnostics<'_>],
    analysis_diagnostics: &[Diagnostic],
    disable_all: bool,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let mut partition = |d: Diagnostic| {
        if d.code.severity() == Severity::Error {
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
        let mut by_file: HashMap<FileId, Vec<Diagnostic>> = HashMap::new();
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
