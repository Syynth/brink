mod blocks;
mod chunk;
mod content;
mod context;
mod decls;
mod expr;
mod recognize;
mod stmts;
mod structs;
mod temps;

use brink_format::CountingFlags;

use crate::FileId;
use crate::determinism::{LookupMap, LookupSet};
use crate::hir;
use crate::symbols::{ResolutionMap, SymbolIndex};

use super::types as lir;
use context::{LowerCtx, NameTable, ResolutionLookup, TempMap};

pub use chunk::ScopeChunk;
pub use context::{
    AnalyzerTables, CoalesceLookup, CoalesceShape, TypeMode, UfcsLookup, UfcsVerdict,
};
pub use structs::{StructFieldEntry, StructShapeData, StructShapeEntry, build_struct_shape_data};

/// Defensive backstop for `brink-analyzer`'s dialect gate (E051/E052).
///
/// `brink-syntax` always parses the full superset grammar and `brink-ir`
/// always lowers it to HIR; whether T1b brink-extension constructs
/// (`~ { … }` logic blocks, `#[…]`/`#{…}` sigil literals, postfix indexing)
/// are *allowed* is decided by the dialect gate, which runs as an
/// *analysis* diagnostic. Analysis diagnostics are suppressible
/// (`// brink-disable-all` / line directives — see `crate::suppressions`),
/// so "the gate already rejected this" is not provably true by the time
/// `lower_to_program` runs: a suppressed gate lets a residual
/// `LogicBlock`/`ArrayLiteral`/`MapLiteral`/`Index` HIR node flow in here.
///
/// Scan for that and refuse to lower rather than falling through to the
/// `lower_stmt`/`lower_expr` fallback arms, which would otherwise silently
/// drop the construct (`None`) or replace it with `Null` — a real data-loss
/// bug, not just a `debug_assert!` that's a no-op in release builds. See
/// #572 review.
/// T1b-2 (#570) retirement note: through T1b-1, this module ran a
/// non-suppressible pre-scan (E053) that refused to lower a `LogicBlock`/
/// `ArrayLiteral`/`MapLiteral`/`Index` HIR node reaching here — a defensive
/// backstop for `brink-analyzer`'s dialect gate (E051/E052), which is a
/// *suppressible* analysis diagnostic (`// brink-disable-all`), because the
/// T1b-1 fallback arms for these node kinds were `debug_assert!`-guarded
/// stubs that silently dropped data (`None`) or corrupted it (`Null`) in
/// release builds if the gate was bypassed (#572 review).
///
/// T1b-2 replaces that rejection with real lowering for all four node kinds
/// (`blocks::lower_logic_block` below; `expr::lower_expr`'s
/// `ArrayLiteral`/`MapLiteral`/`Index` arms) — the correctness hazard the
/// backstop existed to catch (silent drop/corruption) no longer exists,
/// because there is no longer a "residual" case: every brink-extension HIR
/// node now lowers to a correct program under both dialects. `strict-ink`
/// enforcement is unchanged and rests solely on E051 (as with every other
/// suppressible diagnostic in this codebase) — see the T1b-2 PR description.
/// A *future* extension construct that lands parse/HIR-only again (as this
/// one briefly did) should reintroduce a scoped version of this backstop
/// for exactly its own node kind(s), not resurrect this one.
///
/// Lower analyzed HIR into a resolved LIR `Program`.
///
/// All references are resolved — the returned `Program` is self-contained
/// and does not need the `SymbolIndex` or `ResolutionMap`.
///
/// `file_paths` maps each `FileId` to its source file path for populating
/// `SourceLocation` on recognized lines.
#[expect(
    clippy::implicit_hasher,
    reason = "internal API, no need to generalize"
)]
pub fn lower_to_program(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    file_paths: &LookupMap<FileId, String>,
) -> (Option<lir::Program>, Vec<crate::Diagnostic>) {
    lower_to_program_with_type_mode(
        files,
        index,
        resolutions,
        file_paths,
        context::TypeMode::Gradual,
        context::AnalyzerTables {
            ufcs: &context::UfcsLookup::new(),
            coalesce: &context::CoalesceLookup::new(),
        },
    )
}

/// [`lower_to_program`] with an explicit `types` policy (TM-4c,
/// `docs/typed-mode-spec.md` §6).
///
/// **Not on the compile path.** Through FG-4c/d/e, `brink-db`'s production
/// link phase (`lir_lowering_query`, backing `ProjectDb::lir_product`/
/// `story_data`) stopped calling this whole-project entry: it composes the
/// same three phases directly — its own `lir_prelude_decls_query` +
/// `assemble_prelude`, [`lower_root_content_for_prelude`], and the per-knot
/// [`lower_knot_chunk_incremental`] memoized per `DefinitionId` — so a
/// knot-body edit re-lowers one chunk instead of the whole project. FG-6
/// (#841) then removed `brink-compiler`'s own direct call, so every batch
/// consumer (CLI, `brink-web`, `brink-intl`, the oracle harness) now reaches
/// codegen through `ProjectDb::story_data()` too; there is exactly one
/// compile path in production.
///
/// This function stays `pub` regardless — issue #841's audit (grep for
/// external callers) found two real, deliberate direct consumers this
/// composition does not supersede, both outside this crate: the
/// `compile_bench` benchmark's staged/legacy-path rows, which exist
/// specifically to measure this whole-project one-shot call *as the
/// baseline* against the `ProjectDb`-driven per-chunk path (narrowing would
/// delete the comparison, not the redundancy); and `golden_i078.rs`, a
/// golden pipeline test that pins this function's exact LIR output for one
/// fixture in isolation, deliberately bypassing `ProjectDb`. Narrowing to
/// `pub(crate)` would break both for no correctness gain. `brink-ir`'s own
/// `lir_lowering.rs` integration tests are the remaining caller (needs
/// `pub`, not `pub(crate)`, since `tests/` compiles as a separate crate).
///
/// Every other caller (the tests above) gets the gradual default via
/// [`lower_to_program`], which is always semantically valid — gradual never
/// emits a static-offset op gated on `types = strict` (see `expr::
/// known_shape`'s doc).
#[expect(
    clippy::implicit_hasher,
    reason = "internal API, no need to generalize"
)]
pub fn lower_to_program_with_type_mode(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    file_paths: &LookupMap<FileId, String>,
    type_mode: context::TypeMode,
    tables: context::AnalyzerTables<'_>,
) -> (Option<lir::Program>, Vec<crate::Diagnostic>) {
    // FG-4d/e: this whole-project entry runs the same three pure phases
    // `brink-db`'s production link phase (`lir_lowering_query`) composes
    // from its own per-def/whole-project memos (prelude decls, per-knot
    // chunk, assembly) — so the two are byte-identical by construction, even
    // though `brink-db` no longer routes through this function at all (see
    // this fn's doc). The remaining callers here (this crate's own
    // `lir_lowering.rs` tests, `compile_bench`, `golden_i078.rs`) keep a
    // single whole-project call; `brink-db` caches the per-knot phase
    // individually, per `DefinitionId`, instead.
    let prelude = build_prelude(files, index, resolutions, file_paths, type_mode);
    let resolutions = ResolutionLookup::build(resolutions);
    let struct_ctx = prelude.struct_ctx();

    // Root content (all files, one shared frame) then each knot, collected in
    // the exact interleaved walk order the assembler dedups names against.
    let prelude_files = prelude.files();
    let (root_chunks, root_temp_slots) = lower_root_content_chunks(
        &prelude_files,
        &resolutions,
        index,
        prelude.root_id,
        file_paths,
        &struct_ctx,
        tables,
    );

    // Diagnostic order mirrors the old monolithic path exactly: declaration
    // diagnostics first, then per-file (root content then that file's knots).
    let mut lir_diagnostics = prelude.decl_diagnostics.clone();
    let mut ordered_chunks = Vec::new();
    let mut root_iter = root_chunks.into_iter();
    for &(file_id, hir_file) in &prelude_files {
        if let Some((chunk, diags)) = root_iter.next() {
            ordered_chunks.push(chunk);
            lir_diagnostics.extend(diags);
        }
        for knot in &hir_file.knots {
            let (chunk, diags) = lower_knot_chunk(
                hir_file,
                knot,
                index,
                &resolutions,
                file_paths,
                &struct_ctx,
                prelude.root_id,
                file_id,
                tables,
            );
            ordered_chunks.push(chunk);
            lir_diagnostics.extend(diags);
        }
    }

    let program = assemble_program(&prelude, ordered_chunks, root_temp_slots, index);
    (Some(program), lir_diagnostics)
}

// ─── FG-4d: incremental lowering seam ───────────────────────────────
//
// `lower_to_program_with_type_mode` above is the composition of the three
// functions below. `brink-db`'s salsa pipeline calls them separately so the
// middle one (`lower_knot_chunk`) can be memoized per `DefinitionId`: an
// edit that doesn't touch a knot leaves its chunk memo pointer-identical,
// and the whole-project `assemble_program` link re-runs but backdates on the
// `StoryData` `Eq` firebreak (`docs/fine-grained-salsa-proposal.md` §5 + the
// three-resolution-moments appendix).

