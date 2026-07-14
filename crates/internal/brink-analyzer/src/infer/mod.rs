//! `signature`/`infer_body`/`type_diagnostics` — the checker substrate
//! (typed-mode-spec §2, TM-1). **Advisory-only**: this module produces
//! inference *results* for later consumers (hover, TM-3 strict mode); it
//! changes no compiler behavior and the LIR/codegen/runtime never read it.
//!
//! ## The firewall
//!
//! `infer_body(A)` (here, [`body::infer_def_body`] driven by
//! [`infer_project`]) reads only `signature(B)` for every def `B` it calls —
//! never `B`'s body. Two kinds of "signature" satisfy that rule:
//!
//! - A **global** (`VAR`/`CONST`) reads its declaration-derived
//!   [`crate::Sig::value_type`] — the existing phase-0 stub, untouched by
//!   this module (see `crate::signature`'s `signature_is_declaration_derived_only`
//!   test, which this module must never make fail).
//! - A **callable** (knot/stitch) reads its entry in `known_sigs`: either
//!   another SCC's already-finalized [`InferredSig`], or — for a call
//!   *within* the SCC currently being solved — that SCC's current fixpoint
//!   estimate. Call-site-driven inference (letting a caller's argument
//!   types flow backward into a callee's params) is never done; only a
//!   callee's own already-computed signature flows forward into how the
//!   caller's argument expressions are typed.
//! - An **`EXTERNAL` binding** (issue #786) reads its own entry in
//!   `known_sigs` too, seeded once up front by [`collect_external_sigs`]
//!   from the registered `HostManifest` rather than solved by the fixpoint
//!   (an external has no body to infer) — a call to it types its arguments
//!   exactly like a callable's, through the same `known_sigs` lookup.
//!
//! ## SCC fixpoint
//!
//! [`graph::topo_order`] batches every inferable def into strongly-connected
//! components, ordered so a component is solved only after every *other*
//! component it calls is finalized. A component may contain more than one
//! def (mutual recursion) — those solve together: seed every member with an
//! `Unknown` working signature, re-run [`body::infer_def_body`] across the
//! whole component, and repeat until no member's signature changes (or
//! [`MAX_SCC_ITERATIONS`] is hit — guard against unbounded growth, house
//! rule). This is the monomorphic Haskell-style binding-group solve spec §2
//! asks for.
//!
//! ## Laziness (perf)
//!
//! [`infer_project`] is not wired into `analysis_query`, `lir_query`,
//! `diagnostics_query`, or `story_data_query` — nothing in the existing
//! compile/IDE path calls it. The brink-db salsa wrapper
//! (`type_inference_query`) is therefore only ever computed when a consumer
//! explicitly asks for `infer_body`/`type_diagnostics`, which today is
//! nobody: this slice is pure substrate. See the PR's benchmark report for
//! the before/after warm/cold numbers this predicts (no measurable delta on
//! the existing paths).

mod body;
mod graph;
mod ty;

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::{
    BaseType, Block, ContainerPtr, DocBlock, FileId, HirFile, HostManifest, Param, ResolutionMap,
    SymbolIndex, SymbolKind, TypeExpr, TypeRef,
};
use rowan::TextRange;

pub use graph::{CallGraph, SccGraph, scc_graph};
pub use ty::{Ty, unify, unify_all};

use body::{BodyCtx, infer_def_body};
use graph::topo_order;

/// `TextRange` has no `Ord` impl (ranges have no single natural total
/// order), so every `BTreeMap` keyed by a reference's source range in this
/// module uses this `(start, end)` `u32` pair instead.
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// Caps the number of re-solve rounds for one SCC batch (guard against
/// unbounded growth, house rule). Convergence is expected within a handful
/// of rounds for this finite, monomorphic, no-overloading type universe —
/// a genuinely pathological program that never stabilizes still terminates
/// with whatever partial signature this cap leaves it at, which is legal
/// (unresolved slots read as `Unknown`), not a hang.
const MAX_SCC_ITERATIONS: usize = 8;

/// A def's inferred signature: positional param types (declaration order)
/// plus a return type. The generalized, per-def result of a body's fixpoint
/// solve — what a *caller* reads (never the caller reading the callee's
/// body directly; that's the firewall).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InferredSig {
    pub params: Vec<Ty>,
    pub return_ty: Ty,
}

/// The full inferred picture of one def's body: params, every local
/// (params ∪ temps) by name, and the return type. A superset of
/// [`InferredSig`] — `signatures` is the firewall-facing projection,
/// `bodies` is what a hover/diagnostic consumer (TM-5) wants.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyTypes {
    pub params: Vec<(String, Ty)>,
    pub locals: BTreeMap<String, Ty>,
    pub return_ty: Ty,
    /// T1c (docs/t1c-spec.md §4): statically-checkable facts about calls
    /// *through a value* (a callee resolving to a param/temp/VAR/CONST
    /// rather than a callable def) observed in this body, in source-walk
    /// order. Recorded unconditionally during inference (the walk is the
    /// only place argument expressions have types); **reported only by
    /// strict mode** (`strict::check` — gradual stays advisory, the runtime
    /// fault is its backstop, spec §3/§4).
    pub value_calls: Vec<ValueCallFact>,
}

/// One statically-checkable fact about a call through a function value
/// (T1c, docs/t1c-spec.md §4 — "under `types = strict`, calls through
/// function values are statically checked").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueCallFact {
    /// The callee reference's source range (the diagnostic site).
    pub range: TextRange,
    /// The callee's display name (`f` in `f(5)`).
    pub callee: String,
    pub kind: ValueCallKind,
}

/// What a [`ValueCallFact`] observed. Strict mode maps these onto the
/// existing TM-3 machinery — escape codes for unresolved callees, the
/// typed-mismatch code for known-type disagreements — rather than minting
/// parallel codes (docs/t1c-spec.md §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueCallKind {
    /// The callee's type is `Unknown` in call position — a strict-mode
    /// escape (`E065` class): the call can't be checked, so a strict author
    /// must annotate or restructure.
    UnknownCallee,
    /// The callee's type is `Conflicted` (#627) in call position (`E066`
    /// class).
    ConflictedCallee,
    /// The callee has a known concrete type that isn't `fn(T…): R` (and
    /// isn't `divert` — calling through a divert-ref variable is a
    /// pre-existing ink pattern this slice deliberately leaves unchecked).
    NotCallable(Ty),
    /// Known `fn(T…): R` callee, wrong argument count.
    ArityMismatch { expected: usize, got: usize },
    /// Known `fn(T…): R` callee; argument `index` (0-based) has a concrete
    /// type that neither matches the row's param type nor coerces to it
    /// (`int -> float` is the one legal directional coercion, spec §4).
    ArgMismatch {
        index: usize,
        expected: Ty,
        found: Ty,
    },
    /// `bind(f, args…)` (T1c-3, issue #733) supplied more args than remain
    /// in the known `fn(T…): R` callee's param row — over-binding, distinct
    /// from [`Self::ArityMismatch`] because `bind` has no fixed target arity
    /// to match (binding fewer than the remaining params is legal; only
    /// binding *more* is an error, mirroring the runtime's
    /// `FunctionValueArity` fault and `#fn`'s own `E081` over-binding check).
    OverBind { available: usize, got: usize },
}

/// The whole-project inference result (mirrors `AnalysisResult`'s shape:
/// one pure function over already-computed inputs, callable directly or
/// wrapped as a salsa query).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferenceResult {
    /// Every inferable (knot/stitch) def's finalized signature.
    pub signatures: BTreeMap<DefinitionId, InferredSig>,
    /// Every inferable def's full body type picture.
    pub bodies: BTreeMap<DefinitionId, BodyTypes>,
}

impl From<crate::InferredType> for Ty {
    fn from(t: crate::InferredType) -> Self {
        match t {
            crate::InferredType::Int => Ty::Int,
            crate::InferredType::Float => Ty::Float,
            crate::InferredType::Bool => Ty::Bool,
            crate::InferredType::String => Ty::String,
            crate::InferredType::Divert => Ty::Divert,
            // The initializer-derived stub records only "this was a list
            // literal", not which LIST it belongs to — conservatively
            // Unknown rather than guessing (legal; advisory-only slice).
            crate::InferredType::List => Ty::Unknown,
        }
    }
}

/// One inferable definition: its own id, declaring file, declared params,
/// and body.
///
/// `pub` (FG-2.1, issue #638): `brink-db`'s `solve_scc_query` builds these
/// itself from per-def `def_body_query` results (Ruling 2b's narrowed HIR
/// projection) and passes them into [`solve_scc`] directly, instead of
/// [`solve_scc`] rebuilding them via [`collect_defs`] over a whole-project
/// (or even whole-file) HIR slice.
#[derive(Debug, Clone, Copy)]
pub struct Def<'a> {
    pub id: DefinitionId,
    pub file: FileId,
    pub params: &'a [Param],
    pub body: &'a Block,
    /// The function-header return annotation (`): type ===`), when the def
    /// is a knot that carries one (T1c — the boundary-annotation firewall
    /// applied to the return slot: an `Unknown` inferred return overlays to
    /// the annotated type, so `#fn` rows built from this signature are
    /// concrete). `None` for stitches and unannotated knots.
    pub return_annotation: Option<&'a TypeExpr>,
}

/// Per-file resolution lookup: a `Path`'s range is only unique within its
/// own file, so resolutions must never be merged across files.
fn index_resolutions_by_file(
    resolutions: &ResolutionMap,
) -> BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>> {
    let mut by_file: BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>> = BTreeMap::new();
    for r in resolutions {
        by_file
            .entry(r.file)
            .or_default()
            .insert(range_key(r.range), r.target);
    }
    by_file
}

