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
//! [`harvest_index_query`] (the project-db harvest obligation over cue
//! payloads and markup span kinds, issue #2114 — a sibling merge over the
//! same per-file [`lowered_query`] outputs, not a per-file query),
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

use brink_analyzer::{
    AnalysisOptions, CallGraph, HarvestIndex, InferenceResult, SccGraph, Sig, TypePolicy,
};
use brink_format::{
    CallAtom, CapabilityParam, DefinitionId, DirectEffects, EffectRowEntry, NameId, StoryData,
};
use brink_ir::suppressions::{Suppressions, apply_suppressions, parse_suppressions};
use brink_ir::symbols::project_manifest;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolutionMap, Severity, SymbolIndex, SymbolKind,
    SymbolManifest, lower, lower_single_knot, lower_top_level,
};
use brink_syntax::Parse;
use brink_syntax_native::Parse as NativeParse;

use crate::db::resolve_include_path;
use crate::determinism::{LookupMap, LookupSet};
use crate::include_graph::IncludeGraph;

mod analysis;
mod heap_size;

pub use analysis::ResolvedProject;
pub(crate) use analysis::{
    analysis_diagnostics_query, analysis_query, await_purity_diagnostics_query,
    call_site_diagnostics_query, call_site_metas_query, coalesce_types_query,
    comparator_contract_diagnostics_query, contributor_diagnostics_query,
    conventions_confinement_diagnostics_query, conventions_projection_query, diagnostics_query,
    effects_assertion_diagnostics_query, external_meta_query, has_errors_in_closure_query,
    has_errors_query, import_closure_query, inline_docs_query, per_file_diagnostics_query,
    resolutions_index_query, ufcs_resolution_query, value_meta_query,
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
                // B0.10a native compile seam (issue #1106): the frontend-
                // specific parse ingredient, dispatched by `lowered_query`.
                .ingredient::<parse_native_query>()
                .ingredient::<lowered_query>()
                .ingredient::<suppressions_query>()
                .ingredient::<include_graph_query>()
                // Layer 2.
                .ingredient::<module_map_query>()
                .ingredient::<symbol_index_query>()
                .ingredient::<harvest_index_query>()
                .ingredient::<resolution_index_query>()
                .ingredient::<resolve_query>()
                .ingredient::<signature_query>()
                // Issue #530: the per-file locals path signature_query
                // itself can't take — see local_signature_query's doc.
                .ingredient::<local_signature_query>()
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
                // B3a UFCS (issue #1506): the verdict table, shared by
                // `whole_project_diagnostics_query` (diagnostics half) and
                // LIR lowering (`lir_knot_chunk_query`/`lir_lowering_query`).
                .ingredient::<ufcs_resolution_query>()
                // B1 `or`-coalescing (issue #1471/#1492): the recorded
                // per-step chain shapes LIR lowering consumes
                // (`lir_knot_chunk_query`/`lir_lowering_query`).
                .ingredient::<coalesce_types_query>()
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
                // Issue #1032 collapse ruling: the closure-scoped counterpart
                // `compileProject`'s artifact path (`story_data_query`) reads
                // instead of the whole-project `has_errors_query`/`lir_query`
                // above — see `has_errors_in_closure_query`'s doc comment.
                .ingredient::<has_errors_in_closure_query>()
                .ingredient::<type_policy_query>()
                // The `.lints` sibling projection (issue #1160) — see
                // `lint_policy_query`'s doc comment.
                .ingredient::<lint_policy_query>()
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
                // Issue #460: the knot-invariant half of a chunk's lowering
                // environment, hoisted out of the per-knot memo so it is
                // built once per revision instead of once per knot.
                .ingredient::<chunk_lowering_ctx_query>()
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
                .ingredient::<external_signatures_query>()
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
                // T2-2 (issue #861): the `#@effects(…)` assertion's
                // per-file exceedance check, reading `effects_query` only
                // for defs that actually carry an assertion.
                .ingredient::<effects_assertion_diagnostics_query>()
                // FS-2 (issue #928): the `await`-condition purity gate (E105),
                // reading `effects_query` only for defs a condition calls.
                .ingredient::<await_purity_diagnostics_query>()
                // NS-A4 (issue #1110, extended to the fn-value verb trio by
                // issue #1679): the comparator-contract gate (E119),
                // reading `effects_query` only for defs named as inline
                // `#fn` comparators/callbacks of `sort_by`/`sorted_by`/
                // `map`/`filter`/`fold`.
                .ingredient::<comparator_contract_diagnostics_query>()
                // Conventions-module confinement gate (E169, issue #1844):
                // the MODULE half of the §9.1 claiming-handler confinement
                // ruling. Reads `module_map_query` only for a file that
                // declared at least one claiming handler.
                .ingredient::<conventions_confinement_diagnostics_query>()
                // The reusable transitive `IMPORT` closure (issue #2111
                // finding 3): generic over any entry file, so #2167 can
                // reuse it for E169 confinement relaxation.
                .ingredient::<import_closure_query>()
                // The serialized conventions projection (issue #2111, NS-T
                // seam 1/6): the editor-facing artifact, reading the
                // resolved conventions module's import closure to resolve
                // `attach = StructName` schemas (finding 1).
                .ingredient::<conventions_projection_query>()
                // Layer 3.
                .ingredient::<lir_query>()
                .ingredient::<lir_in_closure_query>()
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
/// entry point, the analysis options (host manifest + external-check
/// severity), and the native source root.
#[salsa::input]
pub(crate) struct ProjectInput {
    #[returns(ref)]
    pub files: Vec<SourceFile>,
    pub entry: Option<FileId>,
    #[returns(ref)]
    pub analysis_options: AnalysisOptions,
    /// The directory native `.brink` keys are root-relative *to*, for a
    /// consumer that registers files under some other prefix (issue #1572:
    /// the LSP keys by absolute OS path). `None` — every compile path, where
    /// `discover_native` already keys root-relative — means "the keys are
    /// already root-relative", and is byte-identical to the pre-#1572 world.
    /// Only [`crate::modules::root_relative_key`] reads it, for native module
    /// identity ([`module_map_query`]'s native branch).
    #[returns(ref)]
    pub native_root: Option<String>,
    /// The directory `.ink` keys are root-relative *to*, for
    /// [`hir::root_content_scope_path`](brink_ir::hir::root_content_scope_path)'s
    /// qualifier (issue #1696) — ink's sibling of `native_root` above, reusing
    /// the same [`crate::modules::root_relative_key`] mechanism #1572 built.
    /// Unlike native, ink's CLI discovery has no `RealFs`-scoped tree to key
    /// root-relative "for free": `brink-driver`'s `discover` BFS registers
    /// files under whatever spelling the caller passed `prepare_driver`
    /// (`brink-compiler/src/driver.rs`), so `main.ink`, `./main.ink`, and an
    /// absolute spelling of the same file used to mint different anonymous
    /// root-content `DefinitionId`s for byte-identical source. `None` (no
    /// caller has registered a root) is byte-identical to the pre-#1696
    /// world — `root_relative_key` returns every path unchanged.
    #[returns(ref)]
    pub ink_root: Option<String>,
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

/// Parse one native `.brink` file's text into a lossless CST.
///
/// The frontend-specific sibling of [`parse_query`] (B0.10a, the native
/// compile seam, issue #1106). `brink_syntax_native::Parse` is structurally
/// identical to `brink_syntax::Parse` (`{green, errors}`, `Clone + Eq`) but a
/// distinct nominal type, so it needs its own tracked ingredient — matching
/// attrs (`returns(ref)`, `lru = 4096`, the same per-file runaway-guard
/// ceiling, issue #647). Only ever executed for files [`file_language`]
/// classifies as [`Language::Native`], so an `.ink` file never runs the native
/// parser and vice-versa — see [`lowered_query`].
#[salsa::tracked(returns(ref), lru = 4096)]
pub(crate) fn parse_native_query(db: &dyn salsa::Database, file: SourceFile) -> NativeParse {
    brink_syntax_native::parse(file.text(db))
}

/// Per-file lowering output: assembled HIR, symbol manifest, and lowering +
/// syntax diagnostics — the exact product the retired `FileState` cached.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoweredFile {
    pub hir: HirFile,
    pub manifest: SymbolManifest,
    pub diagnostics: Vec<Diagnostic>,
    /// B0.3 `validate_admission` output (docs/hir-admission-contract.md
    /// §4.2, issue #1172), plus — for a native `.brink` file only — B0.9's
    /// `validate_native_accept_list` output appended after it (issue
    /// #1179) — kept deliberately separate from `diagnostics`: both
    /// admission gates are non-suppressible (NF-6, always-on), so neither
    /// must ever flow through `apply_suppressions` the way lowering/syntax
    /// diagnostics do (`partition_diagnostics` below). Computed here so it
    /// runs on every lowering, matching the "the validator runs on every
    /// keystroke in the editor" perf posture — see `heap_size.rs`'s
    /// `lowered_file_heap_size` for the matching estimator update.
    pub admission: Vec<Diagnostic>,
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
    let file_id = file.file_id(db);
    // Decide the frontend from the path *before* touching either parser
    // (B0.10a, the native compile seam, issue #1106): this branch precedes
    // the parse call, so an `.ink` file never runs the native parser and a
    // native file never runs the ink one. The ink arm is byte-identical to
    // the pre-seam body, keeping the oracle invariance a tautology.
    match file_language(file.path(db)) {
        Language::Ink => lower_file(file_id, parse_query(db, file)),
        Language::Native => lower_native_file(file_id, parse_native_query(db, file)),
    }
}

/// Which frontend a source file feeds — decided purely from its path (B0.10a,
/// issue #1106). Deliberately *not* stored on any input, HIR, or
/// `AnalysisOptions`: the "no dialect tag near HIR" posture keeps this an
/// internal, ephemeral classification used only as [`file_language`]'s return.
/// It is a different axis from `brink_analyzer::Dialect` (an ink-extension
/// gate) — do not conflate the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Language {
    Ink,
    Native,
}