/// Whole-project data every chunk lowering and the final assembly need but
/// that is not chunk-local: normalized+stamped HIR (topological order), the
/// collected declarations, the struct-shape table, the seeded project name
/// table (decl + struct names, in the fixed order chunk-local names dedup
/// against), and the `StoryData`-bound `private_defs`/`aliases`.
pub struct LirPrelude {
    normalized: Vec<(FileId, hir::HirFile)>,
    root_id: brink_format::DefinitionId,
    globals: Vec<lir::GlobalDef>,
    lists: Vec<lir::ListDef>,
    list_items: Vec<lir::ListItemDef>,
    externals: Vec<lir::ExternalDef>,
    shape_table: structs::ShapeTable,
    global_shapes: structs::GlobalShapeMap,
    name_seed: Vec<String>,
    type_mode: context::TypeMode,
    private_defs: Vec<brink_format::DefinitionId>,
    aliases: Vec<brink_format::AliasEntry>,
    /// Declaration-phase diagnostics (`collect_globals`'s constant-eval
    /// errors) — the diagnostics the monolithic path pushes before any chunk.
    pub decl_diagnostics: Vec<crate::Diagnostic>,
}

impl LirPrelude {
    /// The prelude's normalized+stamped HIR as borrow pairs (topo order).
    #[must_use]
    pub fn files(&self) -> Vec<(FileId, &hir::HirFile)> {
        self.normalized.iter().map(|(id, h)| (*id, h)).collect()
    }

    /// The `root` container's `DefinitionId`.
    #[must_use]
    pub fn root_id(&self) -> brink_format::DefinitionId {
        self.root_id
    }

    fn struct_ctx(&self) -> context::StructCtx<'_> {
        context::StructCtx {
            shapes: &self.shape_table,
            global_shapes: &self.global_shapes,
            type_mode: self.type_mode,
        }
    }
}

/// The declaration-level half of [`LirPrelude`] (issue #839 / FG-4e): every
/// collected `VAR`/`CONST`/`LIST`/`EXTERNAL`/`STRUCT` declaration, the seeded
/// project name table, and the `StoryData`-bound `private_defs`/`aliases` —
/// everything [`build_prelude_decls`] produces. Deliberately *not* the
/// normalized+stamped HIR (`LirPrelude::normalized`): [`collect_globals`],
/// [`collect_lists`], [`collect_externals`], [`build_shape_table`], and
/// [`build_global_shape_map`] read only a file's `constants`/`variables`/
/// `lists`/`structs`/`externals` fields — never `root_content`/`knots`, which
/// [`hir::normalize_file`]/[`hir::stamp_container_ids`] are the only passes
/// that touch — so this half is byte-identical whether it's built from raw,
/// decl-only-projected, or normalized+stamped HIR (`brink-db`'s
/// `lir_prelude_decls_query` exploits exactly this: it reads a per-file
/// decl-only projection that backdates across a body-only edit, so a knot
/// body edit doesn't force this struct's declarations/name table/shape table
/// to be recomputed — the FG-4d `struct_shape_data_query` precedent, applied
/// to the rest of the prelude).
///
/// [`collect_globals`]: decls::collect_globals
/// [`collect_lists`]: decls::collect_lists
/// [`collect_externals`]: decls::collect_externals
/// [`build_shape_table`]: structs::build_shape_table
/// [`build_global_shape_map`]: structs::build_global_shape_map
#[derive(Clone)]
pub struct PreludeDecls {
    root_id: brink_format::DefinitionId,
    globals: Vec<lir::GlobalDef>,
    lists: Vec<lir::ListDef>,
    list_items: Vec<lir::ListItemDef>,
    externals: Vec<lir::ExternalDef>,
    shape_table: structs::ShapeTable,
    global_shapes: structs::GlobalShapeMap,
    name_seed: Vec<String>,
    type_mode: context::TypeMode,
    private_defs: Vec<brink_format::DefinitionId>,
    aliases: Vec<brink_format::AliasEntry>,
    decl_diagnostics: Vec<crate::Diagnostic>,
}

impl PreludeDecls {
    /// The empty prelude decls — no reachable entry (`brink-db`'s
    /// `lir_prelude_decls_query`/`lir_lowering_query` early-return case).
    #[must_use]
    pub fn empty(type_mode: context::TypeMode) -> Self {
        Self {
            root_id: context::root_definition_id(),
            globals: Vec::new(),
            lists: Vec::new(),
            list_items: Vec::new(),
            externals: Vec::new(),
            shape_table: structs::ShapeTable::default(),
            global_shapes: structs::GlobalShapeMap::default(),
            name_seed: Vec::new(),
            type_mode,
            private_defs: Vec::new(),
            aliases: Vec::new(),
            decl_diagnostics: Vec::new(),
        }
    }
}

/// Collect declaration-level LIR data — `VAR`/`CONST`/`LIST`/`EXTERNAL`/
/// `STRUCT` — and seed the project name table, without touching
/// `root_content`/`knots` (see [`PreludeDecls`]'s doc for why that's safe:
/// none of the collection passes below ever read a body). This is exactly
/// steps 1–2 of the old monolithic `build_prelude` (name collection +
/// struct-shape table), factored out so `brink-db` can memoize it
/// independently of the normalize+stamp step (step 0).
#[must_use]
pub fn build_prelude_decls(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    type_mode: context::TypeMode,
) -> PreludeDecls {
    let resolutions_lookup = ResolutionLookup::build(resolutions);
    let mut names = NameTable::new();
    let root_id = context::root_definition_id();

    // The struct-shape table is built *first* (issue #1530): a `VAR`/`CONST`
    // whose default is a construction literal folds into
    // `lir::ConstValue::Record`, which needs the shape's id and declaration
    // field order. Nothing in the shape table depends on the collected
    // declarations, so this is a pure reordering — it only moves the struct
    // and field names ahead of the declaration names in the seeded
    // `NameTable`, and a `NameId` is an index into that same seed, emitted
    // alongside it.
    let shape_table = structs::build_shape_table(files, &mut names);
    let global_shapes = structs::build_global_shape_map(files, index, &shape_table);

    let mut decl_diagnostics = Vec::new();
    let mut globals = decls::collect_globals(
        files,
        index,
        &mut names,
        &resolutions_lookup,
        &shape_table,
        &mut decl_diagnostics,
    );
    let (lists, list_items, list_globals) = decls::collect_lists(files, index, &mut names);
    globals.extend(list_globals);
    let externals = decls::collect_externals(files, index, &mut names);

    let name_seed = names.into_entries();

    let mut private_defs: Vec<brink_format::DefinitionId> = index
        .symbols
        .iter()
        .filter(|(_, info)| info.visibility == crate::symbols::Visibility::Private)
        .map(|(id, _)| *id)
        .collect();
    private_defs.sort_by_key(|id| id.to_raw());

    let mut aliases = index.aliases.clone();
    aliases.sort_unstable();

    PreludeDecls {
        root_id,
        globals,
        lists,
        list_items,
        externals,
        shape_table,
        global_shapes,
        name_seed,
        type_mode,
        private_defs,
        aliases,
        decl_diagnostics,
    }
}

/// Assemble a [`LirPrelude`] from independently-computed [`PreludeDecls`]
/// (issue #839 / FG-4e) plus the normalized+stamped HIR (`brink-db`'s link
/// builds `normalized` from the already-memoized per-file
/// `normalized_stamped_query` instead of recomputing normalize+stamp
/// inline). Pure assembly — no lowering work of its own.
#[must_use]
pub fn assemble_prelude(
    decls: PreludeDecls,
    normalized: Vec<(FileId, hir::HirFile)>,
) -> LirPrelude {
    LirPrelude {
        normalized,
        root_id: decls.root_id,
        globals: decls.globals,
        lists: decls.lists,
        list_items: decls.list_items,
        externals: decls.externals,
        shape_table: decls.shape_table,
        global_shapes: decls.global_shapes,
        name_seed: decls.name_seed,
        type_mode: decls.type_mode,
        private_defs: decls.private_defs,
        aliases: decls.aliases,
        decl_diagnostics: decls.decl_diagnostics,
    }
}

/// Build the whole-project [`LirPrelude`]: normalize + stamp HIR, collect
/// declarations, and build the struct-shape table — steps 0–2 of the old
/// monolithic `lower_to_program`, verbatim and in the same order, so the
/// seeded name table is byte-identical. Now a thin composition of
/// [`build_prelude_decls`] (steps 1–2) over the normalized files (step 0) —
/// see [`PreludeDecls`]'s doc for why running decl collection on normalized
/// vs. raw HIR is byte-identical.
///
/// `file_paths` reaches the stamping pass, which qualifies each file's
/// root-content scope path with it (#1504 — see
/// [`hir::root_content_scope_path`]).
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "internal API, no need to generalize"
)]
pub fn build_prelude(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    file_paths: &LookupMap<FileId, String>,
    type_mode: context::TypeMode,
) -> LirPrelude {
    let mut normalized: Vec<(FileId, hir::HirFile)> = files
        .iter()
        .map(|(id, hir_file)| {
            let mut h = (*hir_file).clone();
            hir::normalize_file(&mut h);
            (*id, h)
        })
        .collect();
    hir::stamp_container_ids(&mut normalized, index, file_paths);

    let normalized_refs: Vec<(FileId, &hir::HirFile)> =
        normalized.iter().map(|(id, h)| (*id, h)).collect();
    let decls = build_prelude_decls(&normalized_refs, index, resolutions, type_mode);
    assemble_prelude(decls, normalized)
}