/// Declaration-derived global (VAR/CONST) types — read via `signature()`,
/// the firewall boundary for every non-callable reference in a body.
///
/// `value_type` covers the scalar/list/divert domain; `fn_type` (T1c
/// follow-up, issue #712) covers `Ty::Fn` separately since `InferredType`
/// has no `Fn` form (`Sig::fn_type`'s doc) — the two are mutually exclusive
/// per declaration, so trying `value_type` first and falling back to
/// `fn_type` never masks either.
fn collect_globals(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    manifest: Option<&HostManifest>,
) -> BTreeMap<DefinitionId, Ty> {
    let mut globals: BTreeMap<DefinitionId, Ty> = BTreeMap::new();
    for (&id, info) in &index.symbols {
        if matches!(info.kind, SymbolKind::Variable | SymbolKind::Constant)
            && let Some(sig) = crate::signature::signature(id, index, files, manifest)
        {
            if let Some(vt) = sig.value_type {
                globals.insert(id, Ty::from(vt));
            } else if let Some(ft) = sig.fn_type.clone() {
                globals.insert(id, ft);
            }
        }
    }
    globals
}

/// Declaration-derived `EXTERNAL` signatures (issue #786, docs/t1d-spec.md
/// §3: "a binding declared to take `handle<AudioInstance>` rejects a
/// `handle<Timer>` argument at compile time" under `types = strict`; issue
/// #805 widens this to the manifest's full scalar-semantic-type vocabulary
/// and to inline-doc-only bindings).
///
/// `EXTERNAL name(params)` has no ink-side type-annotation grammar (unlike a
/// knot/stitch's `(x: T)`/`): T ===`), so a binding's *declared*
/// parameter/return types can only come from two sources — exactly the two
/// [`crate::external_check::analyze_externals`] already merges for its
/// `SymbolMeta`/`E039`-`E042` enrichment: a matching entry in the registered
/// [`HostManifest`]'s [`brink_ir::ManifestExternal`] list, and/or an inline
/// `///` `@param`/`@returns` [`DocBlock`] parsed off the declaration itself.
/// #805 reuses that same merge order here (inline wins by param name, else
/// the registered entry wins by position) rather than re-deriving a second,
/// narrower rule — an `EXTERNAL` documented purely via `///` tags, with no
/// corresponding `ManifestExternal` entry at all, now seeds a signature too.
///
/// Every resolved [`TypeRef`] — handle-kinded or scalar — goes through
/// [`type_ref_to_ty`], which looks the name up in the registered
/// [`SemanticTypeDef`](brink_ir::SemanticTypeDef) table regardless of which
/// source (manifest or inline doc) supplied the ref; a scalar semantic type
/// (e.g. `switch_id`, `base: Int`) now types as its own `base` (`Ty::Int`)
/// exactly like a `handle<K>`-based one types as `Ty::Handle(K)` — the same
/// `known_sigs`/`observe`/`unify` call-checking path applies to both, so a
/// literal-typed argument that disagrees with a declared scalar semantic
/// type folds to `Ty::Conflicted` and reports through the pre-existing
/// `E066` classification, no new diagnostic code. This also covers
/// return-position kind checking uniformly: `reg`/`inline`'s `returns` ref
/// resolves through the identical `type_ref_to_ty` call as every param, so a
/// binding's declared return kind (handle or scalar) becomes the call
/// expression's own `Ty` wherever it's assigned or compared, through
/// `infer_call`'s existing `sig.return_ty.clone()` — no separate return-only
/// code path exists to fall out of sync with the param path.
///
/// No HIR read: entirely index + manifest + [`DocBlock`] derived (mirrors
/// [`collect_globals`]'s shape) — `inline_docs` is itself HIR-free
/// ([`DocBlock`] carries parsed doc content only, no source ranges), so this
/// still has no per-file dependency edge to narrow.
///
/// An `EXTERNAL` with neither a registered manifest entry nor an inline doc
/// contributes no signature at all — call sites stay exactly as unchecked as
/// before this issue. A param/return whose resolved [`TypeRef`] names
/// neither a base keyword nor a registered [`SemanticTypeDef`] types
/// `Ty::Unknown` — the same conservative fallback every other unresolved
/// slot in this module gets. `Ty::Unknown` params are inert at the
/// call-checking site (`BodyCtx::observe` is a documented no-op against
/// `Ty::Unknown`), so this never fabricates a false mismatch.
fn collect_external_sigs(
    index: &SymbolIndex,
    manifest: Option<&HostManifest>,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> BTreeMap<DefinitionId, InferredSig> {
    let mut sigs = BTreeMap::new();
    let (types, registered) = crate::manifest_maps(manifest);
    for (&id, info) in &index.symbols {
        if info.kind != SymbolKind::External {
            continue;
        }
        let inline = inline_docs.get(&(SymbolKind::External, info.name.clone()));
        let reg = registered.get(info.name.as_str()).copied();
        if inline.is_none() && reg.is_none() {
            continue; // no declared signature at all — stays unchecked
        }

        // Param types: inline `@param` (by name) wins, else registered (by
        // position) — the exact merge order `external_check::analyze_externals`
        // uses for the same two sources.
        let params: Vec<Ty> = info
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let tref: Option<&TypeRef> = inline
                    .and_then(|d| d.params.iter().find(|(n, _)| n == &p.name).map(|(_, t)| t))
                    .or_else(|| reg.and_then(|r| r.params.get(i).map(|mp| &mp.ty)));
                tref.map_or(Ty::Unknown, |t| type_ref_to_ty(t, &types))
            })
            .collect();
        let return_ty = inline
            .and_then(|d| d.returns.as_ref())
            .or_else(|| reg.map(|r| &r.returns))
            .map_or(Ty::Unknown, |t| type_ref_to_ty(t, &types));
        sigs.insert(id, InferredSig { params, return_ty });
    }
    sigs
}

/// Resolve a [`TypeRef`] (manifest- or inline-doc-sourced — both are the
/// bare name form, resolution is identical either way) to a checker [`Ty`]
/// (issue #805 — the full scalar-plus-handle slice of
/// `external_check::resolve_type`'s domain, closed-domain constraints
/// excluded: the checker substrate only needs a `Ty`, never a
/// [`Constraint`](brink_ir::Constraint)). A base scalar keyword
/// (`string`/`int`/`float`/`bool`) resolves directly; a name registered in
/// `types` resolves through its own [`SemanticTypeDef::base`] — `Ty::String`/
/// `Ty::Int`/`Ty::Float`/`Ty::Bool` for a scalar specialization (e.g.
/// `switch_id`, `base: Int`), `Ty::Handle(name)` for a `base: Handle` kind
/// definition (T1d-2, docs/t1d-spec.md §3 — the def's own `name` *is* the
/// declared handle-kind name `handle<K>` annotations resolve `K` against).
/// `void` (either the bare keyword or a registered `base: Void` def) has no
/// `Ty` (return-only, same as an annotation's `void`); an unresolved name —
/// no manifest at all, or a name neither a base keyword nor a registered
/// semantic type — types `Ty::Unknown` (unresolved — never a hard failure).
fn type_ref_to_ty(t: &TypeRef, types: &BTreeMap<String, brink_ir::SemanticTypeDef>) -> Ty {
    let name = t.0.trim();
    if name.is_empty() {
        return Ty::Unknown;
    }
    match BaseType::from_keyword(name) {
        Some(BaseType::String) => Ty::String,
        Some(BaseType::Int) => Ty::Int,
        Some(BaseType::Float) => Ty::Float,
        Some(BaseType::Bool) => Ty::Bool,
        // The `void`/`handle` keyword literals, and any name that isn't a
        // base keyword at all (a registered semantic-type name — handle or
        // scalar specialization), all fall through to the `types` table — a
        // registered def's own `base` decides the resolved `Ty` (issue
        // #805: this now covers scalar specializations like `switch_id`
        // too, not just `base: Handle` kinds).
        Some(BaseType::Void | BaseType::Handle) | None => match types.get(name) {
            Some(def) => match def.base {
                BaseType::String => Ty::String,
                BaseType::Int => Ty::Int,
                BaseType::Float => Ty::Float,
                BaseType::Bool => Ty::Bool,
                BaseType::Void => Ty::Unknown,
                BaseType::Handle => Ty::Handle(name.to_string()),
            },
            None => Ty::Unknown,
        },
    }
}

/// Every inferable (knot/stitch) def in the project, resolved back to its
/// own `DefinitionId` via `(file, kind, qualified name)` — HIR `Knot`/
/// `Stitch` nodes carry only a bare `Name`, not their own id.
///
/// A *floating* stitch (`= stitch`, declared before any `== knot ==`
/// header) lowers into `hir.knots` as a `Knot` node (`ContainerPtr::Stitch`)
/// but was declared `SymbolKind::Stitch` with a bare name by
/// `lower_top_level_stitch` — never `SymbolKind::Knot`, and never qualified
/// with a knot prefix (there is no enclosing knot). So the symbol-kind used
/// for the `def_of` lookup must track `knot.ptr`, not assume every
/// `hir.knots` entry is a real `SymbolKind::Knot` (#626).
fn collect_defs<'a>(files: &[(FileId, &'a HirFile)], index: &SymbolIndex) -> Vec<Def<'a>> {
    let mut def_of: BTreeMap<(FileId, SymbolKind, String), DefinitionId> = BTreeMap::new();
    for (&id, info) in &index.symbols {
        def_of.insert((info.file, info.kind, info.name.clone()), id);
    }

    let mut defs: Vec<Def<'a>> = Vec::new();
    for &(file_id, hir) in files {
        for knot in &hir.knots {
            let knot_symbol_kind = match knot.ptr {
                ContainerPtr::Knot(_) => SymbolKind::Knot,
                ContainerPtr::Stitch(_) => SymbolKind::Stitch,
            };
            if let Some(&id) = def_of.get(&(file_id, knot_symbol_kind, knot.name.text.clone())) {
                defs.push(Def {
                    id,
                    file: file_id,
                    params: &knot.params,
                    body: &knot.body,
                    return_annotation: knot.return_type.as_ref(),
                });
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(&id) = def_of.get(&(file_id, SymbolKind::Stitch, qualified)) {
                    defs.push(Def {
                        id,
                        file: file_id,
                        params: &stitch.params,
                        body: &stitch.body,
                        return_annotation: None,
                    });
                }
            }
        }
    }
    defs.sort_by_key(|d| d.id);
    defs
}