/// Classify a source file's frontend from its path. A pure, deterministic
/// extension test (`.brink` → native, everything else → ink) — no schema
/// change, no `HashMap` iteration. Uses `Path::extension` to match the
/// codebase's existing extension convention (e.g. `brink-lsp`'s `ext ==
/// "ink"`).
pub(crate) fn file_language(path: &str) -> Language {
    if std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext == "brink")
    {
        Language::Native
    } else {
        Language::Ink
    }
}

/// Parsed suppression/expectation directives for one file — both channels
/// merged into the one value every [`apply_suppressions`] call site reads.
///
/// The `//brink-disable`/`//brink-expect` comment channel is a pure text
/// scan ([`parse_suppressions`]). The `@[allow(Exxx, …)]` annotation channel
/// (issue #1161) rides the real `@[…]` grammar, so its declaration-scoped
/// records are produced by lowering and picked up here off
/// [`brink_ir::HirFile::allow_scopes`] — always empty for an ink file, whose
/// annotation channel has no `allow` tenant. Reading [`lowered_query`] costs
/// nothing extra in practice: every consumer of this query already reads the
/// same file's lowering diagnostics alongside it.
///
/// `lru = 4096`: per-file runaway-guard ceiling (issue #647).
#[salsa::tracked(returns(ref), lru = 4096)]
pub(crate) fn suppressions_query(db: &dyn salsa::Database, file: SourceFile) -> Suppressions {
    let mut out = parse_suppressions(file.text(db));
    out.allow_scopes
        .clone_from(&lowered_query(db, file).hir.allow_scopes);
    out
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

/// The ordered set of files that participate in codegen for `project`, in
/// compile (paste-before) order — the single native-vs-ink codegen-closure
/// decision, shared by every codegen-scoping site
/// ([`struct_shape_data_query`], [`lir_prelude_decls_query`],
/// [`lir_lowering_query`], [`lir_in_closure_query`], and
/// [`has_errors_in_closure_query`](crate::queries::analysis::has_errors_in_closure_query)).
///
/// **Ink** projects thread reachability through `INCLUDE`: the closure is
/// `entry`'s transitive `INCLUDE` closure ([`IncludeGraph::topological_order`],
/// the issue #815 narrowing every codegen path already used). This is exactly
/// the previous behavior — a project whose entry is an `.ink` file is
/// unaffected.
///
/// **Native** projects have no `INCLUDE` edges, so the ink closure would reach
/// only `entry` and every sibling `.brink` module would silently miss codegen
/// (issue #1296). The decision-log ruling *"Native multi-file linking"*
/// (2026-07-23) makes the **discovered module set the compilation unit**: the
/// closure is *every* discovered `.brink` module, ordered by `FileId` — which
/// `brink_driver::discover_native` mints in sorted-key order, so the order is
/// deterministic and mount-independent. Consequences that follow directly:
///
/// - All discovered modules link into the one `StoryData`; the entry file
///   still only designates the *start flow* (compilation universe ≠ execution
///   entry).
/// - A `.brink` file that fails to compile is an error **even if no other
///   module references it** — its diagnostics are inside the closure the build
///   gate reads, so it fails the build (Rust parity: the whole module tree is
///   the unit).
///
/// A project is "native" iff its entry file is a `.brink` module; the closure
/// then ranges over the `.brink` files only (any stray `.ink` file sharing the
/// session db is not a discovered native module and never enters it).
/// Reachability-based dead-module elimination is an explicitly-deferred future
/// subtraction (decision-log) — this closure is the full discovered set.
pub(crate) fn compilation_closure_files(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Vec<FileId> {
    let Some(entry) = project.entry(db) else {
        return Vec::new();
    };
    let files = project.files(db);
    let entry_is_native = files
        .iter()
        .find(|f| f.file_id(db) == entry)
        .is_some_and(|f| file_language(f.path(db)) == Language::Native);

    if entry_is_native {
        // Every discovered `.brink` module is the compilation unit. Sort by
        // `FileId` (minted in sorted-key order by `discover_native`) so the
        // order is deterministic regardless of session-db insertion order.
        let mut ids: Vec<FileId> = files
            .iter()
            .filter(|f| file_language(f.path(db)) == Language::Native)
            .map(|f| f.file_id(db))
            .collect();
        ids.sort_unstable_by_key(|id| id.0);
        ids
    } else {
        include_graph_query(db, project).topological_order(entry)
    }
}

/// Whether `project` is a native compilation unit — its entry file is a
/// `.brink` module (the same "entry file decides the frontend" rule
/// [`compilation_closure_files`] documents). `false` when there is no entry
/// file at all.
///
/// The T1b dialect-gate decoupling (issue #1348) reads this to skip the
/// ink-only `E064` config error (`strict::config_error`, via
/// [`brink_analyzer::strict_diagnostics`]'s `is_native` flag) for a native
/// project — the whole-project sibling of [`per_file_diagnostics_query`]'s
/// own per-file `file_language(file.path(db)) == Language::Native` check.
pub(crate) fn project_is_native(db: &dyn salsa::Database, project: ProjectInput) -> bool {
    let Some(entry) = project.entry(db) else {
        return false;
    };
    project
        .files(db)
        .iter()
        .find(|f| f.file_id(db) == entry)
        .is_some_and(|f| file_language(f.path(db)) == Language::Native)
}

/// Whether every file currently tracked in `project` is a native `.brink`
/// module — `false` for an empty project or one holding even a single ink
/// file.
///
/// [`project_is_native`]'s "entry file decides the frontend" rule is right
/// for a codegen-shaped question ("which frontend am I compiling"), which
/// always has an explicit entry (the CLI's compile target). `symbol_index_query`
/// asks a different question — "does this project have any ink file whose
/// `dialect` could actually be wrong" — for `ProjectDb`'s single
/// whole-workspace `ProjectInput`, which a long-lived LSP session never
/// anchors to an entry at all (`Backend` never calls `ProjectDb::set_entry`;
/// issue #1562 review finding). A project every one of whose files is native
/// has no such file, by the same "native has no dialect to be wrong about"
/// reasoning [`project_is_native`]'s own doc gives — regardless of whether
/// anything ever called `set_entry`.
pub(crate) fn project_is_all_native(db: &dyn salsa::Database, project: ProjectInput) -> bool {
    let files = project.files(db);
    !files.is_empty()
        && files
            .iter()
            .all(|f| file_language(f.path(db)) == Language::Native)
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

    // Native `.brink` files derive their module PURELY from their
    // root-relative path (decision-log 2026-07-22), bypassing `resolve_modules`
    // entirely — they have no `#@module` inheritance and no INCLUDE graph, so
    // routing them through the ink resolver would only couple their save-key
    // identity to machinery they never use. Only ink files feed `resolve_modules`.
    let ink_inputs: Vec<crate::modules::FileModuleInput> = files
        .iter()
        .filter(|f| file_language(f.path(db)) == Language::Ink)
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
    let (mut map, diags) =
        crate::modules::resolve_modules(&ink_inputs, include_graph_query(db, project));

    // A native file's module NAME is `native_module_path(root-relative key)`,
    // marked `declared` so it always qualifies `DefinitionId` (path on disk =
    // identity). Only the *name* is path-derived and bypasses the resolver —
    // that is the save-key-critical isolation (the name is a pure function of
    // the path; `FileId` is only the map key, never hashed, so adding a file
    // cannot shift another file's identity).
    //
    // The rest of the module system is the SAME feature, just path-spelled:
    // `was` (rename migration, so a moved file's old saves still resolve) is
    // read from the file's own `@[was("old::path")]` annotation via its HIR,
    // exactly as the ink path reads `#@was` — never hard-dropped (issue #1286
    // wired the native parse/lower; `lower_native::module`). `None` when the
    // file authored no `@[was]`.
    // The key handed to `native_module_path` is the file's path made
    // root-relative to the project's registered `native_root` (issue #1572) —
    // a no-op for every compile path (`discover_native` already keys
    // root-relative, so `native_root` is `None`), and the normalization that
    // makes a long-lived editor session's absolute-path keys mint the *same*
    // module identity a real compile of the same tree does.
    let native_root = project.native_root(db).as_deref();
    for f in files {
        if file_language(f.path(db)) == Language::Native {
            let was = lowered_query(db, *f)
                .hir
                .module
                .as_ref()
                .and_then(|m| m.was.as_ref().map(|(old, _)| old.clone()));
            let key = crate::modules::root_relative_key(native_root, f.path(db));
            map.insert(
                f.file_id(db),
                brink_analyzer::ResolvedModule {
                    name: crate::modules::native_module_path(&key),
                    declared: true,
                    was,
                },
            );
        }
    }

    (map, diags)
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
    // `is_native` (issue #1562 review finding): a native project has no
    // dialect to be wrong about — the same reasoning `project_is_native`
    // gives `whole_project_diagnostics_query` for skipping the ink-only
    // `E064` config error — so M-2d cross-declared-module coexistence must
    // not depend on a client having declared `dialect: "brink"`. Every
    // `.brink` file's module is its path and always *declared*, so without
    // this a native workspace under the (default) `StrictInk` dialect would
    // drop one of two same-name definitions from the index instead of
    // letting them coexist.
    //
    // `project_is_all_native`, not `project_is_native`: this `project` is
    // `ProjectDb`'s single whole-workspace `ProjectInput`, which a
    // long-lived LSP session never anchors to a compile `entry`
    // (`project_is_native` always answers `false` without one) — see
    // `project_is_all_native`'s own doc.
    let is_native = project_is_all_native(db, project);
    let (index, mut diagnostics) =
        brink_analyzer::symbol_index_with_modules(&manifest_refs, module_map, dialect, is_native);
    diagnostics.extend(module_diags.clone());
    (index, diagnostics)
}

/// The project-wide harvest index (issue #2114, `docs/prose-dialect-spec.md`
/// §5): every `@NAME` cue payload and every inline-markup span kind/
/// attribute name, harvested from every file's HIR and upgraded by the
/// registered host manifest's `markup` vocabulary — the compiler-side
/// sibling of [`symbol_index_query`]. Its dependency set is every file's
/// [`lowered_query`] output *plus* `project.analysis_options(db).host_manifest`
/// (read below to build the manifest upgrade) — a manifest edit does
/// invalidate this memo, unlike a plain per-file prose edit. That still
/// gives this query the same per-file early cutoff `symbol_index_query`
/// has for the `lowered_query` half: an edit to file A's prose only
/// recomputes this merge when file A's own `LoweredFile` output changes,
/// not on every keystroke project-wide.
///
/// Thin wrapper over [`brink_analyzer::harvest`] — see that function's own
/// doc, and `crate::db::ProjectDb::harvest_index` for the public surface a
/// completion consumer calls.
#[salsa::tracked(returns(ref))]
pub(crate) fn harvest_index_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<HarvestIndex> {
    let files = project.files(db);
    let hir_refs: Vec<(FileId, &HirFile)> = files
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();
    let manifest = project.analysis_options(db).host_manifest.as_ref();
    Arc::new(brink_analyzer::harvest(&hir_refs, manifest))
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

/// Interned key for [`signature_query`] and [`local_signature_query`].
/// Keyed on the content-addressed [`DefinitionId`] alone: colliding ids
/// among non-local declarations (duplicate names across files) map to a
/// *single* index entry chosen deterministically by the merge, so the memo
/// cannot diverge from what a non-memoized `signature(def)` call would
/// return for the same id. For `signature_query`, local (`Param`/`Temp`)
/// ids no longer collide across files in a way that matters here —
/// [`resolution_index_query`] drops locals entirely (issue #517). A local's
/// `DefinitionId` itself carries no file component, so a colliding id
/// *would* matter for [`local_signature_query`] — that query disambiguates
/// by taking its own explicit `file` parameter alongside this same `DefKey`
/// (issue #530), rather than relying on uniqueness of the id alone.
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
/// projection exists to avoid. Locals stay permanently non-addressable via
/// this query — see [`local_signature_query`] for the per-file path hover
/// now uses instead (issue #530).
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
/// reads `project.analysis_options(db).host_manifest` so `Handle<K>`
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

/// The per-file locals path [`signature_query`] itself cannot take (issue
/// #530): [`resolution_index_query`] drops `Param`/`Temp` locals entirely
/// (issue #517), so `signature_query(def)` short-circuits to `None` for
/// any local `DefinitionId` — a silent "hover shows nothing" trap for
/// whoever wires hover/signature to a local next. A local's `DefinitionId`
/// carries no file component (content hash of `(scope, name, kind)` alone —
/// `brink_analyzer::local_signature`'s doc), so unlike [`signature_query`]
/// it cannot recover its declaring file from the project-wide index without
/// either a whole-project scan (reintroducing exactly the invalidation
/// #517's cutoff exists to kill) or a caller-supplied file. This query
/// takes `file` explicitly instead — the same per-file-only shape
/// [`resolve_query`] already uses for local lookups (a local's body lives
/// in exactly one file, issue #517) — so a body edit in a *different* file
/// leaves this memo untouched.
///
/// Per #531 (converge `symbol_index_query` to decls-only): this is
/// deliberately a *separate* query, not a widening of `signature_query`'s
/// own index read — it serves locals without merging the decls-only and
/// full indexes back together.
///
/// `lru = 4096`: per-(file, def) runaway-guard ceiling (issue #647,
/// decision log "FG-5 memory bounding"), matching the other per-file
/// families' ceiling — a `Sig` is small and this query reads only its own
/// file's `manifest.locals`, so it carries none of `signature_query`'s
/// wider per-def fanout. `heap_size = heap_size::signature_heap_size`
/// (issue #538/#530): the output is the identical `Option<Arc<Sig>>` shape
/// `signature_query` already estimates, so the same walk is reused rather
/// than duplicated — see `heap_size.rs`'s module doc.
#[salsa::tracked(lru = 4096, heap_size = heap_size::signature_heap_size)]
pub(crate) fn local_signature_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
    def: DefKey<'db>,
) -> Option<Arc<Sig>> {
    let index = resolution_index_query(db, project);
    let manifest = &lowered_query(db, file).manifest;
    let opts = project.analysis_options(db);
    brink_analyzer::local_signature(def.def(db), manifest, index, opts.host_manifest.as_ref())
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
    /// Which frontend produced the declaring file (`HirFile::native`, issue
    /// #1862). Projected per def because this narrowed projection is all
    /// `solve_scc_query` holds — it never sees the whole `HirFile` — and
    /// `brink_analyzer::Def::native` needs it for the native bare-name
    /// fn-value typing rule (issue #1876).
    pub native: bool,
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
        native: hir.native,
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
///
/// **Does not carry `EXTERNAL` signatures (issue #1921).** `batch` never
/// contains an `EXTERNAL` (see [`brink_analyzer::solve_scc`]'s own doc), so
/// `signatures` here is scoped to the SCC's own knot/stitch members, same as
/// before #1921. [`type_inference_query`] is where every `EXTERNAL`'s
/// declaration-derived signature is merged in — once, at the aggregation,
/// not once per SCC — so it agrees with the pure whole-project
/// `infer_project` path without every `solve_scc_query` memo paying to
/// clone the project's whole external-signature map (that per-SCC
/// duplication would also make every memo's *value* depend on the host
/// manifest, which would cost `solve_scc_query`/`inferred_signature_query`/
/// `infer_body_query` the FG-2 cutoff `fg1_dependency_edges.rs` pins even
/// for an SCC that never calls an external).
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
/// [`brink_analyzer::solve_scc`] so a `Handle<K>` param/return/temp
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
/// (not just a `Handle<K>` kind) resolves to its own base `Ty`. Range-free
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
            native: b.native,
        })
        .collect();

    // Pre-scan + narrow map (Ruling 1): union of every member's
    // referenced_globals, each resolved through the existing
    // per-declaring-file `signature_query`.
    let mut global_ids: BTreeSet<DefinitionId> = BTreeSet::new();
    for &member in batch {
        global_ids.extend(referenced_globals_query(db, project, DefKey::new(db, member)).iter());
    }
    // `value_ty` carries the declaration's type at full `Ty` fidelity —
    // scalars, `List<L>`, and (since issue #1540) `Array`/`Map`/`Struct`/
    // `Fn`/`Handle` alike (`Option`/`Range` have no annotation grammar yet,
    // so they never reach here). Mirrors `brink_analyzer::infer::
    // collect_globals`'s own single read exactly, so this narrowed path
    // stays composed-equals-monolithic with it.
    let mut globals: BTreeMap<DefinitionId, brink_analyzer::Ty> = BTreeMap::new();
    for gid in global_ids {
        if let Some(sig) = signature_query(db, project, DefKey::new(db, gid))
            && let Some(ty) = sig.value_ty.clone()
        {
            globals.insert(gid, ty);
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

/// Every `EXTERNAL`'s declaration-derived signature, project-wide (issue
/// #1921 — [`brink_analyzer::collect_external_sigs`] as its own memo).
/// `Arc<plain>`, `Eq`-derived, so a `host_manifest`/`inline_docs` edit that
/// leaves every `EXTERNAL`'s declared signature unchanged backdates this
/// memo exactly like [`solve_scc_query`] already backdates its own
/// `host_manifest`-dependent `Ty::Handle` resolution (T1d-2b, issue #774).
/// That backdating is *why* this is its own `#[salsa::tracked]` query and
/// not an inline call inside [`type_inference_query`]: reading
/// `project.analysis_options(db)` — a raw salsa input, never backdated on
/// its own — directly inside `type_inference_query` would tie
/// `type_inference_query`'s *own* memo to that input's revision instead of
/// to this query's `Eq`-cutoff output, forcing `type_inference_query` to
/// re-execute (a fresh `Arc::new`, breaking the pointer-identity guarantee
/// `fg1_dependency_edges.rs` pins) on *any* `AnalysisOptions` edit,
/// including a diagnostics-only one like `external_check` that never
/// touches an external's declared signature at all.
#[salsa::tracked(returns(ref))]
pub(crate) fn external_signatures_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<BTreeMap<DefinitionId, brink_analyzer::InferredSig>> {
    let index = inference_index_query(db, project);
    let inline_docs = inline_docs_query(db, project);
    let opts = project.analysis_options(db);
    Arc::new(brink_analyzer::collect_external_sigs(
        index,
        opts.host_manifest.as_ref(),
        inline_docs,
    ))
}

/// Whole-project type inference — now an aggregation over
/// [`scc_membership_query`] + [`solve_scc_query`] (FG-2, issue #631; was a
/// single monolithic [`brink_analyzer::infer_project`] call
/// pre-decomposition). Still re-sourced off `inference_index`/`resolve`,
/// never `analysis_query` (FG-1 §3) — every query this reads
/// (`scc_membership_query` -> `call_graph_query` -> `call_edges_query` ->
/// `inference_inputs`, plus [`external_signatures_query`] below) traces
/// back to the same two roots (or its own `Eq`-cutoff memo), so the
/// pointer-identity guarantee `fg1_dependency_edges.rs` pins (a
/// diagnostics-only edit leaves this memo fully validated, never
/// re-executed) still holds after this refactor.
///
/// **Merges in every `EXTERNAL`'s signature once, here (issue #1921).**
/// [`solve_scc_query`]'s own `signatures` never carries one — `batch` is
/// never an `EXTERNAL` (see [`brink_analyzer::solve_scc`]'s own doc) — so
/// without this, a UFCS call into an `EXTERNAL` went argument-unchecked on
/// this db-backed path even though the identical call was already checked
/// through the pure `infer_project` path (whose `solve_batches` sibling
/// returns `known_sigs` wholesale, no batch filter). This re-merge reads
/// [`external_signatures_query`] — its own backdating memo, see its doc —
/// exactly once, at this single aggregation point, deliberately *not*
/// inside [`solve_scc_query`] itself: doing it per-SCC would (a) make every
/// `solve_scc_query` memo hold a full clone of the project's whole
/// external-signature map, multiplying `solve_scc_heap_size`'s per-memo
/// heap accounting by the SCC count, and (b) make every SCC's memoized
/// *value* depend on every `EXTERNAL`'s declared type, so a `host_manifest`
/// edit touching one external would invalidate every SCC's cutoff —
/// including SCCs that never call that external — costing
/// `inferred_signature_query`/`infer_body_query` the exact FG-2 per-def
/// cutoff `fg1_dependency_edges.rs` pins, project-wide. Merging only here
/// keeps that per-def cutoff intact; only this one aggregation memo
/// re-executes (cheaply — no fixpoint solving, just a map merge) when a
/// manifest edit changes an external's declared type.
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
    signatures.extend(
        external_signatures_query(db, project)
            .iter()
            .map(|(k, v)| (*k, v.clone())),
    );
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
    if project.entry(db).is_none() {
        return brink_ir::lir::StructShapeData::default();
    }
    let files = project.files(db);
    let by_id: LookupMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let topo = compilation_closure_files(db, project);
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
    // #1504: the file's own path qualifies its root-content scope path, so
    // two files' root weaves no longer mint the same anonymous ids. Reading
    // `path` here adds no invalidation edge this memo did not already have —
    // it is an input field of the `SourceFile` it is already keyed on.
    //
    // #1696: the qualifier is the file's *root-relative* key, not its raw
    // registered path — `crate::modules::root_relative_key` against the
    // project's registered `ink_root` (`None` for every ordinary compile,
    // where it is a no-op), the same normalization `native_root` already
    // gives `.brink` module identity (issue #1572).
    let ink_root = project.ink_root(db).as_deref();
    let file_paths: LookupMap<FileId, String> = std::iter::once((
        file.file_id(db),
        crate::modules::root_relative_key(ink_root, file.path(db)).into_owned(),
    ))
    .collect();
    brink_ir::stamp_container_ids(&mut slice, &resolved.index, &file_paths);
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
    if project.entry(db).is_none() {
        return PreludeDeclsResult {
            decls: Arc::new(brink_ir::lir::PreludeDecls::empty(type_mode)),
        };
    }
    let files = project.files(db);
    let by_id: LookupMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let topo = compilation_closure_files(db, project);
    let decl_refs: Vec<(FileId, &HirFile)> = topo
        .iter()
        .filter_map(|id| {
            by_id
                .get(id)
                .map(|f| (*id, decl_hir_query(db, project, *f)))
        })
        .collect();
    let resolved = resolutions_index_query(db, project);
    // #1774: reaches `decls::collect_globals`'s lambda-lifting path, which
    // qualifies a lambda-literal decl default's synthesized function by the
    // owning file — same #1696 root-relative convention as
    // `chunk_lowering_ctx_query`/`lir_lowering_query`'s own `file_paths`.
    // `project.files(db)` is already read just above (`by_id`), so reading
    // each file's `.path(db)` here adds no new dependency edge.
    let ink_root = project.ink_root(db).as_deref();
    let file_paths: LookupMap<FileId, String> = files
        .iter()
        .map(|f| {
            (
                f.file_id(db),
                crate::modules::root_relative_key(ink_root, f.path(db)).into_owned(),
            )
        })
        .collect();
    // Review finding on #1774: a decl-default lambda body is lowered through
    // the same `lower_lambda` machinery as any other lambda (issue #1709),
    // so it needs the same UFCS/`or`-coalescing verdict tables any other
    // lambda body gets — not the empty placeholder pair every *other*
    // caller of `AnalyzerTables` uses because those callers genuinely never
    // ran an analyzer pass. Same construction `chunk_lowering_ctx_query`
    // (:1970-1972) and `lir_lowering_query` (:2119-2121) already use; no new
    // dependency edge risk (`ufcs_resolution_query`/`coalesce_types_query`
    // are re-sourced off `resolutions_index_query`/`lowered_query`/
    // `type_inference_query`, never off this query or anything downstream of
    // it, so this cannot introduce a salsa cycle).
    let ufcs = &ufcs_resolution_query(db, project).table;
    let coalesce = coalesce_types_query(db, project);
    let tables = brink_ir::lir::AnalyzerTables { ufcs, coalesce };
    let decls = brink_ir::lir::build_prelude_decls(
        &decl_refs,
        &resolved.index,
        &resolved.resolutions,
        &file_paths,
        type_mode,
        tables,
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

/// [`brink_ir::lir::ChunkLoweringCtx`] wrapped so
/// [`chunk_lowering_ctx_query`] (`no_eq`: it holds the same
/// `ShapeTable`/`GlobalShapeMap` `PreludeDeclsResult` cannot make `Eq`) can
/// satisfy salsa's `Update` bound. `Arc`-wrapped so a validated memo hands
/// back the same allocation — same pattern as [`PreludeDeclsResult`].
#[derive(Clone)]
pub(crate) struct ChunkLoweringCtxResult {
    pub ctx: Arc<brink_ir::lir::ChunkLoweringCtx>,
}

impl PartialEq for ChunkLoweringCtxResult {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ctx, &other.ctx)
    }
}

/// The knot-invariant half of [`lir_knot_chunk_query`]'s lowering
/// environment, built once per project revision instead of once per knot
/// (issue #460).
///
/// Every input here is whole-project — the flattened resolution lookup, the
/// reconstructed struct-shape tables, the `FileId`→path map, the type mode —
/// so each of the project's K per-knot memos used to rebuild all of it,
/// making the per-knot LIR layer `O(K × project size)`. The measured cost on
/// `compile_bench`'s 50-file × 20-knot synthetic project was the dominant
/// share of cold LIR lowering; hoisting it here makes that share `O(1)` in K.
///
/// This query's dependency set is exactly the subset of
/// [`lir_knot_chunk_query`]'s dependencies it took over
/// ([`resolutions_index_query`], [`struct_shape_data_query`],
/// [`type_policy_query`], and the files' `path` fields), so no chunk memo
/// gains or loses an invalidation edge: anything that re-executes this
/// re-executed every chunk before.
#[salsa::tracked(no_eq)]
pub(crate) fn chunk_lowering_ctx_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> ChunkLoweringCtxResult {
    let resolved = resolutions_index_query(db, project);
    let shape_data = struct_shape_data_query(db, project);
    // Narrow `.types` projection (issue #806/#809) — not the raw
    // `AnalysisOptions` field — so an unrelated options edit doesn't
    // re-execute this memo (and through it, every knot chunk).
    let type_mode = match type_policy_query(db, project) {
        TypePolicy::Strict => brink_ir::lir::TypeMode::Strict,
        TypePolicy::Gradual => brink_ir::lir::TypeMode::Gradual,
    };
    // #1696: root-relative, not raw — see `normalized_stamped_query`'s doc.
    let ink_root = project.ink_root(db).as_deref();
    let file_paths: LookupMap<FileId, String> = project
        .files(db)
        .iter()
        .map(|f| {
            (
                f.file_id(db),
                crate::modules::root_relative_key(ink_root, f.path(db)).into_owned(),
            )
        })
        .collect();
    ChunkLoweringCtxResult {
        ctx: Arc::new(brink_ir::lir::ChunkLoweringCtx::new(
            &resolved.resolutions,
            shape_data,
            file_paths,
            type_mode,
        )),
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
    // The knot-invariant half of the lowering environment (resolution
    // lookup, struct-shape tables, file paths, type mode), built once per
    // project revision rather than once per knot — issue #460.
    let ctx = &chunk_lowering_ctx_query(db, project).ctx;

    // The file's normalized+stamped HIR, shared across all its knots'
    // memos (so a K-knot file normalizes once, not K times).
    let hir_file = normalized_stamped_query(db, project, source);
    let Some(knot) = hir_file.knots.get(knot_index) else {
        return LoweredChunk::default();
    };

    let ufcs = &ufcs_resolution_query(db, project).table;
    let coalesce = coalesce_types_query(db, project);
    let tables = brink_ir::lir::AnalyzerTables { ufcs, coalesce };
    let (chunk, diagnostics) = brink_ir::lir::lower_knot_chunk_incremental(
        hir_file,
        knot,
        &resolved.index,
        ctx,
        file_id,
        tables,
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
///
/// Since the #1127 default flip this projects the *resolved* policy
/// (`AnalysisOptions::type_policy()` — explicit `types` or the dialect-keyed
/// default), so the cutoff argument is unchanged: same narrow `TypePolicy`
/// value, resolved one query-hop later.
#[salsa::tracked]
pub(crate) fn type_policy_query(db: &dyn salsa::Database, project: ProjectInput) -> TypePolicy {
    project.analysis_options(db).type_policy()
}

/// The project's resolved `[lints]` policy (issue #1160) as its own narrow
/// projection query — [`type_policy_query`]'s sibling, same cutoff argument:
/// [`lir_lowering_query`]'s severity partition needs `AnalysisOptions.lints`,
/// but reading it through `project.analysis_options(db)` directly would
/// register a dependency on the *whole* input field, so an unrelated options
/// edit (registering a host manifest, say) would force the `no_eq` lowering
/// memo to fully re-execute. `LintPolicy`'s derived `Eq` gives the same
/// cheap-cutoff property `TypePolicy` already has here.
#[salsa::tracked]
pub(crate) fn lint_policy_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> brink_analyzer::LintPolicy {
    project.analysis_options(db).lints.clone()
}

/// FG-4d **link phase**: assemble the per-knot chunk memos and the whole-root
/// chunk into a `Program`. See the section comment above for the
/// byte-identity and non-re-execution arguments.
///
/// Gated on [`has_errors_in_closure_query`] (issue #1032 collapse ruling),
/// not the whole-project [`has_errors_query`] — this function has two
/// callers with different pre-conditions: [`lir_query`] already gates on the
/// (stronger) whole-project check before ever calling here, so for that
/// caller this is inert (whole-project-clean implies closure-clean, since
/// the closure is a subset); [`lir_in_closure_query`] gates on exactly this
/// (weaker) check itself, so this must use the same one to actually permit
/// lowering when only a file outside `entry`'s closure is broken. The
/// per-file lowering below was already scoped to `topological_order(entry)`
/// (issue #815) regardless of which gate is used here.
#[salsa::tracked(no_eq)]
pub(crate) fn lir_lowering_query(db: &dyn salsa::Database, project: ProjectInput) -> LirLowering {
    if project.entry(db).is_none() {
        return LirLowering::default();
    }
    if has_errors_in_closure_query(db, project) {
        return LirLowering::default();
    }

    let files = project.files(db);
    let resolved = resolutions_index_query(db, project);

    // LIR inputs in compile (paste-before) order, mirroring
    // `Driver::lir_inputs`. The order comes from [`compilation_closure_files`]:
    // for an ink project this is `entry`'s transitive `INCLUDE` closure
    // (issue #815); for a native project it is every discovered `.brink`
    // module (issue #1296 — native files have no `INCLUDE` edges, so the whole
    // discovered tree is the compilation unit). Files outside it never lower
    // here; their diagnostics still run independently via
    // `analysis_diagnostics_query`/`diagnostics_query` below and in
    // `super::diagnostics_query`, which iterate `project.files(db)`
    // directly rather than through this order.
    let by_id: LookupMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    let topo = compilation_closure_files(db, project);
    // #1696: root-relative, not raw — see `normalized_stamped_query`'s doc.
    // Feeds `lower_root_content_for_prelude`'s `IdAllocator::set_path_prefix`
    // call below, which must agree with `normalized_stamped_query`'s
    // pre-stamped HIR ids byte-for-byte, so both use the same normalization.
    let ink_root = project.ink_root(db).as_deref();
    let paths: LookupMap<FileId, String> = topo
        .iter()
        .filter_map(|id| {
            by_id.get(id).map(|f| {
                (
                    *id,
                    crate::modules::root_relative_key(ink_root, f.path(db)).into_owned(),
                )
            })
        })
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
    // [`lint_policy_query`]'s sibling narrow projection (issue #1160) —
    // same cutoff rationale as `type_policy_query` above.
    let lints = lint_policy_query(db, project);

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
    let ufcs = &ufcs_resolution_query(db, project).table;
    let coalesce = coalesce_types_query(db, project);
    let tables = brink_ir::lir::AnalyzerTables { ufcs, coalesce };
    let (root_chunks, root_temp_slots) = brink_ir::lir::lower_root_content_for_prelude(
        &prelude,
        &resolved.index,
        &resolved.resolutions,
        &paths,
        tables,
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
    let (lir_errors, lir_warnings): (Vec<Diagnostic>, Vec<Diagnostic>) =
        lir_diagnostics.into_iter().partition(|d| {
            brink_analyzer::effective_severity(d.code, types, &lints) == Severity::Error
        });

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
    let opts = project.analysis_options(db);
    let types = opts.type_policy();
    let (mut errors, mut warnings) =
        partition_diagnostics(&inputs, diagnostics, disable_all, types, &opts.lints);

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

/// The compile-scoped counterpart to [`lir_query`] (issue #1032 collapse
/// ruling, option (a) "both, scoped"): gates on [`has_errors_in_closure_query`]
/// — `entry`'s transitive `INCLUDE` closure — instead of the whole-project
/// [`has_errors_query`]. [`lir_query`] itself is untouched: `db.lir_product()`
/// and `db.has_errors()` stay whole-project-gated, exactly as FG-4a's
/// dependency-edge tests (`fg4a_dependency_edges.rs`) pin.
///
/// This is what [`story_data_query`] reads, so `db.story_data()` —
/// `compileProject`'s artifact path — no longer fails a clean entry just
/// because some other file loaded into the same session db (a WIP scratch
/// file, a second unrelated story) happens to have an error. That error
/// still surfaces through `diagnostics_query`/`db.diagnostics(file)`
/// (unchanged, whole-project) — this only narrows the *build gate*, not
/// what's diagnosed. For the CLI driver (`brink-compiler`), whose db is
/// already built from `brink-driver::discover(entry)` — entry plus its
/// transitive `INCLUDE`s only — `project.files(db)` and the closure coincide,
/// so this is behaviorally identical to the old whole-project gate there.
///
/// `errors`/`warnings` are computed the same closure-filtered way
/// [`has_errors_in_closure_query`] computes its verdict, so a file outside
/// `entry`'s closure never contributes to `compileProject`'s own error/
/// warning list either.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn lir_in_closure_query(db: &dyn salsa::Database, project: ProjectInput) -> LirProduct {
    let files = project.files(db);
    let Some(entry) = project.entry(db) else {
        return LirProduct::default();
    };

    let closure: LookupSet<FileId> = compilation_closure_files(db, project).into_iter().collect();

    let diagnostics: Vec<Diagnostic> = analysis_diagnostics_query(db, project)
        .iter()
        .filter(|d| closure.contains(&d.file))
        .cloned()
        .collect();

    let disable_all = files
        .iter()
        .find(|f| f.file_id(db) == entry)
        .is_some_and(|f| suppressions_query(db, *f).disable_all);
    let inputs: Vec<FileDiagnostics<'_>> = files
        .iter()
        .filter(|f| closure.contains(&f.file_id(db)))
        .map(|f| FileDiagnostics {
            file: f.file_id(db),
            source: f.text(db),
            suppressions: suppressions_query(db, *f),
            lowering: &lowered_query(db, *f).diagnostics,
        })
        .collect();
    let opts = project.analysis_options(db);
    let types = opts.type_policy();
    let (mut errors, mut warnings) =
        partition_diagnostics(&inputs, &diagnostics, disable_all, types, &opts.lints);

    if has_errors_in_closure_query(db, project) {
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
/// - **#882 freeze semantics**: `is_entry` is `false` exactly when `def` is in
///   `story.private_defs` (already populated by codegen from
///   `Program::private_defs` — `#@private`, `docs/modules-spec.md` §4 — by
///   the time this runs), `true` otherwise. This is the *only* filter T2-3 was
///   missing: every row still ships regardless (a `#@private` knot/stitch can
///   still be captured as a fn-value token a *public* path holds, and the
///   dispatch-narrowing machinery resolves that token by `DefinitionId`, not
///   by name — `#@private` hides the name, not the cell). `is_entry` only
///   gates whether the row is a legitimate *host-lookup* target; it is never
///   used to drop a row from this table.
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

    // `story.private_defs` is sorted ascending by raw id (codegen hands it
    // straight from `Program::private_defs`, itself sorted by
    // `brink_ir::lir::lower::build_prelude_decls` — see that fn's doc). Cloned
    // once up front (small — one `u64` per `#@private` def, empty for the
    // all-public pre-modules world) so the loop below can freely mutate
    // `story.name_table` alongside without a field-borrow conflict; membership
    // is then a deterministic, order-independent binary search (mirrors
    // `brink_runtime::Program::is_private`).
    let private_defs = story.private_defs.clone();
    let is_private = |def: DefinitionId| {
        private_defs
            .binary_search_by_key(&def.to_raw(), |d| d.to_raw())
            .is_ok()
    };

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
            is_entry: !is_private(def),
            direct: DirectEffects {
                reads: row.reads.iter().copied().collect(),
                writes: row.writes.iter().copied().collect(),
                calls,
                // §6.1 (issue #1680): the wire's `EffectRows` section stays
                // one **ground** row per def, so a row still carrying a row
                // variable is *closed* to opaque here — the conservative
                // direction, and byte-identical to what shipped before holes
                // existed. Fork C's ruled encoding (an explicit hole slot,
                // filled by §7's token lookup) is the remaining wire half and
                // lands with runtime narrowing (#1723); the section is
                // section-locally versioned so it can grow without a format
                // bump.
                opaque: row.is_pessimal(),
                // NS-A2 (issue #1108): the three new row dimensions ship
                // straight from the analyzer's inferred row.
                emits: row.emits,
                tags: row.tags,
                faults: row.faults,
            },
            dispatches: Vec::new(),
        });
    }
    // `inferable` is a `BTreeSet`, so `rows` is already sorted by `def`.
    story.effect_rows = rows;
}

/// Reads [`lir_in_closure_query`], not [`lir_query`] (issue #1032 collapse
/// ruling): `db.story_data()` — `compileProject`'s artifact path — gates on
/// `entry`'s `INCLUDE` closure only, so an error in some other file sharing
/// the session db no longer blocks this entry's build. `db.lir_product()`/
/// `db.has_errors()` stay on [`lir_query`]/[`has_errors_query`], whole-project
/// as before.
#[salsa::tracked(returns(ref))]
pub(crate) fn story_data_query(db: &dyn salsa::Database, project: ProjectInput) -> CompileProduct {
    let lir = lir_in_closure_query(db, project);
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
    allow_scopes: Vec::new(),
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
///
/// `lints`: the project's resolved `[lints]` policy (issue #1160), the other
/// input [`brink_analyzer::effective_severity`] partitions by — per-code
/// `deny`/`warn`/`allow`/`info`/`hint` overrides (issue #1162 added the
/// latter two) plus `deny-warnings`.
#[must_use]
pub fn partition_diagnostics(
    files: &[FileDiagnostics<'_>],
    analysis_diagnostics: &[Diagnostic],
    disable_all: bool,
    types: brink_analyzer::TypePolicy,
    lints: &brink_analyzer::LintPolicy,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let mut partition = |d: Diagnostic| {
        if brink_analyzer::effective_severity(d.code, types, lints) == Severity::Error {
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
/// intact so the assembled `HirFile` stays byte-identical to the previous
/// pipeline. The manifest is no longer assembled by merging per-knot/
/// top-level manifest fragments (B0.4, docs/hir-admission-contract.md
/// Q3(b), issue #1173): `project_manifest` derives the whole
/// `SymbolManifest` from the fully assembled `HirFile` in one pass, so
/// `lower_single_knot`/`lower_top_level` no longer need to produce a
/// manifest at all.
fn lower_file(file_id: FileId, parse: &Parse) -> LoweredFile {
    let tree = parse.tree();

    // Per-knot lowering (document order).
    let knot_entries: Vec<_> = tree
        .knots()
        .map(|knot_ast| lower_single_knot(file_id, &knot_ast))
        .collect();

    // Top-level lowering (everything outside knots).
    let (root_content, top_level_knots, top_diagnostics) = lower_top_level(file_id, &tree);

    // Assemble a complete `HirFile`: use `lower()` for the declarations
    // (variables, constants, lists, externals, includes), then replace knots
    // and root content with the per-knot/top-level products above.
    let (mut hir, _full_manifest, _full_diag) = lower(file_id, &tree);
    hir.knots = knot_entries
        .iter()
        .filter_map(|(knot, _)| knot.clone())
        .collect();
    hir.knots.extend(top_level_knots);
    hir.root_content = root_content;

    let manifest = project_manifest(&hir);

    // Merge diagnostics, then surface parser/syntax errors as compile
    // diagnostics (`E037`) so malformed source fails the compile.
    let mut diagnostics = top_diagnostics;
    for (_, knot_diags) in &knot_entries {
        diagnostics.extend(knot_diags.iter().cloned());
    }
    diagnostics.extend(parse.errors().iter().map(|e| Diagnostic {
        file: file_id,
        range: e.range,
        message: e.message.clone(),
        code: DiagnosticCode::E037,
    }));

    // Anonymous-container state lint (`E157`, issue #1674): off/info by
    // default, `Warning`-flow-shaped otherwise — folded into `diagnostics`
    // (never `admission`) so it flows through `apply_suppressions`/
    // `effective_severity` in `partition_diagnostics` exactly like `E151`
    // below does for native, and is configurable through `[lints]`/
    // `//brink-disable` like any other tier-able diagnostic.
    diagnostics.extend(brink_analyzer::check_anonymous_stateful(file_id, &hir));

    // B0.3 admission validator (docs/hir-admission-contract.md §4.2, issue
    // #1172): a loud, non-suppressible pass wired directly at this seam so
    // it runs on every lowering (NF-6, always-on). Kept in its own field —
    // never folded into `diagnostics` above, which flows through
    // `apply_suppressions` in `partition_diagnostics`.
    let file_len = parse.syntax().text_range().end();
    let admission = brink_analyzer::validate_admission(file_id, &hir, &manifest, file_len);

    LoweredFile {
        hir,
        manifest,
        diagnostics,
        admission,
    }
}

/// Lower one native `.brink` file to HIR (B0.10a, the native compile seam,
/// issue #1106) — the frontend-specific sibling of [`lower_file`], producing
/// the *same* [`LoweredFile`] so everything downstream (analysis, LIR,
/// codegen) is byte-for-byte frontend-agnostic.
///
/// Unlike ink, native lowering is a single whole-file entry point
/// (`lower_native::lower`) — there is no per-knot / top-level split to
/// reassemble, and it returns its own [`project_manifest`]-derived manifest
/// (B0.4) already, so this composes rather than re-deriving. Error-severity
/// syntax errors are surfaced as the same non-suppressible `E037` compile
/// diagnostic `lower_file` uses; Warning-severity ones (issue #1263 — e.g.
/// `<-` outside a choice point) map to `E131` instead, which
/// `DiagnosticCode::severity` reports as `Severity::Warning` so it never
/// gates `has_errors_query`/`has_errors_in_closure_query`. The B0.3
/// admission validator runs at the same seam (NF-6, always-on).
fn lower_native_file(file_id: FileId, parse: &NativeParse) -> LoweredFile {
    let tree = parse.tree();

    let (hir, manifest, mut diagnostics) = brink_ir::hir::lower_native::lower(file_id, &tree);

    // Surface parser diagnostics as compile diagnostics, split by severity:
    // `Error` becomes the non-suppressible `E037` (malformed source fails
    // the compile, mirrors `lower_file`); `Warning` becomes `E131`, which
    // is advisory only and must never block compilation.
    diagnostics.extend(parse.errors().iter().map(|e| Diagnostic {
        file: file_id,
        range: e.range,
        message: e.message.clone(),
        code: match e.severity {
            brink_syntax_native::ParseSeverity::Error => DiagnosticCode::E037,
            brink_syntax_native::ParseSeverity::Warning => DiagnosticCode::E131,
        },
    }));

    // Native lint: asymmetric choice-branch dead-end (`E151`, issue #1219,
    // decision-log 2026-07-22 "Flows end implicitly (native)" item 4) — the
    // relocated residual value of ink's retired "ran out of content" error.
    // Deliberately folded into `diagnostics`, never `admission`: unlike the
    // B0.9 accept-list below, this is `Severity::Warning`-base, on by
    // default (not opt-in — see the lint module's own doc), and
    // configurable/suppressible through `[lints]`/`//brink-disable` like
    // any other tier-able diagnostic — it must flow through
    // `apply_suppressions`/`effective_severity` in `partition_diagnostics`,
    // which only `diagnostics` does.
    diagnostics.extend(brink_analyzer::check_native_choice_dead_end(file_id, &hir));

    // Anonymous-container state lint (`E157`, issue #1674) — see `lower_file`'s
    // identical wiring comment; the check itself is frontend-agnostic (it
    // only reads `Choice::is_sticky`/`label` and `Sequence::kind`/branches,
    // both populated the same way by ink and native lowering).
    diagnostics.extend(brink_analyzer::check_anonymous_stateful(file_id, &hir));

    // B0.3 admission validator (docs/hir-admission-contract.md §4.2, issue
    // #1172): the same loud, non-suppressible pass `lower_file` runs, kept in
    // its own `LoweredFile` field so it never flows through
    // `apply_suppressions`.
    let file_len = parse.syntax().text_range().end();
    let mut admission = brink_analyzer::validate_admission(file_id, &hir, &manifest, file_len);

    // B0.9 native accept-list gate (docs/hir-admission-contract.md §4.4/§5
    // Q6, docs/b0-sequencing.md §B0.9, issue #1179): the inverse of the ink
    // `dialect_gate` reject-list, and native-only — this is the seam that
    // keys it off the producing frontend at the pipeline level (F-I#10):
    // `lower_native_file` only ever runs for a `.brink` file, never an
    // `.ink` one, so calling this here (and nowhere in `lower_file`) is the
    // whole dispatch, with no tag carried on the tree itself. Appended into
    // the same non-suppressible `admission` field B0.3 populates above —
    // both are loud, always-on checks at this exact seam.
    admission.extend(brink_analyzer::validate_native_accept_list(file_id, &hir));

    LoweredFile {
        hir,
        manifest,
        diagnostics,
        admission,
    }
}

#[cfg(test)]
mod tests {
    use super::{DefKey, call_graph_query, def_effect_atoms_query, inferable_defs_query};
    use crate::db::ProjectDb;

    /// Issue #1736 finding (BLOCKING): the parity tests in
    /// `crates/internal/brink-db/tests/query_equivalence.rs` compare the two
    /// call-graph constructions' *outputs* on a fixture where they provably
    /// cannot disagree — `resolve_pending_value_calls` re-records every
    /// traced `#fn`/`bind` target as a `direct_calls` edge at its call site,
    /// so a fixture that ever calls what it creates can't exercise a
    /// `creates_fn_values`-outside-`direct_calls` shape. This test instead
    /// asserts the *edge set* directly: for every def, `call_graph_query`'s
    /// outgoing edges must cover `EffectAtoms.direct_calls ∪
    /// EffectAtoms.creates_fn_values` — the subset property
    /// `docs/effects-spec.md` §6.1a documents and
    /// `every_fn_value_creation_target_is_also_a_call_graph_edge`
    /// (`brink-analyzer`'s `infer::mod` tests) pins from the atom side.
    /// Unlike the output-parity tests, this goes red the day #1727 (lambda
    /// literals) breaks that subset property, independent of whether any
    /// particular fixture's diagnostics happen to still agree.
    #[test]
    fn call_graph_covers_direct_calls_and_creates_fn_values() {
        let mut db = ProjectDb::new();
        db.set_file(
            "main.ink",
            "VAR total = 0\nVAR extra = 0\n\
             === function bar(): int ===\n~ total = total + 1\n~ return total\n\
             === function baz(): int ===\n~ extra = extra + 100\n~ return extra\n\
             === function user(cond: int): int ===\n\
             ~ temp f = #fn(bar)\n{cond:\n  ~ f = #fn(baz)\n}\n~ return f()\n"
                .to_owned(),
        );

        let (salsa, project) = db.salsa_and_project();
        let graph = call_graph_query(salsa, project);

        for &def in inferable_defs_query(salsa, project) {
            let atoms = def_effect_atoms_query(salsa, project, DefKey::new(salsa, def));
            let outgoing = graph.edges.get(&def).cloned().unwrap_or_default();
            for &callee in atoms
                .direct_calls
                .iter()
                .chain(atoms.creates_fn_values.iter())
            {
                assert!(
                    outgoing.contains(&callee),
                    "call_graph_query's edges for {def:?} do not cover \
                     direct_calls ∪ creates_fn_values: missing edge to \
                     {callee:?} (direct_calls={:?}, creates_fn_values={:?}, \
                     graph edges={outgoing:?})",
                    atoms.direct_calls,
                    atoms.creates_fn_values,
                );
            }
        }
    }
}
