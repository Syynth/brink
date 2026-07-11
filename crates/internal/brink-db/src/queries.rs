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
//! # The `resolution_index` cutoff seam (slice-A findings 1+2)
//!
//! The full [`SymbolIndex`] carries a `TextRange` per symbol, so nearly any
//! edit shifts ranges and defeats `Eq`-cutoff on the index — dependents of
//! `symbol_index` would re-run on every keystroke. [`resolution_index_query`]
//! sits between the index and reference resolution: it is the full index with
//! ranges *zeroed for every non-local symbol*. Resolution reads symbol ranges
//! in exactly one place — `lookup_local_in_scope`'s closest-preceding pick,
//! which only inspects `Param`/`Temp` symbols — so stripping the other ranges
//! cannot change any resolution and the strip is behavior-neutral by
//! construction (locked by `analysis_matches_monolithic_analyzer` tests and
//! the oracle gate). Locals stay in the projection because cross-file
//! duplicate scoped locals share a `DefinitionId` (finding 4): removing or
//! re-keying them would change last-writer-wins resolution in duplicate-name
//! projects, which is out of scope for a behavior-neutral slice.

use std::collections::HashMap;
use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, AnalysisResult, Sig};
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
                // Layer 3.
                .ingredient::<lir_index_query>()
                .ingredient::<normalized_hir_query>()
                .ingredient::<lir_decls_query>()
                .ingredient::<lir_chunk_query>()
                .ingredient::<lir_names_query>()
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

/// The early-cutoff projection of the symbol index used by resolution: the
/// full index with ranges zeroed for every non-local symbol (see module
/// docs). Body/whitespace edits that only shift global declaration ranges
/// backdate here, so every other file's `resolve` memo survives untouched.
#[salsa::tracked(returns(ref))]
pub(crate) fn resolution_index_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> Arc<SymbolIndex> {
    let (index, _diags) = symbol_index_query(db, project);
    let mut stripped: SymbolIndex = (**index).clone();
    for info in stripped.symbols.values_mut() {
        if !matches!(info.kind, SymbolKind::Param | SymbolKind::Temp) {
            info.range = rowan::TextRange::default();
        }
    }
    Arc::new(stripped)
}

/// Resolve one file's references against the project-wide names. Thin
/// wrapper over [`brink_analyzer::resolve`], fed the cutoff projection.
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
/// [`DefinitionId`] alone: colliding ids (duplicate scoped locals across
/// files, slice-A finding 4) map to a *single* index entry chosen
/// deterministically by the merge, so the memo cannot diverge from what a
/// non-memoized `signature(def)` call would return for the same id.
#[salsa::interned]
pub(crate) struct DefKey<'db> {
    pub def: DefinitionId,
}

/// Per-declaration signature stub (spec §4 layer 2). Reads the range-stripped
/// index projection — [`Sig`] carries no ranges, so this is output-identical
/// to reading the full index while backdating across whitespace/body edits.
#[salsa::tracked]
pub(crate) fn signature_query<'db>(
    db: &'db dyn salsa::Database,
    project: ProjectInput,
    def: DefKey<'db>,
) -> Option<Arc<Sig>> {
    let index = resolution_index_query(db, project);
    let files = project.files(db);
    let hir_refs: Vec<(FileId, &HirFile)> = files
        .iter()
        .map(|f| (f.file_id(db), &lowered_query(db, *f).hir))
        .collect();
    brink_analyzer::signature(def.def(db), index, &hir_refs)
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

// ─── Layer 3: lowering / codegen (per-file chunks, slice C) ──────────

/// Fully range-stripped symbol-index projection for LIR lowering and
/// container-ID stamping (spec §4 layer 3, slice C).
///
/// [`resolution_index_query`] keeps `Param`/`Temp` ranges because
/// resolution's closest-preceding pick reads them. LIR lowering reads only
/// `kind`/`id`/`name`/`params` (and `by_name`) — never a range — so this
/// projection zeroes the local ranges too. Result: edits that only shift
/// local declaration ranges backdate here, and every file's `lir_chunk`
/// memo survives. Locked by the incremental fuzz harness and the
/// `story_data_matches_monolithic_pipeline` equivalence test.
#[salsa::tracked(returns(ref))]
pub(crate) fn lir_index_query(db: &dyn salsa::Database, project: ProjectInput) -> Arc<SymbolIndex> {
    let index = resolution_index_query(db, project);
    let mut stripped: SymbolIndex = (**index).clone();
    for info in stripped.symbols.values_mut() {
        info.range = rowan::TextRange::default();
    }
    Arc::new(stripped)
}

/// The compile file set in topological include order from the entry point
/// (paste-before semantics), mirroring `Driver::lir_inputs`. Empty when no
/// entry point is set.
///
/// A plain helper, not a tracked query: the computation is O(files) off the
/// memoized (`Eq`-cutoff) `include_graph`, and every one-shot compile pays
/// per-ingredient database-construction cost — measured at ~0.9 µs per
/// ingredient — so cheap derivations stay functions.
fn topo_source_files(db: &dyn salsa::Database, project: ProjectInput) -> Vec<SourceFile> {
    let Some(entry) = project.entry(db) else {
        return Vec::new();
    };
    let files = project.files(db);
    let graph = include_graph_query(db, project);
    let all_ids: Vec<FileId> = files.iter().map(|f| f.file_id(db)).collect();
    let by_id: HashMap<FileId, SourceFile> = files.iter().map(|f| (f.file_id(db), *f)).collect();
    graph
        .topological_order(entry, &all_ids)
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .collect()
}

/// Per-file normalized + container-ID-stamped HIR — the input to LIR
/// lowering (steps 0+1, memoized per file). The stored `lowered` HIR stays
/// pristine for the IDE; this is the regularized copy lowering consumes.
#[salsa::tracked(returns(ref))]
pub(crate) fn normalized_hir_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> HirFile {
    let index = lir_index_query(db, project);
    brink_ir::lir::normalize_and_stamp(file.file_id(db), &lowered_query(db, file).hir, index)
}

/// Declaration-derived program data (globals, lists, externals) plus the
/// post-declaration name-table state that seeds the first file chunk, and
/// the top-level `~ temp` slot map (root content shares one scope across
/// every file, so it is inherently whole-project). One query for both:
/// every chunk depends on both anyway, so splitting them buys no finer
/// invalidation — only another per-database ingredient. Recomputes cheaply
/// on any edit (declarations + root temp walk, no body lowering) and
/// backdates when the results are unchanged — the usual case.
#[salsa::tracked(returns(ref))]
pub(crate) fn lir_decls_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
) -> (brink_ir::lir::DeclProduct, brink_ir::lir::TempMap) {
    let index = lir_index_query(db, project);
    let files = topo_source_files(db, project);
    let hir_refs: Vec<(FileId, &HirFile)> = files
        .iter()
        .map(|f| (f.file_id(db), normalized_hir_query(db, project, *f)))
        .collect();
    let mut resolutions = ResolutionMap::new();
    for f in &files {
        let (file_map, _diags) = resolve_query(db, project, *f);
        resolutions.extend(file_map.iter().cloned());
    }
    let lookup = brink_ir::lir::ResolutionLookup::build(&resolutions);
    let decls = brink_ir::lir::collect_declarations(&hir_refs, index, &lookup);
    let root_temps = brink_ir::lir::root_temp_map(&hir_refs);
    (decls, root_temps)
}