/// Shared read-only context every pass over `defs` needs.
struct ProjectCtx<'a> {
    index: &'a SymbolIndex,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    by_file: &'a BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>>,
    inferable: &'a BTreeSet<DefinitionId>,
    /// Declared `LIST`/`STRUCT` names, computed once per context — needed
    /// by the T1c annotation-firewall overlay (`annotations::resolve` of
    /// param/return/temp annotations inside [`body::infer_def_body`]).
    list_names: BTreeSet<String>,
    struct_names: BTreeSet<String>,
    /// Declared handle-kind names from the registered `HostManifest`
    /// (T1d-2b, issue #774, docs/t1d-spec.md §3) — computed once per
    /// context, same shape as `list_names`/`struct_names`, so `handle<K>`
    /// param/return/temp annotations resolve during body inference too, not
    /// just at the `signature()`/annotation-firewall seam.
    handle_names: BTreeSet<String>,
}

impl<'a> ProjectCtx<'a> {
    fn new(
        index: &'a SymbolIndex,
        globals: &'a BTreeMap<DefinitionId, Ty>,
        by_file: &'a BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>>,
        inferable: &'a BTreeSet<DefinitionId>,
        manifest: Option<&HostManifest>,
    ) -> Self {
        Self {
            index,
            globals,
            by_file,
            inferable,
            list_names: crate::annotations::declared_list_names(index),
            struct_names: crate::annotations::declared_struct_names(index),
            handle_names: crate::annotations::declared_handle_kinds(manifest),
        }
    }

    fn body_ctx(
        &'a self,
        def: &Def<'_>,
        known_sigs: &'a BTreeMap<DefinitionId, InferredSig>,
    ) -> BodyCtx<'a> {
        static EMPTY: BTreeMap<(u32, u32), DefinitionId> = BTreeMap::new();
        BodyCtx {
            resolution_by_range: self.by_file.get(&def.file).unwrap_or(&EMPTY),
            index: self.index,
            globals: self.globals,
            known_sigs,
            inferable: self.inferable,
            list_names: &self.list_names,
            struct_names: &self.struct_names,
            handle_names: &self.handle_names,
        }
    }
}