/// Lower every file's root-level content into one chunk per file, sharing a
/// single temp/block-slot frame across the whole root scope (files share one
/// call frame — `LowerCtx::next_block_slot`). Returns each `(chunk,
/// lowering-diagnostics)` pair in `files` order plus the total root temp-slot
/// count. This is the root-content half of the old `lower_root`, unchanged.
///
/// The synthesized root terminus ([`attach_root_final_gather`]) is attached to
/// the **last** chunk only — see that function's doc for why that is the one
/// place C# puts it.
#[must_use]
fn lower_root_content_chunks(
    files: &[(FileId, &hir::HirFile)],
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    root_id: brink_format::DefinitionId,
    file_paths: &LookupMap<FileId, String>,
    struct_ctx: &context::StructCtx<'_>,
    tables: context::AnalyzerTables<'_>,
) -> (Vec<(chunk::ScopeChunk, Vec<crate::Diagnostic>)>, u16) {
    let mut chunks = Vec::new();

    // Content-pure allocator (seq resets per chunk; `alloc_address` is a
    // deterministic hash) — a fresh one is byte-identical to the shared one.
    let mut ids = context::IdAllocator::new();
    let _ = ids.alloc_address("");

    let root_blocks: Vec<&hir::Block> = files.iter().map(|(_, hir)| &hir.root_content).collect();
    let temp_map = temps::alloc_temps(&[], &[], &root_blocks);
    let mut block_slot = temp_map.total_slots();

    // Only the last root-content chunk — the tail of the assembled root body —
    // may carry the synthesized terminus (issue #1502). See
    // `attach_root_final_gather`.
    let last_chunk = files.len().saturating_sub(1);

    for (chunk_index, &(file_id, hir_file)) in files.iter().enumerate() {
        let mut local_names = NameTable::new();
        let mut diagnostics = Vec::new();
        // #1504: every path this allocator mints for the chunk below (inline
        // sequence wrappers, the synthesized terminus) restarts per file, so
        // qualify them by the owning file — the same qualifier the stamping
        // pass gave this file's anonymous choice/gather containers. `ctx
        // .scope_path` deliberately stays empty: it also drives author-label
        // lookup (`LowerCtx::qualify_label`), and a root-level label is
        // addressed by its bare name.
        ids.set_path_prefix(hir::root_content_scope_path(
            file_paths.get(&file_id).map(String::as_str),
        ));
        let (stmts, mut block_children) = {
            let mut ctx = make_ctx(
                file_id,
                resolutions,
                index,
                &temp_map,
                &mut local_names,
                &mut ids,
                root_id,
                String::new(),
                true,
                &[],
                file_paths,
                &mut block_slot,
                &mut diagnostics,
                struct_ctx,
                tables,
            );
            let mut cc = 0;
            let mut gc = 0;
            ctx.ids.reset_seq_counter();
            lower_block_with_children(&hir_file.root_content, &mut ctx, &mut cc, &mut gc)
        };
        if chunk_index == last_chunk {
            attach_root_final_gather(&mut block_children, &mut ids);
        }
        chunks.push((
            chunk::ScopeChunk::root_content(stmts, block_children, local_names.into_entries()),
            diagnostics,
        ));
    }

    (chunks, block_slot)
}

/// Lower one knot (its body, stitches, and inline children) into a
/// self-contained [`chunk::ScopeChunk`] against a fresh local name table and
/// a fresh content-pure allocator — the per-`DefinitionId` unit `brink-db`
/// memoizes. Byte-identical to the knot's slice of the old `lower_root`.
#[expect(clippy::too_many_arguments)]
#[must_use]
fn lower_knot_chunk(
    hir_file: &hir::HirFile,
    knot: &hir::Knot,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file_paths: &LookupMap<FileId, String>,
    struct_ctx: &context::StructCtx<'_>,
    root_id: brink_format::DefinitionId,
    file_id: FileId,
    tables: context::AnalyzerTables<'_>,
) -> (chunk::ScopeChunk, Vec<crate::Diagnostic>) {
    let mut local_names = NameTable::new();
    let mut ids = context::IdAllocator::new();
    let _ = ids.alloc_address("");
    let mut diagnostics = Vec::new();
    let knot_container = lower_knot(
        file_id,
        hir_file,
        knot,
        resolutions,
        index,
        &mut local_names,
        &mut ids,
        root_id,
        file_paths,
        &mut diagnostics,
        struct_ctx,
        tables,
    );
    (
        chunk::ScopeChunk::knot(knot_container, local_names.into_entries()),
        diagnostics,
    )
}

/// The part of a knot chunk's lowering environment that is the *same* for
/// every knot in the project: the flattened resolution lookup, the
/// reconstructed throwaway `ShapeTable`/`GlobalShapeMap`, the `FileId`→path
/// map, and the type mode.
///
/// Built once per project revision and shared by every
/// [`lower_knot_chunk_incremental`] call (issue #460 — `brink-db` memoizes it
/// in `chunk_lowering_ctx_query`). Before this existed, each per-knot memo
/// rebuilt all of it from scratch, so a K-knot project paid
/// `K × O(project resolutions + struct shapes + files)` on every cold compile
/// and on every recompile that invalidated the chunk memos — the measured
/// dominant cost of the per-knot LIR layer.
///
/// Contents are byte-identical to what the per-knot build produced: same
/// inputs, same constructors, and the throwaway `NameTable` the shape table
/// is interned into is never read (every name is re-interned into the
/// chunk's own local table), so sharing one instance across knots cannot
/// change a chunk's bytes.
pub struct ChunkLoweringCtx {
    resolutions: ResolutionLookup,
    shapes: structs::ShapeTable,
    global_shapes: structs::GlobalShapeMap,
    file_paths: LookupMap<FileId, String>,
    type_mode: context::TypeMode,
}

impl ChunkLoweringCtx {
    /// Build the shared context from the same cutoff-friendly inputs the
    /// per-knot memo already depends on.
    #[must_use]
    pub fn new(
        resolutions: &ResolutionMap,
        shape_data: &StructShapeData,
        file_paths: LookupMap<FileId, String>,
        type_mode: context::TypeMode,
    ) -> Self {
        let mut throwaway = NameTable::new();
        let shapes = structs::rebuild_shape_table(shape_data, &mut throwaway);
        let global_shapes = structs::rebuild_global_shape_map(shape_data);
        Self {
            resolutions: ResolutionLookup::build(resolutions),
            shapes,
            global_shapes,
            file_paths,
            type_mode,
        }
    }
}

/// Incremental entry point (`brink-db`'s per-knot salsa memo): lower a single
/// knot from cutoff-friendly inputs — the declaring file's already
/// normalized+stamped HIR, the whole-project symbol index, and the
/// project-wide [`ChunkLoweringCtx`] (which the memo reads through its own
/// query, so every knot shares one build of it).
#[must_use]
pub fn lower_knot_chunk_incremental(
    hir_file: &hir::HirFile,
    knot: &hir::Knot,
    index: &SymbolIndex,
    ctx: &ChunkLoweringCtx,
    file_id: FileId,
    tables: context::AnalyzerTables<'_>,
) -> (chunk::ScopeChunk, Vec<crate::Diagnostic>) {
    let struct_ctx = context::StructCtx {
        shapes: &ctx.shapes,
        global_shapes: &ctx.global_shapes,
        type_mode: ctx.type_mode,
    };
    lower_knot_chunk(
        hir_file,
        knot,
        index,
        &ctx.resolutions,
        &ctx.file_paths,
        &struct_ctx,
        context::root_definition_id(),
        file_id,
        tables,
    )
}

/// Lower the whole root scope from an already-built [`LirPrelude`] — the
/// link phase's own root-content step. Uses the prelude's real struct-shape
/// table (not the reconstructed projection), so it is byte-identical to the
/// monolithic composition's root-content lowering. `brink-db`'s link query
/// calls this; the per-knot memos use [`lower_knot_chunk_incremental`]
/// (cutoff-friendly `StructShapeData`) instead.
#[expect(
    clippy::implicit_hasher,
    reason = "internal API called only by brink-db"
)]
#[must_use]
pub fn lower_root_content_for_prelude(
    prelude: &LirPrelude,
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    file_paths: &LookupMap<FileId, String>,
    tables: context::AnalyzerTables<'_>,
) -> (Vec<(chunk::ScopeChunk, Vec<crate::Diagnostic>)>, u16) {
    let resolutions = ResolutionLookup::build(resolutions);
    let struct_ctx = prelude.struct_ctx();
    lower_root_content_chunks(
        &prelude.files(),
        &resolutions,
        index,
        prelude.root_id,
        file_paths,
        &struct_ctx,
        tables,
    )
}

