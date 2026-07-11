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

    if let Some(program) = program {
        let mut warnings = warnings;
        warnings.extend(lir_diagnostics);
        LirProduct {
            program: Some(Arc::new(program)),
            errors,
            warnings,
        }
    } else {
        // `lower_to_program` refused to lower: its residual-extension
        // backstop fired (E053), meaning a T1b brink-extension HIR node
        // reached LIR lowering despite the dialect gate — only possible if
        // the gate's analysis diagnostic was suppressed. Surface it as a
        // compile error; never return a corrupt or partial program. See
        // #572 review.
        let mut errors = errors;
        errors.extend(lir_diagnostics);
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
