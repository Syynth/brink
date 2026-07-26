//! Per-definition body-uses inference (typed-mode-spec §2): "parameter types
//! are inferred from uses inside the body".
//!
//! [`infer_def_body`] walks one knot/stitch body and returns the types its
//! params, temps, and return value were observed to have, given the
//! **already-known** signatures of every def it calls ([`BodyCtx::known_sigs`]
//! — the firewall: this module never reads another def's body, only its
//! signature). Calling this repeatedly as callee signatures refine (the SCC
//! fixpoint driven by [`super::infer_project`]) is what makes mutual
//! recursion resolve: each round feeds the previous round's signatures back
//! in as `known_sigs` until the round-over-round result stops changing.
//!
//! Condition positions (choice conditions, `{cond: ...}` branches, `if`/
//! `while`) call [`InferPass::infer_expr`] like any other expression and
//! never force the result to `bool` — this is the int-truthiness modeling
//! spec §4 requires "from day one": a condition typed `int` here is not an
//! error, just the observed type, exactly the idiom
//! `{visited_knot: ...}` relies on. TM-3 strict mode is the slice that
//! turns "not bool and not int" in this position into a diagnostic; TM-1
//! only has to make sure today's inference doesn't quietly assume `bool`
//! and poison that future check.

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart, DivertPath,
    DivertTarget, ElseBranch, Expr, IfStmt, InfixOp, LogicBlock, Path as HirPath, PrefixOp, Stmt,
    StringPart, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use super::ty::{TowerTy, Ty, coalesce, unify, unify_all};
use super::{InferredSig, ValueCallFact, ValueCallKind, range_key};

/// Read-only context shared by every body inferred in the same SCC round.
pub(super) struct BodyCtx<'a> {
    /// This file's resolved references, keyed by [`range_key`] of the
    /// reference's own `TextRange` — a `Path`'s range in
    /// `Expr::Path`/`DivertTarget`/`ListLiteral` items always matches one of
    /// these keys when resolvable. `TextRange` itself has no `Ord` impl, so
    /// the key is the `(start, end)` `u32` pair.
    pub resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    /// The project-wide symbol index (locals included) — used to turn a
    /// resolved `DefinitionId` back into a bare name + kind.
    pub index: &'a SymbolIndex,
    /// Declaration-derived types for globals (VAR/CONST/LIST/LIST item),
    /// read from `signature()` (spec: the firewall — this is `signature(B)`,
    /// never `infer_body(B)`, for every def outside this body).
    pub globals: &'a BTreeMap<DefinitionId, Ty>,
    /// The current best-known signature of every callable def, cross-SCC
    /// (finalized) or in-round (this SCC's current fixpoint estimate).
    pub known_sigs: &'a BTreeMap<DefinitionId, InferredSig>,
    /// Defs with an inferable body (knots/stitches) — used to decide whether
    /// a resolved call target is worth recording as a call-graph edge.
    pub inferable: &'a BTreeSet<DefinitionId>,
    /// Declared `LIST`/`STRUCT` names (from `ProjectCtx`) — needed to
    /// resolve param/return/temp annotations for the T1c firewall overlay
    /// and the call-position annotated-callee fallback.
    pub list_names: &'a BTreeSet<String>,
    pub struct_names: &'a BTreeSet<String>,
    /// Declared handle-kind names from the registered `HostManifest`
    /// (T1d-2b, issue #774, docs/t1d-spec.md §3) — the manifest mirror of
    /// `list_names`/`struct_names`'s ink-source-declared vocabularies.
    /// Threaded from `ProjectCtx::new`'s `manifest` parameter through
    /// `infer_project`/`solve_scc`/`call_edges`/`referenced_globals`; empty
    /// when no manifest is registered, same degrade posture as every other
    /// manifest-driven check.
    pub handle_names: &'a BTreeSet<String>,
}

impl BodyCtx<'_> {
    /// A [`crate::annotations::TypeNames`] bundle for this ctx's
    /// annotation-resolution call sites — `handles` now carries the real
    /// registered handle-kind vocabulary (T1d-2b, issue #774), so a
    /// `handle<K>` annotation on a param/return/temp resolves to
    /// `Ty::Handle(K)` during body inference whenever `K` is declared in the
    /// registered manifest, exactly like `list<L>`/`STRUCT` already do
    /// against their own declared-name sets.
    fn type_names(&self) -> crate::annotations::TypeNames {
        crate::annotations::TypeNames {
            lists: self.list_names.clone(),
            structs: self.struct_names.clone(),
            handles: self.handle_names.clone(),
        }
    }
}

/// The result of walking one definition's body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the NS-A2 effect-dimension flags plus the pre-existing walk \
              facts are independent harvested facts, not a state machine"
)]
pub(super) struct BodyResult {
    /// Declared params, in declaration order, with their observed type.
    pub params: Vec<(String, Ty)>,
    /// Every local (params ∪ temps) by bare name — `params` is the ordered
    /// subset callers care about for a `Sig`-shaped view; `locals` is the
    /// full picture for hover/diagnostics consumers (TM-5).
    pub locals: BTreeMap<String, Ty>,
    pub return_ty: Ty,
    /// Issue #1028: whether the body contains at least one `return <expr>`
    /// (a value-carrying return) anywhere — a bare `return`/`return ->
    /// onwards` never sets this, and neither does falling off the end of the
    /// body with no `return` at all. `false` is the "this function never
    /// produces a value" signal strict mode's void inference (spec §3's
    /// `void` annotation, applied without requiring the annotation) reads
    /// instead of trusting `return_ty.is_unknown()` alone — `return_ty` stays
    /// `Unknown` in *both* the "genuinely never returns a value" case and the
    /// "returns a value whose type inference couldn't pin down" case, and
    /// those two need different strict-mode treatment (the former is clean
    /// void; the latter is a real Unknown-escape).
    pub has_value_return: bool,
    /// Resolved call targets encountered (only those in `inferable`) — feeds
    /// call-graph construction on the first pass; harmless to recompute on
    /// later passes.
    pub calls: BTreeSet<DefinitionId>,
    /// Every VAR/CONST `DefinitionId` this body looked up, recorded in
    /// `ty_of_def` regardless of whether `ctx.globals` actually had an entry
    /// for it (FG-2.1, issue #638 — the `referenced_globals(def)` body-facts
    /// projection, same family as `calls`). This is the exact reference set
    /// a lazy globals resolver would consult, pre-scanned so `brink-db` can
    /// build a narrow `BTreeMap` before the real solve walk runs (spec
    /// Ruling 1) — also the per-def global *read set* future T2 effect rows
    /// need.
    pub referenced_globals: BTreeSet<DefinitionId>,
    /// T2-1 (docs/effects-spec.md §2/§4, issue #860): the effect-atom
    /// *write* set — VAR/CONST globals this body assigns to. The read set is
    /// [`Self::referenced_globals`] (spec §4 names it exactly that); the
    /// call-kind set is [`Self::external_calls`]; the direct-callee edges the
    /// effect fixpoint follows are [`Self::calls`]. Harvested by the same walk
    /// that drives type inference — zero extra passes.
    pub effect_writes: BTreeSet<DefinitionId>,
    /// T2-1: the effect-atom *call-kind* set — the `EXTERNAL` binding names
    /// this body directly calls (spec §2's `call external-kind` atoms).
    pub external_calls: BTreeSet<String>,
    /// T2-1: the body performed a call *through a function value* whose
    /// origin couldn't be narrowed (or another construct whose callee
    /// effects inference cannot summarize), so its effect row is pessimal
    /// (docs/effects-spec.md §3/§4 — the conservative-total floor). Issue
    /// #872 (§8 precision rung) reads a concrete origin back off a
    /// write-once local's stored `Ty::Fn` when one is statically known —
    /// see `InferPass::resolve_pending_value_calls` — so this only stays
    /// `true` when no such origin could be proven; the heap (VAR/CONST
    /// cells) case remains coarse/pessimal, still unaddressed.
    pub effect_opaque: bool,
    /// NS-A2 (issue #1108): the body directly contains a content-producing
    /// construct — see `EffectAtoms::emits`.
    pub effect_emits: bool,
    /// NS-A2: the body directly touches the tag channel — see
    /// `EffectAtoms::tags`.
    pub effect_tags: bool,
    /// NS-A2: the body directly contains a construct that can raise a
    /// turn-terminating fault — see `EffectAtoms::faults`.
    pub effect_faults: bool,
    /// NS-A4 / F29(a): the refined faults bit — the same charge sites with
    /// local type-evidence discharges applied (see
    /// `EffectRow::faults_refined`). `effect_faults_refined →
    /// effect_faults`.
    pub effect_faults_refined: bool,
    /// T1c: statically-checkable call-through-value facts (see
    /// [`super::ValueCallFact`]) — recorded here because this walk is the
    /// only place argument expressions have types; reported by strict mode
    /// only.
    pub value_calls: Vec<ValueCallFact>,
}

/// Infer one definition's body against `ctx`. `def.params` are the declared
/// params in order (a knot's or stitch's `params`); `def.body` is its
/// `Block`. `params` in the result echoes the declaration order.
///
/// ## The T1c annotation-firewall overlay
///
/// After the body walk, a param or return slot whose *body-derived* type is
/// still `Unknown` overlays to its resolvable annotation type (TM-2's
/// "annotation wins over nothing" firewall applied to the signature —
/// docs/t1c-spec.md §4: `#fn` consumes "the target's (inferred or
/// annotated) signature", which requires annotated-but-body-unconstrained
/// slots to surface concretely). Deliberately overlay-not-seed: a body use
/// that *disagrees* with the annotation still infers its own concrete type
/// so `annotations::mismatches` (E063) keeps comparing two independent
/// derivations, and a genuinely conflicted body still comes out
/// `Conflicted` (E066) — the #627 lattice is untouched.
pub(super) fn infer_def_body(def: &super::Def<'_>, ctx: &BodyCtx<'_>) -> BodyResult {
    let names = ctx.type_names();
    let annotated: BTreeMap<String, Ty> = def
        .params
        .iter()
        .filter_map(|p| {
            let te = p.annotation.as_ref()?;
            let ty = crate::annotations::resolve(te, &names)?;
            Some((p.name.text.clone(), ty))
        })
        .collect();

    let mut pass = InferPass {
        ctx,
        locals: BTreeMap::new(),
        return_ty: Ty::Unknown,
        has_value_return: false,
        calls: BTreeSet::new(),
        referenced_globals: BTreeSet::new(),
        effect_writes: BTreeSet::new(),
        external_calls: BTreeSet::new(),
        effect_opaque: false,
        effect_emits: false,
        effect_tags: false,
        effect_faults: false,
        effect_faults_refined: false,
        annotated,
        value_calls: Vec::new(),
        local_write_counts: BTreeMap::new(),
        local_fn_origin: BTreeMap::new(),
        pending_value_calls: Vec::new(),
    };
    // NS-A2 fault dimension (issue #1108, from #1097): a `ref` parameter's
    // dereference inside this body goes through a pointer/projection whose
    // resolution can raise `ProjectionInvalidated` (docs/t1e-spec.md §1(2))
    // — a designed turn-terminating fault charged to the *callee* (the deref
    // site), so a def declaring any `ref` param conservatively faults.
    if def.params.iter().any(|p| p.is_ref) {
        pass.effect_faults = true;
        // F29: no discharge — the deref fault is value-dependent.
        pass.effect_faults_refined = true;
    }
    pass.infer_block(def.body);
    // T2 §8 precision rung (issue #872): resolve every value-call site now
    // that the whole body's write counts are final — see
    // `resolve_pending_value_calls`'s doc for why this must run post-walk.
    pass.resolve_pending_value_calls();

    let param_types = def
        .params
        .iter()
        .map(|p| {
            let inferred = pass
                .locals
                .get(&p.name.text)
                .cloned()
                .unwrap_or(Ty::Unknown);
            let ty = if inferred.is_unknown() {
                pass.annotated
                    .get(&p.name.text)
                    .cloned()
                    .unwrap_or(inferred)
            } else {
                inferred
            };
            (p.name.text.clone(), ty)
        })
        .collect();

    let return_ty = if pass.return_ty.is_unknown() {
        def.return_annotation
            .and_then(|te| crate::annotations::resolve(te, &names))
            .unwrap_or(pass.return_ty)
    } else {
        pass.return_ty
    };

    BodyResult {
        params: param_types,
        locals: pass.locals,
        return_ty,
        has_value_return: pass.has_value_return,
        calls: pass.calls,
        referenced_globals: pass.referenced_globals,
        effect_writes: pass.effect_writes,
        external_calls: pass.external_calls,
        effect_opaque: pass.effect_opaque,
        effect_emits: pass.effect_emits,
        effect_tags: pass.effect_tags,
        effect_faults: pass.effect_faults,
        effect_faults_refined: pass.effect_faults_refined,
        value_calls: pass.value_calls,
    }
}