/// Assemble the per-chunk lowering products into a finished [`lir::Program`]
/// — the FG-4 **link phase** (`docs/fine-grained-salsa-proposal.md` §5).
/// Merges every chunk's local name table into the seeded project table in
/// walk order, relocates ids, applies counting flags over the whole tree,
/// and attaches the struct-shape / private-def / alias `StoryData` tables.
/// `chunks` must be in the interleaved walk order (per file: root content
/// then that file's knots), so the assembled name ids are byte-identical to
/// the single shared-table walk.
#[must_use]
pub fn assemble_program(
    prelude: &LirPrelude,
    chunks: Vec<chunk::ScopeChunk>,
    root_temp_slots: u16,
    _index: &SymbolIndex,
) -> lir::Program {
    let mut names = NameTable::from_entries(prelude.name_seed.clone());
    let (mut root_body, root_children) = chunk::assemble_scopes(chunks, &mut names);

    let ends_with_divert = root_body
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::Divert(_)));
    if !ends_with_divert {
        root_body.push(lir::Stmt::Divert(lir::Divert {
            target: lir::DivertTarget::Done,
            args: Vec::new(),
        }));
    }

    let mut root = lir::Container {
        id: prelude.root_id,
        name: None,
        kind: lir::ContainerKind::Root,
        params: Vec::new(),
        body: root_body,
        children: root_children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: root_temp_slots,
        labeled: false,
        inline: false,
        is_function: false,
        local: false,
    };

    apply_counting_flags(&mut root, &prelude.globals);

    let struct_shapes = structs::struct_shape_defs(&prelude.shape_table);

    lir::Program {
        root,
        globals: prelude.globals.clone(),
        lists: prelude.lists.clone(),
        list_items: prelude.list_items.clone(),
        externals: prelude.externals.clone(),
        name_table: names.into_entries(),
        struct_shapes,
        private_defs: prelude.private_defs.clone(),
        aliases: prelude.aliases.clone(),
    }
}

// ─── Tree-building lowering ─────────────────────────────────────────

#[expect(clippy::too_many_arguments)]
fn lower_knot(
    file_id: FileId,
    _hir_file: &hir::HirFile,
    knot: &hir::Knot,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    ids: &mut context::IdAllocator,
    root_id: brink_format::DefinitionId,
    file_paths: &LookupMap<FileId, String>,
    diagnostics: &mut Vec<crate::Diagnostic>,
    structs: &context::StructCtx<'_>,
    tables: context::AnalyzerTables<'_>,
) -> lir::Container {
    let knot_name = &knot.name.text;
    let knot_id = lookup_container_id(index, knot_name).unwrap_or(root_id);

    let mut scope_blocks: Vec<&hir::Block> = vec![&knot.body];
    for stitch in &knot.stitches {
        scope_blocks.push(&stitch.body);
    }

    let temp_map = temps::alloc_temps(&knot.params, &knot.stitches, &scope_blocks);
    let params = lower_params(&knot.params, names, &temp_map);
    // Shared across the knot body + every one of its stitches — they share
    // one call frame, so block-scoped slots must not restart per stitch
    // (see `LowerCtx::next_block_slot` doc).
    let mut block_slot = temp_map.total_slots();

    let knot_param_names: Vec<&str> = knot.params.iter().map(|p| p.name.text.as_str()).collect();
    let mut ctx = make_ctx(
        file_id,
        resolutions,
        index,
        &temp_map,
        names,
        ids,
        root_id,
        knot_name.clone(),
        false,
        &knot_param_names,
        file_paths,
        &mut block_slot,
        diagnostics,
        structs,
        tables,
    );
    let mut cc = 0;
    let mut gc = 0;
    ctx.ids.reset_seq_counter();
    let (body, mut children) = lower_block_with_children(&knot.body, &mut ctx, &mut cc, &mut gc);

    // Add stitches as children
    for stitch in &knot.stitches {
        children.push(lower_stitch(
            file_id,
            knot,
            stitch,
            &temp_map,
            resolutions,
            index,
            names,
            ids,
            root_id,
            file_paths,
            &mut block_slot,
            diagnostics,
            structs,
            tables,
        ));
    }

    // First-stitch auto-enter: if knot body is empty, divert to first stitch
    let mut final_body = body;
    if final_body.is_empty()
        && !knot.stitches.is_empty()
        && let Some(first_stitch) = children
            .iter()
            .find(|c| c.kind == lir::ContainerKind::Stitch)
    {
        final_body.push(lir::Stmt::Divert(lir::Divert {
            target: lir::DivertTarget::Address(first_stitch.id),
            args: Vec::new(),
        }));
    }

    lir::Container {
        id: knot_id,
        name: Some(knot_name.clone()),
        kind: lir::ContainerKind::Knot,
        params,
        body: final_body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: block_slot,
        labeled: false,
        inline: false,
        is_function: knot.is_function,
        local: knot.is_local,
    }
}

#[expect(clippy::too_many_arguments)]
fn lower_stitch(
    file_id: FileId,
    knot: &hir::Knot,
    stitch: &hir::Stitch,
    temp_map: &TempMap,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    names: &mut NameTable,
    ids: &mut context::IdAllocator,
    root_id: brink_format::DefinitionId,
    file_paths: &LookupMap<FileId, String>,
    block_slot: &mut u16,
    diagnostics: &mut Vec<crate::Diagnostic>,
    structs: &context::StructCtx<'_>,
    tables: context::AnalyzerTables<'_>,
) -> lir::Container {
    let stitch_name = &stitch.name.text;
    let stitch_path = format!("{}.{stitch_name}", knot.name.text);
    let stitch_id = lookup_container_id(index, &stitch_path).unwrap_or(root_id);
    let params = lower_params(&stitch.params, names, temp_map);

    let stitch_param_names: Vec<&str> =
        stitch.params.iter().map(|p| p.name.text.as_str()).collect();
    let mut ctx = make_ctx(
        file_id,
        resolutions,
        index,
        temp_map,
        names,
        ids,
        root_id,
        stitch_path,
        false,
        &stitch_param_names,
        file_paths,
        block_slot,
        diagnostics,
        structs,
        tables,
    );
    let mut cc = 0;
    let mut gc = 0;
    ctx.ids.reset_seq_counter();
    let (body, children) = lower_block_with_children(&stitch.body, &mut ctx, &mut cc, &mut gc);

    lir::Container {
        id: stitch_id,
        name: Some(stitch_name.clone()),
        kind: lir::ContainerKind::Stitch,
        params,
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
        local: stitch.is_local,
    }
}