/// Memo wrapper for one file's LIR chunk. `Arc` because the chunk holds LIR
/// trees without `PartialEq` — identity is the only cheap equality proxy
/// (same stance as [`LirProduct`]). Backdating is disabled via `no_eq` on
/// [`lir_chunk_query`]; the inter-file cutoff seam is [`lir_names_query`].
#[derive(Clone)]
pub(crate) struct ChunkProduct(pub(crate) Arc<brink_ir::lir::FileChunk>);

impl PartialEq for ChunkProduct {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The name-table state after one file's chunk — the early-cutoff firewall
/// between files. Body lowering only ever interns *new temp names*, so an
/// edit that introduces none recomputes this to an equal value, salsa
/// backdates, and every downstream file's chunk memo survives.
#[salsa::tracked(returns(ref))]
pub(crate) fn lir_names_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> Vec<String> {
    lir_chunk_query(db, project, file).0.name_entries.clone()
}

/// One file's LIR chunk (its slice of container lowering): root statements,
/// child containers, counting refs, and outgoing name-table state. Only
/// ever pulled for files in [`lir_topo_query`] order, so the recursive
/// `lir_names_query` dependency on the predecessor is always memoized
/// (recursion depth stays O(1) when pulled in order).
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn lir_chunk_query(
    db: &dyn salsa::Database,
    project: ProjectInput,
    file: SourceFile,
) -> ChunkProduct {
    let file_id = file.file_id(db);
    let files = topo_source_files(db, project);
    let pos = files.iter().position(|f| f.file_id(db) == file_id);
    let names_in: &[String] = match pos {
        // First file — seeded by declaration interning. (`None` cannot
        // happen for files pulled via the topo order; the fallback keeps
        // the query total.)
        Some(0) | None => &lir_decls_query(db, project).0.name_entries,
        Some(p) => match files.get(p - 1) {
            Some(prev) => lir_names_query(db, project, *prev),
            None => &lir_decls_query(db, project).0.name_entries,
        },
    };

    let index = lir_index_query(db, project);
    let normalized = normalized_hir_query(db, project, file);
    let (file_resolutions, _diags) = resolve_query(db, project, file);
    let lookup = brink_ir::lir::ResolutionLookup::build(file_resolutions);
    let root_temps = &lir_decls_query(db, project).1;

    ChunkProduct(Arc::new(brink_ir::lir::lower_file_chunk(
        file_id,
        normalized,
        index,
        &lookup,
        file.path(db),
        names_in,
        root_temps,
    )))
}

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

    // LIR lowering: one memoized chunk per file in topological include
    // order (paste-before semantics), assembled behind this query. Pulling
    // chunks in topo order keeps the predecessor `lir_names` recursion
    // memoized (slice C, #460).
    let topo_files = topo_source_files(db, project);
    let decls = lir_decls_query(db, project).0.clone();
    let chunks: Vec<brink_ir::lir::FileChunk> = topo_files
        .iter()
        .map(|f| (*lir_chunk_query(db, project, *f).0).clone())
        .collect();
    let name_table = chunks
        .last()
        .map_or_else(|| decls.name_entries.clone(), |c| c.name_entries.clone());

    let (program, lir_warnings) = brink_ir::lir::assemble_program(decls, chunks, name_table);

    let mut warnings = warnings;
    warnings.extend(lir_warnings);

    LirProduct {
        program: Some(Arc::new(program)),
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
#[salsa::tracked(returns(ref))]
pub(crate) fn story_data_query(db: &dyn salsa::Database, project: ProjectInput) -> CompileProduct {
    let lir = lir_query(db, project);
    CompileProduct {
        story: lir
            .program
            .as_ref()
            .map(|p| Arc::new(brink_codegen_inkb::emit(p))),
        errors: lir.errors.clone(),
        warnings: lir.warnings.clone(),
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