/// Unwrap a write-target expression down to its root path — an explicit
/// `ref` sigil (T1e, docs/t1e-spec.md §2), a dotted field-access chain, and
/// an index expression all peel away to the same root `Expr` a plain
/// unmarked `ref` argument already is, mirroring `brink_ir::lir::lower::
/// expr`'s `decompose_projection` walk. Two call sites rely on this same
/// unwrapping: a `ref` call-site argument (`record_ref_param_writes`) and a
/// plain assignment target (`record_write`) — `~ arr[i] = v` and
/// `~ memo[k] = v` write through the *root* container's cell exactly like
/// `writeback_lvalue_container_chain`'s codegen does (issue #880's audit of
/// the #856 map-insert-on-assign path: the same silent-drop class as the
/// ref-param write bug, just on the plain-assignment path instead of the
/// call-site path).
fn ref_arg_root(expr: &Expr) -> &Expr {
    match expr {
        Expr::RefArg(ra) => ref_arg_root(&ra.operand),
        Expr::FieldAccess(fa) => ref_arg_root(&fa.base),
        Expr::Index(idx) => ref_arg_root(&idx.base),
        other => other,
    }
}

/// Fold a range-literal bound to its compile-time int value, LITERALS ONLY
/// (NS-A5): an integer literal or a unary-negated integer literal. This is
/// deliberately narrower than the strict checker's own fold (which also
/// resolves CONST refs — see `crate::range_refinement`): inference is
/// firewalled from other definitions' HIR, so CONST-bounded literals mint
/// their evidence at the strict checker instead. `i64` so `-2147483648`
/// folds without edge cases.
fn fold_literal_int_bound(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(n) => Some(i64::from(*n)),
        Expr::Prefix(brink_ir::PrefixOp::Negate, inner) => {
            fold_literal_int_bound(inner).map(|n| -n)
        }
        _ => None,
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "accumulator for the same independent facts BodyResult carries"
)]
struct InferPass<'a, 'b> {
    ctx: &'a BodyCtx<'b>,
    locals: BTreeMap<String, Ty>,
    return_ty: Ty,
    /// See [`BodyResult::has_value_return`] — set the moment any
    /// `return <expr>` is walked, anywhere in the body.
    has_value_return: bool,
    calls: BTreeSet<DefinitionId>,
    referenced_globals: BTreeSet<DefinitionId>,
    effect_writes: BTreeSet<DefinitionId>,
    external_calls: BTreeSet<String>,
    effect_opaque: bool,
    /// NS-A2 output dimensions (issue #1108): direct content emission /
    /// tag-channel touch harvested from this body's weave statements
    /// (`infer_content`/`infer_choice`), and the direct fault sources
    /// (indexing, `/`‐`mod`, faulting intrinsics, value calls, `ref`
    /// params, `for` iteration) — see `EffectAtoms`'s field docs.
    effect_emits: bool,
    effect_tags: bool,
    effect_faults: bool,
    /// NS-A4 / F29(a): the refined faults bit (see `BodyResult`'s field).
    effect_faults_refined: bool,
    /// Resolvable annotation/ascription types by local name — params up
    /// front, temps added as their declarations are walked. Consulted only
    /// as a *fallback* at consumption sites where the body-derived type is
    /// still `Unknown` (call-position callee lookup, the end-of-walk
    /// signature overlay) — never joined into the lattice, so E063's
    /// two-independent-derivations comparison stays intact.
    annotated: BTreeMap<String, Ty>,
    value_calls: Vec<ValueCallFact>,
    /// T2 §8 precision rung (docs/effects-spec.md §6 item 3/§8, issue #872): every
    /// write (`TempDecl` initializer or bare-`Path` `Assignment`) to a
    /// Temp local, by name, counted regardless of whether the write's
    /// value traces to a known `#fn` origin. Param writes are deliberately
    /// not counted here for narrowing purposes (see
    /// [`InferPass::local_call_origin`]'s doc) — a Param carries an implicit
    /// caller-provided initial value no write count can ever see. Consulted only after the whole
    /// body is walked ([`InferPass::resolve_pending_value_calls`]) — a
    /// single-pass "as accumulated so far" read would miss a reassignment
    /// that appears later in program order but, inside a loop body, executes
    /// *before* an earlier-positioned call on the next iteration: reading a
    /// partial count there would risk narrowing a call whose live value
    /// could actually vary, an unsound under-report. Whole-body-final counts
    /// close that hole: a name is trusted only when it was written *exactly
    /// once*, period, so loop-carried reassignment always poisons it.
    local_write_counts: BTreeMap<String, u32>,
    /// The `#fn(target, …)`/`bind(…)`-chain origin recorded at each local's
    /// write (last-write-wins bookkeeping only) — meaningful only once
    /// `local_write_counts` confirms that name had exactly one write.
    local_fn_origin: BTreeMap<String, Option<DefinitionId>>,
    /// Every value-call site's narrowing candidate, recorded during the walk
    /// and resolved once by [`InferPass::resolve_pending_value_calls`] after
    /// the whole body (and therefore `local_write_counts`) is final.
    pending_value_calls: Vec<ValueCallOrigin>,
}

/// T2 §8 precision rung (docs/effects-spec.md §6 item 3/§8, issue #872): one
/// value-call site's callee, classified for effect-row narrowing purposes.
/// The optimizer-not-gatekeeper doctrine (spec §8) means every arm here is
/// free to fall back to `Unknown` — narrowing only ever *removes* the
/// pessimal floor when it holds a positive, whole-body-checked proof of a
/// single origin; it never manufactures a new failure mode.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ValueCallOrigin {
    /// The callee is a bare Temp local reference (never a Param — see
    /// [`InferPass::local_call_origin`]'s doc for why a Param can't be
    /// trusted here). Resolved post-walk: narrowable only if `name` was
    /// written exactly once in the whole body and that write's value traced
    /// to a single known def.
    Local(String),
    /// The callee is *itself* (optionally through one or more `bind(…)`
    /// wrappers) an `#fn(target, …)` literal evaluated fresh at this call
    /// site — no stored value, so no write-count question applies; always
    /// narrowable.
    Inline(DefinitionId),
    /// Anything else (a VAR/CONST global — the heap case, still coarse per
    /// spec §5/§8 — an index, a direct call's return value, a param with no
    /// further trace, …): the pessimal floor, unchanged from today.
    Unknown,
}

impl InferPass<'_, '_> {
    // ── Definitions / resolution ──────────────────────────────────────

    fn resolve(&self, range: TextRange) -> Option<DefinitionId> {
        self.ctx.resolution_by_range.get(&range_key(range)).copied()
    }