/// Lower a block, returning both statements and any child containers
/// (choice targets, gathers) produced by choice sets within the block.
///
/// When a `ChoiceSet` with a gather is encountered, remaining statements
/// go into the gather's body (not the current block).
#[expect(clippy::too_many_lines)]
fn lower_block_with_children(
    block: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> (Vec<lir::Stmt>, Vec<lir::Container>) {
    let mut stmts = Vec::new();
    let mut children = Vec::new();
    let mut pos = 0;

    while pos < block.stmts.len() {
        let stmt = &block.stmts[pos];
        match stmt {
            hir::Stmt::ChoiceSet(cs) => {
                // Every choice set gets a gather target — read from stamped HIR.
                let gather_target = cs.gather_id;
                *gather_counter += 1;

                // Build choice target children
                let mut choice_children = Vec::new();
                let choices: Vec<lir::Choice> = cs
                    .choices
                    .iter()
                    .map(|choice| {
                        let (lir_choice, child) =
                            lower_choice_with_child(choice, ctx, choice_counter, gather_target);
                        if let Some(c) = child {
                            choice_children.push(c);
                        }
                        lir_choice
                    })
                    .collect();

                stmts.push(lir::Stmt::ChoiceSet(lir::ChoiceSet {
                    choices,
                    gather_target,
                }));
                children.append(&mut choice_children);

                // Build gather container from the continuation block.
                // The HIR nests all post-gather content into the continuation,
                // so no trailing-stmt consumption is needed.
                let gather_container = build_continuation_container(
                    &cs.continuation,
                    ctx,
                    gather_target,
                    *gather_counter - 1,
                    choice_counter,
                    gather_counter,
                );
                children.push(gather_container);
                pos += 1;
            }
            hir::Stmt::LabeledBlock(labeled) => {
                // Labeled block wrapping content (standalone gather or opening
                // gather pattern). Enter the wrapper container so execution
                // returns to the parent when the child finishes — this allows
                // sibling LabeledBlocks to chain (e.g. `- (opts) ... - (test)`).
                let wrapper_id = labeled.container_id.unwrap_or(ctx.root_id);
                *gather_counter += 1;

                stmts.push(lir::Stmt::EnterContainer(wrapper_id));

                let display_name = labeled
                    .label
                    .as_ref()
                    .map_or_else(|| format!("g-{}", *gather_counter - 1), |l| l.text.clone());

                let labeled_flag = labeled
                    .label
                    .as_ref()
                    .is_some_and(|label| ctx.lookup_address_id(&label.text).is_some());

                // Lower the labeled block's contents
                let (mut inner_stmts, inner_children) =
                    lower_block_with_children(labeled, ctx, choice_counter, gather_counter);

                // If inside a choice body, append goto gather so the
                // container is self-sufficient when entered via divert.
                if let Some(gather_id) = ctx.choice_gather_target {
                    let ends_terminal = inner_stmts.last().is_some_and(|s| {
                        matches!(
                            s,
                            lir::Stmt::Divert(d) if matches!(
                                d.target,
                                lir::DivertTarget::Done
                                    | lir::DivertTarget::End
                                    | lir::DivertTarget::Address(_)
                            )
                        ) || matches!(s, lir::Stmt::ChoiceSet(_))
                    });
                    if !ends_terminal {
                        inner_stmts.push(lir::Stmt::Divert(lir::Divert {
                            target: lir::DivertTarget::Address(gather_id),
                            args: Vec::new(),
                        }));
                    }
                }

                children.push(lir::Container {
                    id: wrapper_id,
                    name: Some(display_name),
                    kind: lir::ContainerKind::Gather,
                    params: Vec::new(),
                    body: inner_stmts,
                    children: inner_children,
                    counting_flags: CountingFlags::empty(),
                    temp_slot_count: 0,
                    labeled: labeled_flag,
                    inline: true,
                    is_function: false,
                    local: false,
                });
                pos += 1;
            }
            hir::Stmt::Conditional(cond) => {
                // Lower conditional branches with lower_block_with_children
                // so ChoiceSets inside branches produce child containers.
                // Each branch body is wrapped in its own child container.
                //
                // The `in_conditional_branch` flag in codegen suppresses `Done`
                // inside branch containers. This is correct because ink
                // conditionals can gate choice visibility — choices across all
                // branches form a single logical ChoiceSet, and the runtime
                // auto-presents pending choices on frame/container exhaustion
                // (vm.rs handle_frame_exhaustion), so no explicit `Done` is needed.
                let kind = match &cond.kind {
                    hir::CondKind::InitialCondition => lir::CondKind::InitialCondition,
                    hir::CondKind::IfElse => lir::CondKind::IfElse,
                    hir::CondKind::Switch(expr) => {
                        lir::CondKind::Switch(expr::lower_expr(expr, ctx))
                    }
                };

                let cond_idx = ctx.ids.next_seq_index();

                // Push a scope prefix for this conditional so nested
                // conditionals inside branches get unique container paths.
                let cond_scope = format!("b-{cond_idx}");
                let old_scope = ctx.scope_path.clone();

                let branches = cond
                    .branches
                    .iter()
                    .enumerate()
                    .map(|(branch_idx, b)| {
                        // B1b (issue #1475): the block-level `{if EXPR as
                        // n: … else: …}` template form. The branch body
                        // becomes its own container, but containers share
                        // the enclosing call frame's temp slots, so the
                        // binding's slot is visible inside it — the scope
                        // bracket below is the lowering-time name scope,
                        // and it closes before the next branch is walked.
                        ctx.push_block_scope();
                        let condition = match (b.condition.as_ref(), b.binding.as_ref()) {
                            (Some(e), Some(binding)) => {
                                Some(blocks::lower_bound_condition(e, binding, ctx))
                            }
                            (Some(e), None) => Some(expr::lower_expr(e, ctx)),
                            (None, _) => None,
                        };

                        // Set scope_path for this branch so nested containers
                        // (choices, gathers, nested conditionals) get unique IDs.
                        let branch_scope = if old_scope.is_empty() {
                            format!("{cond_scope}.{branch_idx}")
                        } else {
                            format!("{old_scope}.{cond_scope}.{branch_idx}")
                        };
                        ctx.scope_path = branch_scope;

                        // Pass through parent choice/gather counters — a ChoiceSet
                        // inside a conditional shares the enclosing scope and must
                        // not collide with sibling gathers/choices.
                        let (body, branch_children) =
                            lower_block_with_children(&b.body, ctx, choice_counter, gather_counter);

                        // Read pre-stamped container ID from HIR.
                        let branch_id = b.container_id.unwrap_or(ctx.root_id);

                        let branch_container = lir::Container {
                            id: branch_id,
                            name: Some(format!("{branch_idx}")),
                            kind: lir::ContainerKind::ConditionalBranch,
                            params: Vec::new(),
                            body,
                            children: branch_children,
                            counting_flags: CountingFlags::empty(),
                            temp_slot_count: 0,
                            labeled: false,
                            inline: false,
                            is_function: false,
                            local: false,
                        };
                        children.push(branch_container);
                        // Closes the `as`-binding scope opened above — the
                        // next branch (an `else`) must not see the name.
                        ctx.pop_block_scope();

                        // The branch body in the Conditional struct is just EnterContainer
                        lir::CondBranch {
                            condition,
                            body: vec![lir::Stmt::EnterContainer(branch_id)],
                        }
                    })
                    .collect();

                // Restore scope_path after processing branches.
                ctx.scope_path = old_scope;

                stmts.push(lir::Stmt::Conditional(lir::Conditional { kind, branches }));
                pos += 1;
            }
            hir::Stmt::Sequence(seq) => {
                // Read pre-stamped wrapper container ID; keep counter in sync.
                let seq_idx = ctx.ids.next_seq_index();
                let wrapper_id = seq.container_id.unwrap_or(ctx.root_id);

                // Push the wrapper's name onto the scope path so that nested
                // sequences inside branches get unique IDs (e.g. `scope.s-0.s-0`
                // instead of colliding with the parent's `scope.s-0`).
                let display_name = format!("s-{seq_idx}");
                let old_scope = ctx.scope_path.clone();
                ctx.scope_path = if old_scope.is_empty() {
                    display_name.clone()
                } else {
                    format!("{old_scope}.{display_name}")
                };

                // Lower each sequence branch into its own child container.
                // The wrapper's Sequence.branches hold [EnterContainer(branch_id)]
                // for each branch, and the actual branch content lives in child
                // containers.
                let mut wrapper_children = Vec::new();
                let branches: Vec<Vec<lir::Stmt>> = seq
                    .branches
                    .iter()
                    .enumerate()
                    .map(|(branch_idx, b)| {
                        let mut bc = 0;
                        let mut gc = 0;
                        let (body, branch_children) =
                            lower_block_with_children(&b.body, ctx, &mut bc, &mut gc);

                        // Read pre-stamped container ID from HIR branch block.
                        let branch_id = b.body.container_id.unwrap_or(ctx.root_id);

                        let branch_container = lir::Container {
                            id: branch_id,
                            name: Some(format!("{branch_idx}")),
                            kind: lir::ContainerKind::SequenceBranch,
                            params: Vec::new(),
                            body,
                            children: branch_children,
                            counting_flags: CountingFlags::empty(),
                            temp_slot_count: 0,
                            labeled: false,
                            inline: false,
                            is_function: false,
                            local: false,
                        };
                        wrapper_children.push(branch_container);

                        // The branch body in the Sequence struct is just EnterContainer
                        vec![lir::Stmt::EnterContainer(branch_id)]
                    })
                    .collect();

                ctx.scope_path = old_scope;
                let wrapper = lir::Container {
                    id: wrapper_id,
                    name: Some(display_name),
                    kind: lir::ContainerKind::Sequence,
                    params: Vec::new(),
                    body: vec![lir::Stmt::Sequence(lir::Sequence {
                        kind: seq.kind,
                        branches,
                    })],
                    children: wrapper_children,
                    counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
                    temp_slot_count: 0,
                    labeled: false,
                    inline: false,
                    is_function: false,
                    local: false,
                };
                children.push(wrapper);

                stmts.push(lir::Stmt::EnterContainer(wrapper_id));
                pos += 1;
            }
            hir::Stmt::Content(content) => {
                // Try direct recognition first.
                if let Some(emission) = recognize::try_recognize(content, ctx) {
                    stmts.push(lir::Stmt::EmitLine(emission));
                }
                // Try with boundary glue stripping.
                else if let Some((leading, emission, trailing)) =
                    recognize::try_recognize_with_glue(content, ctx)
                {
                    if leading {
                        stmts.push(lir::Stmt::EmitContent(lir::Content {
                            parts: vec![lir::ContentPart::Glue],
                            tags: vec![],
                        }));
                    }
                    stmts.push(lir::Stmt::EmitLine(emission));
                    if trailing {
                        stmts.push(lir::Stmt::EmitContent(lir::Content {
                            parts: vec![lir::ContentPart::Glue],
                            tags: vec![],
                        }));
                    }
                }
                // Fallback: emit content parts individually.
                else {
                    stmts.push(lir::Stmt::EmitContent(content::lower_content(content, ctx)));
                }
                children.append(&mut ctx.pending_children);
                pos += 1;
            }
            hir::Stmt::LogicBlock(lb) => {
                // T1b `~ { … }` block (docs/t1b-surface-spec.md §2) — pure
                // logic, spliced directly into the enclosing container's
                // flat statement sequence (never a child container: block
                // bodies never contain weave concepts, so there's nothing
                // that needs container isolation).
                stmts.extend(blocks::lower_logic_block(&lb.stmts, ctx));
                pos += 1;
            }

            // A classic (non-block) `~ p.field = expr` logic line (TM-4c,
            // docs/typed-mode-spec.md §6) — same single-level RMW
            // desugaring `~ { … }` block statements use, splicing
            // possibly-multiple `lir::Stmt`s here since `stmts::lower_stmt`'s
            // `Option<Stmt>` return can't express that. Falls through to the
            // ordinary `stmts::lower_stmt` path (the `_` arm below) for
            // every other assignment (plain variable, indexed).
            hir::Stmt::Assignment(assign)
                if blocks::try_lower_field_assignment(assign, ctx, &mut stmts) =>
            {
                children.append(&mut ctx.pending_children);
                pos += 1;
            }

            // A classic (non-block) `~ push(a, v)` logic line — same
            // mutator recognition/RMW desugaring `~ { … }` block statements
            // use (docs/t1b-surface-spec.md §5), splicing possibly-multiple
            // `lir::Stmt`s here since `stmts::lower_stmt`'s `Option<Stmt>`
            // return can't express that. Falls through to the ordinary
            // `stmts::lower_stmt` path (the `_` arm below) for every other
            // expression statement, including a shadowed `push`/`insert`/
            // `remove` user function.
            hir::Stmt::ExprStmt(expr) if blocks::try_lower_mutator_stmt(expr, ctx, &mut stmts) => {
                children.append(&mut ctx.pending_children);
                pos += 1;
            }

            _ => {
                if let Some(s) = stmts::lower_stmt(stmt, ctx) {
                    stmts.push(s);
                }
                // Drain any inline sequence containers created during content lowering.
                children.append(&mut ctx.pending_children);
                pos += 1;
            }
        }
    }

    (stmts, children)
}

/// Build a gather container from a `ChoiceSet`'s continuation block.
///
/// The continuation's label becomes the container name, its stmts become
/// the body (lowered via `lower_block_with_children` to handle nested
/// `ChoiceSet`s in gather-choice chains).
fn build_continuation_container(
    continuation: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    gather_id: Option<brink_format::DefinitionId>,
    gather_index: usize,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> lir::Container {
    let id = gather_id.unwrap_or(ctx.root_id);
    let display_name = continuation
        .label
        .as_ref()
        .map_or_else(|| format!("g-{gather_index}"), |l| l.text.clone());

    // Check if the gather has a source-level label that resolves.
    let labeled = continuation
        .label
        .as_ref()
        .is_some_and(|label| ctx.lookup_address_id(&label.text).is_some());

    if continuation.stmts.is_empty() && continuation.label.is_none() {
        // Empty continuation with no label — the choice set is the last
        // thing in its enclosing block. At the story's root content this
        // is a safe implicit end (real ink lets root content run out), so
        // emit the same `-> DONE` a genuine `-> DONE` statement would
        // produce. Inside a knot/stitch, though, running off the end
        // without an explicit `-> DONE`/`-> END` is a real ink runtime
        // error ("ran out of content") — leaving the body empty here lets
        // the VM's normal frame-exhaustion path (`handle_frame_exhaustion`)
        // surface that instead of masking it as a safe exit (issue #1503).
        let body = if ctx.is_root_content_scope {
            vec![lir::Stmt::Divert(lir::Divert {
                target: lir::DivertTarget::Done,
                args: Vec::new(),
            })]
        } else {
            Vec::new()
        };
        return lir::Container {
            id,
            name: Some(display_name),
            kind: lir::ContainerKind::Gather,
            params: Vec::new(),
            body,
            children: Vec::new(),
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
            labeled: false,
            inline: false,
            is_function: false,
            local: false,
        };
    }

    // Lower continuation stmts — may contain nested ChoiceSets (gather-choice chains)
    let (body, children) =
        lower_block_with_children(continuation, ctx, choice_counter, gather_counter);

    lir::Container {
        id,
        name: Some(display_name),
        kind: lir::ContainerKind::Gather,
        params: Vec::new(),
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        labeled,
        inline: false,
        is_function: false,
        local: false,
    }
}

#[expect(clippy::too_many_lines, reason = "choice lowering has many parts")]
fn lower_choice_with_child(
    choice: &hir::Choice,
    ctx: &mut LowerCtx<'_>,
    choice_counter: &mut usize,
    gather_target: Option<brink_format::DefinitionId>,
) -> (lir::Choice, Option<lir::Container>) {
    *choice_counter += 1;

    let target = choice.container_id.unwrap_or(ctx.root_id);

    // Preserve the three-part content split for codegen backends.
    let start_content = choice
        .start_content
        .as_ref()
        .map(|c| content::lower_content(c, ctx));
    let choice_only_content = choice
        .bracket_content
        .as_ref()
        .map(|c| content::lower_content(c, ctx));
    let inner_content = choice
        .inner_content
        .as_ref()
        .map(|c| content::lower_content(c, ctx));

    // ── Compose and recognize display/output content at HIR level ──
    // Display = start + bracket, Output = start + inner.
    let display_hir = recognize::compose_hir_content_opt(
        choice.start_content.as_ref(),
        choice.bracket_content.as_ref(),
    );
    let output_hir = recognize::compose_hir_content_opt(
        choice.start_content.as_ref(),
        choice.inner_content.as_ref(),
    );

    // Skip recognition when composed content starts with whitespace-only
    // text — the inline emission path's `push_text` suppresses leading whitespace
    // that `EvalLine`/`EmitLine` would preserve, changing observable behavior.
    let display_ws = display_hir
        .as_ref()
        .is_some_and(recognize::starts_with_whitespace_only_text);
    let output_ws = output_hir
        .as_ref()
        .is_some_and(recognize::starts_with_whitespace_only_text);

    let display_emission = if display_ws {
        None
    } else {
        display_hir
            .as_ref()
            .and_then(|c| recognize::try_recognize(c, ctx))
    };
    let output_emission = if output_ws {
        None
    } else {
        output_hir
            .as_ref()
            .and_then(|c| recognize::try_recognize(c, ctx))
    };

    let condition = choice.condition.as_ref().map(|e| expr::lower_expr(e, ctx));
    let tags: Vec<Vec<lir::ContentPart>> = choice
        .tags
        .iter()
        .map(|t| content::lower_content_parts_pub(&t.parts, ctx))
        .collect();

    // Lower choice body into a child container.
    // Update scope_path to match the planner's convention so nested
    // choice/gather keys resolve to the correct container IDs.
    // Set choice_gather_target so labeled containers within the body
    // can include an explicit goto to the gather.
    let old_scope = ctx.scope_path.clone();
    let old_gather_target = ctx.choice_gather_target;
    ctx.scope_path = format!("{}.c{}", old_scope, *choice_counter - 1);
    ctx.choice_gather_target = gather_target;
    let mut cc = 0;
    let mut gc = 0;
    let (body_stmts, mut children) = lower_block_with_children(&choice.body, ctx, &mut cc, &mut gc);
    ctx.scope_path = old_scope;
    ctx.choice_gather_target = old_gather_target;

    // Build the choice target container body. The output after selecting
    // a choice is: ChoiceOutput(content) + body stmts.
    // The HIR body already contains the inline divert and EndOfLine as
    // its first statements, so they flow naturally into the LIR body.
    let mut body: Vec<lir::Stmt> = Vec::new();

    // 1. Choice output preamble: start+inner content with their tags.
    // Tags on start/inner content appear in the output after choosing;
    // bracket-only tags are suppressed (they only affect choice display).
    {
        let mut output_parts = Vec::new();
        let mut output_tags = Vec::new();
        if let Some(ref sc) = start_content {
            output_parts.extend(sc.parts.clone());
            output_tags.extend(sc.tags.clone());
        }
        if let Some(ref ic) = inner_content {
            output_parts.extend(ic.parts.clone());
            output_tags.extend(ic.tags.clone());
        }
        if !output_parts.is_empty() || !output_tags.is_empty() {
            body.push(lir::Stmt::ChoiceOutput {
                content: lir::Content {
                    parts: output_parts,
                    tags: output_tags,
                },
                emission: output_emission.clone(),
            });
        }
    }

    // 2. Body statements from the choice's block (includes inline divert + EndOfLine)
    body.extend(body_stmts);

    // 5. Auto-gather divert when the body doesn't end with Done/End.
    let ends_with_terminal = body.last().is_some_and(|s| {
        matches!(
            s,
            lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Done | lir::DivertTarget::End)
        )
    });
    if !ends_with_terminal && let Some(gather_id) = gather_target {
        let body_ends_with_choice_set = body
            .last()
            .is_some_and(|s| matches!(s, lir::Stmt::ChoiceSet(_)));

        let divert = lir::Divert {
            target: lir::DivertTarget::Address(gather_id),
            args: Vec::new(),
        };

        if body_ends_with_choice_set {
            // The body ends with a ChoiceSet → `done` stops execution,
            // so a divert appended to the body would be dead code.
            // Instead, patch the innermost gather container so that
            // after the inner gather's content, execution flows to the
            // outer gather. This recurses through nested choice-set-
            // in-gather chains (multi-level weaves).
            patch_innermost_gather(&mut children, divert);
        } else {
            body.push(lir::Stmt::Divert(divert));
        }
    }

    // Check if the choice has a source-level label that resolves.
    let labeled = choice
        .label
        .as_ref()
        .is_some_and(|label| ctx.lookup_address_id(&label.text).is_some());

    let child_name = format!("c-{}", *choice_counter - 1);
    let child = lir::Container {
        id: target,
        name: Some(child_name),
        kind: lir::ContainerKind::ChoiceTarget,
        params: Vec::new(),
        body,
        children,
        counting_flags: if choice.is_sticky {
            CountingFlags::empty()
        } else {
            CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY
        },
        temp_slot_count: 0,
        labeled,
        inline: false,
        is_function: false,
        local: false,
    };

    let lir_choice = lir::Choice {
        is_sticky: choice.is_sticky,
        is_fallback: choice.is_fallback,
        condition,
        start_content,
        choice_only_content,
        inner_content,
        display_emission,
        output_emission,
        target,
        tags,
    };

    (lir_choice, Some(child))
}

// `lower_gather_choice_chain` and `build_gather_container` removed in Phase 2.
// Gather-choice chains are now handled via nested continuation blocks in the
// HIR, lowered naturally by `lower_block_with_children` + `build_continuation_container`.

// ─── Helpers ────────────────────────────────────────────────────────

#[expect(clippy::too_many_arguments)]
fn make_ctx<'a>(
    file: FileId,
    resolutions: &'a ResolutionLookup,
    index: &'a SymbolIndex,
    temps: &'a TempMap,
    names: &'a mut NameTable,
    ids: &'a mut context::IdAllocator,
    root_id: brink_format::DefinitionId,
    scope_path: String,
    is_root_content_scope: bool,
    param_names: &[&str],
    file_paths: &'a LookupMap<FileId, String>,
    next_block_slot: &'a mut u16,
    diagnostics: &'a mut Vec<crate::Diagnostic>,
    structs: &'a context::StructCtx<'a>,
    tables: context::AnalyzerTables<'a>,
) -> LowerCtx<'a> {
    LowerCtx {
        file,
        resolutions,
        index,
        temps,
        names,
        ids,
        scope_path,
        is_root_content_scope,
        pending_children: Vec::new(),
        visible_temps: param_names.iter().map(|s| (*s).to_string()).collect(),
        file_paths,
        root_id,
        choice_gather_target: None,
        next_block_slot,
        block_scopes: Vec::new(),
        as_binding_slots: LookupSet::new(),
        block_scoped_temp_names: LookupSet::new(),
        diagnostics,
        loop_depth: 0,
        structs,
        temp_shapes: LookupMap::new(),
        tables,
    }
}