/// Pass 1: call-graph edges only. `known_sigs` is empty here — every call
/// resolves to `Unknown` and the resulting types are discarded — this pass
/// exists solely to discover which defs call which, which the SCC batching
/// (pass 2) needs before any real solving can start.
fn build_call_graph(defs: &[Def<'_>], ctx: &ProjectCtx<'_>) -> CallGraph {
    let no_sigs: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
    let mut graph = CallGraph::new();
    for d in defs {
        graph.add_node(d.id);
        let body_ctx = ctx.body_ctx(d, &no_sigs);
        let result = infer_def_body(d, &body_ctx);
        for callee in result.calls {
            graph.add_edge(d.id, callee);
        }
    }
    graph
}

/// Solve one SCC batch's fixpoint in place (the per-batch body of pass 2):
/// extends `known_sigs` with `batch`'s own members' finalized signatures —
/// seeded `Unknown`, re-run until stable or [`MAX_SCC_ITERATIONS`] — and
/// returns `batch`'s finalized [`BodyTypes`]. `known_sigs` must already carry
/// the finalized signature of every def *outside* `batch` that a member of
/// `batch` calls (every earlier batch's signature, for [`solve_batches`]'s
/// whole-project loop; every condensation-predecessor SCC's signature, for
/// the public [`solve_scc`] — FG-2, issue #631).
///
/// Shared by [`solve_batches`] (`ctx`/`by_id` built once, looped over every
/// batch — unchanged cost from before this function was extracted) and
/// [`solve_scc`] (`ctx`/`by_id` rebuilt per call, one batch at a time — the
/// new per-SCC query boundary).
fn solve_one_batch(
    batch: &BTreeSet<DefinitionId>,
    by_id: &BTreeMap<DefinitionId, &Def<'_>>,
    ctx: &ProjectCtx<'_>,
    known_sigs: &mut BTreeMap<DefinitionId, InferredSig>,
) -> BTreeMap<DefinitionId, BodyTypes> {
    for &id in batch {
        known_sigs.entry(id).or_insert_with(|| {
            let param_count = by_id.get(&id).map_or(0, |d| d.params.len());
            InferredSig {
                params: vec![Ty::Unknown; param_count],
                return_ty: Ty::Unknown,
            }
        });
    }

    let mut last_round: BTreeMap<DefinitionId, body::BodyResult> = BTreeMap::new();
    for _round in 0..MAX_SCC_ITERATIONS {
        let mut round: BTreeMap<DefinitionId, body::BodyResult> = BTreeMap::new();
        let mut changed = false;
        for &id in batch {
            let Some(&d) = by_id.get(&id) else { continue };
            let body_ctx = ctx.body_ctx(d, known_sigs);
            let result = infer_def_body(d, &body_ctx);
            let new_sig = InferredSig {
                params: result.params.iter().map(|(_, t)| t.clone()).collect(),
                return_ty: result.return_ty.clone(),
            };
            if known_sigs.get(&id) != Some(&new_sig) {
                changed = true;
            }
            known_sigs.insert(id, new_sig);
            round.insert(id, result);
        }
        last_round = round;
        if !changed {
            break;
        }
    }

    last_round
        .into_iter()
        .map(|(id, result)| {
            (
                id,
                BodyTypes {
                    params: result.params,
                    locals: result.locals,
                    return_ty: result.return_ty,
                    value_calls: result.value_calls,
                },
            )
        })
        .collect()
}

/// Pass 2: solve every SCC batch in dependency order, mutually-recursive
/// batches by fixpoint (spec §2's SCC rule — see the module doc).
///
/// `external_sigs` (issue #786): every `EXTERNAL`'s declaration-derived
/// signature ([`collect_external_sigs`]), seeded into `known_sigs` before any
/// batch solves — a call to an external now resolves through the exact same
/// `known_sigs` lookup + [`body::BodyCtx::observe`] unify path an ordinary
/// knot/stitch call already uses, so a `handle<K>`-mismatched argument folds
/// its local to `Ty::Conflicted` and reports through the pre-existing `E066`
/// classification, no parallel checking surface. Externals are never SCC
/// members (never in any `batch`), so this seed is never touched again by
/// the per-batch fixpoint loop below.
fn solve_batches(
    batches: &[BTreeSet<DefinitionId>],
    by_id: &BTreeMap<DefinitionId, &Def<'_>>,
    ctx: &ProjectCtx<'_>,
    external_sigs: &BTreeMap<DefinitionId, InferredSig>,
) -> (
    BTreeMap<DefinitionId, InferredSig>,
    BTreeMap<DefinitionId, BodyTypes>,
) {
    let mut known_sigs: BTreeMap<DefinitionId, InferredSig> = external_sigs.clone();
    let mut bodies: BTreeMap<DefinitionId, BodyTypes> = BTreeMap::new();

    for batch in batches {
        let batch_bodies = solve_one_batch(batch, by_id, ctx, &mut known_sigs);
        bodies.extend(batch_bodies);
    }

    (known_sigs, bodies)
}

/// Infer types for every knot/stitch body across the whole project.
///
/// Pure function of already-computed inputs (`index`/`resolutions`, the
/// same shape `finish_analysis`/`signature` take) — safe to call directly in
/// tests, and the exact function `type_inference_query` wraps for salsa
/// memoization. `manifest` (T1d-2b, issue #774): the registered host
/// manifest, threaded through to `signature()`/annotation resolution so
/// `handle<K>` param/return/temp annotations resolve to `Ty::Handle(K)`
/// during body inference — `None` degrades to an empty handle-kind set,
/// same posture as every other manifest-driven check. Also threaded to
/// [`collect_external_sigs`] (issue #786) so a call to a manifest-registered
/// `EXTERNAL` checks its arguments against the binding's declared param
/// types the same way a knot/stitch call already does. `inline_docs` (issue
/// #805): the project-wide merged `///` doc-comment map
/// ([`crate::project_inline_docs`]'s output), the second of
/// [`collect_external_sigs`]'s two signature sources — an empty map degrades
/// to manifest-only seeding, byte-identical to pre-#805 behavior.
#[must_use]
pub fn infer_project(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    manifest: Option<&HostManifest>,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> InferenceResult {
    let by_file = index_resolutions_by_file(resolutions);
    let globals = collect_globals(files, index, manifest);
    let defs = collect_defs(files, index);
    let inferable: BTreeSet<DefinitionId> = defs.iter().map(|d| d.id).collect();
    let by_id: BTreeMap<DefinitionId, &Def<'_>> = defs.iter().map(|d| (d.id, d)).collect();

    let ctx = ProjectCtx::new(index, &globals, &by_file, &inferable, manifest);
    let external_sigs = collect_external_sigs(index, manifest, inline_docs);

    let graph = build_call_graph(&defs, &ctx);
    let batches = topo_order(&graph);
    let (signatures, bodies) = solve_batches(&batches, &by_id, &ctx, &external_sigs);

    InferenceResult { signatures, bodies }
}

// ─── Per-def/per-SCC query boundary (FG-2, issue #631) ────────────────
//
// `docs/fine-grained-salsa-proposal.md` §2 decomposes `infer_project` into
// `call_edges(def) -> scc_membership() -> solve_scc(SccId) ->
// inferred_signature(def)`. This module keeps every algorithm exactly as
// `infer_project` already used it (SCC/condensation in `graph.rs`, the
// single-batch fixpoint above); `brink-db` owns the query *keys*, *edges*,
// and `SccId` interning (a plain `DefinitionId` — the component's minimum
// member, already `graph.rs`'s own sort key) that turn these pure functions
// into salsa-memoized, per-def/per-SCC-cacheable ones.

/// Every inferable (knot/stitch) definition's id in the project (FG-2, issue
/// #631). A cheap structural scan — needs the whole project's HIR to
/// enumerate every def's body. Superseded, for `brink-db`'s per-def/per-SCC
/// query wiring, by [`inferable_defs_from_index`] (FG-2.1, issue #638,
/// Ruling 2b — the same id set, sourced from the index alone, no HIR read);
/// kept for direct pure-function callers (e.g. [`infer_project`]) and as the
/// equivalence anchor `inferable_defs_from_index_matches_hir_derived_set`
/// pins.
#[must_use]
pub fn inferable_defs(files: &[(FileId, &HirFile)], index: &SymbolIndex) -> BTreeSet<DefinitionId> {
    collect_defs(files, index).iter().map(|d| d.id).collect()
}

/// The same inferable (knot/stitch) def id set as [`inferable_defs`], read
/// directly off the index's `SymbolKind` — no HIR (FG-2.1, issue #638,
/// Ruling 2b: "`inferable` comes from an index-sourced `inferable_defs_query`
/// (dep = `inference_index_query`, not HIR)"). A knot/stitch symbol is
/// always indexed at exactly the same moment its `hir.knots` entry is
/// lowered (`lower_single_knot`/`lower_top_level`), so filtering
/// `index.symbols` by kind here is output-identical to walking every file's
/// HIR the way [`inferable_defs`] does — pinned by
/// `inferable_defs_from_index_matches_hir_derived_set`.
#[must_use]
pub fn inferable_defs_from_index(index: &SymbolIndex) -> BTreeSet<DefinitionId> {
    index
        .symbols
        .iter()
        .filter(|(_, info)| matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch))
        .map(|(&id, _)| id)
        .collect()
}

/// Find one inferable def's own params + body from a declaring-file-scoped
/// HIR slice alone (FG-2.1, issue #638, Ruling 2b — backs `brink-db`'s
/// per-def `def_body_query(def)` projection, the `inference_index_query`
/// precedent applied to bodies). A thin filter over the same
/// [`collect_defs`] walk [`call_edges`]/[`solve_scc`] already used
/// project-wide, scoped here to exactly `def`'s declaring file so the salsa
/// wrapper records a read-edge on only that file's `lowered_query` — not
/// every project file's. Returns owned data (`Vec<Param>`/`Block` both
/// `Clone`) since the salsa caller stores the result in a long-lived memo,
/// past the borrow of any one `lowered_query` call.
#[must_use]
pub fn def_body(
    def: DefinitionId,
    declaring_file_hir: &[(FileId, &HirFile)],
    index: &SymbolIndex,
) -> Option<(Vec<Param>, Option<TypeExpr>, Block)> {
    collect_defs(declaring_file_hir, index)
        .into_iter()
        .find(|d| d.id == def)
        .map(|d| {
            (
                d.params.to_vec(),
                d.return_annotation.cloned(),
                d.body.clone(),
            )
        })
}

/// Pass 1, exposed per one definition (FG-2, issue #631 — `call_edges(def)`).
/// Computes exactly what [`build_call_graph`]'s loop body computes for one
/// def: infer this def's body with `known_sigs` empty (every call resolves
/// `Unknown`; only the *set* of resolved call targets is kept, matching the
/// design doc's explicit "keep reusing `infer_def_body` and discard types,
/// as today" allowance for this query). Returns an empty set for an
/// unknown/non-inferable def id — same "absent data reads as empty, never
/// panics" contract as the rest of this module.
///
/// **Narrowed inputs (FG-2.1, issue #638, Ruling 2a).** `declaring_file_hir`
/// need only cover `def`'s own declaring file (pass 1 never needs any other
/// file's HIR to find one def's own body); `inferable` is caller-supplied
/// (index-sourced — see [`inferable_defs_from_index`]) rather than
/// recomputed via [`collect_defs`] over the narrowed slice, because a
/// resolved call target can land in a *different* file than `def`'s own.
/// `collect_globals` is dropped entirely — pass 1 discards every computed
/// type (spec §5), so a permanently-empty globals map is behavior-identical
/// and strictly cheaper.
///
/// `manifest` (T1d-2b, issue #774): threaded through to `ProjectCtx` for the
/// same reason every other per-def FG-2 seam now carries it — `call_edges`
/// discards every computed type (only the *set* of call targets survives),
/// so which handle kinds are registered can never change this function's
/// output; the parameter exists so `brink-db`'s `call_edges_query` doesn't
/// need a second, differently-shaped code path just to reach the manifest
/// `referenced_globals`/`solve_scc` also need.
#[must_use]
pub fn call_edges(
    def: DefinitionId,
    declaring_file_hir: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    inferable: &BTreeSet<DefinitionId>,
    manifest: Option<&HostManifest>,
) -> BTreeSet<DefinitionId> {
    let by_file = index_resolutions_by_file(resolutions);
    let defs = collect_defs(declaring_file_hir, index);
    let Some(d) = defs.iter().find(|d| d.id == def) else {
        return BTreeSet::new();
    };
    let empty_globals: BTreeMap<DefinitionId, Ty> = BTreeMap::new();
    let ctx = ProjectCtx::new(index, &empty_globals, &by_file, inferable, manifest);
    let no_sigs: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
    let body_ctx = ctx.body_ctx(d, &no_sigs);
    infer_def_body(d, &body_ctx).calls
}

/// Pass 1b, exposed per one definition (FG-2.1, issue #638, Ruling 1 —
/// `referenced_globals(def)`, the same per-def body-facts family as
/// [`call_edges`]). The VAR/CONST global ids `def`'s body references,
/// recorded by [`body::BodyResult::referenced_globals`] regardless of
/// whether a real globals map was supplied — this call passes an empty one,
/// exactly [`call_edges`]'s "discard the computed types, keep the
/// structural fact" shape. `brink-db` resolves each returned id via
/// `signature_query` and hands the walk a small narrow `BTreeMap` before the
/// *real* solve runs (two walks: this scan, then [`solve_scc`] — see the
/// spec's Ruling 1 tradeoff note). Also the per-def global *read set* a
/// future T2 effect row needs — named and shaped for that reuse now, no
/// speculative machinery added.
///
/// `manifest` (T1d-2b, issue #774): same rationale as [`call_edges`]'s own
/// parameter — this pass discards every computed type too (only the
/// *referenced-def-id set* survives), so it can never change this
/// function's output; threaded so `brink-db`'s `referenced_globals_query`
/// shares one uniform per-def-seam shape with `call_edges_query`/
/// `solve_scc_query`.
#[must_use]
pub fn referenced_globals(
    def: DefinitionId,
    declaring_file_hir: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    manifest: Option<&HostManifest>,
) -> BTreeSet<DefinitionId> {
    let by_file = index_resolutions_by_file(resolutions);
    let defs = collect_defs(declaring_file_hir, index);
    let Some(d) = defs.iter().find(|d| d.id == def) else {
        return BTreeSet::new();
    };
    let empty_globals: BTreeMap<DefinitionId, Ty> = BTreeMap::new();
    let empty_inferable: BTreeSet<DefinitionId> = BTreeSet::new();
    let ctx = ProjectCtx::new(index, &empty_globals, &by_file, &empty_inferable, manifest);
    let no_sigs: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
    let body_ctx = ctx.body_ctx(d, &no_sigs);
    infer_def_body(d, &body_ctx).referenced_globals
}

/// Solve exactly one SCC batch (FG-2, issue #631 — `solve_scc(SccId)`).
///
/// `known_sigs` must already carry the finalized signature of every def
/// *outside* `batch` that a member of `batch` calls — in practice, every def
/// in every condensation-predecessor SCC. `brink-db`'s `solve_scc_query`
/// gets these by recursively reading its own dependency SCCs'
/// `solve_scc_query` results first; the condensation is a DAG (SCCs are
/// maximal by construction), so that recursion is always acyclic — no salsa
/// cycles anywhere (Fork 1 ruling, design doc §8).
///
/// **Narrowed inputs (FG-2.1, issue #638, Ruling 2b/Ruling 1).** `defs` is
/// caller-supplied (built from per-def `def_body_query` results — only
/// `batch`'s own members' declaring files are ever read); `globals` is the
/// small narrow map `brink-db` built from every member's
/// [`referenced_globals`] pre-scan, resolved through `signature_query`
/// (never [`collect_globals`]'s whole-project scan); `inferable` is
/// index-sourced ([`inferable_defs_from_index`]). None of this changes the
/// fixpoint mechanics below — only how the read-only context feeding it is
/// assembled, and how narrow the salsa dependency edges recording that
/// assembly turn out to be.
///
/// `manifest` (T1d-2b, issue #774): the registered host manifest, threaded
/// through to `ProjectCtx` so a `handle<K>` param/return/temp annotation
/// resolves to `Ty::Handle(K)` here too — this is the seam that makes
/// strict-mode handle-kind rejection reachable end-to-end (docs/t1d-spec.md
/// §3, the #767 acceptance criterion): once two locals of different
/// declared handle kinds are unified together (e.g. compared or
/// reassigned), the #627 lattice already folds them to `Ty::Conflicted`,
/// which `strict::check`'s existing `E066` classification reports — this
/// function is what was missing to let a genuine `Ty::Handle` ever reach
/// that lattice from body-usage inference at all. `brink-db`'s
/// `solve_scc_query` reads it off `project.analysis_options(db)`, the same
/// coarse project-wide dependency shape `per_file_diagnostics_query`
/// already reads `host_manifest` at.
///
/// **`EXTERNAL` call-site checking (issue #786; widened by issue #805 to
/// scalar semantic types and inline-doc-only bindings).** `known_sigs` is
/// also seeded (idempotently, every call — cheap index+manifest+doc scan, no
/// HIR) with [`collect_external_sigs`]'s declaration-derived signatures
/// before this batch solves, so a call to a manifest-registered or
/// inline-doc-only `EXTERNAL` types its arguments (and its return value,
/// wherever the call expression is used) against the binding's declared
/// types through the exact same [`body::BodyCtx::observe`] path a
/// knot/stitch call already uses — same #627 `Ty::Conflicted` lattice, same
/// `E066` report, no parallel checking surface. `index`/`manifest` are both
/// already read by this function for every other reason above; `inline_docs`
/// (issue #805) is the project-wide merged `///` doc-comment map
/// (`brink-db`'s `inline_docs_query`, the same memo `external_meta_query`
/// already reads it from), so this adds exactly one new salsa dependency
/// edge on `brink-db`'s `solve_scc_query` side — the same coarse,
/// range-free, `Eq`-cutoff shape `inline_docs_query` already gives every
/// other reader.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "the FG-2 per-SCC solve boundary (issue #631) — each parameter is an \
              independently-narrowed input `brink-db`'s solve_scc_query assembles from its \
              own per-def salsa queries; bundling them into a struct would just move the same \
              shape one level down for no clarity gain, and this is the one call site (the \
              salsa wrapper) plus tests, not a widely-called API"
)]
pub fn solve_scc(
    batch: &BTreeSet<DefinitionId>,
    defs: &[Def<'_>],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    globals: &BTreeMap<DefinitionId, Ty>,
    inferable: &BTreeSet<DefinitionId>,
    mut known_sigs: BTreeMap<DefinitionId, InferredSig>,
    manifest: Option<&HostManifest>,
    inline_docs: &BTreeMap<(SymbolKind, String), DocBlock>,
) -> (
    BTreeMap<DefinitionId, InferredSig>,
    BTreeMap<DefinitionId, BodyTypes>,
) {
    known_sigs.extend(collect_external_sigs(index, manifest, inline_docs));
    let by_file = index_resolutions_by_file(resolutions);
    let by_id: BTreeMap<DefinitionId, &Def<'_>> = defs.iter().map(|d| (d.id, d)).collect();
    let ctx = ProjectCtx::new(index, globals, &by_file, inferable, manifest);

    let bodies = solve_one_batch(batch, &by_id, &ctx, &mut known_sigs);
    let signatures: BTreeMap<DefinitionId, InferredSig> = batch
        .iter()
        .filter_map(|id| known_sigs.get(id).map(|sig| (*id, sig.clone())))
        .collect();
    (signatures, bodies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::lower;

    fn build(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        (hir, (*index).clone(), (*resolutions).clone())
    }

    /// [`build`], plus the project-wide merged `///` doc map (issue #805 —
    /// the inline-doc-only `collect_external_sigs` source, mirroring
    /// `whole_project_diagnostics`'s own `collect_inline_docs` call).
    fn build_with_docs(
        src: &str,
    ) -> (
        HirFile,
        SymbolIndex,
        ResolutionMap,
        BTreeMap<(SymbolKind, String), DocBlock>,
    ) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) = crate::resolve(FileId(0), &manifest, &index);
        let inline_docs = crate::project_inline_docs(&[(FileId(0), &manifest)]);
        (hir, (*index).clone(), (*resolutions).clone(), inline_docs)
    }

    fn sig_of<'a>(result: &'a InferenceResult, index: &SymbolIndex, name: &str) -> &'a InferredSig {
        let id = index
            .by_name
            .get(name)
            .and_then(|ids| ids.first())
            .copied()
            .expect("no def with this name");
        result
            .signatures
            .get(&id)
            .expect("no inferred signature for this def")
    }

    #[test]
    fn param_type_inferred_from_arithmetic_use() {
        // A knot whose param is used arithmetically against an int literal.
        let (hir, index, res) = build("=== heal(hp) ===\n~ temp x = hp + 1\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "heal");
        assert_eq!(sig.params, vec![Ty::Int]);
    }

    #[test]
    fn param_type_inferred_from_comparison_with_float_literal() {
        let (hir, index, res) = build("=== spend(gold) ===\n{gold > 1.5:\n  ok\n}\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "spend");
        assert_eq!(sig.params, vec![Ty::Float]);
    }

    #[test]
    fn floating_stitch_body_is_inferred() {
        // A *floating* stitch — `= name`, declared before any `== knot ==`
        // header — lowers into `hir.knots` (as `ContainerPtr::Stitch`) but
        // is declared `SymbolKind::Stitch` with a bare name, not
        // `SymbolKind::Knot`. Before #626, `collect_defs` always looked the
        // entry up as `SymbolKind::Knot`, the lookup silently failed, and
        // this def never made it into `defs` — no signature, no body types,
        // total silent skip.
        let (hir, index, res) = build("= heal(hp)\n~ temp x = hp + 1\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "heal");
        assert_eq!(sig.params, vec![Ty::Int]);
    }

    #[test]
    fn floating_stitch_coexists_with_real_knot_and_its_nested_stitch() {
        // Regression guard for the fix itself: distinguishing floating
        // stitches (`ContainerPtr::Stitch`) from real knots
        // (`ContainerPtr::Knot`) in `collect_defs` must not disturb the
        // existing, already-working real-knot / nested-stitch lookup path.
        let (hir, index, res) = build(
            "= intro(hp)\n~ temp x = hp + 1\n-> DONE\n\
             === knot_a(gold) ===\n{gold > 1.5:\n  ok\n}\n-> stitch_a ->\n\
             = stitch_a(silver)\n~ temp y = silver + 1\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        assert_eq!(sig_of(&result, &index, "intro").params, vec![Ty::Int]);
        assert_eq!(sig_of(&result, &index, "knot_a").params, vec![Ty::Float]);
        let stitch_a_id = index
            .by_name
            .get("knot_a.stitch_a")
            .and_then(|ids| ids.first())
            .copied()
            .expect("no def for knot_a.stitch_a");
        let stitch_a_sig = result
            .signatures
            .get(&stitch_a_id)
            .expect("no inferred signature for knot_a.stitch_a");
        assert_eq!(stitch_a_sig.params, vec![Ty::Int]);
    }

    #[test]
    fn unused_param_is_unknown_and_legal() {
        let (hir, index, res) = build("=== noop(x) ===\nHello.\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "noop");
        assert_eq!(sig.params, vec![Ty::Unknown]);
    }

    #[test]
    fn return_type_inferred_from_return_statement() {
        let (hir, index, res) = build("=== function double(x) ===\n~ return x + x\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "double");
        // `x` only ever appears added to itself — Unknown stays Unknown
        // under `unify(Unknown, Unknown) == Unknown`; the *return type*
        // still comes out Unknown too (nothing ever pins `x` concrete).
        assert_eq!(sig.return_ty, Ty::Unknown);
    }

    #[test]
    fn call_site_propagates_callee_param_type_to_caller_local() {
        let (hir, index, res) = build(
            "=== main ===\n~ temp v = 1\n~ use_it(v)\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let use_it = sig_of(&result, &index, "use_it");
        assert_eq!(use_it.params, vec![Ty::Float]);
        // `main`'s own local `v` isn't a param, so we check it via `bodies`.
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(main_body.locals.get("v"), Some(&Ty::Float));
    }

    // ─── `EXTERNAL` call-site checking (issue #786) ─────────────────────

    fn audio_manifest_with_external(param_kind: &str) -> brink_ir::HostManifest {
        brink_ir::HostManifest {
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "Timer".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            externals: vec![brink_ir::ManifestExternal {
                name: "play_sound".to_string(),
                params: vec![brink_ir::ManifestParam {
                    name: "inst".to_string(),
                    ty: brink_ir::TypeRef(param_kind.to_string()),
                }],
                returns: brink_ir::TypeRef::default(),
                kind: brink_ir::ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            }],
        }
    }

    /// The #786 mechanism, isolated: a manifest-registered `EXTERNAL`'s
    /// declared `handle<K>` param type propagates into `known_sigs` exactly
    /// like a knot/stitch callee's declared param type does
    /// ([`call_site_propagates_callee_param_type_to_caller_local`]'s own
    /// pattern) — a caller's local passed as the argument picks up the
    /// binding's declared kind.
    #[test]
    fn external_call_propagates_declared_handle_kind_to_caller_local() {
        let (hir, index, res) = build(
            "EXTERNAL play_sound(inst)\n=== main ===\n~ temp s = get_sound(1)\n\
             ~ play_sound(s)\n-> DONE\n=== function get_sound(id): handle<AudioInstance> ===\n~ return id\n",
        );
        let manifest = audio_manifest_with_external("AudioInstance");
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("s"),
            Some(&Ty::Handle("AudioInstance".to_string())),
            "s picks up its own declared return kind cleanly: {main_body:?}"
        );
    }

    /// Positive case: a local declared with one handle kind, passed as the
    /// argument to a binding declared for a *different* kind, folds to
    /// `Ty::Conflicted` at the call site through `observe`/`unify` — the
    /// same #627 lattice a mismatched knot/stitch call argument already
    /// used, no parallel checking surface (`strict.rs`'s
    /// `external_call_cross_kind_argument_is_conflicted_under_strict` pins
    /// the resulting `E066` diagnostic end to end).
    #[test]
    fn external_call_with_cross_kind_argument_conflicts_the_caller_local() {
        let (hir, index, res) = build(
            "EXTERNAL play_sound(inst)\n=== main ===\n~ temp t = get_timer(1)\n\
             ~ play_sound(t)\n-> DONE\n=== function get_timer(id): handle<Timer> ===\n~ return id\n",
        );
        let manifest = audio_manifest_with_external("AudioInstance");
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("t"),
            Some(&Ty::Conflicted),
            "t is Timer-kinded but play_sound declares AudioInstance: {main_body:?}"
        );
    }

    /// No manifest registered at all: an `EXTERNAL` call contributes no
    /// signature (`collect_external_sigs` degrades to empty, same posture as
    /// every other manifest-driven check) — the call types `Ty::Unknown`,
    /// exactly today's byte-identical behavior. Pins the "gradual mode is
    /// unaffected" half of the #786 acceptance criterion at the inference
    /// level (strict mode itself never even runs without `types = strict`).
    #[test]
    fn external_call_with_no_manifest_stays_unknown() {
        let (hir, index, res) = build(
            "EXTERNAL play_sound(inst)\n=== main ===\n~ temp t = 1\n~ play_sound(t)\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        // `t` is pinned Int by its own `= 1` initializer, unaffected by the
        // unchecked external call.
        assert_eq!(main_body.locals.get("t"), Some(&Ty::Int));
    }

    /// An `EXTERNAL` with no matching registered manifest entry AND no
    /// inline `///` doc — truly undeclared — contributes no signature
    /// either, same conservative "absent data reads as no signature"
    /// contract as every other lookup miss in this module. (Issue #805
    /// widens the *inline-doc-only* case — a registered `///` doc with no
    /// matching `ManifestExternal` — to contribute a real signature; see
    /// `inline_only_external_*` below for that case specifically.)
    #[test]
    fn external_call_with_unregistered_name_stays_unknown() {
        let (hir, index, res) = build(
            "EXTERNAL other_call(inst)\n=== main ===\n~ temp t = 1\n~ other_call(t)\n-> DONE\n",
        );
        let manifest = audio_manifest_with_external("AudioInstance");
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(main_body.locals.get("t"), Some(&Ty::Int));
    }

    // ─── Issue #805: scalar semantic types, inline-only externals, and
    // return-position kind checking ─────────────────────────────────────

    /// A manifest declaring a *scalar* semantic type (`switch_id`, `base:
    /// Int`) alongside the two handle kinds — the vocabulary
    /// `collect_external_sigs` now resolves param/return `TypeRef`s against
    /// uniformly, handle or scalar.
    fn manifest_with_scalar_and_handle_types() -> brink_ir::HostManifest {
        let mut manifest = audio_manifest_with_external("AudioInstance");
        manifest.types.push(brink_ir::SemanticTypeDef {
            name: "switch_id".to_string(),
            base: brink_ir::BaseType::Int,
            constraint: None,
            values: None,
            widget: None,
        });
        manifest
    }

    /// Point (1): a manifest-registered `EXTERNAL`'s param declared with a
    /// *scalar* semantic type (not a `handle<K>` kind) now resolves to its
    /// own `base` (`switch_id` -> `Ty::Int`) and propagates into the
    /// caller's local exactly like a `handle<K>`-declared param already did
    /// (mirrors `external_call_propagates_declared_handle_kind_to_caller_local`).
    #[test]
    fn external_call_scalar_semantic_type_param_propagates_to_caller_local() {
        let mut manifest = manifest_with_scalar_and_handle_types();
        manifest.externals.push(brink_ir::ManifestExternal {
            name: "toggle".to_string(),
            params: vec![brink_ir::ManifestParam {
                name: "id".to_string(),
                ty: brink_ir::TypeRef("switch_id".to_string()),
            }],
            returns: brink_ir::TypeRef::default(),
            kind: brink_ir::ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        });
        let (hir, index, res) =
            build("EXTERNAL toggle(id)\n=== main ===\n~ temp s = 1\n~ toggle(s)\n-> DONE\n");
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("s"),
            Some(&Ty::Int),
            "s unifies cleanly against toggle's declared switch_id (base int): {main_body:?}"
        );
    }

    /// Point (1), negative: a caller's local pinned to a *different*
    /// concrete type (string) than the binding's declared scalar semantic
    /// type (`switch_id`, base int) folds to `Ty::Conflicted` at the call
    /// site — the same #627 lattice a `handle<K>` mismatch already used, no
    /// new diagnostic code.
    #[test]
    fn external_call_scalar_semantic_type_mismatch_conflicts_the_caller_local() {
        let mut manifest = manifest_with_scalar_and_handle_types();
        manifest.externals.push(brink_ir::ManifestExternal {
            name: "toggle".to_string(),
            params: vec![brink_ir::ManifestParam {
                name: "id".to_string(),
                ty: brink_ir::TypeRef("switch_id".to_string()),
            }],
            returns: brink_ir::TypeRef::default(),
            kind: brink_ir::ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        });
        let (hir, index, res) = build(
            "EXTERNAL toggle(id)\n=== main ===\n~ temp s = \"harbor\"\n~ toggle(s)\n-> DONE\n",
        );
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("s"),
            Some(&Ty::Conflicted),
            "s is a string but toggle declares switch_id (base int): {main_body:?}"
        );
    }

    /// Point (2): an `EXTERNAL` documented *purely* via an inline `///
    /// @param` doc comment — no corresponding `ManifestExternal` entry at
    /// all in the registered manifest — now seeds a signature too
    /// (`collect_external_sigs`'s inline-doc merge). The manifest here only
    /// registers the `AudioInstance`/`Timer` handle-kind *vocabulary*
    /// (`types`), never a `play_sound` entry under `externals`.
    #[test]
    fn inline_only_external_param_type_propagates_to_caller_local() {
        let (hir, index, res, inline_docs) = build_with_docs(
            "/// @param inst {AudioInstance}\n\
             EXTERNAL play_sound(inst)\n\
             === main ===\n~ temp s = get_sound(1)\n~ play_sound(s)\n-> DONE\n\
             === function get_sound(id): handle<AudioInstance> ===\n~ return id\n",
        );
        let manifest = brink_ir::HostManifest {
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "Timer".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            externals: Vec::new(), // deliberately no `play_sound` entry
        };
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &inline_docs,
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("s"),
            Some(&Ty::Handle("AudioInstance".to_string())),
            "s unifies cleanly against play_sound's inline-doc-declared AudioInstance: {main_body:?}"
        );
    }

    /// Point (2), negative: same inline-doc-only `play_sound`, but the
    /// caller's local is declared a *different* handle kind (`Timer`) —
    /// folds to `Ty::Conflicted`, proving the inline-only signature is
    /// actually checked, not just recorded.
    #[test]
    fn inline_only_external_cross_kind_argument_conflicts_the_caller_local() {
        let (hir, index, res, inline_docs) = build_with_docs(
            "/// @param inst {AudioInstance}\n\
             EXTERNAL play_sound(inst)\n\
             === main ===\n~ temp t = get_timer(1)\n~ play_sound(t)\n-> DONE\n\
             === function get_timer(id): handle<Timer> ===\n~ return id\n",
        );
        let manifest = brink_ir::HostManifest {
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "Timer".to_string(),
                    base: brink_ir::BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            externals: Vec::new(),
        };
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &inline_docs,
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("t"),
            Some(&Ty::Conflicted),
            "t is Timer-kinded but play_sound's inline doc declares AudioInstance: {main_body:?}"
        );
    }

    /// Point (3): return-position kind checking. `spawn_timer`'s
    /// *registered* return type is `Timer` (a handle kind) — assigning its
    /// result directly to a local already pinned `AudioInstance` (by a
    /// second call) must fold that local to `Ty::Conflicted`, proving the
    /// binding's declared *return* kind is checked, not just its params.
    /// (`external_call_propagates_declared_handle_kind_to_caller_local`
    /// already pins the positive return-position case implicitly, via
    /// `play_sound`'s *param* absorbing `get_sound`'s knot-return-annotated
    /// kind; this test isolates an `EXTERNAL`'s own declared return kind
    /// instead of a knot's.)
    #[test]
    fn external_call_return_position_kind_mismatch_conflicts_the_caller_local() {
        let mut manifest = audio_manifest_with_external("AudioInstance");
        manifest.externals.push(brink_ir::ManifestExternal {
            name: "spawn_timer".to_string(),
            params: Vec::new(),
            returns: brink_ir::TypeRef("Timer".to_string()),
            kind: brink_ir::ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        });
        let (hir, index, res) = build(
            "EXTERNAL play_sound(inst)\nEXTERNAL spawn_timer()\n\
             === main ===\n~ temp x = spawn_timer()\n~ play_sound(x)\n-> DONE\n",
        );
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("x"),
            Some(&Ty::Conflicted),
            "x is spawn_timer's declared Timer return, passed where play_sound declares \
             AudioInstance: {main_body:?}"
        );
    }

    /// Point (3), positive: `spawn_timer`'s declared return kind matches the
    /// declared param kind it's immediately passed to — unifies cleanly, no
    /// escape.
    #[test]
    fn external_call_return_position_kind_match_unifies_cleanly() {
        let mut manifest = audio_manifest_with_external("AudioInstance");
        manifest.externals.push(brink_ir::ManifestExternal {
            name: "spawn_audio".to_string(),
            params: Vec::new(),
            returns: brink_ir::TypeRef("AudioInstance".to_string()),
            kind: brink_ir::ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        });
        let (hir, index, res) = build(
            "EXTERNAL play_sound(inst)\nEXTERNAL spawn_audio()\n\
             === main ===\n~ temp x = spawn_audio()\n~ play_sound(x)\n-> DONE\n",
        );
        let result = infer_project(
            &[(FileId(0), &hir)],
            &index,
            &res,
            Some(&manifest),
            &BTreeMap::new(),
        );
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            main_body.locals.get("x"),
            Some(&Ty::Handle("AudioInstance".to_string())),
            "x is spawn_audio's declared AudioInstance return, matching play_sound's own \
             declared param kind: {main_body:?}"
        );
    }

    #[test]
    #[expect(
        clippy::similar_names,
        reason = "ping/pong are the clearest names for this pair"
    )]
    fn mutual_recursion_params_stay_firewalled_to_each_defs_own_body() {
        // `ping` and `pong` call each other with an arithmetic expression
        // (`n - 1`), not a bare local — so nothing about the *callee's*
        // declared param type can flow backward onto the *caller's* own `n`
        // (call-site-driven inference is forbidden by the firewall). Each
        // def's own param type is pinned only by its own body's comparison:
        // `ping` compares `n` to an int literal, `pong` to a float literal.
        let (hir, index, res) = build(
            "=== function ping(n) ===\n{n > 0:\n  ~ return pong(n - 1)\n}\n~ return n\n\
             === function pong(n) ===\n{n > 0.5:\n  ~ return ping(n - 1)\n}\n~ return n\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let ping_sig = sig_of(&result, &index, "ping");
        let pong_sig = sig_of(&result, &index, "pong");
        assert_eq!(
            ping_sig.params,
            vec![Ty::Int],
            "ping's own body only compares n to an int"
        );
        assert_eq!(
            pong_sig.params,
            vec![Ty::Float],
            "pong's own body only compares n to a float"
        );
    }

    #[test]
    #[expect(
        clippy::similar_names,
        reason = "ping/pong are the clearest names for this pair"
    )]
    fn mutual_recursion_return_type_converges_by_fixpoint() {
        // `ping`'s return type is `unify(Float, pong's return type)`; `pong`'s
        // return type is exactly `ping`'s return type. Neither def has a
        // concrete return type on its own — round 0 sees the other's
        // `Unknown` placeholder — so this only converges to `Float` because
        // the batch is re-solved until stable (the SCC fixpoint), not in a
        // single pass.
        let (hir, index, res) = build(
            "=== function ping(n) ===\n{n == 0:\n  ~ return 0.0\n}\n~ return pong(n - 1)\n\
             === function pong(n) ===\n~ return ping(n)\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let ping_sig = sig_of(&result, &index, "ping");
        let pong_sig = sig_of(&result, &index, "pong");
        assert_eq!(ping_sig.return_ty, Ty::Float);
        assert_eq!(pong_sig.return_ty, Ty::Float);
    }

    #[test]
    fn intrinsic_len_types_int() {
        let (hir, index, res) =
            build("=== main ===\n~ temp arr = #[1, 2, 3]\n~ temp n = len(arr)\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(body.locals.get("arr"), Some(&Ty::Array(Box::new(Ty::Int))));
        assert_eq!(body.locals.get("n"), Some(&Ty::Int));
    }

    #[test]
    fn determinism_same_input_same_output() {
        let src = "=== function fib(n) ===\n{n < 2.0:\n  ~ return n\n}\n~ return fib(n - 1) + fib(n - 2)\n";
        let (hir_a, index_a, res_a) = build(src);
        let (hir_b, index_b, res_b) = build(src);
        let a = infer_project(
            &[(FileId(0), &hir_a)],
            &index_a,
            &res_a,
            None,
            &BTreeMap::new(),
        );
        let b = infer_project(
            &[(FileId(0), &hir_b)],
            &index_b,
            &res_b,
            None,
            &BTreeMap::new(),
        );
        assert_eq!(a, b, "same input must infer identical types every run");
    }

    // ─── Conflicted lattice point (#627) ───────────────────────────────

    #[test]
    fn genuinely_disjoint_uses_infer_param_as_conflicted() {
        // `hp` is compared against an int literal and a string literal —
        // a genuine, irreconcilable conflict. Pre-#627 this degraded to
        // `Unknown`, indistinguishable from "never observed".
        let (hir, index, res) = build(
            "=== conflict_case(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "conflict_case");
        assert_eq!(sig.params, vec![Ty::Conflicted]);
    }

    #[test]
    fn conflict_detection_is_order_independent_across_real_source() {
        // The exact bug #627 exists to close: `unify(Int, String)` used to
        // degrade to `Unknown`, and `observe` short-circuits on an
        // `Unknown` candidate (a legitimate optimization for the true
        // identity element) — so which concrete type "won" depended on
        // which comparison the walk reached *last*, silently masking the
        // conflict as a normal concrete type instead of surfacing it. Both
        // source orderings below must now infer the same `Conflicted`
        // param, proving detection no longer depends on declaration order.
        let forward =
            "=== conflict_fwd(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n";
        let reversed =
            "=== conflict_rev(hp) ===\n{hp == \"no\":\n  no\n}\n{hp > 5:\n  ok\n}\n-> DONE\n";

        let (hir_f, index_f, res_f) = build(forward);
        let result_f = infer_project(
            &[(FileId(0), &hir_f)],
            &index_f,
            &res_f,
            None,
            &BTreeMap::new(),
        );
        let sig_f = sig_of(&result_f, &index_f, "conflict_fwd");

        let (hir_r, index_r, res_r) = build(reversed);
        let result_r = infer_project(
            &[(FileId(0), &hir_r)],
            &index_r,
            &res_r,
            None,
            &BTreeMap::new(),
        );
        let sig_r = sig_of(&result_r, &index_r, "conflict_rev");

        assert_eq!(sig_f.params, vec![Ty::Conflicted], "int-then-string order");
        assert_eq!(sig_r.params, vec![Ty::Conflicted], "string-then-int order");
        assert_eq!(
            sig_f.params, sig_r.params,
            "conflict detection must not depend on observation order"
        );
    }

    #[test]
    #[expect(
        clippy::similar_names,
        reason = "ping/pong are the clearest names for this pair"
    )]
    fn conflicted_absorbs_through_the_scc_fixpoint() {
        // `ping`'s own base case returns a string; `pong`'s own base case
        // returns an int; each recursive case returns whatever the other
        // member currently resolves to. Neither member's own body is
        // internally conflicted (each sees only one concrete literal type
        // directly), but the two base cases can never agree once threaded
        // through the SCC's shared fixpoint — join stays monotone, so once
        // either member's estimate becomes `Conflicted` mid-fixpoint it
        // must propagate to the other and never get diluted back to
        // `Unknown` in a later round (the #627 ruling's "SCC fixpoint
        // convergence is unaffected" clause, proven end to end here rather
        // than just at the `unify` unit level).
        let (hir, index, res) = build(
            "=== function ping(n) ===\n{n == 0:\n  ~ return \"done\"\n}\n~ return pong(n - 1)\n\
             === function pong(n) ===\n{n == 0:\n  ~ return 1\n}\n~ return ping(n - 1)\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let ping_sig = sig_of(&result, &index, "ping");
        let pong_sig = sig_of(&result, &index, "pong");
        assert_eq!(ping_sig.return_ty, Ty::Conflicted);
        assert_eq!(pong_sig.return_ty, Ty::Conflicted);
    }

    // ─── T1c: #fn typing + the annotation-firewall overlay ─────────────

    /// The spec's own worked example (docs/t1c-spec.md §2/§4): with the
    /// target fully annotated, `#fn(heal, player_hp)` consumes the bound
    /// prefix and types as `fn(int): int`.
    #[test]
    fn fn_literal_consumes_the_bound_prefix_of_the_targets_signature() {
        let (hir, index, res) = build(
            "=== function heal(ref hp: int, amount: int): int ===\n~ hp = hp + amount\n~ return hp\n\
             VAR player_hp = 10\n\
             === main ===\n~ temp heal_player = #fn(heal, player_hp)\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            body.locals.get("heal_player"),
            Some(&Ty::Fn(vec![Ty::Int], Box::new(Ty::Int)))
        );
    }

    #[test]
    fn fn_literal_over_an_inferred_signature_needs_no_annotations() {
        // The target's row can come from body inference alone.
        let (hir, index, res) = build(
            "=== function double(x) ===\n~ return x * 2\n\
             === main ===\n~ temp f = #fn(double)\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(
            body.locals.get("f"),
            Some(&Ty::Fn(vec![Ty::Int], Box::new(Ty::Int)))
        );
    }

    #[test]
    fn fn_literal_with_unresolvable_target_stays_unknown() {
        let (hir, index, res) = build("=== main ===\n~ temp f = #fn(nowhere)\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(body.locals.get("f"), Some(&Ty::Unknown));
    }

    /// T1c overlay: an annotated param the body never constrains surfaces
    /// its annotation type in the inferred signature (the firewall applied
    /// to the signature — a `#fn` row built from it must be concrete).
    #[test]
    fn annotated_but_unconstrained_param_overlays_to_the_annotation_type() {
        let (hir, index, res) = build("=== noop(x: int) ===\nHello.\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "noop");
        assert_eq!(sig.params, vec![Ty::Int]);
    }

    /// Overlay is Unknown-only: a body use that disagrees with the
    /// annotation keeps its own derivation (E063's two-independent-
    /// derivations comparison, and the #627 Conflicted lattice, untouched).
    #[test]
    fn overlay_never_replaces_a_concrete_body_derivation() {
        let (hir, index, res) = build("=== heal(hp: string) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "heal");
        assert_eq!(sig.params, vec![Ty::Int], "body derivation wins");
    }

    #[test]
    fn return_annotation_overlays_an_unconstrained_return() {
        // `return hp` types Unknown from the body alone (nothing pins hp
        // before the return); the `): int` annotation overlays it.
        let (hir, index, res) = build("=== function passthru(hp): int ===\n~ return hp\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let sig = sig_of(&result, &index, "passthru");
        assert_eq!(sig.return_ty, Ty::Int);
    }

    // ─── Per-def/per-SCC decomposition (FG-2, issue #631) ─────────────

    #[test]
    fn call_edges_matches_the_calls_infer_project_discovers() {
        let (hir, index, res) = build(
            "=== main ===\n~ temp v = 1\n~ use_it(v)\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n",
        );
        let files = [(FileId(0), &hir)];
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let use_it_id = index
            .by_name
            .get("use_it")
            .and_then(|ids| ids.first())
            .copied()
            .expect("use_it");

        let inferable = inferable_defs_from_index(&index);
        let edges = call_edges(main_id, &files, &index, &res, &inferable, None);
        assert_eq!(
            edges,
            BTreeSet::from([use_it_id]),
            "main's only call edge is to use_it"
        );
        let leaf_edges = call_edges(use_it_id, &files, &index, &res, &inferable, None);
        assert!(leaf_edges.is_empty(), "use_it calls nothing");
    }

    #[test]
    fn call_edges_is_empty_for_an_unknown_def() {
        let (hir, index, res) = build("=== main ===\nHello.\n-> DONE\n");
        let files = [(FileId(0), &hir)];
        let bogus = DefinitionId::new(brink_format::DefinitionTag::Address, 0xDEAD_BEEF);
        let inferable = inferable_defs_from_index(&index);
        assert!(call_edges(bogus, &files, &index, &res, &inferable, None).is_empty());
    }

    // ─── Lazy per-reference globals (FG-2.1, issue #638) ───────────────

    #[test]
    fn inferable_defs_from_index_matches_hir_derived_set() {
        // Same three fixtures the FG-2/#626 tests already carry (plain
        // mutual call, floating stitch, nested stitch) — the index-only
        // projection must agree with the HIR-walking one on every shape
        // `collect_defs` handles, not just the easy case.
        let fixtures = [
            "=== main ===\n~ temp v = 1\n~ use_it(v)\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n",
            "= heal(hp)\n~ temp x = hp + 1\n-> DONE\n",
            "= intro(hp)\n~ temp x = hp + 1\n-> DONE\n\
             === knot_a(gold) ===\n{gold > 1.5:\n  ok\n}\n-> stitch_a ->\n\
             = stitch_a(silver)\n~ temp y = silver + 1\n-> DONE\n",
        ];
        for src in fixtures {
            let (hir, index, _res) = build(src);
            let files = [(FileId(0), &hir)];
            assert_eq!(
                inferable_defs_from_index(&index),
                inferable_defs(&files, &index),
                "index-sourced and HIR-walking inferable sets diverged for: {src}"
            );
        }
    }

    #[test]
    fn referenced_globals_finds_every_var_and_const_read_in_a_body() {
        let (hir, index, res) = build(
            "VAR gold = 10\nCONST max_gold = 100\n\
             === spend(cost) ===\n~ gold = gold - cost\n{gold > max_gold:\n  rich\n}\n-> DONE\n",
        );
        let files = [(FileId(0), &hir)];
        let spend_id = index
            .by_name
            .get("spend")
            .and_then(|ids| ids.first())
            .copied()
            .expect("spend");
        let gold_id = index
            .by_name
            .get("gold")
            .and_then(|ids| ids.first())
            .copied()
            .expect("gold");
        let max_gold_id = index
            .by_name
            .get("max_gold")
            .and_then(|ids| ids.first())
            .copied()
            .expect("max_gold");

        let global_refs = referenced_globals(spend_id, &files, &index, &res, None);
        assert_eq!(
            global_refs,
            BTreeSet::from([gold_id, max_gold_id]),
            "spend's body reads both gold and max_gold"
        );
    }

    #[test]
    fn referenced_globals_is_empty_when_a_body_reads_no_globals() {
        let (hir, index, res) = build("=== main ===\n~ temp v = 1\n-> DONE\n");
        let files = [(FileId(0), &hir)];
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        assert!(referenced_globals(main_id, &files, &index, &res, None).is_empty());
    }

    #[test]
    fn inferable_defs_matches_every_knot_and_stitch() {
        let (hir, index, res) = build(
            "=== main ===\n~ temp v = 1\n~ use_it(v)\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n",
        );
        let files = [(FileId(0), &hir)];
        let _ = &res;
        let defs = inferable_defs(&files, &index);
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let use_it_id = index
            .by_name
            .get("use_it")
            .and_then(|ids| ids.first())
            .copied()
            .expect("use_it");
        assert_eq!(defs, BTreeSet::from([main_id, use_it_id]));
    }

    /// The decomposition equivalence gate the design doc's §9 FG-2 bullet
    /// asks for: composing `call_edges` -> `scc_graph` -> `solve_scc` per
    /// component, in dependency order, must equal a single `infer_project`
    /// call over the exact same inputs. Uses the mutual-recursion fixture
    /// (a real multi-round SCC fixpoint, not just a linear chain) so the
    /// composed path actually exercises cross-SCC signature threading.
    #[test]
    fn composed_per_scc_solve_equals_monolithic_infer_project() {
        let src = "=== function ping(n) ===\n{n == 0:\n  ~ return 0.0\n}\n~ return pong(n - 1)\n\
                   === function pong(n) ===\n~ return ping(n)\n\
                   === caller ===\n~ temp x = ping(3)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let files = [(FileId(0), &hir)];

        let monolithic = infer_project(&files, &index, &res, None, &BTreeMap::new());

        // Compose: call_edges per def -> merged CallGraph -> scc_graph ->
        // solve_scc per component, threading known_sigs in dependency order
        // exactly like `solve_batches` does internally. Every per-def input
        // (defs, globals, inferable) is built the same narrowed way
        // `brink-db`'s query wiring builds it (FG-2.1, issue #638), not by
        // handing the whole-project HIR straight to `collect_defs`/
        // `collect_globals` the way the pre-#638 version of this test did.
        let defs = inferable_defs_from_index(&index);
        let mut graph = CallGraph::new();
        for &def in &defs {
            graph.add_node(def);
            for callee in call_edges(def, &files, &index, &res, &defs, None) {
                graph.add_edge(def, callee);
            }
        }
        let sg = scc_graph(&graph);

        let mut known_sigs: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
        let mut signatures: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
        let mut bodies: BTreeMap<DefinitionId, BodyTypes> = BTreeMap::new();
        for batch in &sg.order {
            // Per-def HIR projection (Ruling 2b): only this batch's own
            // members' bodies, never the whole project's.
            let owned: Vec<(DefinitionId, Vec<Param>, Option<TypeExpr>, Block)> = batch
                .iter()
                .filter_map(|&id| def_body(id, &files, &index).map(|(p, ra, b)| (id, p, ra, b)))
                .collect();
            let batch_defs: Vec<Def<'_>> = owned
                .iter()
                .map(|(id, params, return_annotation, body)| Def {
                    id: *id,
                    file: FileId(0),
                    params,
                    body,
                    return_annotation: return_annotation.as_ref(),
                })
                .collect();

            // Pre-scan + narrow map (Ruling 1): union of every member's
            // referenced_globals, resolved through signature() — never
            // collect_globals's whole-project scan.
            let mut global_ids: BTreeSet<DefinitionId> = BTreeSet::new();
            for &id in batch {
                global_ids.extend(referenced_globals(id, &files, &index, &res, None));
            }
            let mut globals: BTreeMap<DefinitionId, Ty> = BTreeMap::new();
            for gid in global_ids {
                if let Some(sig) = crate::signature::signature(gid, &index, &files, None)
                    && let Some(vt) = sig.value_type
                {
                    globals.insert(gid, Ty::from(vt));
                }
            }

            let (sigs, bods) = solve_scc(
                batch,
                &batch_defs,
                &index,
                &res,
                &globals,
                &defs,
                known_sigs.clone(),
                None,
                &BTreeMap::new(),
            );
            known_sigs.extend(sigs.iter().map(|(k, v)| (*k, v.clone())));
            signatures.extend(sigs);
            bodies.extend(bods);
        }
        let composed = InferenceResult { signatures, bodies };

        assert_eq!(
            composed, monolithic,
            "per-SCC composed inference must equal a single infer_project call"
        );
    }
}