    fn ty_of_def(&mut self, def: DefinitionId) -> Ty {
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return Ty::Unknown;
        };
        match info.kind {
            SymbolKind::Param | SymbolKind::Temp => {
                self.locals.get(&info.name).cloned().unwrap_or(Ty::Unknown)
            }
            SymbolKind::Variable | SymbolKind::Constant => {
                // Record the reference regardless of whether `ctx.globals`
                // has an entry for it — a pre-scan call (empty/placeholder
                // globals map) still needs to discover *which* ids get
                // looked up (FG-2.1, issue #638, Ruling 1).
                self.referenced_globals.insert(def);
                self.ctx.globals.get(&def).cloned().unwrap_or(Ty::Unknown)
            }
            SymbolKind::List => Ty::List(info.name.clone()),
            SymbolKind::ListItem => info
                .name
                .split_once('.')
                .map_or(Ty::Unknown, |(list, _)| Ty::List(list.to_string())),
            // Knot/Stitch/External/Label referenced as a bare value: no
            // function-value type exists in this slice (T1c fence).
            SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::External | SymbolKind::Label => {
                Ty::Unknown
            }
            // A bare `Expr::Path` never resolves to a struct shape name
            // (TM-4b's `RefKind::Struct` is a disjoint resolution pass from
            // the `RefKind::Variable` one `ty_of_def` serves) — kept for
            // match exhaustiveness only.
            SymbolKind::Struct => Ty::Unknown,
        }
    }

    /// Join `ty` into the accumulated type of `expr`'s target local, if
    /// `expr` is a bare `Path` resolving to a `Param`/`Temp` in this body.
    /// A no-op for any other expression shape or an unresolved/global path
    /// — this is how a use elsewhere in the body ("uses inside the body")
    /// narrows a param/temp's inferred type.
    ///
    /// Issue #994: a *dotted* `Path` (`t.field`, `t.field.nested`) whose head
    /// resolves to a `Param`/`Temp` reaches here too — the TM-4b resolution
    /// fallback (`resolve::resolve_variable` step 11, docs/typed-mode-spec.md
    /// §6) maps the whole multi-segment path's range to the *head*
    /// variable's `DefinitionId`, since no static field-type table exists
    /// yet (`Expr::FieldAccess`'s own doc: "out of scope for this slice").
    /// Joining the field-context type (`ty`, the RHS of `t.field = 5` or a
    /// sibling operand's type in `t.field + 1`) into the *head*'s own
    /// accumulated type conflates two unrelated types — `t`'s own shape and
    /// one of its fields' — and manufactures a spurious Conflicted-escape
    /// (`E066`) whenever they statically disagree, even though `t` itself is
    /// never actually misused. A dotted head resolving to a global
    /// `VAR`/`CONST` never reaches this join at all (excluded by the
    /// `kind` guard below, since cross-type-reassignment detection for
    /// globals isn't implemented in this slice) — this segment-count guard
    /// makes a `Param`/`Temp` head behave the same way: observed only for a
    /// genuine bare reference, never for a dotted field read.
    fn observe(&mut self, expr: &Expr, ty: &Ty) {
        if ty.is_unknown() {
            return;
        }
        let Expr::Path(p) = expr else { return };
        if p.segments.len() > 1 {
            return;
        }
        let Some(def) = self.resolve(p.range) else {
            return;
        };
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return;
        };
        if !matches!(info.kind, SymbolKind::Param | SymbolKind::Temp) {
            return;
        }
        let name = info.name.clone();
        let cur = self.locals.get(&name).cloned().unwrap_or(Ty::Unknown);
        self.locals.insert(name, unify(&cur, ty));
    }

    /// Record a `~ temp name: T = …` ascription in the annotated-fallback
    /// map (same role as a param annotation — see the field's doc).
    fn register_ascription(&mut self, t: &brink_ir::TempDecl) {
        if let Some(te) = &t.annotation
            && let Some(ty) = crate::annotations::resolve(te, &self.ctx.type_names())
        {
            self.annotated.insert(t.name.text.clone(), ty);
        }
    }

    fn bind_local(&mut self, name: &str, ty: &Ty) {
        let cur = self.locals.get(name).cloned().unwrap_or(Ty::Unknown);
        self.locals.insert(name.to_string(), unify(&cur, ty));
    }

    fn record_call_edge(&mut self, def: DefinitionId) {
        if self.ctx.inferable.contains(&def) {
            self.calls.insert(def);
        } else if let Some(info) = self.ctx.index.symbols.get(&def)
            && info.kind == SymbolKind::External
        {
            // T2-1 (docs/effects-spec.md §2): a direct call/divert to an
            // `EXTERNAL` binding is a call-kind atom. Externals have no
            // inferable body of their own, so they never enter `calls` (the
            // fixpoint-edge set) — the binding *name* is the terminal kind the
            // host acts on.
            self.external_calls.insert(info.name.clone());
        }
    }

    // ── T2 §8 precision rung (docs/effects-spec.md §6 item 3/§8, issue #872) ──

    /// The single def a `#fn(target, …)` literal (optionally through one or
    /// more `bind(…)` wrappers — partial application never changes *which*
    /// def eventually runs) traces to, if `expr` is exactly that shape.
    /// `bind`'s intrinsic form is matched the same way `infer_intrinsic`
    /// classifies it: an unresolved single-segment path named `bind` with a
    /// non-empty argument list (a real `bind`-named def always wins
    /// resolution first, same shadow-fallback precedent as every other
    /// intrinsic dispatch in this module).
    ///
    /// Deliberately gated on [`Self::is_effect_edge_target`], **not**
    /// `ctx.known_sigs`: `def_effect_atoms` (this pass's caller, for the
    /// effects fixpoint specifically) always runs with an *empty*
    /// `known_sigs` — effects inference is advisory and doesn't thread the
    /// type-inference SCC fixpoint's signatures through (see
    /// `def_effect_atoms`'s doc) — so a `known_sigs` check here would never
    /// once succeed and this whole rung would be dead code. `inferable` and
    /// the symbol index, unlike `known_sigs`, are always the real
    /// project-wide sets in this context.
    fn fn_literal_write_origin(&self, expr: &Expr) -> Option<DefinitionId> {
        match expr {
            Expr::FnLiteral(fl) => {
                let def = self.resolve(fl.target.range)?;
                self.is_effect_edge_target(def).then_some(def)
            }
            Expr::Call(path, args)
                if path.segments.len() == 1
                    && path.segments[0].text == "bind"
                    && self.resolve(path.range).is_none()
                    && !args.is_empty() =>
            {
                self.fn_literal_write_origin(&args[0])
            }
            _ => None,
        }
    }

    /// Whether `def` is a legitimate effect-fixpoint edge target — an
    /// inferable knot/stitch (joins via the SCC fixpoint) or an `EXTERNAL`
    /// binding (a call-kind atom) — exactly the two cases
    /// [`Self::record_call_edge`] does something with. Anything else (an
    /// unresolvable/stray id) must **not** be treated as a known origin: a
    /// narrowed call resolves through `record_call_edge`, which would
    /// silently no-op for a target neither branch recognizes, dropping the
    /// call atom entirely with no `effect_opaque` fallback — an under-report.
    /// This gate is what keeps that path unreachable.
    fn is_effect_edge_target(&self, def: DefinitionId) -> bool {
        self.ctx.inferable.contains(&def)
            || self
                .ctx
                .index
                .symbols
                .get(&def)
                .is_some_and(|info| info.kind == SymbolKind::External)
    }

    /// Classify a resolved call-through-value target for narrowing: a bare
    /// Temp local (checked post-walk against its whole-body write count) is
    /// [`ValueCallOrigin::Local`]; anything else (a VAR/CONST global — the
    /// heap case, still coarse per spec §5/§8 — a Param, or an
    /// unresolvable/non-local kind) is [`ValueCallOrigin::Unknown`].
    ///
    /// **Param is deliberately excluded** (soundness, not a missed
    /// optimization): a Temp cannot be referenced before its defining
    /// `TempDecl`, so "written exactly once in the whole body" really does
    /// bound its value at every read. A Param, by contrast, carries an
    /// implicit caller-provided initial value that
    /// [`Self::local_write_counts`] never counts — a param reassigned
    /// exactly once inside the body would reach `write_count == 1` and
    /// [`Self::resolve_pending_value_calls`] would narrow *every* call site
    /// through it, including ones reachable before that single
    /// reassignment, where the param still holds the caller's arbitrary
    /// (unknown) fn value. That would violate the conservative-total
    /// invariant this rung promises to preserve.
    fn local_call_origin(&self, def: DefinitionId) -> ValueCallOrigin {
        match self.ctx.index.symbols.get(&def) {
            Some(info) if info.kind == SymbolKind::Temp => {
                ValueCallOrigin::Local(info.name.clone())
            }
            _ => ValueCallOrigin::Unknown,
        }
    }

    /// Classify an arbitrary callee *expression* (the `call(f, …)`/
    /// `bind(f, …)` intrinsic forms' `f`, an arbitrary expression rather
    /// than a resolvable `Path`) for narrowing: an inline `#fn`/`bind`-chain
    /// literal is [`ValueCallOrigin::Inline`] (trusted immediately — no
    /// stored value); a bare local reference (Param or Temp) defers to
    /// [`Self::local_call_origin`], which only ever narrows the Temp case;
    /// anything else is [`ValueCallOrigin::Unknown`].
    fn value_call_origin(&self, expr: &Expr) -> ValueCallOrigin {
        if let Some(def) = self.fn_literal_write_origin(expr) {
            return ValueCallOrigin::Inline(def);
        }
        if let Expr::Path(p) = expr
            && p.segments.len() == 1
            && let Some(def) = self.resolve(p.range)
        {
            return self.local_call_origin(def);
        }
        ValueCallOrigin::Unknown
    }

    /// Record one write to a Param/Temp local for later write-count-gated
    /// narrowing — a no-op for any other resolution (mirrors [`Self::observe`]'s
    /// own guard: a VAR/CONST/other target isn't tracked by this local-only
    /// rung at all, the heap case stays pessimal per spec §5/§8).
    fn bump_local_write(&mut self, name: &str, origin: Option<DefinitionId>) {
        *self.local_write_counts.entry(name.to_string()).or_insert(0) += 1;
        self.local_fn_origin.insert(name.to_string(), origin);
    }

    /// [`Self::bump_local_write`] for an `Assignment`/`BlockStmt::Assignment`
    /// target expression — only a bare single-segment `Path` resolving to a
    /// Temp counts (a dotted/indexed target reassigns a *nested* slot, not
    /// the local's own value, same guard [`Self::observe`] applies).
    ///
    /// Param is excluded here too — not for soundness (bumping a count only
    /// ever makes [`Self::local_call_origin`]'s write-once check *stricter*,
    /// since Param is never classified as [`ValueCallOrigin::Local`] in the
    /// first place, so this can't under-report), but to avoid spuriously
    /// pessimizing a same-named Temp elsewhere in the body: without this
    /// guard, a Param write would bump `local_write_counts` under that
    /// param's name, and if a Temp happens to share the name the two
    /// tallies would collide in the same map entry.
    fn record_fn_write(&mut self, target: &Expr, origin: Option<DefinitionId>) {
        let Expr::Path(p) = target else { return };
        if p.segments.len() != 1 {
            return;
        }
        let Some(def) = self.resolve(p.range) else {
            return;
        };
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return;
        };
        if info.kind != SymbolKind::Temp {
            return;
        }
        let name = info.name.clone();
        self.bump_local_write(&name, origin);
    }

    /// Resolve every value-call site recorded during the walk
    /// ([`Self::check_value_call`]'s `pending_value_calls.push`) against the
    /// now-final `local_write_counts`/`local_fn_origin` — **must** run after
    /// [`InferPass::infer_block`] finishes the whole body (see
    /// `local_write_counts`'s field doc for the loop-carried-reassignment
    /// hazard a mid-walk read would risk). `Inline` narrows unconditionally;
    /// `Local` narrows only if its name was written exactly once and that
    /// write traced to a known def; `Unknown` (and any `Local` that fails
    /// the write-once check) keeps the pessimal floor — `effect_opaque =
    /// true`, exactly what every value call unconditionally set before this
    /// rung existed. A narrowed call routes through
    /// [`Self::record_call_edge`], the same edge a direct call/`#fn` creation
    /// site records, so it correctly lands in `self.calls` (inferable
    /// knot/stitch — joins via the SCC effect fixpoint) or
    /// `self.external_calls` (an `EXTERNAL` binding — a call-kind atom) per
    /// what the origin actually is.
    fn resolve_pending_value_calls(&mut self) {
        let pending = std::mem::take(&mut self.pending_value_calls);
        for origin in pending {
            let narrowed = match origin {
                ValueCallOrigin::Inline(def) => Some(def),
                ValueCallOrigin::Local(name) => {
                    if self.local_write_counts.get(&name).copied() == Some(1) {
                        self.local_fn_origin.get(&name).copied().flatten()
                    } else {
                        None
                    }
                }
                ValueCallOrigin::Unknown => None,
            };
            match narrowed {
                Some(def) => self.record_call_edge(def),
                None => self.effect_opaque = true,
            }
        }
    }

    /// T2-1: record an effect *write* atom (docs/effects-spec.md §2) — an
    /// assignment target (`~ x = …`) resolving to a VAR/CONST global. `target`
    /// is unwrapped to its root via `ref_arg_root` first (issue #880's
    /// audit): `~ arr[i] = v` and `~ memo[newKey] = v` (the #856
    /// insert-on-assign path) write through the *container's* cell, not
    /// through some cell named by the index expression itself, exactly like
    /// `writeback_lvalue_container_chain`'s codegen — recording only bare
    /// `Expr::Path` targets silently dropped every indexed-assignment write
    /// to a global (the same silent-drop shape as the ref-param write bug,
    /// #866). A no-op for a temp/param root (flow-private, spec §2) or any
    /// non-`Path` root. Over-reporting the same id as a read too (the
    /// existing `~ x = x + 1` walk records it in `referenced_globals`) is
    /// conservatively sound — the row never under-reports (spec §3).
    fn record_write(&mut self, target: &Expr) {
        let Expr::Path(p) = ref_arg_root(target) else {
            return;
        };
        let Some(def) = self.resolve(p.range) else {
            return;
        };
        if let Some(info) = self.ctx.index.symbols.get(&def)
            && matches!(info.kind, SymbolKind::Variable | SymbolKind::Constant)
        {
            self.effect_writes.insert(def);
        }
    }

    /// T2-1 fix (review finding on issue #860's PR): a direct call/divert
    /// passing an argument into a `ref` parameter slot writes through that
    /// parameter (docs/effects-spec.md §5 "through parameters") — the
    /// callee mutates the *caller's* cell, not a private copy. `record_write`
    /// alone only sees the callee's own body (where the target is a `Param`,
    /// never a `Variable`/`Constant`), so it can never discover this; the
    /// write has to be recorded here, at the call site, against whichever
    /// global the caller actually passed in. `def` is the resolved call
    /// target (knot/stitch/`EXTERNAL`) whose declared `params` (from the
    /// symbol index, not `known_sigs` — `InferredSig` carries no `is_ref`
    /// bit) says which positions are `ref`; `args` are the call's own
    /// argument expressions, matched positionally exactly like
    /// `lir::lower::expr::lower_call_args` matches them for codegen. An
    /// explicit `ref` sigil (T1e, docs/t1e-spec.md §2 — `ref npc.hp`, `ref
    /// inventory[idx]`) is unwrapped down to its root path first: mutating a
    /// projection still writes through the root global's own cell.
    fn record_ref_param_writes(&mut self, def: DefinitionId, args: &[Expr]) {
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return;
        };
        for (i, arg) in args.iter().enumerate() {
            if info.params.get(i).is_some_and(|p| p.is_ref) {
                let root = ref_arg_root(arg);
                self.record_write(root);
            }
        }
    }

    // ── Expressions ──────────────────────────────────────────────────

    fn infer_expr(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::Int(_) => Ty::Int,
            Expr::Float(_) => Ty::Float,
            Expr::Bool(_) => Ty::Bool,
            Expr::String(s) => {
                for part in &s.parts {
                    if let StringPart::Interpolation(e) = part {
                        // Interpolation accepts every type (spec §4:
                        // "display is universal, not a coercion") — infer
                        // for its side effects (nested calls/uses) only.
                        self.infer_expr(e);
                    }
                }
                Ty::String
            }
            Expr::Null => Ty::Unknown,
            Expr::Path(p) => self.infer_path(p),
            Expr::DivertTarget(_) => Ty::Divert,
            Expr::ListLiteral(items) => self.infer_list_literal(items),
            Expr::Prefix(PrefixOp::Negate, inner) | Expr::Postfix(inner, _) => {
                self.infer_expr(inner)
            }
            Expr::Prefix(PrefixOp::Not, inner) => {
                self.infer_expr(inner); // condition position — no forcing.
                Ty::Bool
            }
            Expr::Infix(lhs, op, rhs) => self.infer_infix(lhs, *op, rhs),
            Expr::Call(path, args) => self.infer_call(path, args),
            Expr::ArrayLiteral(a) => {
                let elems: Vec<Ty> = a.elements.iter().map(|e| self.infer_expr(e)).collect();
                Ty::Array(Box::new(unify_all(elems)))
            }
            Expr::MapLiteral(m) => {
                let mut keys = Vec::with_capacity(m.entries.len());
                let mut vals = Vec::with_capacity(m.entries.len());
                for (k, v) in &m.entries {
                    keys.push(self.infer_expr(k));
                    vals.push(self.infer_expr(v));
                }
                Ty::Map(Box::new(unify_all(keys)), Box::new(unify_all(vals)))
            }
            Expr::Index(idx) => {
                // NS-A2 fault dimension (issue #1108, from #1097): indexing
                // is a faulting construct — array OOB, map missing-key read,
                // not-indexable/invalid-index (docs/value-model-spec.md §11c).
                // F29: no discharge — OOB/missing-key are value-dependent.
                self.effect_faults = true;
                self.effect_faults_refined = true;
                let base = self.infer_expr(&idx.base);
                match base {
                    Ty::Array(elem) => {
                        self.infer_expr(&idx.index);
                        *elem
                    }
                    Ty::Map(k, v) => {
                        self.observe(&idx.index, &k);
                        self.infer_expr(&idx.index);
                        *v
                    }
                    _ => {
                        self.infer_expr(&idx.index);
                        Ty::Unknown
                    }
                }
            }
            // TM-4b (docs/typed-mode-spec.md §6). A construction literal's
            // nominal type is the shape name itself — resolved-vs-declared
            // validity is `brink-analyzer::structs`' construction-check job,
            // not inference's; field values are still visited for their
            // side effects (nested calls/uses), same as every other
            // aggregate literal above.
            Expr::StructLiteral(sl) => {
                for (_name, val) in &sl.fields {
                    self.infer_expr(val);
                }
                Ty::Struct(sl.shape.text.clone())
            }
            // Field-type propagation through a struct's declared shape is
            // out of scope for this slice (no static field-type table is
            // threaded through inference yet) — the base is still inferred
            // for its own escape-checking purposes.
            Expr::FieldAccess(fa) => {
                self.infer_expr(&fa.base);
                Ty::Unknown
            }
            // T1c `#fn(target, args…)` (docs/t1c-spec.md §4): the creation
            // consumes the bound prefix from the target's signature — the
            // value's type is `fn(remaining…): R`. The target's signature
            // arrives through `known_sigs` exactly like a direct call's
            // (firewall intact; the call edge recorded here is what orders
            // the target's SCC before this one). Bound args are observed
            // against the target's param row, same as direct-call args.
            Expr::FnLiteral(fl) => self.infer_fn_literal(fl),
            // T1e `ref lvalue-path` (docs/t1e-spec.md §2): no projection
            // type is modeled in this slice (T1e-1 ships grammar/HIR/
            // analyzer checks only, not runtime representation) — same
            // "out of scope, still visited for its own escape-checking
            // purposes" posture as `Expr::FieldAccess` just above.
            Expr::RefArg(ra) => {
                self.infer_expr(&ra.operand);
                Ty::Unknown
            }
            // NS-A5 range literals (docs/stdlib-spec.md §7, F7): both
            // bounds are ints (`observe` narrows int-typed slots used as
            // bounds; the runtime op faults on anything else — harvested
            // in the faults dimension). The `non_empty` refinement bit is
            // minted RIGHT HERE when the literal's bounds fold statically
            // and denote at least one element (`1..=6`, `5..=5`, `-2..0`):
            // the "provably-inhabited literals coerce in free" leg of the
            // F7 evidence rule. Bounds referencing CONSTs don't fold at
            // this layer (inference is firewalled from other defs' HIR) —
            // the strict-mode E117 checker (`range_refinement`) folds
            // CONST refs itself for literal-in-argument-position, so
            // `int(1..=SIDES)` still coerces free under strict.
            Expr::Range(r) => {
                self.effect_faults = true;
                self.observe(&r.start, &Ty::Int);
                self.observe(&r.end, &Ty::Int);
                let start_ty = self.infer_expr(&r.start);
                let end_ty = self.infer_expr(&r.end);
                // F29 discharge: int-typed bounds make construction total
                // (the only fault path is a non-int bound).
                if !(start_ty == Ty::Int && end_ty == Ty::Int) {
                    self.effect_faults_refined = true;
                }
                let non_empty = match (
                    fold_literal_int_bound(&r.start),
                    fold_literal_int_bound(&r.end),
                ) {
                    (Some(start), Some(end)) => {
                        if r.inclusive {
                            start <= end
                        } else {
                            start < end
                        }
                    }
                    _ => false,
                };
                Ty::Range { non_empty }
            }
        }
    }

    fn infer_path(&mut self, p: &HirPath) -> Ty {
        let Some(def) = self.resolve(p.range) else {
            // NS-A1 (`docs/stdlib-spec.md` §1.4): an unresolved bare `none`
            // is the Option absence literal — `Option[Unknown]`, its element
            // narrowed only by context (the join at its use site). A `none`
            // that resolved to a real user symbol took the branch below
            // instead, exactly like the stdlib call names in `infer_call`.
            if let [seg] = p.segments.as_slice()
                && seg.text == "none"
            {
                return Ty::Option(Box::new(Ty::Unknown));
            }
            return Ty::Unknown;
        };
        self.ty_of_def(def)
    }

    fn infer_list_literal(&self, items: &[HirPath]) -> Ty {
        for item in items {
            if let Some(def) = self.resolve(item.range)
                && let Some(info) = self.ctx.index.symbols.get(&def)
                && info.kind == SymbolKind::ListItem
                && let Some((list, _)) = info.name.split_once('.')
            {
                return Ty::List(list.to_string());
            }
        }
        Ty::Unknown
    }

    fn infer_infix(&mut self, lhs: &Expr, op: InfixOp, rhs: &Expr) -> Ty {
        let l = self.infer_expr(lhs);
        let r = self.infer_expr(rhs);
        // NS-A2 fault dimension (issue #1108): integer `/` and `mod` raise
        // the turn-terminating `DivisionByZero` fault on a zero divisor —
        // a value-dependent domain fault on a well-typed program, so the
        // construct conservatively faults (the type-free structural harvest
        // cannot rule out the int case; float division is IEEE-total).
        if matches!(op, InfixOp::Div | InfixOp::Mod) {
            self.effect_faults = true;
            // F29 discharge: float division/modulo is IEEE-total — the
            // `DivisionByZero` fault is the *int* case only.
            if !(l == Ty::Float && r == Ty::Float) {
                self.effect_faults_refined = true;
            }
        }
        match op {
            InfixOp::Add
            | InfixOp::Sub
            | InfixOp::Mul
            | InfixOp::Div
            | InfixOp::Mod
            | InfixOp::Intersect => {
                let joined = unify(&l, &r);
                self.observe(lhs, &joined);
                self.observe(rhs, &joined);
                joined
            }
            InfixOp::Eq
            | InfixOp::NotEq
            | InfixOp::Lt
            | InfixOp::Gt
            | InfixOp::LtEq
            | InfixOp::GtEq => {
                let joined = unify(&l, &r);
                self.observe(lhs, &joined);
                self.observe(rhs, &joined);
                Ty::Bool
            }
            // Both operands are condition position — visited above for
            // their side effects, never forced to bool (spec §4 truthiness).
            InfixOp::And | InfixOp::Or | InfixOp::Has | InfixOp::HasNot => Ty::Bool,
            // B1 `or`-coalescing (`docs/stdlib-spec.md` §1.6a, issue
            // #1460): asymmetric by design (`lhs`: `Option[T]`, `rhs`: `T`
            // or `Option[U]`), so unlike the arithmetic/comparison arms
            // above there is no single shared "joined" type for both
            // operands. There IS a one-directional signal worth feeding
            // back, though: unlike And/Or/Has/HasNot's condition operands
            // (genuinely no useful bool/int constraint to add — spec §4
            // truthiness), a coalescing `lhs` is never optional-vs-leniency,
            // it is *required* to be `Option[T]` — so if `lhs` is a bare
            // param/temp path, `rhs`'s already-inferred type tells us the
            // shape to expect: `rhs`'s own type when `rhs` is itself
            // `Option[U]` (the two-Option form), else `Option[rhs's type]`
            // (the collapse form). `observe` is a no-op for every other
            // expression shape (only bare single-segment param/temp paths
            // feed back), so this is safe to call unconditionally. A
            // mismatch collapses to `Ty::Conflicted` (the same
            // infallible-absorption idiom `unify` uses elsewhere) rather
            // than a bespoke diagnostic — TM-3's existing Conflicted-escape
            // check (`strict::check`, `E066`) already catches it generically
            // the moment the result reaches a signature or body-local slot;
            // unrouted results keep the runtime `TypeError` fault as the
            // backstop, the same "Unknown/Conflicted never disagrees, the
            // runtime catches the rest" doctrine `option_conditions`'s F27
            // check documents.
            InfixOp::Coalesce => {
                let expected_lhs = if matches!(r, Ty::Option(_)) {
                    r.clone()
                } else {
                    Ty::Option(Box::new(r.clone()))
                };
                self.observe(lhs, &expected_lhs);
                coalesce(&l, &r).unwrap_or(Ty::Conflicted)
            }
        }
    }

    fn infer_call(&mut self, path: &HirPath, args: &[Expr]) -> Ty {
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer_expr(a)).collect();
        if let Some(def) = self.resolve(path.range) {
            // T1c: a callee resolving to a *value* (param/temp/VAR/CONST)
            // is a call through a function value, not a direct call — its
            // check runs against the value's own type, not `known_sigs`.
            // Classified by `SymbolKind` when the index carries the symbol,
            // by `DefinitionTag::LocalVar` when it doesn't (brink-db's
            // inference index projection can strip locals).
            let is_value_callee = match self.ctx.index.symbols.get(&def) {
                Some(info) => matches!(
                    info.kind,
                    SymbolKind::Param
                        | SymbolKind::Temp
                        | SymbolKind::Variable
                        | SymbolKind::Constant
                ),
                None => def.tag() == brink_format::DefinitionTag::LocalVar,
            };
            if is_value_callee {
                return self.infer_value_call(path, def, args, &arg_tys);
            }
            self.record_call_edge(def);
            self.record_ref_param_writes(def, args);
            return self.ctx.known_sigs.get(&def).map_or(Ty::Unknown, |sig| {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(param_ty) = sig.params.get(i) {
                        self.observe(arg, param_ty);
                    }
                }
                sig.return_ty.clone()
            });
        }
        // Unresolved single-segment name: try the T1b stdlib intrinsics
        // (shadow-fallback, matching `brink_analyzer::resolve::
        // is_t1b_stdlib_name` — a real def always wins the resolve() above).
        if let [seg] = path.segments.as_slice() {
            return self.infer_intrinsic(&seg.text, path.range, args, &arg_tys);
        }
        Ty::Unknown
    }

    /// `#fn(target, args…)` — docs/t1c-spec.md §4: consume the bound prefix
    /// from the target's signature. An unresolved target or a target with
    /// no known signature (a variable, an external, an E079 case) types as
    /// `Unknown` — the creation-site diagnostics own the error reporting.
    fn infer_fn_literal(&mut self, fl: &brink_ir::FnLiteral) -> Ty {
        for arg in &fl.args {
            self.infer_expr(arg);
        }
        let Some(def) = self.resolve(fl.target.range) else {
            return Ty::Unknown;
        };
        self.record_call_edge(def);
        let Some(sig) = self.ctx.known_sigs.get(&def) else {
            return Ty::Unknown;
        };
        for (i, arg) in fl.args.iter().enumerate() {
            if let Some(param_ty) = sig.params.get(i) {
                self.observe(arg, param_ty);
            }
        }
        let remaining: Vec<Ty> = sig.params.iter().skip(fl.args.len()).cloned().collect();
        Ty::Fn(remaining, Box::new(sig.return_ty.clone()))
    }

    /// A call whose callee resolved to a param/temp/VAR/CONST — a call
    /// through a value (T1c, docs/t1c-spec.md §4). Records the
    /// statically-checkable facts strict mode reports (`strict::check`):
    ///
    /// - known `fn(T…): R` callee → arity + per-arg checks (the `int ->
    ///   float` directional coercion is legal, same rule as
    ///   `annotations::report_if_mismatched`: legal iff
    ///   `unify(param, arg) == param`);
    /// - `Unknown`/`Conflicted` callee → an escape fact (the TM-3 escape
    ///   rule applied to call position);
    /// - any other concrete type → not callable — except `Divert`, the
    ///   pre-existing ink "call through a divert-ref variable" pattern,
    ///   which stays unchecked in this slice.
    ///
    /// The callee's type is its body-inferred one, falling back to its
    /// annotation/ascription when inference alone left it `Unknown` — the
    /// boundary-annotation firewall applied at the consumption site
    /// (`cb: fn(int): int` params must be callable under strict without a
    /// body use pinning them first).
    fn infer_value_call(
        &mut self,
        path: &HirPath,
        def: DefinitionId,
        args: &[Expr],
        arg_tys: &[Ty],
    ) -> Ty {
        let mut callee_ty = self.ty_of_def(def);
        if callee_ty.is_unknown()
            && let Some(ann) = self.annotated_callee_ty(path)
        {
            callee_ty = ann;
        }

        let callee = path
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");

        // T2 §8 (issue #872): `f(args)`'s callee is always a resolvable
        // name (a `HirPath`, never an inline expression), so its narrowing
        // classification is always `local_call_origin(def)` — narrowable
        // only for a bare Temp reference; a Param, a VAR/CONST global (the
        // heap case, still coarse per spec §5/§8), or anything else is
        // `Unknown`.
        let narrow_hint = self.local_call_origin(def);
        self.check_value_call(path.range, &callee, callee_ty, args, arg_tys, narrow_hint)
    }

    /// Boundary-annotation fallback for a callee whose body-inferred type
    /// came back `Unknown` (T1c, spec §4: "the boundary-annotation firewall
    /// applied at the consumption site" — a `cb: fn(int): int` param must be
    /// callable under strict without a body use pinning it first). Shared by
    /// every callee-classification site: direct `f(args)`
    /// ([`Self::infer_value_call`]) and the explicit `call(f, args…)` /
    /// `bind(f, args…)` intrinsic forms ([`Self::infer_intrinsic`]) — same
    /// rule, different syntax (spec §3).
    fn annotated_callee_ty(&self, path: &HirPath) -> Option<Ty> {
        if let Some(def) = self.resolve(path.range)
            && let Some(name) = self.ctx.index.symbols.get(&def).map(|i| i.name.clone())
            && let Some(ann) = self.annotated.get(&name)
        {
            return Some(ann.clone());
        }
        if let [seg] = path.segments.as_slice()
            && let Some(ann) = self.annotated.get(&seg.text)
        {
            // Local callee under a locals-stripped index (see
            // `infer_call`): the bare name is the path's single segment.
            return Some(ann.clone());
        }
        None
    }

    /// Statically-checkable typing rule for a call *through* an
    /// already-classified callee type (T1c spec §4): known `fn(T…): R` →
    /// arity + per-arg checks, return `R`; `Unknown`/`Conflicted` → the TM-3
    /// escape rule applied to call position; any other concrete type (except
    /// `Divert`, the pre-existing "call through a divert-ref variable"
    /// pattern, deliberately left unchecked) → not callable. Shared by
    /// direct calls (`f(args)`) and the explicit `call(f, args…)` intrinsic
    /// form — spec §3: "same semantics, usable where the callee is itself an
    /// expression".
    fn check_value_call(
        &mut self,
        range: TextRange,
        callee: &str,
        callee_ty: Ty,
        args: &[Expr],
        arg_tys: &[Ty],
        narrow_hint: ValueCallOrigin,
    ) -> Ty {
        // T2-1 (docs/effects-spec.md §3/§4): this is a call *through a function
        // value* — `f(args)` or `call(f, args…)`, the one place a real
        // callee's effects escape the static call graph. Rows ride `Ty::Fn`
        // (spec §5, the heap answer). T2 §8 (issue #872): when `narrow_hint`
        // proves a single, statically-known origin def, that def's row gets
        // pulled into this body's row transitively (same edge a direct call
        // records) instead of falling to the pessimal floor — resolved once
        // by `resolve_pending_value_calls` after the whole body is walked, so
        // `effect_opaque` is *not* forced here; every other case (an
        // ambiguous/unknown origin) still degrades to pessimal there, exactly
        // this fn's old unconditional behavior. `bind` creates a value rather
        // than calling one, so it routes through `check_bind_value`, not here.
        self.pending_value_calls.push(narrow_hint);
        // NS-A2 fault dimension (issue #1108): a call through a function
        // value can raise the T1c dispatch faults at runtime regardless of
        // narrowing — `NotCallable`, `FunctionValueArity`, the cross-flow
        // `#@local` `ref`-binding fault, rehydration mismatch (gradual mode
        // reaches all of these) — so the call site conservatively faults
        // even when #872's rung resolves the callee row. F29: no discharge
        // (dispatch faults are value-dependent).
        self.effect_faults = true;
        self.effect_faults_refined = true;
        match callee_ty {
            Ty::Fn(params, ret) => {
                if args.len() != params.len() {
                    self.push_value_call(
                        range,
                        callee,
                        ValueCallKind::ArityMismatch {
                            expected: params.len(),
                            got: args.len(),
                        },
                    );
                }
                for (i, param_ty) in params.iter().enumerate() {
                    if let Some(arg_ty) = arg_tys.get(i)
                        && !param_ty.is_unresolved()
                        && !arg_ty.is_unresolved()
                        && &unify(param_ty, arg_ty) != param_ty
                    {
                        self.push_value_call(
                            range,
                            callee,
                            ValueCallKind::ArgMismatch {
                                index: i,
                                expected: param_ty.clone(),
                                found: arg_ty.clone(),
                            },
                        );
                    }
                    if let Some(arg) = args.get(i) {
                        self.observe(arg, param_ty);
                    }
                }
                *ret
            }
            Ty::Divert => Ty::Unknown,
            Ty::Unknown => {
                self.push_value_call(range, callee, ValueCallKind::UnknownCallee);
                Ty::Unknown
            }
            Ty::Conflicted => {
                self.push_value_call(range, callee, ValueCallKind::ConflictedCallee);
                Ty::Unknown
            }
            other => {
                self.push_value_call(range, callee, ValueCallKind::NotCallable(other));
                Ty::Unknown
            }
        }
    }

    /// Statically-checkable typing rule for `bind(f, args…)` (T1c-3, spec
    /// §3/§4): "consume the head of the param row" — known `fn(T…): R` →
    /// binding more args than remain is an over-bind error (never a
    /// truncated row, mirroring the runtime's `FunctionValueArity` fault and
    /// `#fn`'s own E081 over-binding check), otherwise the bound prefix is
    /// checked per-arg and the result types as `fn(remaining…): R`.
    /// `Unknown`/`Conflicted`/not-callable classify exactly like
    /// [`Self::check_value_call`] — `bind` is effect-transparent and
    /// type-transparent over the same escape rule.
    fn check_bind_value(
        &mut self,
        range: TextRange,
        callee: &str,
        callee_ty: Ty,
        args: &[Expr],
        arg_tys: &[Ty],
    ) -> Ty {
        // NS-A2 (issue #1108): `bind(f, …)` faults at bind time on a
        // non-function callee (`NotCallable` from `bind_fn_value`) —
        // conservatively marked like `check_value_call`'s dispatch faults.
        // F29: no discharge.
        self.effect_faults = true;
        self.effect_faults_refined = true;
        match callee_ty {
            Ty::Fn(params, ret) => {
                if args.len() > params.len() {
                    self.push_value_call(
                        range,
                        callee,
                        ValueCallKind::OverBind {
                            available: params.len(),
                            got: args.len(),
                        },
                    );
                    // Can't compute a meaningful remaining row for a bind
                    // request that doesn't fit the signature.
                    return Ty::Unknown;
                }
                for (i, param_ty) in params.iter().take(args.len()).enumerate() {
                    if let Some(arg_ty) = arg_tys.get(i)
                        && !param_ty.is_unresolved()
                        && !arg_ty.is_unresolved()
                        && &unify(param_ty, arg_ty) != param_ty
                    {
                        self.push_value_call(
                            range,
                            callee,
                            ValueCallKind::ArgMismatch {
                                index: i,
                                expected: param_ty.clone(),
                                found: arg_ty.clone(),
                            },
                        );
                    }
                    if let Some(arg) = args.get(i) {
                        self.observe(arg, param_ty);
                    }
                }
                let remaining: Vec<Ty> = params.into_iter().skip(args.len()).collect();
                Ty::Fn(remaining, ret)
            }
            Ty::Divert => Ty::Unknown,
            Ty::Unknown => {
                self.push_value_call(range, callee, ValueCallKind::UnknownCallee);
                Ty::Unknown
            }
            Ty::Conflicted => {
                self.push_value_call(range, callee, ValueCallKind::ConflictedCallee);
                Ty::Unknown
            }
            other => {
                self.push_value_call(range, callee, ValueCallKind::NotCallable(other));
                Ty::Unknown
            }
        }
    }

    fn push_value_call(&mut self, range: TextRange, callee: &str, kind: ValueCallKind) {
        self.value_calls.push(ValueCallFact {
            range,
            callee: callee.to_string(),
            kind,
        });
    }

    /// Stdlib intrinsic typing rules (typed-mode-spec §2's "facility
    /// doctrine" list): `len`/`keys`/`values`/`contains`/`push`/`insert`/
    /// `remove`, plus the TM-3-completion pure conversion intrinsics
    /// `int`/`float`/`string` (§4, issue #659), plus the T1c-2/T1c-3 explicit
    /// call forms `call`/`bind` (§3/§4, issue #733 — wiring their typing
    /// rules into the checker after they shipped `Unknown`-typed in #730).
    /// These names have no `DefinitionId` of their own (they resolve
    /// specially, not through the symbol index — see
    /// `brink_ir::lir::lower::expr::is_t1b_stdlib_name`), so the rules live
    /// here as a direct match rather than "attached" to a resolved def.
    ///
    /// `range` is the intrinsic name's own source range (`call`/`bind`'s
    /// token, matching the `E031` arity diagnostic's anchor in
    /// `lir::lower::expr::lower_t1b_stdlib_call`) — used as the `call`/`bind`
    /// forms' `ValueCallFact` diagnostic site, since their callee is an
    /// arbitrary expression (`args[0]`), not a resolvable `Path` the way a
    /// direct call's callee always is.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per stdlib intrinsic — the NS-A1 Option verbs grew the table past 100"
    )]
    fn infer_intrinsic(
        &mut self,
        name: &str,
        range: TextRange,
        args: &[Expr],
        arg_tys: &[Ty],
    ) -> Ty {
        // NS-A2 fault dimension (issue #1108, from #1097) + NS-A6 RNG-cell
        // writes (issue #1112, "every draw is an ordinary write"): both
        // harvested from the ONE shared intrinsic effect table
        // (`super::intrinsics`, issue #1128) — `await_purity` consults the
        // same table for an unresolved call directly in an `await`
        // condition, so there is no second membership list to drift. See
        // that module's doc for the full per-verb audit (which ops fault,
        // which draw, and the deliberate exclusions: `string`/`some`/`seed`
        // total, nullary `float()` draw-only, `call`/`bind` marked at
        // `check_value_call`/`check_bind_value`). The RNG entry is what
        // makes the ruled free consequences fall out of existing machinery:
        // pure-gated wake conditions exclude draw-bearing callees (E105),
        // and `@[effects(pure)]` asserts rng-freedom (E103 exceedance names
        // the cell as `rng`).
        let fx = super::intrinsics::intrinsic_effects(name, args.len());
        if fx.faults {
            self.effect_faults = true;
            // F29 discharge (NS-A4): a wrong-type-only intrinsic over
            // provably-right-typed arguments is total — see
            // `intrinsic_fault_discharged`'s per-verb audit.
            if !super::intrinsics::intrinsic_fault_discharged(name, arg_tys) {
                self.effect_faults_refined = true;
            }
        }
        if fx.rng_write {
            self.effect_writes.insert(DefinitionId::RNG_CELL);
        }
        match name {
            "len" => Ty::Int,
            // `int(x)` is ONE value-directed verb (NS-A5,
            // `docs/stdlib-spec.md` §7): over a range it is `rand::int` —
            // one uniform draw, a write to the RNG cell — and over
            // everything else the TM-3 conversion intrinsic (#659). Both
            // legs return `Ty::Int`, so only the *effect row* is
            // type-directed: the draw leg's RNG write is recorded exactly
            // when the argument's inferred type is a range. (A gradual
            // `Unknown`-typed argument that turns out to hold a range at
            // runtime escapes this static harvest — the standard gradual
            // posture, F8: refinement machinery is strict-mode's; under
            // `types = strict` the argument must be `NonEmptyRange`
            // [E117, `range_refinement`], so the harvest is sound there.)
            "int" => {
                if matches!(arg_tys.first(), Some(Ty::Range { .. })) {
                    self.effect_writes.insert(DefinitionId::RNG_CELL);
                }
                Ty::Int
            }
            "keys" => match arg_tys.first() {
                Some(Ty::Map(k, _)) => Ty::Array(k.clone()),
                _ => Ty::Unknown,
            },
            "values" => match arg_tys.first() {
                Some(Ty::Map(_, v)) => Ty::Array(v.clone()),
                _ => Ty::Unknown,
            },
            "contains" => {
                if let Some(needle) = args.get(1) {
                    match arg_tys.first() {
                        Some(Ty::Array(elem)) => self.observe(needle, elem),
                        Some(Ty::Map(k, _)) => self.observe(needle, k),
                        _ => {}
                    }
                }
                Ty::Bool
            }
            // `push`/`insert`/`remove` (T1b stdlib slice 1 mutators,
            // docs/t1b-surface-spec.md §5) all write back through their
            // lvalue first argument at the call site — the same
            // codegen-observed `writeback_lvalue_container_chain` RMW
            // (`brink-ir::lir::lower::blocks::try_lower_mutator_stmt`) a
            // plain indexed assignment uses. Issue #880 (the #870
            // ground-truth harness's first catch, after #866's ref-param
            // class): `effects()`'s atom walk had no case for a mutator
            // *call* at all, so the write to the container's root cell was
            // silently missing from the row even though the bytecode really
            // performs it. Recorded here, at the call site, the same place
            // `record_ref_param_writes` records a `ref` parameter's write —
            // "the intrinsics' effect behavior is declared at introduction
            // per the facility doctrine" (this IS their row). `record_write`
            // unwraps the lvalue (a bare path or an arbitrarily nested
            // index chain, `is_lvalue_expr`'s domain) down to its root
            // itself, so a chained lvalue (`push(grid[y], v)`) still
            // attributes the write to `grid`, not to some cell named by the
            // index expression.
            // NS-A7's `heap_push(ref a, x)` types exactly like `push`: a
            // receiver write with the element observing the array's
            // element type (the min-heap placement is the runtime op's).
            "push" | "heap_push" => {
                if let (Some(Ty::Array(elem)), Some(item)) = (arg_tys.first(), args.get(1)) {
                    self.observe(item, elem);
                }
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                Ty::Unknown
            }
            "insert" => {
                if let (Some(Ty::Map(k, v)), Some(key_arg), Some(val_arg)) =
                    (arg_tys.first(), args.get(1), args.get(2))
                {
                    self.observe(key_arg, k);
                    self.observe(val_arg, v);
                }
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                Ty::Unknown
            }
            "remove" => {
                if let Some(item) = args.get(1) {
                    match arg_tys.first() {
                        Some(Ty::Array(elem)) => self.observe(item, elem),
                        Some(Ty::Map(k, _)) => self.observe(item, k),
                        _ => {}
                    }
                }
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                Ty::Unknown
            }
            // TM-3 completion (docs/typed-mode-spec.md §4, maintainer ruling
            // 2026-07-13, issue #659): pure conversion intrinsics. Each has a
            // fixed return type independent of the argument's own type — the
            // permissive multi-type domain (float/string/bool/int for
            // `int`/`float`; everything for `string`) is a runtime/strict-
            // domain-check concern (`conversions::check`), not a constraint
            // this unification-style inference can express by joining the
            // argument's type into anything, so no `self.observe` call here
            // (unlike `push`/`insert`/`remove` above, which do narrow their
            // container's element type). `int`'s arm lives above, merged
            // with `len` (both fixed `Ty::Int`, per clippy).
            // (`dot` — NS-A8 — shares this fixed-`Ty::Float` return: the
            // multi-kind vec2/3/4 domain is a runtime concern, so no
            // `observe` narrowing, exactly like the conversions here.)
            "float" | "dot" => Ty::Float,
            // `string` and `char_at(s, i)` (T1b stdlib slice 1 completion,
            // issue #857) both return a fixed `Ty::String` independent of
            // the argument — merged into one arm per clippy's
            // `match_same_arms`, same as `len`/`int` above. `char_at`'s
            // domain check (`s` is a `String`, `i` is an `Int`, `i` in
            // range) is entirely a runtime/gradual-mode concern (the
            // `CharAt` VM op) — no `self.observe` narrowing, since this
            // facility's typing rule is only the return type, declared at
            // introduction per the doctrine.
            "string" | "char_at" => Ty::String,
            // ── NS-A1 Option verbs (issue #1107, `docs/stdlib-spec.md`
            // §§3-5 + §1.4). Absence-shaped returns are `Option[…]` — the
            // element narrows from the container's known type where the
            // checker has one, staying `Option[Unknown]` otherwise (the
            // verb's Option-ness is unconditional; only the element is
            // gradual). Runtime domain checks (non-array container,
            // unorderable elements) stay at the ops, same split as
            // `char_at` above. ─────────────────────────────────────────
            //
            // `find(s, sub)` → `Option[int]` (§3, martyr #1). Both
            // arguments are strings by signature — observed so a strict
            // project gets the narrowing.
            "find" => {
                if let Some(s) = args.first() {
                    self.observe(s, &Ty::String);
                }
                if let Some(sub) = args.get(1) {
                    self.observe(sub, &Ty::String);
                }
                Ty::Option(Box::new(Ty::Int))
            }
            // `index_of(a, x)` → `Option[int]` (§4, martyr #2); the needle
            // narrows against the element type like `contains`.
            "index_of" => {
                if let (Some(Ty::Array(elem)), Some(needle)) = (arg_tys.first(), args.get(1)) {
                    self.observe(needle, elem);
                }
                Ty::Option(Box::new(Ty::Int))
            }
            // `first`/`last` → `Option[T]` over `[T]` (§4).
            // NS-A7's `heap_peek(a)` types exactly like `first`: a pure
            // `Option[T]` read (empty → `none`).
            "first" | "last" | "heap_peek" => match arg_tys.first() {
                Some(Ty::Array(elem)) => Ty::Option(elem.clone()),
                _ => Ty::Option(Box::new(Ty::Unknown)),
            },
            // `min`/`max`: two call shapes since NS-A8 — the one-arg NS-A1
            // array extremum (`Option[T]`) and the two-arg tower
            // componentwise form (same-kind vectors → that kind).
            "min" | "max" => {
                if args.len() == 2 {
                    match (arg_tys.first(), arg_tys.get(1)) {
                        (Some(Ty::Tower(a)), Some(Ty::Tower(b))) if a == b => Ty::Tower(*a),
                        _ => Ty::Unknown,
                    }
                } else {
                    match arg_tys.first() {
                        Some(Ty::Array(elem)) => Ty::Option(elem.clone()),
                        _ => Ty::Option(Box::new(Ty::Unknown)),
                    }
                }
            }
            // `pop(ref a)` → `Option[T]` (§4): the one A1 verb that is both
            // mutator and expression — records the receiver write exactly
            // like `push`/`insert`/`remove` above (the #880 lesson: the
            // intrinsics' effect behavior is declared at introduction).
            // NS-A7's `heap_pop(ref a)` shares the shape (mutator AND
            // expression, receiver write + `Option[T]` return).
            "pop" | "heap_pop" => {
                let ret = match arg_tys.first() {
                    Some(Ty::Array(elem)) => Ty::Option(elem.clone()),
                    _ => Ty::Option(Box::new(Ty::Unknown)),
                };
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                ret
            }
            // `get(m, k)` → `Option[V]` (§5, martyr #3); the key narrows
            // against the map's key type.
            "get" => match arg_tys.first() {
                Some(Ty::Map(k, v)) => {
                    if let Some(key_arg) = args.get(1) {
                        self.observe(key_arg, k);
                    }
                    Ty::Option(v.clone())
                }
                _ => Ty::Option(Box::new(Ty::Unknown)),
            },
            // `contains_value(m, v)` → bool (§5); the needle narrows
            // against the map's value type.
            "contains_value" => {
                if let (Some(Ty::Map(_, v)), Some(needle)) = (arg_tys.first(), args.get(1)) {
                    self.observe(needle, v);
                }
                Ty::Bool
            }
            // `clear(ref m)` (§5), `shuffle(ref a)` (§7, NS-A6), and
            // `sort(ref a)` (§4b, NS-A4 — the F0 imperative twin of
            // `sorted`): statement-only in-place mutators — a receiver
            // write, no value (the `push` shape, #880: the intrinsics'
            // effect behavior is declared at introduction). Arms merged
            // per clippy match_same_arms (the #694 `len | int` precedent);
            // `sort`'s mode-dependent NaN behavior is entirely the runtime
            // op's (rows stay mode-independent — the conservative faults
            // bit rides the intrinsic table).
            "clear" | "shuffle" | "sort" => {
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                Ty::Unknown
            }
            // `some(x)` → `Option[typeof x]` (§1.4) — the constructor is
            // where a bare element type becomes optional.
            "some" => Ty::Option(Box::new(arg_tys.first().cloned().unwrap_or(Ty::Unknown))),
            // ── NS-A8 numeric tower (issue #1114,
            // `docs/tower-mini-spec.md`). Constructors return their kind;
            // numeric lanes observe `float` (int lanes coerce through the
            // one directional numeric join); matrix columns observe their
            // column vector kind. Verbs type per the ruled §2b table;
            // runtime domain checks (wrong operand kind) stay at
            // `tower_ops`, the same split as every intrinsic above.
            // (`dot` — fixed `Ty::Float`, multi-kind vec domain, so no
            // `observe` narrowing — is merged into the `float` arm above
            // per clippy match_same_arms.) ─────────────────────────────
            "vec2" | "vec3" | "vec4" | "quat" => {
                for lane in args {
                    self.observe(lane, &Ty::Float);
                }
                match name {
                    "vec2" => Ty::Tower(TowerTy::Vec2),
                    "vec3" => Ty::Tower(TowerTy::Vec3),
                    "vec4" => Ty::Tower(TowerTy::Vec4),
                    _ => Ty::Tower(TowerTy::Quat),
                }
            }
            "mat2" | "mat3" | "mat4" => {
                let (col, ret) = match name {
                    "mat2" => (TowerTy::Vec2, TowerTy::Mat2),
                    "mat3" => (TowerTy::Vec3, TowerTy::Mat3),
                    _ => (TowerTy::Vec4, TowerTy::Mat4),
                };
                for c in args {
                    self.observe(c, &Ty::Tower(col));
                }
                Ty::Tower(ret)
            }
            // `cross(a, b)` → vec3 (vec3-only by signature — observed).
            "cross" => {
                for v in args {
                    self.observe(v, &Ty::Tower(TowerTy::Vec3));
                }
                Ty::Tower(TowerTy::Vec3)
            }
            // `clamp(x, lo, hi)` — three same-kind vectors, componentwise.
            "clamp" => match (arg_tys.first(), arg_tys.get(1), arg_tys.get(2)) {
                (Some(Ty::Tower(x)), Some(Ty::Tower(lo)), Some(Ty::Tower(hi)))
                    if x == lo && x == hi =>
                {
                    Ty::Tower(*x)
                }
                _ => Ty::Unknown,
            },
            // `lerp(a, b, t)` — same-kind vectors/quats with scalar `t`.
            "lerp" => {
                if let Some(t) = args.get(2) {
                    self.observe(t, &Ty::Float);
                }
                match (arg_tys.first(), arg_tys.get(1)) {
                    (Some(Ty::Tower(a)), Some(Ty::Tower(b))) if a == b => Ty::Tower(*a),
                    _ => Ty::Unknown,
                }
            }
            // ── NS-A7 collections+ (issue #1113, `docs/stdlib-spec.md`
            // §8). Effect harvest (the `roll` RNG write, the conservative
            // faults bits, the F29 discharge rules) rides the intrinsic
            // table above; these arms carry only the typing rules. The
            // E120 construction gate lives at the LIR lowering (the
            // recognition site owns the shape errors); runtime domain
            // checks stay at the ops. ─────────────────────────────────────
            //
            // `weighted(w1, v1, …)` → `Weighted[T]`: weights observe
            // `int`; the value element is the join of the odd-position
            // argument types (the array-literal shape).
            "weighted" => {
                let mut vals = Vec::with_capacity(args.len() / 2);
                for (i, arg) in args.iter().enumerate() {
                    if i % 2 == 0 {
                        self.observe(arg, &Ty::Int);
                    } else {
                        vals.push(arg_tys.get(i).cloned().unwrap_or(Ty::Unknown));
                    }
                }
                Ty::Weighted(Box::new(unify_all(vals)))
            }
            // `roll(w)` → `T` — total over any existing table (§8).
            "roll" => match arg_tys.first() {
                Some(Ty::Weighted(elem)) => (**elem).clone(),
                _ => Ty::Unknown,
            },
            // `heap_push`/`heap_pop`/`heap_peek` share the `push`/`pop`/
            // `first` arms below (merged per clippy match_same_arms —
            // identical typing shapes: element observation + receiver
            // write / `Option[T]` absence returns).

            // ── NS-A6 rand verbs (issue #1112, `docs/stdlib-spec.md`
            // §7). Effect harvest (the RNG-cell write) is above, before
            // the match; these arms carry only the typing rules. Runtime
            // domain checks (non-numeric `p`, non-array/subset container)
            // stay at the ops, same split as the NS-A1 verbs. ───────────
            //
            // `chance(p)` → bool. `p` is numeric (int coerces to float per
            // the ink-heritage promotion), so no `observe` narrowing — the
            // multi-type domain is a runtime concern, exactly like the
            // conversion intrinsics' arms above.
            "chance" => Ty::Bool,
            // NS-A5: `non_empty(r)` → `Option[NonEmptyRange]` — the
            // inhabited-range validator (S2 ruled 2026-07-19,
            // parse-don't-validate): the `some` payload carries the
            // checker-minted refinement evidence; the runtime value is the
            // SAME range (the refinement is a view, F7). Pure — no draw,
            // no RNG write; faults only on a wrong-typed argument.
            "non_empty" => Ty::Option(Box::new(Ty::Range { non_empty: true })),
            // `pick(coll)` → `Option[T]`: element from a known array,
            // subset-of-the-same-list from a flags subset.
            "pick" => match arg_tys.first() {
                Some(Ty::Array(elem)) => Ty::Option(elem.clone()),
                Some(Ty::List(origin)) => Ty::Option(Box::new(Ty::List(origin.clone()))),
                // NS-A5: `pick(range)` → `Option[int]` — empty → `none`
                // (dynamic-content absence; contrast `int(range)`, whose
                // emptiness is the E117/fault refinement).
                Some(Ty::Range { .. }) => Ty::Option(Box::new(Ty::Int)),
                _ => Ty::Option(Box::new(Ty::Unknown)),
            },
            // `shuffled(a)` → a new array of the same element type.
            // NS-A4: `sorted(a)` shares the exact shape (arms merged per
            // clippy match_same_arms) — the §4b doctrine order needs no
            // comparator, and the mode-dependent NaN behavior is entirely
            // the runtime op's (rows stay mode-independent: the `faults`
            // bit from the intrinsic table is the conservative union).
            "shuffled" | "sorted" => match arg_tys.first() {
                Some(Ty::Array(elem)) => Ty::Array(elem.clone()),
                _ => Ty::Unknown,
            },
            // NS-A4: the comparator pair. The comparator is a function
            // value the *verb* will call — the one other place (besides
            // `call(f)`/`f(args)`) a real callee's effects escape the
            // static call graph, so its row composes through the same
            // pending-value-call machinery (`⊕cmp`, F14): a provable
            // single origin pulls that def's row in transitively;
            // anything else degrades to the pessimal opaque floor.
            // Dispatch faults (non-function comparator, non-int return,
            // detected inconsistency) ride the intrinsic table's faults
            // bit. The pure·silent contract itself is E119
            // (`comparator_contract`), exceedance-only.
            "sorted_by" => {
                if let Some(cmp) = args.get(1) {
                    let hint = self.value_call_origin(cmp);
                    self.pending_value_calls.push(hint);
                }
                match arg_tys.first() {
                    Some(Ty::Array(elem)) => Ty::Array(elem.clone()),
                    _ => Ty::Unknown,
                }
            }
            "sort_by" => {
                if let Some(cmp) = args.get(1) {
                    let hint = self.value_call_origin(cmp);
                    self.pending_value_calls.push(hint);
                }
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                Ty::Unknown
            }
            // `seed(n)`: statement-only; `n` is an int by signature —
            // observed so a strict project gets the narrowing (the
            // gradual runtime stays lenient: the frozen `SeedRandom` op
            // coerces non-ints to 0).
            "seed" => {
                if let Some(n) = args.first() {
                    self.observe(n, &Ty::Int);
                }
                Ty::Unknown
            }
            // T1c (docs/t1c-spec.md §3/§4, issue #733): the explicit call
            // forms. `f` (args[0]) is the callee — a value, not a
            // statically-named target — so its type comes from the
            // already-inferred `arg_tys[0]`, with the same boundary-
            // annotation fallback direct calls get (`annotated_callee_ty`).
            // An empty `args` (missing callee) is a lowering-owned `E031`
            // arity error (`lir::lower::expr::lower_t1b_stdlib_call`); typing
            // just degrades to `Unknown` rather than duplicating that check.
            "call" | "bind" if args.is_empty() => Ty::Unknown,
            "call" => {
                let mut callee_ty = arg_tys[0].clone();
                if callee_ty.is_unknown()
                    && let Expr::Path(p) = &args[0]
                    && let Some(ann) = self.annotated_callee_ty(p)
                {
                    callee_ty = ann;
                }
                let callee = brink_ir::display_expr(&args[0]);
                // T2 §8 (issue #872): `call(f, …)`'s `f` is an arbitrary
                // expression, not a resolvable `Path` the way a direct
                // call's callee always is — `value_call_origin` covers both
                // the inline-literal and bare-local shapes.
                let narrow_hint = self.value_call_origin(&args[0]);
                self.check_value_call(
                    range,
                    &callee,
                    callee_ty,
                    &args[1..],
                    &arg_tys[1..],
                    narrow_hint,
                )
            }
            // `bind(f, args…)` — spec §3's "consume the head of the param
            // row" rule; the result is a new `fn(remaining…): R` value, not
            // `R` itself (see `check_bind_value`).
            "bind" => {
                let mut callee_ty = arg_tys[0].clone();
                if callee_ty.is_unknown()
                    && let Expr::Path(p) = &args[0]
                    && let Some(ann) = self.annotated_callee_ty(p)
                {
                    callee_ty = ann;
                }
                let callee = brink_ir::display_expr(&args[0]);
                self.check_bind_value(range, &callee, callee_ty, &args[1..], &arg_tys[1..])
            }
            _ => Ty::Unknown,
        }
    }

    fn infer_target(&mut self, target: &DivertTarget) {
        let arg_tys: Vec<Ty> = target.args.iter().map(|a| self.infer_expr(a)).collect();
        let DivertPath::Path(p) = &target.path else {
            return;
        };
        let Some(def) = self.resolve(p.range) else {
            return;
        };
        self.record_call_edge(def);
        self.record_ref_param_writes(def, &target.args);
        if let Some(sig) = self.ctx.known_sigs.get(&def) {
            for (i, arg) in target.args.iter().enumerate() {
                if let Some(param_ty) = sig.params.get(i) {
                    self.observe(arg, param_ty);
                }
            }
        }
        let _ = arg_tys;
    }

    // ── Statements / blocks ─────────────────────────────────────────

    fn infer_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.infer_stmt(stmt);
        }
    }

    fn infer_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Content(c) => self.infer_content(c),
            Stmt::Divert(d) => self.infer_target(&d.target),
            Stmt::TunnelCall(t) => {
                for target in &t.targets {
                    self.infer_target(target);
                }
            }
            Stmt::ThreadStart(t) => self.infer_target(&t.target),
            Stmt::TempDecl(t) => {
                self.register_ascription(t);
                let ty = t.value.as_ref().map_or(Ty::Unknown, |e| self.infer_expr(e));
                self.bind_local(&t.name.text, &ty);
                // T2 §8 (issue #872): track this write towards the local's
                // whole-body write count/origin — see `bump_local_write`.
                let origin = t
                    .value
                    .as_ref()
                    .and_then(|e| self.fn_literal_write_origin(e));
                self.bump_local_write(&t.name.text, origin);
            }
            Stmt::Assignment(a) => {
                self.infer_expr(&a.target);
                let ty = self.infer_expr(&a.value);
                self.observe(&a.target, &ty);
                self.record_write(&a.target);
                // T2 §8 (issue #872): see `record_fn_write`.
                let origin = self.fn_literal_write_origin(&a.value);
                self.record_fn_write(&a.target, origin);
            }
            Stmt::Return(r) => self.infer_return(r.value.as_ref(), &r.onwards_args),
            Stmt::ChoiceSet(cs) => self.infer_choice_set(cs),
            Stmt::LabeledBlock(b) => self.infer_block(b),
            Stmt::Conditional(c) => self.infer_conditional(c),
            Stmt::Sequence(s) => {
                for branch in &s.branches {
                    self.infer_block(branch);
                }
            }
            Stmt::ExprStmt(e) => {
                self.infer_expr(e);
            }
            Stmt::EndOfLine => {}
            Stmt::LogicBlock(lb) => self.infer_logic_block(lb),
            // `~ await <cond>` (docs/flow-suspension-spec.md §3): the condition
            // sits in condition position (no forcing) — its reads become the
            // wake dependency set, and the standalone purity gate (E105) is
            // what rejects a condition that also writes/calls.
            Stmt::Await(a) => {
                if let Some(cond) = &a.condition {
                    self.infer_expr(cond);
                }
            }
        }
    }

    fn infer_return(&mut self, value: Option<&Expr>, onwards: &[Expr]) {
        if let Some(v) = value {
            self.has_value_return = true;
            let ty = self.infer_expr(v);
            self.return_ty = unify(&self.return_ty, &ty);
        }
        for e in onwards {
            self.infer_expr(e);
        }
    }

    fn infer_content(&mut self, content: &Content) {
        // NS-A2 emits/tags harvest (issue #1108, from #1087 + its 2026-07-18
        // ruling refinements): any content *part* — text, glue (glue-only
        // output counts), spring, an interpolation, an inline
        // conditional/sequence — is a content fragment the host renders, so
        // the line emits. Tags ride the separate tag channel: a tag-only
        // line (empty `parts`, non-empty `tags`) sets `tags` WITHOUT setting
        // `emits` ("a flow that only annotates isn't speaking").
        if !content.parts.is_empty() {
            self.effect_emits = true;
        }
        if !content.tags.is_empty() {
            self.effect_tags = true;
        }
        for part in &content.parts {
            match part {
                ContentPart::Interpolation(e) => {
                    self.infer_expr(e);
                }
                ContentPart::InlineConditional(c) => self.infer_conditional(c),
                ContentPart::InlineSequence(s) => {
                    for branch in &s.branches {
                        self.infer_block(branch);
                    }
                }
                ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
            }
        }
    }

    fn infer_choice_set(&mut self, cs: &ChoiceSet) {
        for choice in &cs.choices {
            self.infer_choice(choice);
        }
        self.infer_block(&cs.continuation);
    }

    fn infer_choice(&mut self, choice: &Choice) {
        // NS-A2 (issue #1108): choice text is host-rendered content (the
        // choice list / the selected-line output) — the start/bracket/inner
        // `Content`s below set `emits` via `infer_content`'s own harvest.
        // Choice-level tags touch the tag channel.
        if !choice.tags.is_empty() {
            self.effect_tags = true;
        }
        if let Some(cond) = &choice.condition {
            self.infer_expr(cond); // condition position — no forcing.
        }
        if let Some(c) = &choice.start_content {
            self.infer_content(c);
        }
        if let Some(c) = &choice.bracket_content {
            self.infer_content(c);
        }
        if let Some(c) = &choice.inner_content {
            self.infer_content(c);
        }
        self.infer_block(&choice.body);
    }

    fn infer_conditional(&mut self, cond: &Conditional) {
        if let CondKind::Switch(e) = &cond.kind {
            self.infer_expr(e);
        }
        for branch in &cond.branches {
            if let Some(e) = &branch.condition {
                self.infer_expr(e); // condition position — no forcing.
            }
            self.infer_block(&branch.body);
        }
    }

    // ── T1b `~ { … }` logic blocks ───────────────────────────────────

    fn infer_logic_block(&mut self, lb: &LogicBlock) {
        for bs in &lb.stmts {
            self.infer_block_stmt(bs);
        }
    }

    fn infer_block_stmt(&mut self, bs: &BlockStmt) {
        match bs {
            BlockStmt::TempDecl(t) => {
                self.register_ascription(t);
                let ty = t.value.as_ref().map_or(Ty::Unknown, |e| self.infer_expr(e));
                self.bind_local(&t.name.text, &ty);
                // T2 §8 (issue #872): see `Stmt::TempDecl`'s twin above.
                let origin = t
                    .value
                    .as_ref()
                    .and_then(|e| self.fn_literal_write_origin(e));
                self.bump_local_write(&t.name.text, origin);
            }
            BlockStmt::Assignment(a) => {
                self.infer_expr(&a.target);
                let ty = self.infer_expr(&a.value);
                self.observe(&a.target, &ty);
                self.record_write(&a.target);
                // T2 §8 (issue #872): see `Stmt::Assignment`'s twin above.
                let origin = self.fn_literal_write_origin(&a.value);
                self.record_fn_write(&a.target, origin);
            }
            BlockStmt::Return(r) => self.infer_return(r.value.as_ref(), &r.onwards_args),
            BlockStmt::If(i) => self.infer_if(i),
            BlockStmt::While(w) => {
                self.infer_expr(&w.condition); // condition position — no forcing.
                for s in &w.body {
                    self.infer_block_stmt(s);
                }
            }
            BlockStmt::For(f) => {
                // NS-A2 (issue #1108): `for` compiles to `CollectionKeys`,
                // which raises `NotIndexable` on a non-collection iterable —
                // a tracked domain fault, so the construct conservatively
                // faults (bool v1).
                self.effect_faults = true;
                let iter_ty = self.infer_expr(&f.iterable);
                // F29 discharge: a provably-iterable type (the closed
                // builtin roster) makes the loop's own iteration total.
                if crate::protocols::iterate_element_ty(&iter_ty).is_none() {
                    self.effect_faults_refined = true;
                }
                // NS-A3 (issue #1109, docs/stdlib-spec.md §9.6): the closed
                // builtin iterable set is the iterate protocol's v1 roster —
                // one table (`protocols::iterate_element_ty`) serves `for`,
                // its only v1 consumer (arrays iterate values, maps iterate
                // keys; anything else is not iterable and stays `Unknown`).
                let elem_ty = crate::protocols::iterate_element_ty(&iter_ty).unwrap_or(Ty::Unknown);
                self.bind_local(&f.var_name.text, &elem_ty);
                // Two-binding map iteration (`for k, v in m`, B2 issue
                // #1461): the second binding's type is the map's value
                // type. Only maps have a "value at key" — an iterable
                // outside the closed set (or an array/range, which iterate
                // a single element with no paired value) escapes as
                // `Unknown` rather than refusing to compile, matching
                // `elem_ty`'s own fallback just above (NS-A2: `for`
                // compiles unconditionally; the runtime `Index`/
                // `NotIndexable` fault is the actual gate).
                if let Some(val_name) = &f.val_name {
                    let val_ty = crate::protocols::iterate_val_ty(&iter_ty).unwrap_or(Ty::Unknown);
                    self.bind_local(&val_name.text, &val_ty);
                }
                for s in &f.body {
                    self.infer_block_stmt(s);
                }
            }
            BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
            BlockStmt::ExprStmt(e) => {
                self.infer_expr(e);
            }
            // `await <cond>` inside a `~ { … }` block — condition position, as
            // for the top-level `~ await` (docs/flow-suspension-spec.md §3).
            BlockStmt::Await(a) => {
                if let Some(cond) = &a.condition {
                    self.infer_expr(cond);
                }
            }
        }
    }

    fn infer_if(&mut self, i: &IfStmt) {
        self.infer_expr(&i.condition); // condition position — no forcing.
        for s in &i.body {
            self.infer_block_stmt(s);
        }
        match &i.else_branch {
            Some(ElseBranch::ElseIf(inner)) => self.infer_if(inner),
            Some(ElseBranch::Else(stmts)) => {
                for s in stmts {
                    self.infer_block_stmt(s);
                }
            }
            None => {}
        }
    }
}