fn lower_params(
    params: &[hir::Param],
    names: &mut NameTable,
    temp_map: &TempMap,
) -> Vec<lir::Param> {
    params
        .iter()
        .map(|p| {
            let name = names.intern(&p.name.text);
            let slot = temp_map.get(&p.name.text).unwrap_or(0);
            lir::Param {
                name,
                slot,
                is_ref: p.is_ref,
                is_divert: p.is_divert,
            }
        })
        .collect()
}

/// Look up a container `DefinitionId` by name in the symbol index.
///
/// Checks for knot, stitch, or label symbols — the same container
/// types the analyzer registers.
fn lookup_container_id(index: &SymbolIndex, name: &str) -> Option<brink_format::DefinitionId> {
    use crate::symbols::SymbolKind;
    index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .find(|&&id| {
                index.symbols.get(&id).is_some_and(|info| {
                    matches!(
                        info.kind,
                        SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
                    )
                })
            })
            .copied()
    })
}

// ─── Counting flags ─────────────────────────────────────────────────

fn apply_counting_flags(root: &mut lir::Container, globals: &[lir::GlobalDef]) {
    let mut visit_ids = Vec::new();
    let mut turns_ids = Vec::new();

    // Collect phase: walk entire tree for explicit visit/turn refs
    collect_counting_refs_tree(root, &mut visit_ids, &mut turns_ids);

    // Also scan global variable defaults for DivertTarget values
    // (e.g. `VAR x = -> knot` — the target could be reached via variable divert)
    for g in globals {
        if let lir::ConstValue::DivertTarget(id) = &g.default {
            visit_ids.push(*id);
            turns_ids.push(*id);
        }
    }

    // Apply phase: walk entire tree
    apply_counting_flags_tree(root, &visit_ids, &turns_ids, false);
}

fn collect_counting_refs_tree(
    container: &lir::Container,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    collect_counting_refs(&container.body, visit_ids, turns_ids);
    for child in &container.children {
        collect_counting_refs_tree(child, visit_ids, turns_ids);
    }
}

fn apply_counting_flags_tree(
    container: &mut lir::Container,
    visit_ids: &[brink_format::DefinitionId],
    turns_ids: &[brink_format::DefinitionId],
    in_local_scope: bool,
) {
    // `#@local` on a knot/stitch declares that its counts are per-flow
    // memory (#496): force VISITS on the marked container and on every
    // scope-owning container in its definition subtree (a marked knot
    // covers its stitches — the runtime privatizes the whole subtree at
    // policy resolution), regardless of the read-site analysis below.
    // Interior containers are untouched: sequences already carry VISITS
    // intrinsically (branch selection needs the counter), and unread
    // labels stay compiled out exactly as in unmarked scopes.
    let in_local_scope = in_local_scope || container.local;
    if in_local_scope
        && matches!(
            container.kind,
            lir::ContainerKind::Knot | lir::ContainerKind::Stitch
        )
    {
        container.counting_flags |= CountingFlags::VISITS;
    }

    if visit_ids.contains(&container.id) {
        container.counting_flags |= CountingFlags::VISITS;
        // Labeled containers (gathers with labels like `- (loop)`) need
        // COUNT_START_ONLY so that self-goto loops correctly increment
        // the visit count in the runtime's goto_target handler.
        if container.labeled {
            container.counting_flags |= CountingFlags::COUNT_START_ONLY;
        }
    }
    if turns_ids.contains(&container.id) {
        container.counting_flags |= CountingFlags::TURNS;
    }
    for child in &mut container.children {
        apply_counting_flags_tree(child, visit_ids, turns_ids, in_local_scope);
    }
}

fn collect_counting_refs(
    stmts: &[lir::Stmt],
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    for stmt in stmts {
        match stmt {
            lir::Stmt::EmitContent(content) | lir::Stmt::ChoiceOutput { content, .. } => {
                collect_counting_refs_content(content, visit_ids, turns_ids);
            }
            lir::Stmt::EmitLine(emission) | lir::Stmt::EvalLine(emission) => {
                // Template slot expressions may contain counting refs.
                if let lir::RecognizedLine::Template { slot_exprs, .. } = &emission.line {
                    for e in slot_exprs {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                }
                // Tags may contain dynamic expressions — traverse them.
                for tag in &emission.tags {
                    for part in tag {
                        if let lir::ContentPart::Interpolation(e) = part {
                            collect_counting_refs_expr(e, visit_ids, turns_ids);
                        }
                    }
                }
            }
            lir::Stmt::Assign { value: e, .. }
            | lir::Stmt::DeclareTemp { value: Some(e), .. }
            | lir::Stmt::Return { value: Some(e), .. }
            | lir::Stmt::ExprStmt(e) => {
                collect_counting_refs_expr(e, visit_ids, turns_ids);
            }
            lir::Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    if let Some(ref cond) = choice.condition {
                        collect_counting_refs_expr(cond, visit_ids, turns_ids);
                    }
                    if let Some(ref c) = choice.start_content {
                        collect_counting_refs_content(c, visit_ids, turns_ids);
                    }
                    if let Some(ref c) = choice.choice_only_content {
                        collect_counting_refs_content(c, visit_ids, turns_ids);
                    }
                    if let Some(ref c) = choice.inner_content {
                        collect_counting_refs_content(c, visit_ids, turns_ids);
                    }
                    // Traverse recognized emissions for counting refs in slot exprs.
                    for emission in choice
                        .display_emission
                        .iter()
                        .chain(choice.output_emission.iter())
                    {
                        if let lir::RecognizedLine::Template { slot_exprs, .. } = &emission.line {
                            for e in slot_exprs {
                                collect_counting_refs_expr(e, visit_ids, turns_ids);
                            }
                        }
                    }
                }
            }
            lir::Stmt::Conditional(cond) => {
                for branch in &cond.branches {
                    if let Some(ref e) = branch.condition {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                    collect_counting_refs(&branch.body, visit_ids, turns_ids);
                }
            }
            lir::Stmt::Sequence(seq) => {
                for branch in &seq.branches {
                    collect_counting_refs(branch, visit_ids, turns_ids);
                }
            }
            lir::Stmt::Divert(d) => {
                for arg in &d.args {
                    collect_counting_refs_call_arg(arg, visit_ids, turns_ids);
                }
            }
            lir::Stmt::TunnelCall(tc) => {
                for t in &tc.targets {
                    for arg in &t.args {
                        collect_counting_refs_call_arg(arg, visit_ids, turns_ids);
                    }
                }
            }
            lir::Stmt::ThreadStart(ts) => {
                for arg in &ts.args {
                    collect_counting_refs_call_arg(arg, visit_ids, turns_ids);
                }
            }
            // EnterContainer, DeclareTemp(None), Return(None), etc.
            _ => {}
        }
    }
}

fn collect_counting_refs_content(
    content: &lir::Content,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    for part in &content.parts {
        match part {
            lir::ContentPart::Interpolation(e) => {
                collect_counting_refs_expr(e, visit_ids, turns_ids);
            }
            lir::ContentPart::InlineConditional(cond) => {
                for branch in &cond.branches {
                    if let Some(ref e) = branch.condition {
                        collect_counting_refs_expr(e, visit_ids, turns_ids);
                    }
                    collect_counting_refs(&branch.body, visit_ids, turns_ids);
                }
            }
            lir::ContentPart::InlineSequence(seq) => {
                for branch in &seq.branches {
                    collect_counting_refs(branch, visit_ids, turns_ids);
                }
            }
            // Text, Glue, EnterSequence
            _ => {}
        }
    }
}

/// A `TURNS_SINCE`/`READ_COUNT` reference inside a call argument — a plain
/// `Value` arg's expression, or (T1e) a `RefProjection`'s segment
/// expressions (`ref arr[READ_COUNT(-> x)]` is a legal, if unusual, snapshot
/// segment). `RefGlobal`/`RefTemp` carry no expression to scan.
fn collect_counting_refs_call_arg(
    arg: &lir::CallArg,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    match arg {
        lir::CallArg::Value(e) => collect_counting_refs_expr(e, visit_ids, turns_ids),
        lir::CallArg::RefProjection { segments, .. } => {
            for seg in segments {
                collect_counting_refs_expr(seg, visit_ids, turns_ids);
            }
        }
        lir::CallArg::RefGlobal(_) | lir::CallArg::RefTemp(_, _) => {}
    }
}

fn collect_counting_refs_expr(
    expr: &lir::Expr,
    visit_ids: &mut Vec<brink_format::DefinitionId>,
    turns_ids: &mut Vec<brink_format::DefinitionId>,
) {
    match expr {
        lir::Expr::VisitCount(id) => visit_ids.push(*id),
        lir::Expr::DivertTarget(id) => {
            // Any container whose address is taken could be reached via
            // variable divert/tunnel — conservatively mark for visit tracking.
            visit_ids.push(*id);
            turns_ids.push(*id);
        }
        lir::Expr::CallBuiltin {
            builtin: lir::BuiltinFn::TurnsSince,
            args,
        } => {
            for a in args {
                if let lir::Expr::DivertTarget(id) = a {
                    turns_ids.push(*id);
                }
                collect_counting_refs_expr(a, visit_ids, turns_ids);
            }
        }
        lir::Expr::Prefix(_, inner) | lir::Expr::Postfix(inner, _) => {
            collect_counting_refs_expr(inner, visit_ids, turns_ids);
        }
        // B1 `or`-coalescing (#1471) is a dedicated variant, not generic
        // `Infix` — but the walk is identical (both operands, `shape`
        // carries no reference), so it rides the same arm rather than a
        // duplicate one, matching `chunk::remap_expr`'s precedent.
        lir::Expr::Infix(lhs, _, rhs) | lir::Expr::Coalesce { lhs, rhs, shape: _ } => {
            collect_counting_refs_expr(lhs, visit_ids, turns_ids);
            collect_counting_refs_expr(rhs, visit_ids, turns_ids);
        }
        lir::Expr::Call { args, .. } | lir::Expr::CallExternal { args, .. } => {
            for arg in args {
                collect_counting_refs_call_arg(arg, visit_ids, turns_ids);
            }
        }
        lir::Expr::CallBuiltin { args, .. } => {
            for a in args {
                collect_counting_refs_expr(a, visit_ids, turns_ids);
            }
        }
        lir::Expr::String(s) => {
            for p in &s.parts {
                if let lir::StringPart::Interpolation(e) = p {
                    collect_counting_refs_expr(e, visit_ids, turns_ids);
                }
            }
        }
        _ => {}
    }
}

/// Display name of the synthesized root terminus container. `-` is not a
/// legal character in an ink label and the auto-gather convention is the
/// numeric `g-{index}`, so this segment can never collide with an authored
/// or auto-generated gather name.
const ROOT_TERMINUS_NAME: &str = "g-final";

/// Mirror inklecate's **implicit final gather** at the end of the root weave
/// (`FlowBase.SplitWeaveAndSubFlowContent`, `FlowBase.cs:69-72`, which appends
/// `Gather(null, 1)` + `-> DONE` when lowering the root story): a branch that
/// simply runs out of root-weave content ends the flow cleanly instead of
/// faulting with `RanOutOfContent`.
///
/// The root container's own trailing `Divert(Done)`
/// ([`assemble_program`]) cannot serve this purpose: a gather is reached by
/// `goto`, which clears the container stack, so once execution lands in a
/// gather container the root body is no longer on the frame and its `Done`
/// can never run.
///
/// **Root scope only** (#1448). A knot, stitch, tunnel or function whose
/// content runs out is a genuine authoring error that C# ink reports and
/// brink must keep reporting — extending a terminus to every weave terminus
/// regresses those cases.
///
/// **Entry file only** (#1502). C# appends the implicit gather exactly once,
/// to the *root story's* weave — `SplitWeaveAndSubFlowContent`'s
/// `if (isRootStory)` guard. An `INCLUDE`d file is parsed as
/// `Story(isInclude: true)` and gets none: `Story.PreProcessTopLevelObjects`
/// splices its non-flow content in as the included story's own already-built
/// `Weave`, which becomes a nested weave *container* in the root — so a
/// trailing gather there is entered by divert (clearing the container stack)
/// and running out of it faults with `RanOutOfContent`, exactly as it does in
/// brink. Terminating included files individually would silently end the flow
/// mid-story instead, which is strictly worse than the loud fault it replaces.
///
/// Callers therefore apply this to the **last** root-content chunk only. For
/// an ink project that is the entry file by construction:
/// `compilation_closure_files` orders the closure with
/// `IncludeGraph::topological_order(entry)`, a post-order DFS from `entry`
/// that pushes `entry` after everything it includes, so `entry` is always the
/// final element. (A native project's closure is `FileId`-ordered instead,
/// but `.brink` modules have no root weave to terminate.) Positionally this
/// is also the only correct spot: the chunks concatenate into one root body,
/// and C#'s implicit gather sits at the very end of it.
fn attach_root_final_gather(children: &mut Vec<lir::Container>, ids: &mut context::IdAllocator) {
    // `#` never appears in a lowering scope path, so this key cannot collide
    // with a real container address. Content-pure since #1504: it used to be
    // keyed `#root-terminus.{file_id}`, the one `alloc_address` call in this
    // crate keyed by a `FileId` — an allocation-history-derived id (the
    // editor mints a different `FileId` for the same file when a sibling is
    // registered first), which `docs/fine-grained-salsa-proposal.md` §FG-4d
    // forbids. The owning file now reaches the key through the allocator's
    // path prefix (`IdAllocator::set_path_prefix`), which is derived from
    // that file's project path instead.
    let terminus_id = ids.alloc_address("#root-terminus");

    if !patch_root_loose_end(children, terminus_id) {
        return;
    }

    children.push(lir::Container {
        id: terminus_id,
        name: Some(ROOT_TERMINUS_NAME.to_string()),
        kind: lir::ContainerKind::Gather,
        params: Vec::new(),
        body: vec![lir::Stmt::Divert(lir::Divert {
            target: lir::DivertTarget::Done,
            args: Vec::new(),
        })],
        children: Vec::new(),
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
        local: false,
    });
}

/// Divert the root weave's outermost loose end to `terminus`, returning
/// whether one was found (and therefore whether the terminus container is
/// reachable and worth emitting).
///
/// HIR nests each choice set's post-gather content into that set's
/// continuation, so the root weave's outermost loose end is the tail of the
/// gather chain hanging off the last root-level child: descend while a gather
/// ends with another `ChoiceSet` (its continuation gather holds the deeper
/// content), then patch the first gather that does not.
///
/// Nothing is patched when the tail already ends in a terminal — an authored
/// `-> DONE` / `-> END` / divert is not a loose end, and unlike
/// [`patch_innermost_gather`] this must never overwrite one.
///
/// An `inline` gather (the wrapper a source-level standalone gather lowers to)
/// is never patched in its own right: it is entered with `EnterContainer` and
/// returns to its parent when exhausted, so it already falls through to the
/// root body's own `Done`. Its *children* are still descended into, because a
/// choice set inside it diverts — clearing the container stack — into a
/// continuation gather that is a genuine loose end.
fn patch_root_loose_end(
    children: &mut [lir::Container],
    terminus: brink_format::DefinitionId,
) -> bool {
    let Some(gather) = children
        .last_mut()
        .filter(|c| c.kind == lir::ContainerKind::Gather)
    else {
        return false;
    };

    if gather
        .body
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::ChoiceSet(_)))
    {
        return patch_root_loose_end(&mut gather.children, terminus);
    }

    if gather.inline {
        return false;
    }

    let ends_terminal = gather.body.last().is_some_and(|s| {
        matches!(
            s,
            lir::Stmt::Divert(d)
                if matches!(
                    d.target,
                    lir::DivertTarget::End
                        | lir::DivertTarget::Done
                        | lir::DivertTarget::Address(_)
                )
        )
    });
    if ends_terminal {
        return false;
    }

    gather.body.push(lir::Stmt::Divert(lir::Divert {
        target: lir::DivertTarget::Address(terminus),
        args: Vec::new(),
    }));
    true
}

/// Recursively find the innermost gather container in a chain of
/// gather-contains-`ChoiceSet` nesting and patch it with the given divert.
///
/// When a choice body ends with a `ChoiceSet`, its gather container may
/// itself end with another `ChoiceSet` (multi-level weaves). The divert
/// to the outer gather must be placed in the innermost gather that
/// doesn't end with yet another `ChoiceSet`, otherwise it becomes dead
/// code after the `done` emitted by codegen for the `ChoiceSet`.
fn patch_innermost_gather(children: &mut [lir::Container], divert: lir::Divert) {
    let Some(gather) = children
        .last_mut()
        .filter(|c| c.kind == lir::ContainerKind::Gather)
    else {
        return;
    };

    let gather_body_ends_with_choice_set = gather
        .body
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::ChoiceSet(_)));

    if gather_body_ends_with_choice_set {
        // Recurse into the gather's children to find the deeper gather
        patch_innermost_gather(&mut gather.children, divert);
        return;
    }

    let gather_body_ends_terminal = gather.body.last().is_some_and(|s| {
        matches!(
            s,
            lir::Stmt::Divert(d)
                if matches!(
                    d.target,
                    lir::DivertTarget::End
                        | lir::DivertTarget::Done
                        | lir::DivertTarget::Address(_)
                )
        )
    });

    if gather_body_ends_terminal {
        // Replace the terminal (e.g., Done) with the outer gather divert
        let last_idx = gather.body.len() - 1;
        gather.body[last_idx] = lir::Stmt::Divert(divert);
    } else {
        gather.body.push(lir::Stmt::Divert(divert));
    }
}
