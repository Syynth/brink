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
    AssignOp, Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart,
    DivertPath, DivertTarget, ElseBranch, Expr, IfStmt, InfixOp, LogicBlock, Name, Path as HirPath,
    PrefixOp, Stmt, StringPart, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use super::effects::FnArgOrigins;
use super::ty::{FnRow, TowerTy, Ty, assignable, coalesce, ref_assignable, unify, unify_all};
use super::{
    DirectCallArgMismatch, FieldAssignMismatch, InferredSig, LambdaAnnotationMismatch,
    TypedAssignMismatch, UfcsCallArgs, ValueCallFact, ValueCallKind, range_key,
};

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
    /// Which frontend produced the file this body was declared in: `true`
    /// for the native (`.brink`) surface, `false` for the ink one — the
    /// inference-side mirror of [`brink_ir::HirFile::native`] and
    /// `lir::lower::context::LowerCtx::native` (issue #1862), threaded
    /// through [`super::Def::native`].
    ///
    /// One rule keys off it: a bare name resolving to a **function
    /// definition** is a fn *value* on the native surface (RULED
    /// 2026-08-01, `docs/t1c-spec.md` §2a) and a knot's **visit count** in
    /// ink. See [`InferPass::native_fn_value_target`].
    pub native: bool,
}

impl BodyCtx<'_> {
    /// A [`crate::annotations::TypeNames`] bundle for this ctx's
    /// annotation-resolution call sites — `handles` now carries the real
    /// registered handle-kind vocabulary (T1d-2b, issue #774), so a
    /// `Handle<K>` annotation on a param/return/temp resolves to
    /// `Ty::Handle(K)` during body inference whenever `K` is declared in the
    /// registered manifest, exactly like `List<L>`/`STRUCT` already do
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
    /// #872 (§8 precision rung), widened by Fork A (issue #1726), reads the
    /// concrete creation targets back off a local's whole-body write summary
    /// when *every* write traced to one — see
    /// `InferPass::resolve_pending_value_calls` — so this only stays `true`
    /// when the reaching values were not all created in-project; the heap
    /// (VAR/CONST cells) case remains coarse/pessimal, still unaddressed
    /// (spec §6.3).
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
    /// Issue #1864: statically-checkable argument-type mismatches at
    /// **direct** call sites — see [`super::DirectCallArgMismatch`].
    /// Recorded unconditionally, like `value_calls`; reported only by
    /// strict mode.
    pub direct_call_arg_mismatches: Vec<DirectCallArgMismatch>,
    /// Issue #1877: statically-checkable type mismatches at a `~ temp name:
    /// T = expr` initializer or a plain assignment against the target's
    /// already-known declared type — see [`super::TypedAssignMismatch`].
    /// Recorded unconditionally, like `direct_call_arg_mismatches`; reported
    /// only by strict mode.
    pub typed_assign_mismatches: Vec<TypedAssignMismatch>,
    /// Issue #1900: see [`super::FieldAssignMismatch`]. Recorded
    /// unconditionally, like `typed_assign_mismatches`; resolved and
    /// reported only by strict mode.
    pub field_assign_mismatches: Vec<FieldAssignMismatch>,
    /// Issue #1994: see [`super::LambdaAnnotationMismatch`]. Recorded
    /// unconditionally by every `infer_lambda` call this body walk reaches
    /// (including nested lambdas); reported only by strict mode, as
    /// `Error`-severity `E174` rather than the gradual `E063`
    /// `typed_assign_mismatches`/`field_assign_mismatches` use.
    pub lambda_annotation_mismatches: Vec<LambdaAnnotationMismatch>,
    /// Issue #1881: see [`super::UfcsCallArgs`]. Recorded unconditionally,
    /// like `direct_call_arg_mismatches`; consumed by
    /// `crate::ufcs::UfcsVisitor`, reported only by strict mode.
    pub ufcs_call_args: Vec<UfcsCallArgs>,
    /// Issue #1532 (the #1501 review's `remove`/`remove_at` migration-tail
    /// finding): every `remove(a, i)` call site in this body whose first
    /// argument is statically known to be `Ty::Array` — the pre-#1484 array
    /// leg `remove` no longer serves (`remove_at` does). Each entry is the
    /// call's own `remove` token range (matching `ValueCallFact::range`'s
    /// convention). Recorded unconditionally, like `value_calls`; reported
    /// only by strict mode (`strict::check_array_remove_calls`, `E149`) —
    /// gradual mode keeps the `MapRemove` runtime fault as its backstop.
    pub array_remove_calls: Vec<TextRange>,
    /// Fork A (`docs/decision-log.md` 2026-07-28 "Fork A — fn-value
    /// call-graph edges are harvested STRUCTURALLY", issue #1726): the
    /// inferable targets whose **fn values this body creates** — every
    /// `#fn(target, …)` literal walked here, whether or not the resulting
    /// value is ever called in this body.
    ///
    /// Purely structural, exactly like [`Self::calls`] and
    /// [`Self::referenced_globals`]: the target of a `#fn` literal is a
    /// syntactic name, so no inferred row or signature is ever consulted to
    /// decide membership. That is what keeps the call graph row-independent
    /// (and so keeps `call_graph → scc_membership → solve_scc → call_graph`
    /// acyclic) while still letting the effect fixpoint see fn-value flow.
    /// `bind(f, …)` contributes nothing of its own — it copies an existing
    /// value rather than naming a new target, so its base's own `#fn` literal
    /// (if any) is what gets recorded, by the nested walk.
    ///
    /// A subset of [`Self::calls`] by construction: `infer_fn_literal`
    /// records the same target as a call-graph edge, which is precisely how
    /// these edges reach the SCC batching without any change to it. Kept as
    /// its own set anyway because "creates a value for `g`" and "calls `g`"
    /// are different facts — spec §7's token table and §8 rung 1's
    /// reachability slicing both need the creation sites specifically.
    pub created_fn_values: BTreeSet<DefinitionId>,
    /// §6.1 (issue #1680): the declaration indices of this body's own
    /// `fn`-typed params it **calls through** — the row variables its effect
    /// row is parametric in. See `EffectAtoms::param_holes`.
    pub param_holes: BTreeSet<u32>,
    /// §6.1 (issue #1680): per `(inferable callee, param index)`, what this
    /// body's arguments in that position can hold — the caller-side material
    /// the effect fixpoint instantiates a callee's row variables from. See
    /// `EffectAtoms::call_fn_args`.
    pub call_fn_args: BTreeMap<(DefinitionId, u32), FnArgOrigins>,
}

/// A fresh, empty [`InferPass`] over `ctx` for `def`, seeded with
/// `annotated` (the resolved param-annotation overlay [`infer_def_body`]
/// already computed) and `param_index` (§6.1, issue #1680). Split out of
/// [`infer_def_body`] itself (clippy's `too_many_lines`) — every other
/// field starts at its own empty/zero value, mirroring [`empty_pass`]'s own
/// test-only twin below.
fn new_pass<'a, 'b>(
    ctx: &'a BodyCtx<'b>,
    def: &super::Def<'_>,
    annotated: BTreeMap<String, Ty>,
) -> InferPass<'a, 'b> {
    InferPass {
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
        direct_call_arg_mismatches: Vec::new(),
        typed_assign_mismatches: Vec::new(),
        field_assign_mismatches: Vec::new(),
        lambda_annotation_mismatches: Vec::new(),
        ufcs_call_args: Vec::new(),
        array_remove_calls: Vec::new(),
        created_fn_values: BTreeSet::new(),
        local_fn_origins: BTreeMap::new(),
        pending_value_calls: Vec::new(),
        lambda_param_names: BTreeMap::new(),
        // §6.1 (issue #1680): `ref` params are excluded — see the field doc.
        param_index: def
            .params
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_ref)
            .filter_map(|(i, p)| u32::try_from(i).ok().map(|i| (p.name.text.clone(), i)))
            .collect(),
        param_writes: BTreeSet::new(),
        param_holes: BTreeSet::new(),
        pending_call_fn_args: Vec::new(),
        call_fn_args: BTreeMap::new(),
        dotted_field_read_tainted: false,
    }
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

    let mut pass = new_pass(ctx, def, annotated);
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
    pass.finish_walk();
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
        direct_call_arg_mismatches: pass.direct_call_arg_mismatches,
        typed_assign_mismatches: pass.typed_assign_mismatches,
        field_assign_mismatches: pass.field_assign_mismatches,
        lambda_annotation_mismatches: pass.lambda_annotation_mismatches,
        ufcs_call_args: pass.ufcs_call_args,
        array_remove_calls: pass.array_remove_calls,
        created_fn_values: pass.created_fn_values,
        param_holes: pass.param_holes,
        call_fn_args: pass.call_fn_args,
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

/// Issue #1910: every bare name a lambda's own block body directly binds —
/// `TempDecl`, a `for` loop's `var_name`/`val_name`, or an `if`/`while`
/// `EXPR as NAME` binding — collected recursively through `if`/`while`/`for`
/// nesting. Used by `InferPass::infer_lambda` to extend its `self.locals`
/// shadow past just the lambda's own params (see that call site's own doc):
/// any of these can collide with an enclosing same-named local exactly the
/// way an unshadowed param would.
///
/// Deliberately does **not** recurse into a nested `Expr::Lambda` (there is
/// none to walk into here regardless — `BlockStmt` only carries expressions,
/// and a lambda literal nested inside one of those is a leaf as far as this
/// name collection is concerned): a nested lambda's own body binds its own
/// names, shadowed by *its own* `infer_lambda` call when that literal is
/// walked, not this one's.
fn lambda_own_binding_names(stmts: &[brink_ir::BlockStmt], out: &mut BTreeSet<String>) {
    fn walk_if(i: &brink_ir::IfStmt, out: &mut BTreeSet<String>) {
        if let Some(b) = &i.binding {
            out.insert(b.text.clone());
        }
        lambda_own_binding_names(&i.body, out);
        match &i.else_branch {
            Some(brink_ir::ElseBranch::ElseIf(nested)) => walk_if(nested, out),
            Some(brink_ir::ElseBranch::Else(body)) => lambda_own_binding_names(body, out),
            None => {}
        }
    }
    for s in stmts {
        match s {
            brink_ir::BlockStmt::TempDecl(t) => {
                out.insert(t.name.text.clone());
            }
            brink_ir::BlockStmt::If(i) => walk_if(i, out),
            brink_ir::BlockStmt::While(w) => {
                if let Some(b) = &w.binding {
                    out.insert(b.text.clone());
                }
                lambda_own_binding_names(&w.body, out);
            }
            brink_ir::BlockStmt::For(f) => {
                out.insert(f.var_name.text.clone());
                if let Some(v) = &f.val_name {
                    out.insert(v.text.clone());
                }
                lambda_own_binding_names(&f.body, out);
            }
            brink_ir::BlockStmt::Assignment(_)
            | brink_ir::BlockStmt::Return(_)
            | brink_ir::BlockStmt::Break(_)
            | brink_ir::BlockStmt::Continue(_)
            | brink_ir::BlockStmt::ExprStmt(_)
            | brink_ir::BlockStmt::Await(_) => {}
        }
    }
}

/// Whether `(l, r)` is the `string`/numeric display-concatenation shape
/// `+` accepts under strict types (issue #1911, spec §4): a `string` paired
/// with an `int` or `float`, in either operand order. Mirrors
/// `value_ops::binary_op`'s `String`/`Int` and `String`/`Float` `Add`
/// arms exactly — the runtime is the compatibility floor, so this stays a
/// straight port of which pairs it accepts, not a broader "string + T"
/// rule. `Bool` is deliberately excluded: the runtime has no
/// `String`/`Bool` `Add` arm, so that pair still falls through to the
/// general same-type unify and reports `E066` as before.
///
/// `pub(crate)` (issue #1900 review finding) so `structs::check_field_
/// assign_mismatch` can apply the identical carve-out to a *dotted* target
/// (`~ v.s += 5`) — that check resolves the field's declared type only
/// after the shape walk, well after this module's own `Stmt::Assignment`
/// arm has already computed (and discarded) a root-only `target_ty` that
/// cannot see the field type, so the guard has to be re-applied at the
/// field-resolved point instead of reused from here directly.
pub(crate) fn is_string_numeric_concat(l: &Ty, r: &Ty) -> bool {
    matches!(
        (l, r),
        (Ty::String, Ty::Int | Ty::Float) | (Ty::Int | Ty::Float, Ty::String)
    )
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
    /// See [`BodyResult::direct_call_arg_mismatches`] — accumulated the
    /// same way `value_calls` is, during the one body walk.
    direct_call_arg_mismatches: Vec<DirectCallArgMismatch>,
    /// See [`BodyResult::typed_assign_mismatches`] — accumulated the same
    /// way `direct_call_arg_mismatches` is, during the one body walk.
    typed_assign_mismatches: Vec<TypedAssignMismatch>,
    /// See [`BodyResult::field_assign_mismatches`] — accumulated the same
    /// way `typed_assign_mismatches` is, during the one body walk.
    field_assign_mismatches: Vec<FieldAssignMismatch>,
    /// See [`BodyResult::lambda_annotation_mismatches`] — accumulated the
    /// same way `typed_assign_mismatches` is, pushed by [`Self::infer_lambda`]
    /// itself rather than snapshotted/restored per lambda frame: a mismatch
    /// recorded by a nested lambda must survive into the enclosing body's
    /// own result exactly like every other fact this pass harvests.
    lambda_annotation_mismatches: Vec<LambdaAnnotationMismatch>,
    /// See [`BodyResult::ufcs_call_args`] — accumulated the same way
    /// `direct_call_arg_mismatches` is, during the one body walk.
    ufcs_call_args: Vec<UfcsCallArgs>,
    /// See [`BodyResult::array_remove_calls`] — accumulated the same way
    /// `value_calls` is, during the one body walk.
    array_remove_calls: Vec<TextRange>,
    /// Fork A (docs/decision-log.md 2026-07-28, issue #1726): the structural
    /// fn-value *creation* atom — see [`BodyResult::created_fn_values`].
    created_fn_values: BTreeSet<DefinitionId>,
    /// T2 §8 precision rung (docs/effects-spec.md §6 item 3/§8, issue #872;
    /// widened to a *set* by Fork A, issue #1726): every write (`TempDecl`
    /// initializer or bare-`Path` `Assignment`) to a Temp local, by name,
    /// folded into the [`LocalFnOrigins`] summary of what that name can hold.
    /// Param writes are deliberately not recorded here for narrowing purposes
    /// (see [`InferPass::local_call_origin`]'s doc) — a Param carries an
    /// implicit caller-provided initial value no write summary can ever see.
    ///
    /// Consulted only after the whole body is walked
    /// ([`InferPass::resolve_pending_value_calls`]) — a single-pass "as
    /// accumulated so far" read would miss a reassignment that appears later
    /// in program order but, inside a loop body, executes *before* an
    /// earlier-positioned call on the next iteration. Whole-body-final
    /// summaries close that hole: the narrowed edge set is the join over
    /// *every* write's origin, so a loop-carried reassignment is already
    /// covered rather than needing to poison the name.
    local_fn_origins: BTreeMap<String, LocalFnOrigins>,
    /// Every value-call site's narrowing candidate, recorded during the walk
    /// and resolved once by [`InferPass::resolve_pending_value_calls`] after
    /// the whole body (and therefore `local_fn_origins`) is final.
    pending_value_calls: Vec<ValueCallOrigin>,
    /// Issue #1779: a reference count, by name, of every param belonging to
    /// a lambda we are *currently* walking the body of (any depth of
    /// nesting). Non-empty for a name for as long as
    /// [`InferPass::infer_lambda`] is walking that lambda's `stmts` **and**
    /// its tail/expr value — see that function's doc for why the count must
    /// bracket both.
    ///
    /// A lambda's own params are recorded as `SymbolKind::Temp`, the exact
    /// same project-wide, flat, by-*name* keyspace an enclosing `~ temp` (or
    /// a `for` binding) gets (`symbols/project.rs::walk_lambda`) — so a
    /// lambda param can collide by name with an unrelated enclosing Temp.
    /// Unlike a genuine `~ temp`, though, a lambda param is never written by
    /// a `TempDecl`/`Assignment` this pass observes — it is bound
    /// implicitly by whatever the caller passes at the call site, exactly
    /// like one of *this* definition's own declared params
    /// ([`InferPass::local_call_origin`]'s `SymbolKind::Param` arm already
    /// refuses to trust those as `Local`, for the identical reason: "carries
    /// an implicit caller-provided initial value [the local write summary]
    /// never sees"). `local_call_origin` consults this map to apply the same
    /// refusal to a lambda's own param, for as long as we are inside that
    /// lambda (or a lambda nested within it) — whatever
    /// `self.local_fn_origins[name]` currently holds could be an unrelated
    /// enclosing local's own summary (inherited, unmodified, from before we
    /// ever entered this lambda) or a join corrupted by a write this
    /// lambda made to its *own* same-named param; neither bounds what this
    /// lambda's param can actually hold.
    ///
    /// A reference count, not a flat set, so two sibling lambdas that reuse
    /// the same param name don't leak into each other (each push/pop pair
    /// is exactly balanced), and a nested lambda re-using an enclosing
    /// lambda's own param name still counts once per active frame.
    lambda_param_names: BTreeMap<String, u32>,
    /// §6.1 (issue #1680): this definition's own params by name → declaration
    /// index, restricted to the params a row variable may legally be minted
    /// for. `ref` params are excluded at construction: the slot aliases the
    /// caller's storage, so what it holds at the call-through site is not
    /// pinned by the argument the caller passed.
    param_index: BTreeMap<String, u32>,
    /// §6.1: the indices in [`Self::param_index`] the body **writes** —
    /// assigns to directly, or hands to a `ref` slot. A written param no
    /// longer holds the caller's argument at every read, so it cannot carry a
    /// row variable (the same soundness argument that keeps a Param out of
    /// [`ValueCallOrigin::Local`]); calls through it keep the pessimal floor.
    param_writes: BTreeSet<u32>,
    /// §6.1: the row variables this body ended up minting — the indices of
    /// fn-typed params it calls through, resolved post-walk against
    /// [`Self::param_writes`]. Becomes `EffectAtoms::param_holes`.
    param_holes: BTreeSet<u32>,
    /// §6.1: one `(inferable callee, argument index, origin)` observation per
    /// argument of every direct call to an inferable target, recorded during
    /// the walk and folded post-walk (alongside the value calls, against the
    /// same whole-body-final `local_fn_origins`) into [`Self::call_fn_args`].
    pending_call_fn_args: Vec<(DefinitionId, u32, ValueCallOrigin)>,
    /// §6.1: the folded caller-side row-variable material — see
    /// `EffectAtoms::call_fn_args`.
    call_fn_args: BTreeMap<(DefinitionId, u32), FnArgOrigins>,
    /// Issue #1924 follow-up review on #1910: set the moment [`Self::
    /// infer_path`] resolves a multi-segment path to a *value*-kind symbol
    /// (`Param`/`Temp`/`Variable`/`Constant`) — the "dotted field read
    /// spelled as a path" shape (`p.x`) that, for lack of a static
    /// field-type table (#1924), resolves to its *head* variable's own type
    /// rather than the field's. `infer_lambda` resets this to `false`
    /// before walking a lambda's body and consults it afterward: if the
    /// walk ever hit that mistyped case, the lambda's own `body_ty`/
    /// `narrowed_params` overlay is discarded in favor of the pre-#1910
    /// annotation-only posture, because an ordinary `unify`/`observe`
    /// reachable from that mistyped value can otherwise poison a
    /// completely unrelated param or return (`fold(items, 0, |a, b| p.x +
    /// a + b)`: `p.x`'s mistyped `Struct` joins with `a`, corrupting the
    /// fold accumulator's own type). Not part of [`FrameSnapshot`] — see
    /// `infer_lambda`'s own save/reset/restore of this field for why a
    /// manual scope, not the frame-snapshot mechanism, is correct here.
    dotted_field_read_tainted: bool,
}

/// The `InferPass` fields scoped to a single lambda-body frame — see
/// `infer_lambda`'s doc for the full hazard writeup and
/// `docs/effects-spec.md` §4.1 for the written-up rule.
///
/// This type is deliberately **not** a subset carved out ad hoc at each call
/// site. It, together with [`InferPass::frame_snapshot`] and
/// [`InferPass::restore_frame`] below, is issue #1816's compile-time guard:
///
/// - [`InferPass::frame_snapshot`] destructures `&InferPass` field-by-field
///   with **no `..` rest pattern**. Adding a field to `InferPass` makes that
///   destructuring non-exhaustive — a compile error — until a decision is
///   made about the new field: either fold it in as frame-scoped (add it
///   here and to both destructurings) or explicitly ignore it as cumulative
///   (add `new_field: _` with a comment explaining why it must survive into
///   the enclosing frame). Silence is not an option; the code will not build.
/// - [`InferPass::restore_frame`] destructures `FrameSnapshot` itself the
///   same way, so a field added here but never written back in
///   `restore_frame` is equally a compile error.
///
/// A field-count `const _: () = assert!(...)` was considered instead
/// (issue #1816's ask) and rejected: `std::mem::size_of` counts bytes, not
/// fields, so it cannot pin a field *count*, and there is no stable
/// `std::mem::variant_count`-style field-count reflection for structs. An
/// exhaustive-destructure helper achieves the same "compiler forces an
/// explicit decision" property without needing one, and unlike a bare count
/// it also forces the *restore* side to stay in lockstep, not just the
/// snapshot side.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameSnapshot {
    return_ty: Ty,
    has_value_return: bool,
    locals: BTreeMap<String, Ty>,
    annotated: BTreeMap<String, Ty>,
    local_fn_origins: BTreeMap<String, LocalFnOrigins>,
}

/// Fork A (issue #1726): what one Temp local can hold, summarized over every
/// write to it in the whole body — the structural fact
/// [`InferPass::resolve_pending_value_calls`] narrows a call through that
/// local with.
///
/// The soundness argument is the join: a Temp cannot be read before its
/// defining `TempDecl`, so every value a read can observe was written by one
/// of the writes folded in here — **provided every write site actually folds
/// its write in**. That is not automatic from the Temp/`TempDecl` ordering
/// argument alone: a `ref`-param call-site rebind (`~ poke(f, cb)` where a
/// `ref` parameter reassigns the caller's `f`) mutates the local exactly like
/// an in-body assignment does, but it happens at the *call site*, not inside
/// `f`'s own definition, so it only lands in this summary because
/// [`InferPass::record_ref_param_writes`] explicitly folds it in (as an
/// untraced write — the callee could reassign `f` to any argument it was
/// passed). If *every* one of the folded writes traced to a known creation
/// target, the call reaches one of `targets` and joining all of them
/// over-reports at worst — the conservative-total direction (spec §3). A
/// single write that did not trace (an aliased local, a call's return value,
/// a param read, a heap load, or the `ref`-rebind case above) sets
/// `untraced`, and the whole name falls back to the pessimal floor: the
/// reaching value could have been created anywhere, including outside the
/// project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalFnOrigins {
    /// The creation targets this name's writes traced to, sorted.
    targets: BTreeSet<DefinitionId>,
    /// At least one write's value did not trace to a creation site.
    untraced: bool,
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
    /// trusted here). Resolved post-walk: narrowable to the join over every
    /// creation target `name`'s writes traced to, and only if *every* write
    /// traced (Fork A, issue #1726 — see
    /// [`InferPass::resolve_pending_value_calls`]).
    Local(String),
    /// The callee is *itself* (optionally through one or more `bind(…)`
    /// wrappers) an `#fn(target, …)` literal evaluated fresh at this call
    /// site — no stored value, so no write-count question applies; always
    /// narrowable.
    Inline(DefinitionId),
    /// §6.1 (issue #1680): the callee is one of *this* definition's own
    /// non-`ref` params, at the carried declaration index. Resolved post-walk
    /// to a **row variable** ([`EffectRow::holes`](super::EffectRow::holes))
    /// — the enclosing row stays parametric in it and each caller
    /// instantiates it — unless the body writes that param, in which case it
    /// keeps the pessimal floor.
    ///
    /// In *argument* position this arm never fills anything: passing a param
    /// along would chain one hole into another, which §6.1's shallow
    /// polymorphism deliberately does not do, so it is treated as untraced.
    Param(u32),
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
        // Native bare-name fn value (issue #1876, the typing half of
        // #1862's RULED 2026-08-01 lowering): checked *before* the kind
        // match because the reference's type is decided by the surface it
        // was written on, not by the kind alone — the very same
        // `SymbolKind::Knot` is a visit count in ink and a fn value in
        // native. `native_fn_value_target` is the one gate; everything it
        // declines falls through to the kind match below unchanged.
        if self.native_fn_value_target(def) {
            return self.infer_native_fn_value(def);
        }
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
            // Knot/Stitch/External/Label referenced as a bare value.
            //
            // In **ink** that is a visit count, and no function-value type
            // exists for it (the original T1c fence): `#fn(f)` is ink's
            // only fn-value spelling and it types through
            // `infer_fn_literal`, never here.
            //
            // On the **native** surface the same bare name resolving to a
            // function definition *is* a fn value since issue #1862, and
            // types as one since #1876 — [`Self::infer_native_fn_value`]
            // above already returned for exactly that case. What is left
            // here on native is everything that is *not* a function
            // definition: a plain (non-`fn`) knot/stitch visit count, an
            // `EXTERNAL` (no body to address — `E079`), a label.
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

    /// Whether a bare reference to `def` is a **native bare-name fn value**
    /// (RULED 2026-08-01, `docs/t1c-spec.md` §2a, issues #1862/#1876): the
    /// sigil-free second spelling of `#fn(def)`, admitted on the native
    /// (`.brink`) surface only.
    ///
    /// The predicate is deliberately the *same* two conjuncts
    /// `lir::lower::expr::lower_path` gates its `MakeFnValue` on — this
    /// body's file is native, and the target is a statically-named function
    /// definition ([`brink_ir::SymbolInfo::is_function_definition`], the
    /// shared creation-site predicate) — so typing and lowering can never
    /// disagree about which references are fn values. In ink the same bare
    /// name is a knot's visit count and this is always `false`.
    ///
    /// A local of the same name never reaches here: `resolve` maps the
    /// reference to the local's own `DefinitionId` (`resolve::lookup_variable`
    /// step 1 shadows every global interpretation), exactly as `lower_path`
    /// consults its temp map before its resolution map.
    fn native_fn_value_target(&self, def: DefinitionId) -> bool {
        self.ctx.native
            && self
                .ctx
                .index
                .symbols
                .get(&def)
                .is_some_and(brink_ir::SymbolInfo::is_function_definition)
    }

    /// Type a native bare-name fn value — the zero-bound-argument case of
    /// [`Self::infer_fn_literal`], whose shape it mirrors step for step
    /// (issue #1876). The partial-application form `#fn(f, a)` has no native
    /// spelling, so a bare name always binds zero arguments and the value's
    /// parameter row is the target's whole signature.
    ///
    /// The two structural records happen **before** the `known_sigs` early
    /// return, for the same reason `infer_fn_literal` orders them that way:
    /// `def_effect_atoms` runs this walk with an empty `known_sigs`, so
    /// anything after the return would be dead code in the one pass that
    /// harvests effect atoms. They are what make this reference an ordinary
    /// creation site to the effect machinery — a call-graph edge
    /// ([`Self::record_call_edge`]) plus the Fork A creation atom
    /// ([`Self::record_fn_value_creation`], issue #1726), keeping
    /// `EffectAtoms::creates_fn_values ⊆ EffectAtoms::direct_calls`.
    ///
    /// The row is `FnRow::of_target(def)`, not the unknown top element:
    /// this reference **is** the creation site and `def` is the target it
    /// names syntactically (`docs/effects-spec.md` §5/§6.1a), so the row
    /// rides along through every later join exactly as a `#fn` literal's
    /// does.
    fn infer_native_fn_value(&mut self, def: DefinitionId) -> Ty {
        self.record_call_edge(def);
        self.record_fn_value_creation(def);
        let Some(sig) = self.ctx.known_sigs.get(&def) else {
            return Ty::Unknown;
        };
        Ty::Fn(
            sig.params.clone(),
            Box::new(sig.return_ty.clone()),
            FnRow::of_target(def),
        )
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
    /// `VAR`/`CONST` never reaches this join at all (excluded by the `kind`
    /// guard below) — globals are never accumulated into this `Ty::Conflicted`
    /// lattice at all, dotted or bare (issue #1877's own `E063` check for a
    /// *bare* global assignment target, `check_declared_assign_target`,
    /// reads `ctx.globals` directly rather than joining through here) — this
    /// segment-count guard makes a `Param`/`Temp` head behave the same way:
    /// observed only for a genuine bare reference, never for a dotted field
    /// read.
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

    /// Whether `expr` is exactly the expression shape [`Self::observe`]
    /// itself would join a type into — a bare, single-segment `Path`
    /// resolving to a `Param`/`Temp` in this body (mirrors `observe`'s own
    /// `Expr::Path`/segment-count/`SymbolKind` guards verbatim, minus its
    /// `ty.is_unknown()` check, which is about the *type being joined*,
    /// not this expression's own shape). Issue #1864's direct-call
    /// argument-mismatch check consults this to skip exactly the
    /// arguments `observe` — called unconditionally right after, in
    /// [`Self::infer_call`]'s direct-call branch — is about to (possibly)
    /// drive to `Ty::Conflicted` on its own, which independently escapes
    /// as `E066`; see [`super::DirectCallArgMismatch`]'s doc.
    fn arg_is_observed_local(&self, expr: &Expr) -> bool {
        let Expr::Path(p) = expr else { return false };
        if p.segments.len() > 1 {
            return false;
        }
        let Some(def) = self.resolve(p.range) else {
            return false;
        };
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return false;
        };
        matches!(info.kind, SymbolKind::Param | SymbolKind::Temp)
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

    /// Issue #1877 (the remainder of #1864 PR #1875 left): check a `~ temp
    /// name: T = expr` initializer's own inferred type (`found`) against its
    /// explicit ascription. Unlike a param/return annotation (`annotations::
    /// mismatches`, E063, gradual/advisory only), this ascription is never
    /// otherwise compared against its *own* initializer anywhere: it is
    /// recorded into `self.annotated` purely as an Unknown-escape fallback
    /// (see the field's doc — "consulted only... never joined into the
    /// lattice"), and `bind_local`'s own `unify(Unknown, found)` join right
    /// after this runs never disagrees with a *fresh* local on its first
    /// write. So a single-write disagreement between an ascription and its
    /// own initializer — `~ temp t: int = "hi"` — reaches no other TM-3
    /// check today; no double-report risk *on this exact write* (unlike
    /// `check_declared_assign_target`'s Temp case below), since this runs
    /// once per declaration, before `bind_local` ever touches `self.locals`
    /// for this name. A *later* read of the same temp can still
    /// independently conflict it (e.g. `~ temp t: int = "hi"\n~ return t +
    /// 1`), which `check_escapes` reports as `E066` for the same
    /// disagreement —
    /// [`Self::drop_typed_assign_mismatches_conflicted_by_a_later_read`]
    /// (called post-walk, from `infer_def_body` via [`Self::finish_walk`])
    /// is what actually prevents that double-report, not anything here.
    fn check_declared_temp_init(&mut self, t: &brink_ir::TempDecl, found: &Ty) {
        if found.is_unresolved() {
            return;
        }
        let Some(te) = &t.annotation else { return };
        let Some(declared) = crate::annotations::resolve(te, &self.ctx.type_names()) else {
            return;
        };
        if declared.is_unresolved() || assignable(&declared, found) {
            return;
        }
        self.typed_assign_mismatches.push(TypedAssignMismatch {
            range: t.name.range,
            target: t.name.text.clone(),
            expected: declared,
            found: found.clone(),
        });
    }

    /// Issue #1877: check a plain assignment's RHS type (`found`) against
    /// `target`'s already-known declared type — a VAR/CONST's declaration-
    /// derived type (`ctx.globals`), or an annotated `~ temp`'s ascription
    /// (`self.annotated`). Only a bare, single-segment `Path` is checked
    /// (mirrors `observe`'s own guard — a dotted field-access target's
    /// declared type is its root's shape, not the field's, so comparing
    /// against it would be a category error).
    ///
    /// A `Param` target is deliberately **excluded** here (review finding on
    /// this issue's own PR): a param annotation is a signature-firewall slot
    /// `annotations::mismatches` (E063) already owns — that check compares
    /// the def's declared param annotation against the body's *final*
    /// inferred param type (the T1c overlay in [`infer_def_body`]), so any
    /// disagreement a Param assignment could report here is either already
    /// caught there (the common case: the write's type becomes the param's
    /// final inferred type) or already caught as `E066` once a later use
    /// conflicts it. Recording a fact here too double-reported the identical
    /// disagreement as two separate `E063`s.
    ///
    /// **Must run before [`Self::observe`]** on the same assignment: for a
    /// `Temp` target, `observe`'s join (`unify(cur, found)`) is what may
    /// drive the local to `Ty::Conflicted`, independently reported as
    /// `E066` by `strict::check_escapes`. This reads `cur` — the target's
    /// accumulated type *before* that join — to check whether this exact
    /// write is about to do that; if so, the disagreement is skipped here to
    /// avoid double-reporting the same fact as both `E063` and `E066`
    /// (mirrors [`DirectCallArgMismatch`]'s `arg_is_observed_local`
    /// exclusion, computed per-write here rather than a blanket kind
    /// exclusion — an assignment to an as-yet-`Unknown` local never goes
    /// `Conflicted` on this join and would otherwise go completely
    /// unchecked, e.g. a `~ temp t: int = expr` whose own initializer
    /// infers `Unknown`, later `~ t = "hi"`). This per-write guard only
    /// catches a disagreement that conflicts on *this* write — a later
    /// *read* can still independently conflict the same local after a fact
    /// was already recorded here;
    /// [`Self::drop_typed_assign_mismatches_conflicted_by_a_later_read`]
    /// drops that fact once the local's final type is known (see its own
    /// doc). A
    /// VAR/CONST target is never subject to either exclusion: globals are
    /// never joined into `self.locals`/Conflicted tracking at all (this
    /// module's own doc: "the inference substrate never joins a global's
    /// declaration-derived type against its assignment sites").
    fn check_declared_assign_target(&mut self, target: &Expr, found: &Ty) {
        if found.is_unresolved() {
            return;
        }
        let Expr::Path(p) = target else { return };
        if p.segments.len() > 1 {
            return;
        }
        let Some(def) = self.resolve(p.range) else {
            return;
        };
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return;
        };
        let declared = match info.kind {
            SymbolKind::Variable | SymbolKind::Constant => self.ctx.globals.get(&def).cloned(),
            SymbolKind::Temp => {
                let cur = self.locals.get(&info.name).cloned().unwrap_or(Ty::Unknown);
                if unify(&cur, found).is_conflicted() {
                    return; // `check_escapes` (E066) already reports this write.
                }
                self.annotated.get(&info.name).cloned()
            }
            _ => None,
        };
        let Some(declared) = declared else { return };
        if declared.is_unresolved() || assignable(&declared, found) {
            return;
        }
        self.typed_assign_mismatches.push(TypedAssignMismatch {
            range: p.range,
            target: info.name.clone(),
            expected: declared,
            found: found.clone(),
        });
    }

    /// Issue #1900 (split from #1864/#1877 — PR #1899's own review flagged
    /// the dotted case as excluded): a dotted assignment target (`~ p.x =
    /// expr`) checked against the field's declared type — the sibling of
    /// [`Self::check_declared_assign_target`] above for a multi-segment
    /// `Expr::Path` target, which that method explicitly declines
    /// (`p.segments.len() > 1` guard).
    ///
    /// `~ p.x = expr` lowers as ONE multi-segment `Expr::Path`, not an
    /// `Expr::FieldAccess`: `brink-syntax`'s `indexable_lvalue` consumes a
    /// bare `ident.ident…` prefix whole via `divert::path`'s dotted-path
    /// grammar; a `FIELD_ACCESS_EXPR` node is only ever produced for a
    /// `.field` segment *after* an index (`arr[i].x`). Correction (issue
    /// #1900 review finding): `E074`'s real title is "chained field-write
    /// projection (p.a.b = v) is not supported", and LIR's own
    /// `try_lower_field_assignment` emits it for BOTH shapes — a 3+-segment
    /// `Expr::Path` chain (`p.a.b = v`) *and* this `Expr::FieldAccess` shape
    /// — not only the latter. Only the `Expr::FieldAccess` shape needs no
    /// handling here (this method only ever matches `Expr::Path`, and
    /// `Expr::FieldAccess` never reaches it); the 3+-segment `Expr::Path`
    /// chain is a shape this method *does* see, fenced below to match LIR's
    /// own single-level boundary. The whole path's range resolves (the TM-4b
    /// fallback [`Self::observe`]'s own doc documents) to the *head*
    /// segment's `DefinitionId`; `p.segments[1..]` is the field-access chain
    /// past that root.
    ///
    /// Fenced to exactly `p.segments.len() == 2` (issue #1900 review
    /// finding): a longer chain (`~ o.i.a = expr`) is unsupported at *any*
    /// RHS type — LIR's `try_lower_field_assignment` rejects it outright
    /// with the non-suppressible `E074` above — so recording a
    /// [`FieldAssignMismatch`] for it here would tell the author to fix a
    /// type on a construct that stays rejected either way. Matching LIR's
    /// own boundary means the chained case only ever reports `E074`, never
    /// a shadowed `E063` first.
    ///
    /// This module has no struct-shape table (the firewall: a body never
    /// reads project-wide `STRUCT` declarations) — this only resolves the
    /// ROOT's already-known declared type, from exactly the two sources
    /// [`Self::check_declared_assign_target`] reads for its own Variable/
    /// Constant/Temp arms (`ctx.globals`, `self.annotated`) — never the
    /// live/finalized `self.locals` join, so unlike that method this never
    /// touches the `Ty::Conflicted` lattice or needs its double-report
    /// guard: [`Self::observe`] itself already never joins a dotted target
    /// at all (its own segment-count guard), so there is no accumulated
    /// per-write state to race against here. A `Param` root is included
    /// (unlike `check_declared_assign_target`'s deliberate Param exclusion,
    /// which exists only to avoid double-reporting against
    /// `annotations::mismatches`' own *whole-signature* comparison) — a
    /// param's own signature-level annotation says nothing about one of its
    /// *fields*, so there is no equivalent double-report risk here.
    ///
    /// The field chain itself is left unresolved: [`FieldAssignMismatch`] is
    /// only a candidate fact, walked the rest of the way by
    /// `structs::check_assignments` (strict-mode-only, wired into
    /// `strict::check`) against `structs::declared_shapes`/`ShapeInfo` — the
    /// same shared shape table `structs::check`'s construction-literal trio
    /// and `ref_projection::check_strict`'s E098 already reuse rather than
    /// re-deriving.
    ///
    /// Scope: a plain (non-`ref`) assignment only. A `ref`-mediated field
    /// write (`ref npc.hp` handed to a call, reassigned inside the callee)
    /// is a different aliasing channel (docs/effects-spec.md §6.1a channel
    /// 4 — the fn-value aliasing-channel enumeration, not a typed-mode
    /// struct-write channel list) this method does not reach —
    /// `ref_projection::check_strict`'s E098 already validates
    /// that a `ref` projection's own segments are legal against the root's
    /// shape, but neither it nor this method compares the eventual write's
    /// *value* against the field there; that is a distinct gap, not this
    /// issue's.
    fn check_declared_field_assign_target(&mut self, target: &Expr, op: AssignOp, found: &Ty) {
        if found.is_unresolved() {
            return;
        }
        let Expr::Path(p) = target else { return };
        if p.segments.len() != 2 {
            // Either a bare name (`check_declared_assign_target`'s job) or a
            // chained target (`p.a.b = v`, 3+ segments) — LIR's own
            // `try_lower_field_assignment` fences the writable shape at
            // exactly `segments.len() == 2` and rejects anything longer with
            // a real, non-suppressible `E074` regardless of the RHS's type,
            // so recording a type-mismatch fact for a chain this check can't
            // ever legally reach would only tell the author to fix a type on
            // a construct that stays rejected either way. Fenced here to
            // match that same boundary rather than reported and then
            // shadowed.
            return;
        }
        let Some(def) = self.resolve(p.range) else {
            return;
        };
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return;
        };
        let root_ty = match info.kind {
            SymbolKind::Variable | SymbolKind::Constant => self.ctx.globals.get(&def).cloned(),
            SymbolKind::Param | SymbolKind::Temp => self.annotated.get(&info.name).cloned(),
            _ => None,
        };
        let Some(root_ty) = root_ty else { return };
        if root_ty.is_unresolved() {
            return;
        }
        self.field_assign_mismatches.push(FieldAssignMismatch {
            root: info.name.clone(),
            root_ty,
            path: p.segments[1..].to_vec(),
            op,
            found: found.clone(),
        });
    }

    /// Issue #1877 review finding: [`Self::check_declared_assign_target`]
    /// and [`Self::check_declared_temp_init`]'s own per-write `Conflicted`
    /// guards only see whether *this* write is about to conflict its
    /// target — they cannot see a *later* read (e.g. `~ t = "hi"` then `~
    /// return t + 1`) that independently drives the same local to
    /// `Ty::Conflicted` further down the same body. That later join still
    /// reports as `E066` via `strict::check_escapes`, which reads each
    /// local's *final* whole-body type, not any single write. So a fact
    /// recorded during the walk for a name whose final `locals` entry ends
    /// up `Conflicted` would double-report the same disagreement as both
    /// `E063` and `E066`. Called once, post-walk, once every local's final
    /// type is known — the one point that actually matches what
    /// `check_escapes` keys on. A VAR/CONST target is unaffected: it is
    /// never joined into `locals` at all.
    fn drop_typed_assign_mismatches_conflicted_by_a_later_read(&mut self) {
        self.typed_assign_mismatches
            .retain(|fact| !self.locals.get(&fact.target).is_some_and(Ty::is_conflicted));
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

    /// Fork A (issue #1726): record a `#fn(target, …)` creation site's target
    /// as this body's structural fn-value creation atom — see
    /// [`BodyResult::created_fn_values`].
    ///
    /// Gated on `ctx.inferable` for the same reason
    /// [`Self::record_call_edge`]'s own first arm is: only an inferable
    /// knot/stitch has a body the effect fixpoint can follow, so only those
    /// ids are legal call-graph edges. An `EXTERNAL` `#fn` target is
    /// deliberately *not* folded in here — it has no row to join, and the
    /// call-kind atom for a host binding that is actually invoked is recorded
    /// at the invoking site by [`Self::record_call_edge`]; a value merely
    /// created and handed to the host is §6.2's manifest-declared surface,
    /// not this atom's.
    fn record_fn_value_creation(&mut self, def: DefinitionId) {
        if self.ctx.inferable.contains(&def) {
            self.created_fn_values.insert(def);
        }
    }

    // ── T2 §8 precision rung (docs/effects-spec.md §6 item 3/§8, issue #872) ──

    /// The single def a fn-value creation site traces to, if `expr` is
    /// exactly that shape: a `#fn(target, …)` literal, a native bare name
    /// resolving to a function definition (issue #1876 — the same creation
    /// site, one spelling per surface), either of them optionally through
    /// one or more `bind(…)` wrappers (partial application never changes
    /// *which* def eventually runs).
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
            // Issue #1876: on the native surface a bare name resolving to a
            // function definition is the *same* creation site, spelled
            // without the sigil ([`Self::native_fn_value_target`]) — so it
            // traces to exactly the same origin `#fn(target)` does. Without
            // this arm the value #1862 made writable would be untraceable:
            // `map(items, double)` would poison `map`'s §6.1 row-variable
            // hole and a `let f = double; f(…)` would fall to the pessimal
            // floor, purely because of which spelling was used. Same
            // `is_effect_edge_target` gate as the literal arm.
            Expr::Path(p) => {
                let def = self.resolve(p.range)?;
                (self.native_fn_value_target(def) && self.is_effect_edge_target(def)).then_some(def)
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
    /// Temp local (checked post-walk against its whole-body write summary) is
    /// [`ValueCallOrigin::Local`]; one of this definition's own non-`ref`
    /// params is [`ValueCallOrigin::Param`] (§6.1's row variable, issue
    /// #1680); anything else (a VAR/CONST global — the heap case, still
    /// coarse per spec §5/§8 — or an unresolvable/non-local kind) is
    /// [`ValueCallOrigin::Unknown`].
    ///
    /// **A Param is never narrowed to a write summary** (soundness, not a
    /// missed optimization): a Temp cannot be referenced before its defining
    /// `TempDecl`, so the join over its whole-body writes really does bound
    /// its value at every read. A Param, by contrast, carries an implicit
    /// caller-provided initial value that [`Self::local_fn_origins`] never
    /// sees — a param reassigned inside the body would summarize as fully
    /// traced and [`Self::resolve_pending_value_calls`] would narrow *every*
    /// call site through it, including ones reachable before that
    /// reassignment, where the param still holds the caller's arbitrary
    /// (unknown) fn value. That would violate the conservative-total
    /// invariant this rung promises to preserve.
    ///
    /// §6.1's row variable answers the *same* question from the other end —
    /// don't narrow at the callee, defer to the call site — so it is not
    /// subject to that hazard: the hole is filled with what a caller actually
    /// passed. The caller-provided initial value is precisely the thing being
    /// substituted. What it *does* need is that the body never replaces it,
    /// which [`Self::param_writes`] tracks and
    /// [`Self::resolve_pending_value_calls`] enforces.
    fn local_call_origin(&self, def: DefinitionId) -> ValueCallOrigin {
        match self.ctx.index.symbols.get(&def) {
            // Issue #1779: a Temp whose name is currently shadowed by an
            // active lambda's own param (`self.lambda_param_names`) must not
            // trust `local_fn_origins[name]` — see that field's doc. This
            // check is by *name*, matching the map's own keying, and is
            // strictly more conservative than the pre-fix behavior (it only
            // ever turns a `Local` into an `Unknown`, never the reverse), so
            // it cannot introduce a new under-report of its own.
            Some(info) if info.kind == SymbolKind::Temp => {
                if self.lambda_param_names.contains_key(&info.name) {
                    ValueCallOrigin::Unknown
                } else {
                    ValueCallOrigin::Local(info.name.clone())
                }
            }
            Some(info) if info.kind == SymbolKind::Param => self
                .param_index
                .get(&info.name)
                .copied()
                .map_or(ValueCallOrigin::Unknown, ValueCallOrigin::Param),
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

    /// Fold one write to a Temp local into that name's [`LocalFnOrigins`]
    /// summary — a traced `#fn`/`bind`-chain origin joins `targets`, an
    /// untraced one poisons the name. A no-op for any other resolution
    /// (mirrors [`Self::observe`]'s own guard: a VAR/CONST/other target isn't
    /// tracked by this local-only rung at all, the heap case stays pessimal
    /// per spec §5/§8).
    fn bump_local_write(&mut self, name: &str, origin: Option<DefinitionId>) {
        let entry = self.local_fn_origins.entry(name.to_string()).or_default();
        match origin {
            Some(def) => {
                entry.targets.insert(def);
            }
            None => entry.untraced = true,
        }
    }

    /// [`Self::bump_local_write`] for an `Assignment`/`BlockStmt::Assignment`
    /// target expression — only a bare single-segment `Path` resolving to a
    /// Temp counts (a dotted/indexed target reassigns a *nested* slot, not
    /// the local's own value, same guard [`Self::observe`] applies).
    ///
    /// A Param write is deliberately *not* folded into
    /// [`Self::local_fn_origins`] — not for soundness (folding a param write
    /// in can only ever *add* to a name's summary, and a Param is never
    /// classified as [`ValueCallOrigin::Local`] in the first place, so this
    /// can't under-report), but to avoid contaminating a same-named Temp
    /// elsewhere in the body: without this guard, a Param write would land
    /// under that param's name, and if a Temp happens to share the name the
    /// two summaries would collide in the same map entry. It is instead
    /// recorded in [`Self::param_writes`], which §6.1 (issue #1680) needs to
    /// know about: a param the body reassigns no longer holds the caller's
    /// argument, so it must not carry a row variable.
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
        match info.kind {
            SymbolKind::Temp => {
                let name = info.name.clone();
                self.bump_local_write(&name, origin);
            }
            SymbolKind::Param => {
                if let Some(&idx) = self.param_index.get(&info.name) {
                    self.param_writes.insert(idx);
                }
            }
            _ => {}
        }
    }

    /// Every post-walk finalization step that needs the whole body's final
    /// state (every local's accumulated type, every write counted) rather
    /// than whatever was known mid-walk — called once, right after
    /// [`InferPass::infer_block`] returns. Composes
    /// [`Self::resolve_pending_value_calls`] (T2 §8 precision rung, issue
    /// #872 — see its own doc for why a mid-walk read would be unsound) and
    /// [`Self::drop_typed_assign_mismatches_conflicted_by_a_later_read`]
    /// (issue #1877 review finding — see its own doc).
    fn finish_walk(&mut self) {
        self.resolve_pending_value_calls();
        self.drop_typed_assign_mismatches_conflicted_by_a_later_read();
    }

    /// Resolve every value-call site recorded during the walk
    /// ([`Self::check_value_call`]'s `pending_value_calls.push`) against the
    /// now-final `local_fn_origins` — **must** run after
    /// [`InferPass::infer_block`] finishes the whole body (see
    /// `local_fn_origins`'s field doc for the loop-carried-reassignment
    /// hazard a mid-walk read would risk). `Inline` narrows unconditionally;
    /// `Local` narrows to the **join over every creation target its writes
    /// traced to** (Fork A, issue #1726 — a name written twice with two known
    /// origins reaches one of them, so joining both is conservative);
    /// `Unknown`, a `Local` with an untraced write, and a `Local` never
    /// written at all keep the pessimal floor — `effect_opaque = true`,
    /// exactly what every value call unconditionally set before this rung
    /// existed. A narrowed call routes through [`Self::record_call_edge`],
    /// the same edge a direct call/`#fn` creation site records, so it
    /// correctly lands in `self.calls` (inferable knot/stitch — joins via the
    /// SCC effect fixpoint) or `self.external_calls` (an `EXTERNAL` binding —
    /// a call-kind atom) per what the origin actually is.
    ///
    /// **What Fork A changed and why it is still sound.** The pre-#1726 rule
    /// demanded the name be written *exactly once*, because it narrowed to a
    /// *single* def and a second write would have made that choice arbitrary
    /// — an under-report of whichever write the analysis didn't pick. Joining
    /// every traced write's target instead removes the choice: the row covers
    /// all of them. The one guard that must survive is the untraced write —
    /// a value the body did not create can come from anywhere, including a
    /// host callback, so it keeps the floor.
    fn resolve_pending_value_calls(&mut self) {
        let pending = std::mem::take(&mut self.pending_value_calls);
        for origin in pending {
            // §6.1 (issue #1680): a call through an unwritten non-`ref` param
            // is a *row variable*, not an edge — this body's row stays
            // parametric in it and each caller instantiates it. A param the
            // body writes keeps the pre-#1680 pessimal floor.
            if let ValueCallOrigin::Param(idx) = origin {
                if self.param_writes.contains(&idx) {
                    self.effect_opaque = true;
                } else {
                    self.param_holes.insert(idx);
                }
                continue;
            }
            let narrowed: BTreeSet<DefinitionId> = match origin {
                ValueCallOrigin::Inline(def) => BTreeSet::from([def]),
                ValueCallOrigin::Local(name) => match self.local_fn_origins.get(&name) {
                    Some(summary) if !summary.untraced => summary.targets.clone(),
                    _ => BTreeSet::new(),
                },
                ValueCallOrigin::Param(_) | ValueCallOrigin::Unknown => BTreeSet::new(),
            };
            if narrowed.is_empty() {
                self.effect_opaque = true;
            } else {
                for def in narrowed {
                    self.record_call_edge(def);
                }
            }
        }
        self.resolve_pending_call_fn_args();
    }

    /// §6.1 caller half (issue #1680): record what this call site passes in
    /// each argument position of a direct call to an **inferable** target, so
    /// the effect fixpoint can fill that callee's row variables.
    ///
    /// Only inferable (knot/stitch) callees have a row with holes, so nothing
    /// else is recorded — and an *absent* entry already reads as "cannot
    /// fill", which is why a call site this is never invoked from degrades
    /// safely rather than silently narrowing.
    ///
    /// Every argument is recorded, including the ones that classify to
    /// nothing: the map is keyed by `(callee, position)` and joined over all
    /// of this body's call sites, so a second site passing an untraced value
    /// in a position a first site passed `#fn(g)` in **must** poison that
    /// position. Skipping the unclassifiable case would leave the position
    /// looking fillable and under-report the second site's callback.
    ///
    /// Purely structural (a `#fn` target is a syntactic name, a Temp's origin
    /// summary is a syntactic write set), so this never makes a call-graph
    /// edge depend on an inferred row — §6.1a's acyclicity requirement.
    fn record_call_arg_fn_origins(&mut self, def: DefinitionId, args: &[Expr]) {
        if !self.ctx.inferable.contains(&def) {
            return;
        }
        for (i, arg) in args.iter().enumerate() {
            let Ok(idx) = u32::try_from(i) else { continue };
            let origin = self.value_call_origin(arg);
            self.pending_call_fn_args.push((def, idx, origin));
        }
    }

    /// Fold every recorded call-site argument observation into
    /// [`Self::call_fn_args`], against the now-final `local_fn_origins` — the
    /// same post-walk timing (and for the same loop-carried-reassignment
    /// reason) as [`Self::resolve_pending_value_calls`].
    ///
    /// An `Inline` argument contributes its creation target; a `Local` one
    /// contributes its whole-body write summary, and poisons the position if
    /// any of those writes was untraced; a `Param` argument would chain one
    /// row variable into another, which §6.1's shallow polymorphism does not
    /// do, so it poisons the position too.
    fn resolve_pending_call_fn_args(&mut self) {
        let pending = std::mem::take(&mut self.pending_call_fn_args);
        for (def, idx, origin) in pending {
            let (targets, untraced) = match origin {
                ValueCallOrigin::Inline(target) => (BTreeSet::from([target]), false),
                ValueCallOrigin::Local(name) => match self.local_fn_origins.get(&name) {
                    Some(summary) => (summary.targets.clone(), summary.untraced),
                    None => (BTreeSet::new(), true),
                },
                ValueCallOrigin::Param(_) | ValueCallOrigin::Unknown => (BTreeSet::new(), true),
            };
            let entry = self.call_fn_args.entry((def, idx)).or_default();
            entry.targets.extend(targets);
            entry.untraced |= untraced;
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
    ///
    /// Fork A conservative-total fix (review finding on issue #1726's PR): a
    /// `ref` slot can just as well be handed a *Temp* local (`~ poke(f, cb)`
    /// where `f` is `~ temp f = #fn(bar)`) — the callee can reassign it to
    /// the caller's arbitrary argument (here `h`, which could hold anything),
    /// so from this call site's perspective `root`'s value after the call is
    /// untraced. `record_write` alone only folds VAR/CONST globals
    /// (`local_fn_origins` never sees it), so under Fork A's join-over-writes
    /// rule an all-traced Temp summary would silently narrow a call through
    /// `f` post-`poke` to the pre-`poke` targets only — an under-report. The
    /// old write-*once* rule accidentally covered this (two writes already
    /// forced the pessimal floor); joining removes that incidental cover, so
    /// the untraced write has to be recorded explicitly here too.
    ///
    /// **Creation sites too (issue #1755).** `ref` binds at two distinct
    /// grammar positions, not one: a *call*'s argument list
    /// ([`Self::infer_call`], [`Self::infer_target`]) and a `#fn`
    /// **creation** site's bound prefix ([`Self::infer_fn_literal`] —
    /// `#fn(heal, player_hp)` binds `heal`'s `ref hp` to the cell
    /// `player_hp`, docs/t1c-spec.md §2). Only the first was recorded until
    /// #1755, and the write a creation-site binding causes was consequently
    /// recorded **nowhere**: not here, not in the callee's own body (where the
    /// target resolves as a `Param`, never a `Variable`/`Constant`, exactly as
    /// the first paragraph explains), and not at the eventual `f(5)` call site
    /// (which narrows through `local_fn_origins` to the target def but carries
    /// no record of which cell that value was *created* against). That was a
    /// conservative-total (docs/effects-spec.md §3) under-report — the
    /// direction the row is never allowed to move — so the creation site
    /// charges it, adopting #1755's option (a).
    ///
    /// Charging it at creation is **coarse but sound**: the write lands in the
    /// creating body's row whether or not that body ever calls the value it
    /// made (it may return it, store it, or drop it). Over-reporting is the
    /// permitted direction, and creation is the only site that can still see
    /// the binding — a value handed out carries its bound cell with it and no
    /// later call site names that cell again. The precise alternative — giving
    /// `ref` params their own row-variable/hole treatment analogous to
    /// §6.1b's non-`ref` `param_holes`, resolved against the concrete cell
    /// bound at each creation site — remains open as a §8 precision rung
    /// under #1809, not a soundness question.
    ///
    /// Nothing about this second call path is frame-scoped: the fields it
    /// touches (`effect_writes`, `param_writes` via `record_fn_write`) are
    /// §4.1's *cumulative* bucket, and `local_fn_origins` is reached through
    /// the same `record_fn_write` the call-site path already used, under
    /// whatever frame is live. A `#fn` literal inside a lambda body therefore
    /// absorbs into the enclosing def's row exactly like every other atom,
    /// per §4.1's absorption rule — no new field, and no change to
    /// `infer_lambda`'s snapshot set.
    fn record_ref_param_writes(&mut self, def: DefinitionId, args: &[Expr]) {
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return;
        };
        for (i, arg) in args.iter().enumerate() {
            if info.params.get(i).is_some_and(|p| p.is_ref) {
                let root = ref_arg_root(arg);
                self.record_write(root);
                self.record_fn_write(root, None);
            }
        }
    }

    // ── Expressions ──────────────────────────────────────────────────

    #[expect(
        clippy::too_many_lines,
        reason = "one match arm per Expr variant; splitting would obscure the dispatch"
    )]
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
            Expr::Infix(ie) => self.infer_infix(&ie.lhs, ie.op, &ie.rhs),
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
            // A lambda (issue #1685) is fn-colored always, so its type is a
            // `Ty::Fn` row — since #1910, built from what the lambda's own
            // body infers (mono-HM narrowing read back from the walk, the
            // same overlay a top-level `fn`'s own params/return already get
            // via `infer_def_body`), with what is *written* only the
            // fallback where the body-derived type is still `Unknown`. Mono-
            // HM narrowing of a lambda's own params from its concrete *call
            // sites* is still not modeled in this slice (`scaled`'s own
            // doc, `infer_lambda`'s "boundary this fix does NOT cross") —
            // inventing a type from a call site would be worse than an
            // honest `Unknown`, unlike a type the body itself already pins.
            // See `infer_lambda` for the full read-back mechanics. The
            // body's value expression is inferred for its side effects
            // (nested calls, uses) exactly like an interpolation's is.
            //
            // `Ty::Fn` has carried an effect row since #1680 step 3, but a
            // lambda's is the unknown top element: composing one would mean
            // naming a creation target, and a lambda has no `DefinitionId`
            // until LIR mints it (#1727). See `infer_lambda`.
            Expr::Lambda(l) => self.infer_lambda(l),
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
            // Block capture (issue #1839, `docs/decision-log.md` 2026-08-01
            // "Content-as-value"): a `content`-typed value. The captured
            // statements are inferred through the ordinary `infer_stmt`
            // path — the same one the enclosing body's own statements go
            // through — so any call/effect inside a captured line is
            // observed exactly as it would be at its original top-level
            // position, not silently skipped.
            Expr::Fragment(stmts) => {
                for s in stmts {
                    self.infer_stmt(s);
                }
                Ty::Content
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
        // Issue #1924 follow-up review on #1910: a multi-segment path can be
        // a genuine dotted namespace reference (`knot.stitch`) or a field
        // access spelled as a path (`p.x`) — the latter resolves `def` to
        // its *head* variable, not the field, because no static field-type
        // table exists yet (#1924, the read-side twin of #994's write-side
        // fix). `ty_of_def` below can't tell the two apart and returns the
        // head's own type either way; flag the value-symbol case so
        // `infer_lambda`'s `body_ty`/`narrowed_params` overlay (#1910) knows
        // not to trust anything derived from this walk — see
        // `Self::dotted_field_read_tainted`'s own field doc. A namespace
        // reference resolves to a symbol kind not in this set, so it is
        // unaffected.
        if p.segments.len() > 1
            && let Some(info) = self.ctx.index.symbols.get(&def)
            && matches!(
                info.kind,
                SymbolKind::Param | SymbolKind::Temp | SymbolKind::Variable | SymbolKind::Constant
            )
        {
            self.dotted_field_read_tainted = true;
        }
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
            // String-numeric display concatenation (issue #1911, spec §4):
            // `+` between a `string` and an `int`/`float`, either operand
            // order, is ink's core display-concat idiom ("t:" + total`,
            // `keys + ":" + total`) — the runtime already defines it
            // (`value_ops::binary_op`'s `String`/`Int` and `String`/`Float`
            // `Add` arms stringify the numeric side and produce `string`;
            // it never faults). Treating `+` as a same-type operator here
            // marked the numeric operand `Conflicted` on code that
            // compiles, runs, and is in the golden corpus today
            // (`tests/tier1-native/for-k-v`) — the compatibility floor is
            // "don't reject what the runtime accepts". Scoped narrowly to
            // match that floor exactly: `Add` only (the runtime defines no
            // string-numeric `Sub`/`Mul`/`Div`/`Mod`), and `Int`/`Float`
            // only (there is no `String`/`Bool` `Add` arm, so that
            // combination still falls through to the general same-type
            // unify below and reports `E066` as before). Neither operand's
            // own type is unified with the other's — concatenation
            // observes nothing new about either side, unlike the general
            // arithmetic arm's cross-side `observe` calls.
            InfixOp::Add if is_string_numeric_concat(&l, &r) => Ty::String,
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
            // infallible-absorption idiom `unify` uses elsewhere) — this
            // pass only ever *computes* the type, it never diagnoses.
            // `coalesce::check` is the strict-mode-only pass that
            // re-runs `coalesce` at this same expression's own site and
            // pushes `E066` directly when it disagrees (review finding on
            // PR #1469/#1460): the generic Conflicted-escape check
            // (`strict::check`'s own `E066`) is *not* a sufficient backstop
            // on its own, since it only fires once a `Conflicted` value
            // reaches a signature or body-local slot boundary — a
            // coalescing expression used directly in content/argument
            // position never does. Under `types = gradual` neither compile-
            // time check runs; the runtime `TypeError` fault
            // (`value_ops::coalesce_unwrap_some`, backing
            // `Opcode::CoalesceSome`) is the sole backstop there, and only
            // for a non-Option `lhs` (see that function's own doc for the
            // `Mismatch` case's narrower coverage).
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

    /// Issue #1909: the **result type** of a UFCS-shaped call
    /// (`recv.name(args)`) that desugars to a free function
    /// (`name(recv, args)`), plus the call-graph edge that desugar implies.
    ///
    /// `brink_analyzer::ufcs` owns the *verdict* — which of D1's four
    /// outcomes a UFCS site is — but it runs strictly **after** inference
    /// (it is type-directed: [`ufcs::resolve`] takes the finished
    /// [`InferenceResult`] to type receivers with), so it can never be the
    /// thing that hands this pass a result type. Before this, every UFCS
    /// call expression therefore typed `Ty::Unknown`, and that `Unknown`
    /// propagated: `fn f() { let n = 21; return n.double(); }` reported
    /// `E065` on `f`'s *return type* while the byte-equivalent
    /// `return double(n);` was clean, and an `Unknown` result reaching a
    /// call position is what [`Self::check_value_call`]'s own `Unknown` arm
    /// turns into a further diagnostic downstream (the #1895 precedent).
    ///
    /// This re-derives only the one outcome it can reach without any of
    /// that pass's inputs, and refuses (staying `Unknown`, exactly as
    /// before) everywhere else:
    ///
    /// - **The desugar target** is looked up with
    ///   [`resolve::lookup_unique_by_name`], the scope-free subset of the
    ///   [`resolve::lookup_by_name`] call `ufcs::try_free_fn_desugar` makes
    ///   — a [`BodyCtx`] carries no file imports, so an *ambiguous*
    ///   cross-module name is declined rather than guessed at.
    /// - **The arity must already agree** (`params.len() == arg_count + 1`,
    ///   counting the receiver as the first argument, the same convention
    ///   the desugar's own `E031` uses). A wrong-arity call is a hard error
    ///   one pass later; typing it would be typing a program that does not
    ///   compile.
    /// - **A `Ty::Struct` receiver's *result type* is declined outright.**
    ///   D1 says field access *wins* over a same-named free function, and
    ///   deciding that needs the declared-shape table
    ///   (`structs::declared_shapes`), which is not threaded into inference.
    ///   Declining keeps the *result type* from ever contradicting the
    ///   verdict `ufcs` will reach; a struct receiver's `FieldCall` result
    ///   type stays `Unknown` (tracked separately), it does not silently
    ///   become the free function's. This says nothing about the call-graph
    ///   **edge**, which is already recorded above by the time this gate
    ///   runs — see "Why the edge is recorded before the receiver is even
    ///   typed" below. A struct receiver whose shape declares an `Fn`-typed
    ///   field of the called name still gets an edge to a same-named,
    ///   matching-arity free function that D1 never actually invokes. That
    ///   is a deliberate over-approximation, not a bug: the recorded call
    ///   graph is always a *superset* of the real one, so a spurious edge
    ///   can at worst pull two unrelated defs into the same SCC or union an
    ///   unrelated effect row — never miss a real call. Narrowing it needs
    ///   the same shape table the result-type gate is missing.
    /// - A **prelude verb** (`m.len()`, `UfcsVerdict::PreludeDesugar`) has
    ///   no index symbol, so it never resolves here and keeps typing
    ///   `Unknown`. Typing it means running [`Self::infer_intrinsic`] on
    ///   the desugared argument list, whose arms `observe` operands and
    ///   record `array_remove_calls` — the latter would double-report
    ///   `E149` against `ufcs::strict_verdict_diagnostics`' own copy. That
    ///   is issue #1540's second symptom, deliberately left alone here.
    /// - A **projected receiver** (`party.members.heal(5)`) needs the same
    ///   shape table the struct case does, so only a single-segment
    ///   receiver is typed.
    ///
    /// ## Why the edge is recorded before the receiver is even typed
    ///
    /// [`Self::record_call_edge`] is a *structural* fact, and the three
    /// per-def entry points that harvest structural facts
    /// ([`super::call_edges`], [`super::referenced_globals`],
    /// [`super::def_effect_atoms`] — `brink-db`'s fine-grained salsa
    /// queries) all run this walk with an **empty globals map** and discard
    /// every computed type. Gating the edge on the receiver's type would
    /// make a `VAR`-receiver call an edge in `infer_project`'s monolithic
    /// walk and *not* an edge in the composed one, so the two would infer
    /// different signatures. Everything above the receiver gate reads only
    /// the symbol index, so the edge is identical in both. (The result type
    /// below is free to read `ctx.globals`: it is discarded by exactly
    /// those three callers, and `solve_scc` — the one caller whose types
    /// survive — always gets a real globals map.)
    fn infer_ufcs_free_fn_result(
        &mut self,
        path: &HirPath,
        receiver_def: DefinitionId,
        arg_count: usize,
    ) -> Ty {
        let Some((method, receiver_segs)) = path.segments.split_last() else {
            return Ty::Unknown;
        };
        let Some(target) = crate::resolve::lookup_unique_by_name(
            self.ctx.index,
            &method.text,
            &[SymbolKind::Knot, SymbolKind::External],
        ) else {
            return Ty::Unknown;
        };
        let Some(info) = self.ctx.index.symbols.get(&target) else {
            return Ty::Unknown;
        };
        if info.params.len() != arg_count + 1 {
            return Ty::Unknown;
        }
        self.record_call_edge(target);

        if receiver_segs.len() != 1 {
            return Ty::Unknown;
        }
        let receiver_ty = self.ufcs_receiver_ty(receiver_def);
        if receiver_ty.is_unresolved() || matches!(receiver_ty, Ty::Struct(_)) {
            return Ty::Unknown;
        }
        self.ctx
            .known_sigs
            .get(&target)
            .map_or(Ty::Unknown, |sig| sig.return_ty.clone())
    }

    /// The UFCS receiver's own type, **read-only** — the deliberate
    /// counterpart to [`Self::ty_of_def`], which additionally records a
    /// `VAR`/`CONST` read into `referenced_globals`.
    ///
    /// A receiver is only inspected here to decide whether D1's
    /// field-access-wins rule could possibly apply (see
    /// [`Self::infer_ufcs_free_fn_result`]); it is not a *reference* this
    /// pass newly discovered, and recording one would change
    /// [`super::EffectAtoms::reads`] for every UFCS call on a global — an
    /// effect-row change that has nothing to do with typing the call's
    /// result.
    fn ufcs_receiver_ty(&self, def: DefinitionId) -> Ty {
        let Some(info) = self.ctx.index.symbols.get(&def) else {
            return Ty::Unknown;
        };
        match info.kind {
            SymbolKind::Param | SymbolKind::Temp => {
                self.locals.get(&info.name).cloned().unwrap_or(Ty::Unknown)
            }
            SymbolKind::Variable | SymbolKind::Constant => {
                self.ctx.globals.get(&def).cloned().unwrap_or(Ty::Unknown)
            }
            _ => Ty::Unknown,
        }
    }

    fn infer_call(&mut self, path: &HirPath, args: &[Expr]) -> Ty {
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer_expr(a)).collect();
        // `path.range` here is the callee `Path`'s whole span — this
        // `resolve` lookup is one of the four consumers keyed on the
        // call-path `ResolvedRef::range` contract (issue #1561); see that
        // field's doc. Below, the B3a branch handles a multi-segment
        // (dotted UFCS) path explicitly.
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
                // B3a (issue #1482): a *multi-segment* callee path whose
                // resolution landed on a value is a UFCS-shaped call
                // (`g.greet(3)`) — the resolver records the receiver as the
                // target, the trailing segment being a field name or a free
                // function's name. The receiver is not itself the thing
                // being called, so classifying it as a T1c call-through-a-
                // value would report the receiver's own (non-`Fn`) type as
                // "not callable" — a false `E066` on every legal method
                // call. `brink-analyzer::ufcs` owns this site's *checking*
                // — but not, since issue #1909, the call's own **result
                // type**: see [`Self::infer_ufcs_free_fn_result`].
                if path.segments.len() > 1 {
                    // Issue #1881: this pass cannot check anything against
                    // a UFCS receiver directly (see the comment above), but
                    // `arg_tys` — every *written* argument's type — was
                    // already computed above regardless, as a side effect
                    // of walking each argument expression. That is exactly
                    // the raw data `brink_analyzer::ufcs`'s own resolution
                    // pass is missing to complete its own check (it has the
                    // resolved free-function target and the target's
                    // declared param types; it has no expression-type
                    // inference of its own to type these arguments with).
                    // Record it unconditionally here — the same "recorded
                    // regardless of policy, reported only by strict mode"
                    // posture `direct_call_arg_mismatches` follows — for
                    // `ufcs::UfcsVisitor` to read back via this def's own
                    // `BodyTypes`.
                    self.ufcs_call_args.push(UfcsCallArgs {
                        range: path.range,
                        args: arg_tys.clone(),
                    });
                    return self.infer_ufcs_free_fn_result(path, def, args.len());
                }
                return self.infer_value_call(path, def, args, &arg_tys);
            }
            self.record_call_edge(def);
            self.record_ref_param_writes(def, args);
            self.record_call_arg_fn_origins(def, args);
            // Issue #1995/#1920: which positions are declared `ref` lives
            // on the symbol index's own `params` (`InferredSig` carries no
            // `is_ref` bit — see `record_ref_param_writes`'s doc), so it has
            // to be read from `self.ctx.index` here too, alongside
            // `known_sigs`'s inferred param types.
            let ref_positions = self.ctx.index.symbols.get(&def);
            return self.ctx.known_sigs.get(&def).map_or(Ty::Unknown, |sig| {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(param_ty) = sig.params.get(i) {
                        // Issue #1864: a direct call's argument types were
                        // only ever *observed* below — never checked
                        // against the callee's already-known declared
                        // param type, unlike a call through a function
                        // value (`check_value_call`'s `ArgMismatch`, T1c).
                        // Excludes an argument that `observe` (right
                        // below) is about to join `param_ty` into — a
                        // genuine disagreement there drives that local to
                        // `Ty::Conflicted` on its own, already reported as
                        // `E066`; see `super::DirectCallArgMismatch`'s doc
                        // for why reporting it again here would double up.
                        //
                        // Issue #1995/#1920: a `ref` parameter both reads
                        // and writes through the caller's own cell, so its
                        // argument is checked with `ref_assignable`
                        // (invariant) rather than `assignable`'s covariant
                        // widening — `assignable(Float, Int)` is `true`,
                        // which let a `ref float` parameter accept an `int`
                        // cell and write a `float` back through it.
                        let is_ref_param = ref_positions
                            .and_then(|info| info.params.get(i))
                            .is_some_and(|p| p.is_ref);
                        // Issue #1995 review finding (BLOCKING): the
                        // observed-local skip used to gate the whole check,
                        // which silently dropped a `ref` widening
                        // (`ref float` ← an `int` cell) whenever the
                        // argument was a bare Param/Temp — `unify(Int,
                        // Float)` never goes `Conflicted`, so `observe`
                        // (below) never reports it as `E066` either. The
                        // skip is only sound for the non-ref, "would already
                        // conflict" case; a ref mismatch that stays
                        // `assignable` in the covariant direction (the
                        // widening pair) still needs its own report even
                        // when the argument is an observed local.
                        let observed = self.arg_is_observed_local(arg);
                        if !param_ty.is_unresolved()
                            && let Some(arg_ty) = arg_tys.get(i)
                            && !arg_ty.is_unresolved()
                            && if is_ref_param {
                                !ref_assignable(param_ty, arg_ty)
                                    && (!observed || assignable(param_ty, arg_ty))
                            } else {
                                !observed && !assignable(param_ty, arg_ty)
                            }
                        {
                            let callee = path
                                .segments
                                .iter()
                                .map(|s| s.text.as_str())
                                .collect::<Vec<_>>()
                                .join(".");
                            self.direct_call_arg_mismatches.push(DirectCallArgMismatch {
                                range: path.range,
                                callee,
                                index: i,
                                expected: param_ty.clone(),
                                found: arg_ty.clone(),
                            });
                        }
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
            // Issue #1168 (review correction, w65): NOT every intrinsic arm
            // only inspects `arg_tys` to shape its own result — `contains`,
            // `push`/`heap_push`, `insert`, `remove`, `index_of`, `get`, and
            // `contains_value` all call `self.observe(<sibling arg>, <type
            // derived from arg_tys[0]>)`, writing evidence into
            // `self.locals` for a *second* argument. Passing an
            // annotation-shadowed `arg_tys` into `infer_intrinsic`
            // uniformly would make one param's annotation become body
            // *evidence* for a sibling param — exactly the seeding
            // `infer_def_body`'s "overlay, never replace" design exists to
            // avoid, and would silently discard the sibling's own
            // annotation or manufacture a spurious `E066` if the sibling is
            // later compared against something else.
            //
            // So `arg_tys` (below) stays the original, evidence-only
            // vector — every observe-bearing arm keeps matching on it
            // unchanged. `read_tys` is a *separate* fallback-shadowed copy,
            // threaded through only for arms that shape their own return
            // type from a value that is purely read, never joined against a
            // second operand (`some(x)` → `Option[typeof x]`; `get(m, k)`'s
            // *return type* — its `observe(key_arg, k)` call still matches
            // on the unshadowed `arg_tys`, so `m`'s annotation alone can
            // never seed `k`'s inference). See `own_annotation`'s doc for
            // why `infer_infix` deliberately gets neither slice.
            let read_tys: Vec<Ty> = args
                .iter()
                .zip(arg_tys.iter())
                .map(|(a, ty)| self.or_own_annotation(a, ty.clone()))
                .collect();
            return self.infer_intrinsic(&seg.text, path.range, args, &arg_tys, &read_tys);
        }
        Ty::Unknown
    }

    /// Annotation fallback for a *value-position* read whose observed type
    /// came back `Unknown` purely because nothing in the body compared or
    /// combined it with anything else yet — the "pass a param straight
    /// through" case (issue #1168: `some(x)`, `get(m, k)`, a `for` loop's
    /// iterable). Mirrors [`Self::annotated_callee_ty`]'s exact resolution
    /// shape (a real def's name, or the bare single-segment fallback for a
    /// locals-stripped index) but for the *value*, not the *callee*.
    ///
    /// `infer_infix`'s comparison/arithmetic arms deliberately never call
    /// this: TM-2's "a body use that disagrees with the annotation still
    /// infers its own concrete type" guarantee (`E063` needs two
    /// independent derivations, `overlay_never_replaces_a_concrete_
    /// body_derivation`) depends on those operands seeing `Unknown`, not
    /// the annotation, on their very first read — this fallback is only
    /// safe at read sites that don't themselves produce counter-evidence.
    fn own_annotation(&self, expr: &Expr) -> Option<Ty> {
        let Expr::Path(p) = expr else {
            return None;
        };
        if let Some(def) = self.resolve(p.range)
            && let Some(name) = self.ctx.index.symbols.get(&def).map(|i| i.name.clone())
            && let Some(ann) = self.annotated.get(&name)
        {
            return Some(ann.clone());
        }
        if let [seg] = p.segments.as_slice()
            && let Some(ann) = self.annotated.get(&seg.text)
        {
            return Some(ann.clone());
        }
        None
    }

    /// Apply [`Self::own_annotation`]'s fallback to an already-computed
    /// `ty` only when it's `Unknown` — a concrete or `Conflicted` body
    /// derivation always wins outright (same "overlay, never replace"
    /// posture as every other annotation fallback in this module).
    fn or_own_annotation(&self, expr: &Expr, ty: Ty) -> Ty {
        if ty.is_unknown() {
            self.own_annotation(expr).unwrap_or(ty)
        } else {
            ty
        }
    }

    /// Capture the current value of every frame-scoped field (issue #1816)
    /// — see [`FrameSnapshot`]'s doc for why the exhaustive destructuring
    /// below (no `..` rest pattern) is the point, not an incidental style
    /// choice.
    fn frame_snapshot(&self) -> FrameSnapshot {
        let InferPass {
            ctx: _,
            locals,
            return_ty,
            has_value_return,
            calls: _,
            referenced_globals: _,
            effect_writes: _,
            external_calls: _,
            effect_opaque: _,
            effect_emits: _,
            effect_tags: _,
            effect_faults: _,
            effect_faults_refined: _,
            annotated,
            value_calls: _,
            direct_call_arg_mismatches: _,
            typed_assign_mismatches: _,
            field_assign_mismatches: _,
            // Deliberately cumulative, not frame-scoped (issue #1994): a
            // mismatch a nested lambda's own annotation-precedence overlay
            // records must survive into the enclosing body's own result,
            // exactly like `typed_assign_mismatches` above — never
            // snapshotted/restored around a lambda's own frame window.
            lambda_annotation_mismatches: _,
            ufcs_call_args: _,
            array_remove_calls: _,
            created_fn_values: _,
            local_fn_origins,
            pending_value_calls: _,
            lambda_param_names: _,
            param_index: _,
            param_writes: _,
            param_holes: _,
            pending_call_fn_args: _,
            call_fn_args: _,
            // Deliberately cumulative, not frame-scoped: `infer_lambda`
            // manages this field's lifetime itself (reset before, consulted
            // and restored after each lambda's own body walk) — see its own
            // field doc for why the frame-snapshot mechanism is the wrong
            // tool here.
            dotted_field_read_tainted: _,
        } = self;
        FrameSnapshot {
            return_ty: return_ty.clone(),
            has_value_return: *has_value_return,
            locals: locals.clone(),
            annotated: annotated.clone(),
            local_fn_origins: local_fn_origins.clone(),
        }
    }

    /// Write a [`FrameSnapshot`] back over every frame-scoped field (issue
    /// #1816) — the other half of [`Self::frame_snapshot`]'s guard. The
    /// exhaustive destructure of `snapshot` (no `..`) means a field added to
    /// [`FrameSnapshot`] but never assigned back here is also a compile
    /// error, not just a field added to `InferPass` and never captured.
    fn restore_frame(&mut self, snapshot: FrameSnapshot) {
        let FrameSnapshot {
            return_ty,
            has_value_return,
            locals,
            annotated,
            local_fn_origins,
        } = snapshot;
        self.return_ty = return_ty;
        self.has_value_return = has_value_return;
        self.locals = locals;
        self.annotated = annotated;
        self.local_fn_origins = local_fn_origins;
    }

    /// `|x| …` — a lambda (issue #1685) is fn-colored always, so its type is
    /// a `Ty::Fn` row — since #1910, built from what the lambda's own body
    /// infers (mono-HM narrowing read back from the walk below), with what
    /// is *written* only the fallback where the body-derived type is still
    /// `Unknown`. Mono-HM narrowing of a lambda's own params from its
    /// concrete *call sites* is still not modeled in this slice (inventing a
    /// type from a call site would be worse than an honest `Unknown`,
    /// unlike a type the body itself already pins). The whole body is
    /// inferred for its side effects (nested
    /// calls, uses) exactly like an interpolation's value expression is —
    /// for a `LambdaBody::Block`, that means every `stmts` entry
    /// (`self.infer_block_stmt`) as well as the tail, not the tail alone:
    /// `LambdaBody::value_exprs()` only ever yields the tail, so the
    /// original `value_exprs()`-only walk left a block-bodied lambda's own
    /// temp-decls and assignments unvisited by this pass entirely (issue
    /// #1749 — a conservative-total, spec §3, violation; an
    /// expression-bodied lambda has no `stmts` to miss, which is why only
    /// the block form under-reported). The walk below matches on
    /// `LambdaBody` and takes `stmts`/`tail` directly rather than going
    /// back through `value_exprs()`, so the frame window can wrap both
    /// (#1789) and a future `LambdaBody` variant is a compile error here
    /// rather than a silently unwalked body.
    ///
    /// `infer_block_stmt` is the ENCLOSING def's own per-statement walker,
    /// though, not a lambda-scoped one — left unchanged, it also mutates
    /// frame-scoped bookkeeping that belongs to the lambda's own
    /// (unmodeled) frame: `BlockStmt::Return` flips `return_ty`/
    /// `has_value_return`, and `BlockStmt::TempDecl`'s `bind_local` *unifies
    /// into* `locals` (so even a same-named enclosing local gets corrupted,
    /// not just a fresh key added). `annotated` and `local_fn_origins` are
    /// the same shape of leak for ascriptions and write-narrowing. So the
    /// block-body walk below snapshots and restores all five frame-scoped
    /// fields around **both** the `stmts` and the tail (issue #1789 — the
    /// tail is the block's value position and reads, and via `observe`
    /// writes, the locals those statements just bound, so restoring
    /// between the two inferred it against the enclosing def's `locals`
    /// instead; `locals` is keyed by bare name, so on a shadowed name that
    /// both hid the lambda's own temp from its tail and leaked the tail's
    /// `observe` into the enclosing local). Only the effect-atom
    /// accumulators (`referenced_globals`,
    /// `effect_writes`, `calls`, `external_calls`, `effect_opaque`,
    /// `effect_emits`, `effect_tags`, `effect_faults`,
    /// `effect_faults_refined`) are meant to survive the walk into the
    /// enclosing frame. `admission.rs`'s `Expr::Lambda` walk visits `stmts`
    /// then `value_exprs()` in the same order, for its own, unrelated
    /// provenance-range check — but that walk is a pure `check_range`
    /// traversal accumulating no per-def frame state, so it is not the same
    /// kind of walker and is not, on its own, evidence that reusing
    /// `infer_block_stmt` unguarded here would have been safe.
    ///
    /// This is the worked example of a general rule — the full
    /// frame-scoped-vs-cumulative field split for every `InferPass` field
    /// (not just these five), and why the fix below is a wholesale
    /// snapshot/restore rather than a diff-based undo — is written up in
    /// `docs/effects-spec.md` §4.1 (issue #1762). Keep both in sync if the
    /// field list ever changes.
    ///
    /// A snapshot/restore of these five is not, by itself, enough: a
    /// cumulative field can still refer to frame-scoped state *by name*
    /// rather than by `DefinitionId`, and a lambda param collides with an
    /// enclosing local exactly that way (issue #1779, `docs/effects-spec.md`
    /// §4.1's "third hazard"). `Self::lambda_param_names` guards that
    /// separately, by shadowing this lambda's own param names for the
    /// duration of the whole body walk below (both branches) — see its own
    /// field doc.
    ///
    /// The effect row is the unknown top element. `Ty::Fn` does carry a
    /// [`FnRow`] since #1680 step 3, but a `FnRow` names **creation
    /// targets** by `DefinitionId` (the keys §7's row table is looked up
    /// by), and a lambda has no such id at inference time — it is minted in
    /// LIR by `IdAllocator::alloc_lambda_address` (#1727). Until that
    /// keyspace gap closes, an honest "unknown" is the only sound row a
    /// lambda can carry — which is precisely the coordination issue #1685
    /// flagged.
    ///
    /// Composing a real row here would be **necessary but not sufficient**
    /// to make a lambda's row reachable through a live fn value — the
    /// missing half is a structural gap in the shipped table, not a
    /// follow-on to this function's own analyzer-side work. Walking the
    /// body inside *this* pass absorbs its atoms into the **enclosing**
    /// definition's row — sound (spec §3 allows over-reporting) but it
    /// gives the lambda no row of its own, and nothing downstream can mint
    /// one either: `populate_effect_rows` keys the shipped
    /// `DefinitionId → row` table off `inferable_defs_from_index`, i.e.
    /// `SymbolKind::Knot | SymbolKind::Stitch` symbols, whereas a lifted
    /// lambda's `DefinitionId` is minted by
    /// `lir::lower::context::IdAllocator::alloc_lambda_address` and is never
    /// an indexed symbol at all — a keyspace gap, not a phase-order one. So
    /// effects-spec §7's "a live fn value is a token; its row is a table
    /// lookup" currently *misses* for every lambda token, which blocks the
    /// shipped-table/§7-narrowing path (§6 item 4, an optional host
    /// optimization) — not #1680's own analyzer-side work, which has
    /// landed (Fork D retired T1c item 4's row field outright). Pinned by
    /// `brink-db/tests/issue_1680_lambda_effect_row_gap.rs`.
    #[expect(
        clippy::too_many_lines,
        reason = "one lambda-body walk threading three independent bare-name \
                  shadows (locals, annotated, the #1924 dotted-field-read \
                  taint) through both the Block and Expr body shapes — \
                  splitting the shadow save/restore pairs across helper \
                  functions would scatter the exact frame-scoping hazards \
                  this function's own doc walks through in order"
    )]
    fn infer_lambda(&mut self, l: &brink_ir::LambdaExpr) -> Ty {
        // Issue #1779: shadow this lambda's own param names for the whole
        // duration of walking its body — `stmts` *and* the tail/expr value
        // below, an expression-bodied lambda has no `stmts` to bracket at
        // all, so the shadow must cover both branches uniformly. Balanced
        // with the matching decrement after the tail walk, regardless of
        // which branch ran. See `Self::lambda_param_names`'s field doc for
        // why `local_call_origin` needs this.
        for p in &l.params {
            *self
                .lambda_param_names
                .entry(p.name.text.clone())
                .or_insert(0) += 1;
        }
        // Issue #1910: shadow every name this lambda's own body binds —
        // every param, plus (for a block body) every `TempDecl`/`for`/`as`
        // binding the body itself introduces, collected recursively through
        // `if`/`while`/`for` nesting by `lambda_own_binding_names` (never
        // through a nested `Expr::Lambda`, which shadows its own names when
        // *its* `infer_lambda` runs) — for the same duration as the walk
        // below. `self.locals` is keyed by bare *name* and shared with the
        // enclosing frame; `bind_local`/`observe` UNIFY with whatever
        // already occupies a name, which is correct for repeated writes to
        // the SAME binding but not for a same-spelled *new* binding a
        // nested scope introduces — without this shadow, an enclosing local
        // already sitting under this name would silently seed (or worse,
        // `Conflicted`-poison) the lambda's own fresh one the moment its
        // first `TempDecl` writes it, and — for a param specifically — an
        // enclosing same-named local's type would seed the param itself.
        // `self.annotated` is a *second* bare-name-keyed map with the exact
        // same hazard: it is read through `own_annotation`'s bare-single-
        // segment fallback (used by `or_own_annotation` for every intrinsic
        // argument, and by `annotated_callee_ty` for a direct-call callee)
        // during the body walk below, so an enclosing same-named ANNOTATED
        // local's `~ temp` ascription would otherwise leak into a lambda
        // param or temp of the same name (issue #1910 follow-up review) —
        // shadowed here for the identical reason and duration as `locals`.
        // `frame_snapshot`/`restore_frame` below (issue #1816) already keeps
        // this contamination from ever reaching the *enclosing* frame once
        // the walk finishes; what it does not do is stop this function's own
        // `body_ty`/`narrowed_params` overlay — or any `own_annotation` read
        // *during* the walk — from reading contaminated values, which is
        // what made this shadow necessary the moment #1910 started reading
        // them at all. `saved`/`saved_annotated` record exactly what (if
        // anything) occupied each name in each map so it can be put back
        // verbatim, `None` when nothing did.
        let param_names: BTreeSet<String> = l.params.iter().map(|p| p.name.text.clone()).collect();
        // Collected separately from `shadow_names` below (issue #1941 review
        // finding): the seed loop after it needs to know which param names
        // the lambda's own body *re-binds* via a fresh `TempDecl`/`if`/
        // `while`/`for` binding of the same spelling — see that loop's doc
        // for why a re-bound name must never be seeded.
        let mut body_bound_names: BTreeSet<String> = BTreeSet::new();
        if let brink_ir::LambdaBody::Block { stmts, .. } = &l.body {
            lambda_own_binding_names(stmts, &mut body_bound_names);
        }
        let shadow_names: BTreeSet<String> = param_names
            .iter()
            .cloned()
            .chain(body_bound_names.iter().cloned())
            .collect();
        let saved: Vec<(String, Option<Ty>)> = shadow_names
            .iter()
            .map(|name| (name.clone(), self.locals.remove(name)))
            .collect();
        let saved_annotated: Vec<(String, Option<Ty>)> = shadow_names
            .iter()
            .map(|name| (name.clone(), self.annotated.remove(name)))
            .collect();
        // Issue #1941: seed `self.annotated` with *this* lambda's own
        // resolvable param annotations before walking its body. This seed
        // is read by `own_annotation`'s bare-name fallback at every
        // `or_own_annotation`/`annotated_callee_ty` consumer reachable during
        // the walk below — not only the tail/expr value position — exactly
        // like a `fn`/`flow`'s own `new_pass`-time seed already covers its
        // whole body, not only its `return`s. `infer_def_body` gets this for
        // free at pass-creation time — the `annotated` map handed to
        // `new_pass` is built straight from `def.params` (see this module's
        // top-level doc) — but a lambda opens no such seed anywhere: the
        // shadow above only ever *clears* whatever an enclosing same-named
        // local's annotation left behind, it never installs the lambda's
        // own. Without this, `own_annotation` finds nothing for a lambda's
        // own param even when that param carries a `: T` annotation, which
        // is exactly what let `|t: content| { t }`'s tail read — and
        // `|t: content| t`'s expression body — keep exporting `Unknown`
        // after #1938 fixed the structurally identical `return t;` case for
        // a plain `fn`. Scoped to this walk alone: every name here is one
        // `saved_annotated` just recorded as `None` (nothing occupied it
        // once shadowed), so the restore loop after the walk removes it
        // again, cleanly — a body use that *disagrees* with the annotation
        // still starts from this same seed and can still diverge from it via
        // an ordinary `unify`, exactly like a `fn`/`flow` param's overlay.
        //
        // A param name the body itself *re-binds* (review finding on this
        // issue: `|t: int| { let t = "a"; t = "b"; t }`) is the one case
        // this seed must never cover: `check_declared_assign_target`'s own
        // `SymbolKind::Temp` arm reads this exact bare-name-keyed map to
        // find a Temp's *declared* type — it has no way to tell "the
        // param's own annotation" apart from "this fresh same-named local's
        // own (absent) annotation", so seeding the param's `int` here would
        // leak into the fresh `t: string` local's mismatch check as a false
        // `E063`. Excluded via `body_bound_names` above, computed before any
        // shadowing so it reflects the body's *own* re-binds, not whatever
        // the shadow just cleared.
        let type_names = self.ctx.type_names();
        for p in &l.params {
            if body_bound_names.contains(&p.name.text) {
                continue;
            }
            if let Some(te) = &p.annotation
                && let Some(ty) = crate::annotations::resolve(te, &type_names)
            {
                self.annotated.insert(p.name.text.clone(), ty);
            }
        }
        // Issue #1924 follow-up review on #1910: reset the taint flag to a
        // fresh baseline for this lambda's own walk (mirrors the
        // `return_ty`/`has_value_return` reset below) and save whatever the
        // enclosing frame's value was, so a NESTED lambda's own dotted-field
        // read cannot spuriously taint the ENCLOSING lambda's overlay too —
        // each `infer_lambda` call judges only what its *own* body walk hit.
        let outer_dotted_field_read_tainted =
            std::mem::replace(&mut self.dotted_field_read_tainted, false);
        // Issue #1910: capture the body's own derivation of the lambda's
        // return type and each param's type — read back from `self.locals`
        // *before* any restore below discards it, into plain local
        // variables rather than back into `self.locals`. This is the
        // lambda's counterpart of `infer_def_body`'s own post-walk
        // `param_types`/`return_ty` overlay (`pass.locals.get(&p.name.text)`
        // preferred over the annotation, annotation only as the Unknown
        // fallback) — previously this function walked the body purely for
        // its side effects and then discarded everything it learned,
        // rebuilding `params`/`ret` from written annotations alone, which is
        // exactly what let a pure verb's callback (`|x| x * 2`) and a
        // lambda-bound local (`let f = |x| x + 1;`) escape strict inference
        // as `Unknown` even when the body unambiguously pins the type
        // (issue #1910).
        let (body_ty, narrowed_params): (Ty, Vec<Ty>) = match &l.body {
            brink_ir::LambdaBody::Block { stmts, tail } => {
                // Wholesale snapshot/restore (not a "remove newly-inserted
                // keys" diff) — `bind_local` unifies into a pre-existing entry
                // of the same name, so a diff-based undo would still leak a
                // same-named outer local's corrupted type. `frame_snapshot`/
                // `restore_frame` (issue #1816) are what make this wholesale
                // set mechanically enforced rather than a five-line list a
                // future field can silently fall off of — see
                // `FrameSnapshot`'s doc.
                let snapshot = self.frame_snapshot();
                // #1910: reset `return_ty`/`has_value_return` to a fresh
                // baseline for the lambda's own frame, mirroring `new_pass`'s
                // fresh start for a top-level def — without this, an
                // internal `return` below (see `LambdaBody::Block`'s own
                // doc: "return leaves the lambda") would `unify` into
                // whatever the *enclosing* def's `return_ty` already held
                // (e.g. an earlier `return` in the enclosing body), which
                // `restore_frame` only ever cleaned up *after* the fact —
                // fine when the accumulated value was simply discarded (the
                // pre-#1910 behavior), but wrong the moment it is read back
                // below as this lambda's own signature.
                self.return_ty = Ty::Unknown;
                self.has_value_return = false;
                for s in stmts {
                    self.infer_block_stmt(s);
                }
                // The tail is part of the *same* frame as the `stmts` above
                // (issue #1789): it is the block's value position, and it
                // reads — and, through `observe`, writes — the very locals
                // those statements just bound. Walking it after the restore
                // inferred it against the *enclosing* def's `locals`, which
                // is wrong in both directions on a shadowed name: a
                // lambda-local temp read in tail position saw the enclosing
                // def's same-named local, and an `observe` from the tail
                // (e.g. an argument-position use) unified the lambda's type
                // into the enclosing local — manufacturing a spurious
                // `Conflicted` escape (`E066`) on a temp the enclosing body
                // never misuses. Inside the window, both directions stay in
                // the lambda's own frame and are discarded by the restore.
                // Issue #1941: the tail is this lambda's own value position —
                // the exact structural counterpart of `return t;`, which
                // #1938 already runs through `or_own_annotation` inside
                // `infer_return`. A bare `|t: content| { t }` tail is a pure
                // read of `t`, joined into `self.return_ty` below and never
                // `observe`d back onto anything else, so it is precisely the
                // "read site that doesn't itself produce counter-evidence"
                // `or_own_annotation`'s own doc requires — a body use that
                // *disagrees* with the annotation (`t + 1`, say) still infers
                // its own concrete type here, since `or_own_annotation` only
                // ever overlays an `Unknown`.
                let tail_ty = tail.as_ref().map_or(Ty::Unknown, |t| {
                    let ty = self.infer_expr(t);
                    self.or_own_annotation(t, ty)
                });
                // #1910: the lambda's own return type is the join of its
                // tail value (if any) and every internal `return <expr>;`
                // this walk observed (`self.return_ty`, freshly reset above)
                // — a block-bodied lambda that ends in a `return` rather
                // than a trailing expression (`positives`'s `filter_map`
                // callback: an `if`/`return` shape with no tail at all)
                // still has a real signature, and previously this function
                // never looked at `return_ty` here at all.
                let ty = unify(&self.return_ty, &tail_ty);
                // Read the params' narrowed types *before* `restore_frame`
                // resets `self.locals` back to `snapshot` — after that call
                // every name this walk touched, param or `TempDecl` alike,
                // is gone.
                let narrowed = l
                    .params
                    .iter()
                    .map(|p| {
                        self.locals
                            .get(&p.name.text)
                            .cloned()
                            .unwrap_or(Ty::Unknown)
                    })
                    .collect();
                self.restore_frame(snapshot);
                (ty, narrowed)
            }
            // A single-expression body has no `stmts`, so there is no frame
            // to open around it — it walks under whatever frame was already
            // live when `infer_lambda` was entered, unchanged by #1789.
            brink_ir::LambdaBody::Expr(e) => {
                // Issue #1941: same treatment as the `Block` arm's tail
                // above — an expression-bodied lambda's sole expression
                // *is* its value position (`|t: content| t`), the structural
                // twin of `return t;` one syntax form over.
                let ty = self.infer_expr(e);
                let ty = self.or_own_annotation(e, ty);
                let narrowed = l
                    .params
                    .iter()
                    .map(|p| {
                        self.locals
                            .get(&p.name.text)
                            .cloned()
                            .unwrap_or(Ty::Unknown)
                    })
                    .collect();
                (ty, narrowed)
            }
        };
        // Issue #1924 follow-up review on #1910: if anything during this
        // lambda's own walk hit the mistyped dotted-field-read case (see
        // `Self::dotted_field_read_tainted`'s field doc), the `body_ty`/
        // `narrowed_params` just computed above may have been poisoned by an
        // ordinary `unify`/`observe` reachable from that mistyped value —
        // discard them in favor of the pre-#1910 posture (an honest
        // `Unknown`, overlaid by the written annotation below exactly like
        // it was before this lambda ever read its own body back). Restore
        // the enclosing frame's own taint state — consumed here, not
        // propagated outward, exactly like `saved`/`saved_annotated` above.
        let tainted = std::mem::replace(
            &mut self.dotted_field_read_tainted,
            outer_dotted_field_read_tainted,
        );
        let (body_ty, narrowed_params) = if tainted {
            (Ty::Unknown, vec![Ty::Unknown; l.params.len()])
        } else {
            (body_ty, narrowed_params)
        };
        // Undo the `self.locals` and `self.annotated` shadows (mirrors the
        // `lambda_param_names` decrement below): restore whatever an
        // enclosing same-named local/annotation held, or leave the name
        // absent if nothing did.
        for (name, prior) in saved {
            match prior {
                Some(ty) => {
                    self.locals.insert(name, ty);
                }
                None => {
                    self.locals.remove(&name);
                }
            }
        }
        for (name, prior) in saved_annotated {
            match prior {
                Some(ty) => {
                    self.annotated.insert(name, ty);
                }
                None => {
                    self.annotated.remove(&name);
                }
            }
        }
        for p in &l.params {
            if let std::collections::btree_map::Entry::Occupied(mut o) =
                self.lambda_param_names.entry(p.name.text.clone())
            {
                *o.get_mut() -= 1;
                if *o.get() == 0 {
                    o.remove();
                }
            }
        }
        // Issue #1941 review finding: reuses the `type_names` snapshot taken
        // before the seed loop above rather than cloning `self.ctx.list_names`/
        // `struct_names`/`handle_names` a second time — `self.ctx` is never
        // mutated between that seed and this overlay, so the same snapshot
        // is valid here too.
        //
        // Issue #1994 (RULED 2026-08-01, closing #1932): **narrowed to the
        // unannotated case** — #1910's own posture below (body-derived wins
        // whenever it isn't `Unknown`, annotation only the `Unknown`
        // fallback) is UNCHANGED for a param/return with no written
        // annotation of its own. But a lambda's own *written* annotation —
        // unlike a top-level `fn`/`flow`'s, which stays the #1910/
        // `infer_def_body` fallback-only posture, `annotations::mismatches`
        // (E063) reporting any disagreement only as a gradual advisory —
        // now GOVERNS this slot unconditionally: the annotation is the
        // resulting type, and a body-derived type that disagrees with it is
        // recorded as an eager, `Error`-severity `E174`
        // ([`LambdaAnnotationMismatch`]) rather than silently overridden.
        // `docs/typed-mode-spec.md` §2 records why the two are now ruled to
        // differ. An `Unknown`/`Conflicted` body-derived type never
        // "disagrees" (`Ty::is_unresolved`) — nothing to compare, and a
        // genuinely `Conflicted` body already has its own `E066` escape
        // report elsewhere; double-reporting the same fact here would be
        // exactly the hazard `annotations::report_if_mismatched`'s identical
        // guard exists to avoid.
        //
        // One more exclusion, the same shape as the `#1941`/`#1954` seed's
        // own `body_bound_names` guard above: a param name the body itself
        // *re-binds* (`|t: int| { let t = "a"; t = "b"; t }`) is never run
        // through the new governs-unconditionally precedence at all.
        // `narrowed_params`/`self.locals` is bare-name-keyed, so once the
        // body's own `TempDecl` re-declares `t`, `self.locals["t"]` holds
        // the *fresh local's* accumulated type, not the param's — there is
        // nothing left in this pass's bookkeeping that still distinguishes
        // "the param's own narrowing" from "the shadowing local's". Running
        // the disagreement check against that value would compare the
        // param's annotation against a completely unrelated binding's type
        // and report a false `E174` the instant the two disagree (the exact
        // review-finding shape #1954 already fixed once for the `self.
        // annotated` seed — a rebound name is excluded there for the
        // identical reason). Falls back to #1910's own unannotated-style
        // posture unconditionally for this one param, matching this
        // slot's pre-#1994 behavior exactly (no diagnostic either way,
        // since a rebound name was never seeded into `self.annotated` for
        // `own_annotation` to compare against anyway).
        let params = l
            .params
            .iter()
            .zip(narrowed_params)
            .map(|(p, inferred)| {
                if body_bound_names.contains(&p.name.text) {
                    return if inferred.is_unknown() {
                        p.annotation
                            .as_ref()
                            .and_then(|te| crate::annotations::resolve(te, &type_names))
                            .unwrap_or(inferred)
                    } else {
                        inferred
                    };
                }
                let declared = p
                    .annotation
                    .as_ref()
                    .and_then(|te| crate::annotations::resolve(te, &type_names));
                match declared {
                    Some(declared_ty) => {
                        // Review finding on #1994 (measured at head c1be12d
                        // via `strict::native_strict_diags`): a plain
                        // `!assignable(&declared_ty, &inferred)` is the wrong
                        // comparison direction for a *parameter* — it flags
                        // legal widening (an `int`-annotated param whose body
                        // uses it as a `float`, e.g. `|x: int| { x + 1.0 }`)
                        // as a hard, non-downgradable `E174`, when the
                        // pre-#1994 posture (and the structurally identical
                        // top-level `fn(x: int): float { return x + 1.0; }`)
                        // reported nothing (or only the advisory `E063`).
                        // Only report when the pair is genuinely
                        // irreconcilable in *either* direction — an
                        // int-vs-bool mismatch still fires, since neither
                        // direction is assignable there.
                        if !inferred.is_unresolved()
                            && !assignable(&declared_ty, &inferred)
                            && !assignable(&inferred, &declared_ty)
                        {
                            self.lambda_annotation_mismatches
                                .push(LambdaAnnotationMismatch {
                                    // `p.annotation` is `Some` whenever `declared`
                                    // resolved (the `and_then` above), so this
                                    // always finds the written annotation's own
                                    // range rather than falling back to the
                                    // lambda's whole span.
                                    range: p
                                        .annotation
                                        .as_ref()
                                        .map_or(l.ptr.range, brink_ir::TypeExpr::range),
                                    param_name: Some(p.name.text.clone()),
                                    expected: declared_ty.clone(),
                                    found: inferred,
                                });
                        }
                        declared_ty
                    }
                    None => inferred,
                }
            })
            .collect();
        let declared_ret = l
            .return_type
            .as_ref()
            .and_then(|te| crate::annotations::resolve(te, &type_names));
        let ret = match declared_ret {
            Some(declared_ty) => {
                if !body_ty.is_unresolved() && !assignable(&declared_ty, &body_ty) {
                    self.lambda_annotation_mismatches
                        .push(LambdaAnnotationMismatch {
                            range: l
                                .return_type
                                .as_ref()
                                .map_or(l.ptr.range, brink_ir::TypeExpr::range),
                            param_name: None,
                            expected: declared_ty.clone(),
                            found: body_ty,
                        });
                }
                declared_ty
            }
            None => body_ty,
        };
        // The effect row stays at the top element. A lambda's lifted
        // `DefinitionId` is minted in LIR (`alloc_lambda_address`), so it has
        // no id at inference time to *name* as a creation target — the
        // keyspace gap #1727 tracks. `FnRow` keys the shipped
        // `DefinitionId → row` table (§7), so an honest "unknown" is the only
        // sound row a lambda can carry until that gap closes.
        Ty::Fn(params, Box::new(ret), FnRow::unknown())
    }

    /// `#fn(target, args…)` — docs/t1c-spec.md §4: consume the bound prefix
    /// from the target's signature. An unresolved target or a target with
    /// no known signature (a variable, an external, an E079 case) types as
    /// `Unknown` — the creation-site diagnostics own the error reporting.
    ///
    /// The [`Self::record_ref_param_writes`] call below is the *creation*-site
    /// half of that mechanism (issue #1755, docs/effects-spec.md §6.1a
    /// channel 5) — see its own doc for why the bound cell has to be charged
    /// here. It sits **before** the `known_sigs` early return deliberately:
    /// `def_effect_atoms` runs this walk with an empty `known_sigs` (see
    /// [`Self::fn_literal_write_origin`]'s doc for that posture), so anything
    /// placed after the return would be dead code in the one pass that
    /// harvests effect atoms.
    fn infer_fn_literal(&mut self, fl: &brink_ir::FnLiteral) -> Ty {
        let arg_tys: Vec<Ty> = fl.args.iter().map(|arg| self.infer_expr(arg)).collect();
        let Some(def) = self.resolve(fl.target.range) else {
            return Ty::Unknown;
        };
        self.record_call_edge(def);
        self.record_fn_value_creation(def);
        self.record_ref_param_writes(def, &fl.args);
        let Some(sig) = self.ctx.known_sigs.get(&def) else {
            return Ty::Unknown;
        };
        // Issue #2001 (the tracked remainder of #1995/#1920 after PR #1999):
        // `#fn`'s bound-argument loop **is** the by-ref binding site
        // (docs/t1c-spec.md §2, §6.1a channel 5 — the creation-site half of
        // the aliasing enumeration) — the one place a `Ty::Fn` value's
        // remaining param row can never contain a `ref` param, because it
        // must already be bound here. `fn_values::check`'s `E080` only
        // verifies a `ref` position is bound to *some* durable cell, never
        // that the cell's static type agrees with the declared `ref` param
        // type, so this invariant check is a separate obligation. Mirrors
        // `infer_call`'s direct-call check, but only the `ref`-arm half of
        // it: this loop only checks `ref`-bound arguments, so the
        // observed-local carve-out below is always the *partial* one
        // (`!observed || assignable(..)`), never the *full* skip
        // (`!observed && !assignable(..)`) `infer_call`'s non-ref arm uses.
        // That is deliberate, not an oversight — the #1995 finding on
        // `infer_call` is that the full skip is only sound for the non-ref
        // case (an argument that would already conflict and get reported as
        // `E066`); a `ref` mismatch that stays `assignable` in the
        // covariant direction still needs its own report even when the
        // argument is an observed local, so the ref arm always keeps this
        // narrower, partial carve-out. By-value (non-`ref`) bound arguments
        // are deliberately left unchecked here — #2001's own scope note:
        // adding a by-value check at this creation site is new checking
        // that needs its own design call, not an assumed yes.
        let ref_positions = self.ctx.index.symbols.get(&def);
        for (i, arg) in fl.args.iter().enumerate() {
            if let Some(param_ty) = sig.params.get(i) {
                let is_ref_param = ref_positions
                    .and_then(|info| info.params.get(i))
                    .is_some_and(|p| p.is_ref);
                let observed = self.arg_is_observed_local(arg);
                if is_ref_param
                    && !param_ty.is_unresolved()
                    && let Some(arg_ty) = arg_tys.get(i)
                    && !arg_ty.is_unresolved()
                    && !ref_assignable(param_ty, arg_ty)
                    && (!observed || assignable(param_ty, arg_ty))
                {
                    let callee = fl
                        .target
                        .segments
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    self.direct_call_arg_mismatches.push(DirectCallArgMismatch {
                        range: fl.target.range,
                        callee,
                        index: i,
                        expected: param_ty.clone(),
                        found: arg_ty.clone(),
                    });
                }
                self.observe(arg, param_ty);
            }
        }
        let remaining: Vec<Ty> = sig.params.iter().skip(fl.args.len()).cloned().collect();
        // §5/§6.1a (issue #1680): this literal **is** the creation site, and
        // `def` is the target it names syntactically — the one place a
        // non-top [`FnRow`] is minted inside a body. Every later join
        // (`observe`, `bind_local`, a collection literal's element fold)
        // carries it along, which is what makes the row follow the value
        // "through copies, parameters, returns, and nesting".
        Ty::Fn(
            remaining,
            Box::new(sig.return_ty.clone()),
            FnRow::of_target(def),
        )
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
            Ty::Fn(params, ret, _) => {
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
                        && !assignable(param_ty, arg_ty)
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
            Ty::Fn(params, ret, row) => {
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
                        && !assignable(param_ty, arg_ty)
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
                // The effect row rides through unchanged: partial
                // application never changes *which* def eventually runs
                // (§6.1a — "`bind` copies from an already-known value rather
                // than naming a new target"), which is the same reason
                // `fn_literal_write_origin` traces through `bind` chains.
                Ty::Fn(remaining, ret, row)
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
        read_tys: &[Ty],
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
        if fx.conventions_write {
            // Issue #1840 Q4: `register(...)` is an ordinary write to the
            // conventions handler registry cell — closes the gap where
            // `register` was, in practice, row-exempt (see
            // `super::intrinsics`'s module doc and
            // `crate::register_intrinsic`'s doc for the full history).
            self.effect_writes
                .insert(DefinitionId::CONVENTIONS_REGISTRY_CELL);
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
            // `remove` (issue #1484, decision log "Quick-docket closures"
            // 2026-07-26): map-only now — identity-based, idempotent-total
            // removal (map keys; flags values once flags land). The
            // array-index leg this used to also narrow (observing the
            // second argument against the *element* type, which was wrong
            // for an index argument anyway — a latent bug this split
            // fixes) moved to `remove_at` below.
            "remove" => {
                if let (Some(Ty::Map(k, _)), Some(item)) = (arg_tys.first(), args.get(1)) {
                    self.observe(item, k);
                }
                // Issue #1532: the pre-#1484 array-index leg has no
                // compatibility shim — a statically-known `Ty::Array`
                // receiver here is an un-migrated `remove(a, i)` call site
                // that means `remove_at(a, i)`. Recorded as a fact (see
                // `BodyResult::array_remove_calls`'s doc), not raised
                // directly: this pass is advisory-only, reported by strict
                // mode (`strict::check_array_remove_calls`, `E149`).
                if matches!(arg_tys.first(), Some(Ty::Array(_))) {
                    self.array_remove_calls.push(range);
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
            // against the map's key type — from `arg_tys` (evidence-only:
            // issue #1168's review correction, see `infer_call`'s comment
            // — `m`'s annotation must never become `k`'s evidence). The
            // return type's own map shape comes from `read_tys`, so `m`'s
            // own annotation still resolves `get(m, k)`'s result when `m`
            // is otherwise unevidenced (the confirmed #1168 repro).
            "get" => {
                if let (Some(Ty::Map(k, _)), Some(key_arg)) = (arg_tys.first(), args.get(1)) {
                    self.observe(key_arg, k);
                }
                match read_tys.first() {
                    Some(Ty::Map(_, v)) => Ty::Option(v.clone()),
                    _ => Ty::Option(Box::new(Ty::Unknown)),
                }
            }
            // `contains_value(m, v)` → bool (§5); the needle narrows
            // against the map's value type.
            "contains_value" => {
                if let (Some(Ty::Map(_, v)), Some(needle)) = (arg_tys.first(), args.get(1)) {
                    self.observe(needle, v);
                }
                Ty::Bool
            }
            // `clear(ref m)` (§5), `shuffle(ref a)` (§7, NS-A6), `sort(ref
            // a)` (§4b, NS-A4 — the F0 imperative twin of `sorted`), and
            // `remove_at(a, i)` (issue #1484, joining the `_at`
            // faulting-index family with `char_at`): statement-only
            // in-place mutators — a receiver write, no value (the `push`
            // shape, #880: the intrinsics' effect behavior is declared at
            // introduction). `remove_at`'s `i` is an index, not an element
            // — like `insert`'s array leg (also index-typed), it isn't
            // narrowed against the array's element type; the runtime op's
            // own domain check (`i` must be an `Int`) is the wrong-type
            // backstop, matching `char_at`'s posture. Arms merged per
            // clippy match_same_arms (the #694 `len | int` precedent);
            // `sort`'s mode-dependent NaN behavior is entirely the runtime
            // op's (rows stay mode-independent — the conservative faults
            // bit rides the intrinsic table).
            "clear" | "shuffle" | "sort" | "remove_at" => {
                if let Some(container) = args.first() {
                    self.record_write(container);
                }
                Ty::Unknown
            }
            // `some(x)` → `Option[typeof x]` (§1.4) — the constructor is
            // where a bare element type becomes optional. `x` is read here,
            // never joined against a second operand, so this is the one
            // arm where `read_tys` (issue #1168's annotation fallback) is
            // safe unconditionally: there is no sibling to seed.
            "some" => Ty::Option(Box::new(read_tys.first().cloned().unwrap_or(Ty::Unknown))),
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
            // NS-A4: the comparator pair, merged with `filter`'s fn-value
            // verb (issue #1679) — both push a value-call hint from
            // `args[1]` and share `filter`'s "array in, array of the same
            // element type out" shape (clippy `match_same_arms`: the
            // bodies were byte-identical, so one arm serves both). The
            // comparator is a function value the *verb* will call — the
            // one other place (besides `call(f)`/`f(args)`) a real
            // callee's effects escape the static call graph, so its row
            // composes through the same pending-value-call machinery
            // (`⊕cmp`/`⊕f`, F14): a provable single origin pulls that
            // def's row in transitively; anything else degrades to the
            // pessimal opaque floor. Dispatch faults (non-function
            // comparator/callback, non-int/non-bool return, detected
            // inconsistency) ride the intrinsic table's faults bit. The
            // pure·silent contract itself is E119 (`comparator_contract`),
            // exceedance-only.
            "sorted_by" | "filter" => {
                if let Some(cmp) = args.get(1) {
                    let hint = self.value_call_origin(cmp);
                    self.pending_value_calls.push(hint);
                }
                match arg_tys.first() {
                    Some(Ty::Array(elem)) => Ty::Array(elem.clone()),
                    _ => Ty::Unknown,
                }
            }
            // The fn-value verb layer (issue #1679, `docs/stdlib-spec.md`
            // §4): the pure trio. Like the NS-A4 comparator pair, the
            // callback is a function value the *verb* invokes, so its row
            // composes through the same pending-value-call machinery
            // (`⊕f`); the pure·silent contract itself is E119
            // (`crate::comparator_contract`), exceedance-only.
            //
            // Result typing reads the callback's own `Ty::Fn` return where
            // one is known, and degrades to `Unknown` otherwise — never a
            // guess: `Ty::Fn` is the only evidence there is, this arm itself
            // never walks the callback body. Since #1910, an inline
            // `|x| …` lambda literal's own `Ty::Fn` is no longer
            // annotation-only, though: `infer_lambda` already read that
            // callback's body-derived return back (mono-HM narrowing) by
            // the time `arg_tys` reaches this arm, so `map`'s own result
            // can come from an unannotated callback's body just as well as
            // from an annotated target reached through an inline `#fn(…)`
            // literal — this arm's own job stays "read the ret field",
            // just over a `Ty::Fn` that now carries more real information.
            // `map_each` (§4, issue #1679 slice 2) shares `map`'s exact
            // typing shape — the effectful/pure split is a runtime-contract
            // difference, not a typing one — merged per clippy
            // `match_same_arms` (the #694 `len | int` precedent).
            "map" | "map_each" => {
                if let Some(f) = args.get(1) {
                    let hint = self.value_call_origin(f);
                    self.pending_value_calls.push(hint);
                }
                match (arg_tys.first(), arg_tys.get(1)) {
                    (Some(Ty::Array(_)), Some(Ty::Fn(_, ret, _))) => Ty::Array(ret.clone()),
                    _ => Ty::Unknown,
                }
            }
            // `fold(a, init, f)` → the accumulator type. `f`'s return is
            // the accumulator by signature, so prefer it when known and
            // fall back to `init`'s inferred type (which the mono-HM pass
            // usually has even when the callback is opaque).
            //
            // #1910: "known" means concretely known, not merely *present* —
            // an inline lambda whose own body places no constraint on its
            // params (`|a, b| a + b` with both params otherwise
            // unconstrained) still types as `Ty::Fn(_, Ty::Unknown, _)`, and
            // that `Unknown` used to win over `init`'s already-concrete type
            // unconditionally, discarding real evidence the accumulator
            // slot had. `Ty::Conflicted` is left out of this guard
            // deliberately: unlike `Unknown` it is genuine information (the
            // callback body really did observe two disagreeing types), so it
            // must propagate rather than being masked by falling back to
            // `init`.
            "fold" => {
                if let Some(f) = args.get(2) {
                    let hint = self.value_call_origin(f);
                    self.pending_value_calls.push(hint);
                }
                match arg_tys.get(2) {
                    Some(Ty::Fn(_, ret, _)) if !ret.is_unknown() => (**ret).clone(),
                    _ => arg_tys.get(1).cloned().unwrap_or(Ty::Unknown),
                }
            }
            // `filter_map(a, f)` → `[U]` where `f: fn(T): Option[U]` (§4,
            // issue #1679 slice 2) — the Option-mapper, dropping `none`.
            // Same shape as `map`: read the callback's `Ty::Fn` return, and
            // unwrap one layer of `Option` when it's known; degrade to
            // `Unknown` otherwise (never a guess: this arm itself never
            // walks the callback body, only reads its `Ty::Fn`'s declared
            // return shape — see `map`'s own arm comment above for why,
            // since #1910, that shape is no longer annotation-only for an
            // inline lambda literal).
            "filter_map" => {
                if let Some(f) = args.get(1) {
                    let hint = self.value_call_origin(f);
                    self.pending_value_calls.push(hint);
                }
                match (arg_tys.first(), arg_tys.get(1)) {
                    (Some(Ty::Array(_)), Some(Ty::Fn(_, ret, _))) => match ret.as_ref() {
                        Ty::Option(inner) => Ty::Array(inner.clone()),
                        _ => Ty::Unknown,
                    },
                    _ => Ty::Unknown,
                }
            }
            // `each(a, f)` — the ruled effectful spelling (§4, issue #1679
            // slice 2) — `f: fn(T)` runs per element for its side effects,
            // no result (`Unknown`, the `sort`/`clear` statement-shaped-verb
            // posture — #880). `map_each` is typed above, merged into
            // `map`'s arm (same result shape, different runtime contract).
            // Neither is E119-gated (`crate::comparator_contract`'s module
            // doc), but the callback's row still composes through the same
            // `pending_value_calls` machinery as the pure quartet — being
            // effectful widens what the callback may legally do, not
            // whether its own effects matter to the caller's row.
            "each" => {
                if let Some(f) = args.get(1) {
                    let hint = self.value_call_origin(f);
                    self.pending_value_calls.push(hint);
                }
                Ty::Unknown
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
        // §6.1 (issue #1680): a divert with arguments reaches the target's
        // params exactly like a call does, so its argument origins have to
        // join the same `(callee, position)` summary — recording only the
        // `infer_call` half would let a call site's `#fn(g)` look like the
        // *only* thing that position ever receives.
        self.record_call_arg_fn_origins(def, &target.args);
        // Issue #2127: a `ref` position binds the target's ref param
        // exactly like a call does — `record_ref_param_writes` above
        // already tracks the *write* — but until this fix the argument's
        // *type* was never compared against the declared param type in
        // either direction; `arg_tys` was computed above and then
        // explicitly discarded. Mirrors `infer_call`'s `ref` arm (issue
        // #1995/#1920: a `ref` parameter both reads and writes through the
        // caller's own cell, so its argument is checked with
        // `ref_assignable` — invariant — rather than `assignable`'s
        // covariant widening).
        //
        // By-value positions are deliberately left unchecked here, same
        // posture PR #2014 took for `infer_fn_literal`'s by-value params:
        // that is new checking with its own design call, not an assumed
        // yes (#2127's scope note). `observe` below still runs
        // unconditionally for every position, ref or not, exactly as
        // before this fix.
        let ref_positions = self.ctx.index.symbols.get(&def);
        if let Some(sig) = self.ctx.known_sigs.get(&def) {
            for (i, arg) in target.args.iter().enumerate() {
                if let Some(param_ty) = sig.params.get(i) {
                    let is_ref_param = ref_positions
                        .and_then(|info| info.params.get(i))
                        .is_some_and(|p| p.is_ref);
                    let observed = self.arg_is_observed_local(arg);
                    if is_ref_param
                        && !param_ty.is_unresolved()
                        && let Some(arg_ty) = arg_tys.get(i)
                        && !arg_ty.is_unresolved()
                        && !ref_assignable(param_ty, arg_ty)
                        && (!observed || assignable(param_ty, arg_ty))
                    {
                        let callee = p
                            .segments
                            .iter()
                            .map(|s| s.text.as_str())
                            .collect::<Vec<_>>()
                            .join(".");
                        self.direct_call_arg_mismatches.push(DirectCallArgMismatch {
                            range: p.range,
                            callee,
                            index: i,
                            expected: param_ty.clone(),
                            found: arg_ty.clone(),
                        });
                    }
                    self.observe(arg, param_ty);
                }
            }
        }
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
                // Issue #1877: the initializer's own type checked against
                // this `~ temp`'s explicit ascription, if any.
                self.check_declared_temp_init(t, &ty);
                self.bind_local(&t.name.text, &ty);
                // T2 §8 (issue #872): track this write towards the local's
                // whole-body write summary — see `bump_local_write`.
                let origin = t
                    .value
                    .as_ref()
                    .and_then(|e| self.fn_literal_write_origin(e));
                self.bump_local_write(&t.name.text, origin);
            }
            Stmt::Assignment(a) => {
                let target_ty = self.infer_expr(&a.target);
                let ty = self.infer_expr(&a.value);
                // Issue #1911 review finding: `+=` reaches the exact same
                // string-numeric display-concat shape as infix `+`
                // (`infer_infix`'s `is_string_numeric_concat` arm, spec §4)
                // — `keys += total` desugars to the same runtime
                // `String`/`Int`|`Float` `Add` arm as `keys = keys + total`,
                // so it must skip the same-type declared-check/observe pair
                // below exactly like that arm skips unify: `check_declared_
                // assign_target` would otherwise report `keys`'s declared
                // `string` vs the numeric RHS as E063, and `observe`'s join
                // would otherwise drive `keys` to `Ty::Conflicted` (E066) —
                // both false positives on legal, running ink.
                let is_string_numeric_concat_add =
                    a.op == AssignOp::Add && is_string_numeric_concat(&target_ty, &ty);
                if !is_string_numeric_concat_add {
                    // Issue #1877: the RHS type checked against the target's
                    // already-known declared type — must run *before*
                    // `observe` below, which is what may drive the target
                    // Conflicted (see `check_declared_assign_target`'s doc
                    // for the ordering).
                    self.check_declared_assign_target(&a.target, &ty);
                    // Issue #1900: the dotted sibling of the check above —
                    // see `check_declared_field_assign_target`'s own doc.
                    // `a.op` rides along (issue #1900 review finding) because
                    // `target_ty` above is only the ROOT's type for a dotted
                    // path — the `+=` string-numeric carve-out can only be
                    // decided once the field's own declared type is known,
                    // which happens later in `structs::check_field_assign_
                    // mismatch`.
                    self.check_declared_field_assign_target(&a.target, a.op, &ty);
                    self.observe(&a.target, &ty);
                }
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
                    self.infer_block(&branch.body);
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
            // Issue #1912: `return <annotated param>` is a pure read — the
            // returned value's type is joined into `self.return_ty`, never
            // `observe`d back onto the expression, so this is exactly the
            // "read site that doesn't itself produce counter-evidence"
            // [`Self::or_own_annotation`] is safe at (its own doc names the
            // #1168 siblings: `some(x)`, `get(m, k)`, a `for` iterable).
            //
            // Without it, handing a parameter straight back out lost its
            // declared type: `fn passthru(t: content) { return t; }`
            // reported `E065` on its return type while the annotated-return
            // twin `fn passthru(t: content): content { return t; }` was
            // clean, even though the return type is *exactly* the annotated
            // parameter type. Filed against `content` (#1846 gave it a
            // resolvable `Ty`; #1882's sweep caught the corpus row —
            // `fn radio(chan: string, text: content) { return text; }`),
            // but the gap was never `content`-specific: `int`, `bool` and
            // every other annotation lost the same way.
            //
            // `or_own_annotation` only overlays an `Unknown` — a concrete
            // *or* `Conflicted` body derivation still wins outright, so a
            // body that contradicts the annotation keeps exporting its own
            // type for `annotations::mismatches` (`E063`) to compare
            // against, and `unify` above still drives two returns that
            // disagree (`return t` / `return "x"`) to `Conflicted`.
            let ty = self.or_own_annotation(v, ty);
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
            self.infer_content_part(part);
        }
    }

    fn infer_content_part(&mut self, part: &ContentPart) {
        match part {
            ContentPart::Interpolation(e) => {
                self.infer_expr(e);
            }
            ContentPart::InlineConditional(c) => self.infer_conditional(c),
            ContentPart::InlineSequence(s) => {
                for branch in &s.branches {
                    self.infer_block(&branch.body);
                }
            }
            // A span is presentational (§4.3), not opaque — an
            // interpolation inside `<b>{expr}</b>` still needs its
            // expression type-inferred (and a real type error inside one
            // still needs to be caught).
            ContentPart::Span(span) => {
                for child in &span.children {
                    self.infer_content_part(child);
                }
            }
            ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
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
            let cond_ty = self.infer_expr(cond); // condition position — no forcing.
            // Guard-`as` binding (issue #1508) — same treatment as
            // `infer_conditional`'s `branch.binding`/`infer_if`'s
            // `i.binding`: type the bound name from the condition's
            // `Option[T]`, so a `{n}` read inside the choice's own body
            // isn't stuck at `Unknown`.
            self.bind_as_binding(choice.binding.as_ref(), &cond_ty);
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
                let cond_ty = self.infer_expr(e); // condition position — no forcing.
                self.bind_as_binding(branch.binding.as_ref(), &cond_ty);
            }
            self.infer_block(&branch.body);
        }
    }

    /// Type an `as` binding (B1b, issue #1475): `Option[T]` unwraps to `T`.
    ///
    /// A condition that isn't a statically known `Option` leaves the
    /// binding `Unknown` rather than guessing — `option_conditions::check`
    /// owns that judgment (`E147` for a classifiable non-Option, silence for
    /// `Unknown`/`Conflicted`, the "Unknown never disagrees" rule this
    /// module applies everywhere else).
    fn bind_as_binding(&mut self, binding: Option<&Name>, cond_ty: &Ty) {
        if let Some(name) = binding {
            let bound = match cond_ty {
                Ty::Option(inner) => (**inner).clone(),
                _ => Ty::Unknown,
            };
            self.bind_local(&name.text, &bound);
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
                // Issue #1877: see `Stmt::TempDecl`'s twin above.
                self.check_declared_temp_init(t, &ty);
                self.bind_local(&t.name.text, &ty);
                // T2 §8 (issue #872): see `Stmt::TempDecl`'s twin above.
                let origin = t
                    .value
                    .as_ref()
                    .and_then(|e| self.fn_literal_write_origin(e));
                self.bump_local_write(&t.name.text, origin);
            }
            BlockStmt::Assignment(a) => {
                let target_ty = self.infer_expr(&a.target);
                let ty = self.infer_expr(&a.value);
                // Issue #1911: see `Stmt::Assignment`'s twin above — the
                // string-numeric `+=` display-concat carve-out.
                let is_string_numeric_concat_add =
                    a.op == AssignOp::Add && is_string_numeric_concat(&target_ty, &ty);
                if !is_string_numeric_concat_add {
                    // Issue #1877: see `Stmt::Assignment`'s twin above.
                    self.check_declared_assign_target(&a.target, &ty);
                    // Issue #1900: see `Stmt::Assignment`'s twin above,
                    // `a.op` included for the same reason.
                    self.check_declared_field_assign_target(&a.target, a.op, &ty);
                    self.observe(&a.target, &ty);
                }
                self.record_write(&a.target);
                // T2 §8 (issue #872): see `Stmt::Assignment`'s twin above.
                let origin = self.fn_literal_write_origin(&a.value);
                self.record_fn_write(&a.target, origin);
            }
            BlockStmt::Return(r) => self.infer_return(r.value.as_ref(), &r.onwards_args),
            BlockStmt::If(i) => self.infer_if(i),
            BlockStmt::While(w) => {
                let cond_ty = self.infer_expr(&w.condition); // condition position — no forcing.
                // `while EXPR as n` rebinds each iteration, but every pass
                // binds the SAME static type, so one binding here is exact.
                self.bind_as_binding(w.binding.as_ref(), &cond_ty);
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
                // Issue #1168: same "pass a param straight through" read as
                // a stdlib verb's arg (`infer_call`'s intrinsic dispatch) —
                // the iterable is only ever inspected, never joined against
                // a second operand, so an annotated-but-otherwise-
                // unevidenced param/temp iterable should still carry its
                // own declared type here (`iteration.md`'s `first_over`
                // fence: `for coins in tab` where `tab: Array<int>` is
                // never touched anywhere else in the body).
                let iter_ty = self.or_own_annotation(&f.iterable, iter_ty);
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
        let cond_ty = self.infer_expr(&i.condition); // condition position — no forcing.
        self.bind_as_binding(i.binding.as_ref(), &cond_ty);
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

#[cfg(test)]
mod tests {
    use brink_format::DefinitionTag;
    use brink_ir::{FileId, NodeClass, Provenance, Scope, SymbolInfo, Visibility};

    use super::*;

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(start.into(), end.into())
    }

    fn temp_symbol(id: DefinitionId, name: &str, decl_range: TextRange) -> SymbolInfo {
        SymbolInfo {
            kind: SymbolKind::Temp,
            file: FileId(0),
            range: decl_range,
            id,
            name: name.to_string(),
            params: Vec::new(),
            detail: None,
            scope: Some(Scope::default()),
            param_detail: None,
            module: None,
            visibility: Visibility::Public,
        }
    }

    fn knot_symbol(id: DefinitionId, name: &str) -> SymbolInfo {
        SymbolInfo {
            kind: SymbolKind::Knot,
            file: FileId(0),
            range: range(0, 1),
            id,
            name: name.to_string(),
            params: Vec::new(),
            detail: None,
            scope: None,
            param_detail: None,
            module: None,
            visibility: Visibility::Public,
        }
    }

    /// [`knot_symbol`] with a single declared `ref` param — the shape
    /// [`record_ref_param_writes`] needs to fold a call-site argument's
    /// write into [`InferPass::local_fn_origins`].
    fn knot_symbol_with_ref_param(id: DefinitionId, name: &str, param_name: &str) -> SymbolInfo {
        SymbolInfo {
            params: vec![brink_ir::ParamInfo {
                name: param_name.to_string(),
                is_ref: true,
                is_divert: false,
            }],
            ..knot_symbol(id, name)
        }
    }

    /// A minimal, empty [`InferPass`] over `ctx` — every field literal
    /// [`infer_def_body`]'s own constructor uses, mechanically emptied.
    ///
    /// Reproducing issue #1779's exact hazard requires combining a fn-value
    /// *creation* site (`#fn(target)`, produced only by the ink frontend's
    /// `hir::lower::expr::sigils`) with a lambda (`|…| …`, produced only by
    /// the native frontend's `hir::lower_native::lambda`) in the same body —
    /// no single frontend can parse both together today (confirmed against
    /// `origin/main`: `brink_syntax_native::parse` rejects `#fn(...)` with a
    /// `HASH`-unexpected parse error, and `brink-syntax`'s ink grammar has
    /// no lambda production at all). `InferPass` itself is HIR-shaped and
    /// frontend-agnostic, so a hand-built HIR fragment is the only way to
    /// pin this analyzer-level invariant directly; see the tests below for
    /// why that is still a real, if not-yet-source-reachable, soundness gap
    /// rather than a purely hypothetical one.
    fn empty_pass<'a, 'b>(ctx: &'a BodyCtx<'b>) -> InferPass<'a, 'b> {
        InferPass {
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
            annotated: BTreeMap::new(),
            value_calls: Vec::new(),
            direct_call_arg_mismatches: Vec::new(),
            typed_assign_mismatches: Vec::new(),
            field_assign_mismatches: Vec::new(),
            lambda_annotation_mismatches: Vec::new(),
            ufcs_call_args: Vec::new(),
            array_remove_calls: Vec::new(),
            created_fn_values: BTreeSet::new(),
            local_fn_origins: BTreeMap::new(),
            pending_value_calls: Vec::new(),
            lambda_param_names: BTreeMap::new(),
            param_index: BTreeMap::new(),
            param_writes: BTreeSet::new(),
            param_holes: BTreeSet::new(),
            pending_call_fn_args: Vec::new(),
            call_fn_args: BTreeMap::new(),
            dotted_field_read_tainted: false,
        }
    }

    /// Direct, white-box pin on the classification guard itself: a Temp
    /// resolution named "f" narrows normally when nothing shadows it, and
    /// stops narrowing the instant an active lambda's own param claims that
    /// name — regardless of what `local_fn_origins["f"]` says.
    #[test]
    fn lambda_param_shadow_forces_local_call_origin_to_unknown() {
        let f_id = DefinitionId::new(DefinitionTag::LocalVar, 1);
        let mut index = SymbolIndex::default();
        index
            .symbols
            .insert(f_id, temp_symbol(f_id, "f", range(0, 1)));

        let resolution_by_range = BTreeMap::new();
        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        assert_eq!(
            pass.local_call_origin(f_id),
            ValueCallOrigin::Local("f".to_string()),
            "outside any lambda, a bare Temp reference narrows normally"
        );

        pass.lambda_param_names.insert("f".to_string(), 1);
        assert_eq!(
            pass.local_call_origin(f_id),
            ValueCallOrigin::Unknown,
            "issue #1779: a name shadowed by an active lambda param must \
             never be trusted as Local, regardless of what local_fn_origins \
             holds for that name"
        );
    }

    /// End-to-end regression pin for the issue's own suspected repro shape:
    ///
    /// ```text
    /// ~ temp f = #fn(bar)
    /// ~ temp result = (|f| { ~ f() })(something)
    /// ```
    ///
    /// The enclosing `f` traces to `bar` (seeded into `local_fn_origins`
    /// directly, standing in for the `#fn(bar)` write `record_fn_write`
    /// would otherwise perform). The lambda's *own* param, also named `f`,
    /// is a structurally different `DefinitionId` (real name resolution
    /// disambiguates the two correctly by scope) but the exact same bare
    /// name `local_call_origin`/`local_fn_origins` key on.
    ///
    /// Before the fix: `infer_lambda` walks the lambda's `~ f()`, classifies
    /// it `ValueCallOrigin::Local("f")`, and `resolve_pending_value_calls`
    /// (run after `infer_lambda` returns, exactly as `infer_def_body` always
    /// does) resolves that name against `local_fn_origins["f"]` — which
    /// still holds the *enclosing* `f`'s summary, since nothing inside this
    /// lambda ever writes "f" (a pure read never touches
    /// `local_fn_origins` at all, so no snapshot/restore timing would have
    /// changed this). The call is spuriously narrowed to `bar`: `calls`
    /// gains `bar` and `effect_opaque` stays `false` — an under-report, the
    /// exact direction spec §3 forbids.
    ///
    /// After the fix: the lambda's own param name is shadowed for the
    /// duration of the walk, `local_call_origin` refuses to classify the
    /// call as `Local` at all, and it correctly falls back to the pessimal
    /// floor.
    #[test]
    fn lambda_param_collision_with_a_traced_enclosing_local_stays_pessimal() {
        let bar_id = DefinitionId::new(DefinitionTag::Address, 2);
        let lambda_f_id = DefinitionId::new(DefinitionTag::LocalVar, 3);

        let mut index = SymbolIndex::default();
        index.symbols.insert(bar_id, knot_symbol(bar_id, "bar"));
        index
            .symbols
            .insert(lambda_f_id, temp_symbol(lambda_f_id, "f", range(10, 11)));

        let call_range = range(20, 23);
        let mut resolution_by_range = BTreeMap::new();
        resolution_by_range.insert(range_key(call_range), lambda_f_id);

        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable: BTreeSet<DefinitionId> = [bar_id].into_iter().collect();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);
        // Stands in for the enclosing `~ temp f = #fn(bar)` this pass would
        // already have walked by the time it reaches the lambda.
        pass.local_fn_origins.insert(
            "f".to_string(),
            LocalFnOrigins {
                targets: [bar_id].into_iter().collect(),
                untraced: false,
            },
        );

        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "f".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: None,
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Block {
                stmts: vec![BlockStmt::ExprStmt(Expr::Call(
                    HirPath {
                        segments: vec![Name {
                            text: "f".to_string(),
                            range: call_range,
                        }],
                        range: call_range,
                    },
                    Vec::new(),
                ))],
                tail: None,
            },
        };

        pass.infer_lambda(&lambda);
        pass.resolve_pending_value_calls();

        assert!(
            pass.effect_opaque,
            "issue #1779: a call through a lambda's own param must fall back \
             to the pessimal floor, not silently narrow against an \
             unrelated enclosing local's summary"
        );
        assert!(
            !pass.calls.contains(&bar_id),
            "the lambda's own (unrelated, unmodeled) call must never be \
             attributed to the enclosing local's traced target"
        );
    }

    /// Positive control (PR #1731's review lesson: a fixture that already
    /// hit the safe floor — here, a fixture that already narrowed
    /// correctly — must keep doing so). Same enclosing `f` traced to `bar`,
    /// but the lambda's own param is named `g`, not `f`: the `~ f()` call
    /// inside genuinely captures the *enclosing* local (a legitimate,
    /// non-colliding capture), so it must still narrow to `bar` exactly as
    /// it did before this fix — the guard is by-name and must not fire for
    /// a name it was never asked to shadow.
    #[test]
    fn lambda_capturing_a_non_colliding_enclosing_local_still_narrows() {
        let bar_id = DefinitionId::new(DefinitionTag::Address, 12);
        let enclosing_f_id = DefinitionId::new(DefinitionTag::LocalVar, 13);

        let mut index = SymbolIndex::default();
        index.symbols.insert(bar_id, knot_symbol(bar_id, "bar"));
        index.symbols.insert(
            enclosing_f_id,
            temp_symbol(enclosing_f_id, "f", range(0, 1)),
        );

        let call_range = range(20, 23);
        let mut resolution_by_range = BTreeMap::new();
        resolution_by_range.insert(range_key(call_range), enclosing_f_id);

        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable: BTreeSet<DefinitionId> = [bar_id].into_iter().collect();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);
        pass.local_fn_origins.insert(
            "f".to_string(),
            LocalFnOrigins {
                targets: [bar_id].into_iter().collect(),
                untraced: false,
            },
        );

        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "g".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: None,
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Block {
                stmts: vec![BlockStmt::ExprStmt(Expr::Call(
                    HirPath {
                        segments: vec![Name {
                            text: "f".to_string(),
                            range: call_range,
                        }],
                        range: call_range,
                    },
                    Vec::new(),
                ))],
                tail: None,
            },
        };

        pass.infer_lambda(&lambda);
        pass.resolve_pending_value_calls();

        assert!(
            !pass.effect_opaque,
            "a legitimate capture of a non-colliding enclosing local must \
             still narrow — the fix must not widen the pessimal floor \
             beyond the actual collision case"
        );
        assert!(
            pass.calls.contains(&bar_id),
            "the captured enclosing local's own traced target must still \
             be recorded"
        );
    }

    /// Regression guard for issue #1790: frame-scoped fields must be
    /// snapshot/restored around a lambda body walk.
    ///
    /// A lambda's own locals (both declared `~ temp` and implicit params)
    /// are scoped to the lambda's own frame. If the `locals` field (or any
    /// other frame-scoped field in the snapshot/restore list) is not
    /// properly restored after a lambda walk, a lambda-local shadow would
    /// remain in the enclosing def's `locals` map with the *lambda-local's*
    /// type, corrupting the enclosing def's summary.
    ///
    /// This test constructs exactly that scenario: an enclosing `x` of type
    /// `int`, a lambda that declares its own `x` of type `float` (shadowing
    /// both `locals` and, via an ascription, `annotated`), and a
    /// `BlockStmt::Return` inside the lambda body (exercising `return_ty`/
    /// `has_value_return`). It verifies that after the lambda walk, every
    /// one of the five frame-scoped fields snapshotted/restored in the
    /// `LambdaBody::Block` arm of `infer_lambda` — `return_ty`,
    /// `has_value_return`, `locals`, `annotated`, `local_fn_origins` — has
    /// been restored to what it held before the walk. If any of them is
    /// forgotten in the snapshot or restore, this test will fail.
    ///
    /// ## Why this catches the hazard
    ///
    /// A test that merely asserts "the five fields are restored" would pass
    /// forever even if a sixth field is added and not snapshotted. This test
    /// instead exercises the consequence: a leaked field's effects on the
    /// observable state (in this case, the type of a local). Forgetting to
    /// snapshot *any* of the five fields would cause this assertion to fail.
    /// Adding a sixth field and forgetting to snapshot it would also fail
    /// *if* that field affects locals or another observable surface. (See
    /// the issue discussion for field-count and exhaustive-match mechanical
    /// guards that could strengthen this further.)
    #[test]
    fn lambda_local_shadow_frame_boundary_guard() {
        let index = SymbolIndex::default();
        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let resolution_by_range = BTreeMap::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        // Enclosing def state before the lambda walk, one entry per
        // frame-scoped field, all keyed/shadowed by the same name `x`
        // where that applies:
        //   - `locals`: a prior `~ temp x = 42` in the enclosing body.
        pass.locals.insert("x".to_string(), Ty::Int);
        //   - `annotated`: a prior `~ temp x: int = …` ascription fallback.
        pass.annotated.insert("x".to_string(), Ty::Int);
        //   - `local_fn_origins`: a prior traced `~ temp x = #fn(bar)`-style
        //     write — `untraced: false` so a leaked lambda-local write
        //     (which sets `untraced: true`, see `bump_local_write`) is
        //     distinguishable from the pre-walk state.
        pass.local_fn_origins.insert(
            "x".to_string(),
            LocalFnOrigins {
                targets: BTreeSet::new(),
                untraced: false,
            },
        );
        //   - `return_ty`: the enclosing def has already seen a
        //     `~ return` of its own, of type `int`.
        //   - `has_value_return`: deliberately left `false` here (its
        //     `empty_pass` default) — the lambda body below performs its
        //     *own* `~ return`, which sets this to `true`. Leaving the
        //     enclosing value `false` is what makes a missing restore of
        //     this field observable: post-walk `true` would prove the
        //     lambda's own return leaked out, while a correct restore
        //     brings it back to `false`.
        pass.return_ty = Ty::Int;

        // Lambda body shadows every one of those by name/kind:
        //   - a `TempDecl` for `x` with both a `float`-typed ascription
        //     (exercises `annotated` and `register_ascription`) and a
        //     `float`-typed value (exercises `locals` via `bind_local` and
        //     `local_fn_origins` via `bump_local_write`, which sets
        //     `untraced: true` here since the value isn't a traced `#fn`
        //     literal), and
        //   - a `BlockStmt::Return` of a `bool` value (exercises
        //     `return_ty`/`has_value_return`).
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: Vec::new(),
            return_type: None,
            body: brink_ir::LambdaBody::Block {
                stmts: vec![
                    BlockStmt::TempDecl(brink_ir::TempDecl {
                        ptr: Provenance::synthetic(NodeClass::Stmt, range(8, 12)),
                        name: Name {
                            text: "x".to_string(),
                            range: range(10, 11),
                        },
                        value: Some(Expr::Float(brink_ir::FloatBits(2.5_f64.to_bits()))),
                        annotation: Some(brink_ir::TypeExpr::Named {
                            name: "float".to_string(),
                            range: range(12, 17),
                        }),
                    }),
                    BlockStmt::Return(brink_ir::Return {
                        ptr: None,
                        kind: brink_ir::ReturnKind::Explicit,
                        value: Some(Expr::Bool(true)),
                        onwards_args: Vec::new(),
                    }),
                ],
                tail: None,
            },
        };

        pass.infer_lambda(&lambda);

        // Every frame-scoped field must be restored to its pre-walk value.
        // If any one of them is not properly snapshotted/restored around
        // the `LambdaBody::Block` arm of `infer_lambda`, the corresponding
        // assertion below fails.
        assert_eq!(
            pass.locals.get("x"),
            Some(&Ty::Int),
            "issue #1790: a lambda-local shadow must not leak into the \
             enclosing def's `locals`"
        );
        assert_eq!(
            pass.annotated.get("x"),
            Some(&Ty::Int),
            "issue #1790: a lambda-local ascription must not leak into the \
             enclosing def's `annotated`"
        );
        assert_eq!(
            pass.local_fn_origins.get("x"),
            Some(&LocalFnOrigins {
                targets: BTreeSet::new(),
                untraced: false,
            }),
            "issue #1790: a lambda-local write must not leak into the \
             enclosing def's `local_fn_origins`"
        );
        assert_eq!(
            pass.return_ty,
            Ty::Int,
            "issue #1790: a lambda's own `return` must not leak into the \
             enclosing def's `return_ty`"
        );
        assert!(
            !pass.has_value_return,
            "issue #1790: the lambda's own `has_value_return` must not \
             leak into the enclosing def's `has_value_return`"
        );
    }

    /// Pins the `LambdaBody::Expr` asymmetry (issue #1816): an
    /// expression-bodied lambda opens **no** frame at all — unlike
    /// `LambdaBody::Block`, whose frame-scoped fields are snapshotted before
    /// the walk and restored after (see `infer_lambda`'s doc, and
    /// `docs/effects-spec.md` §4.1). This is a deliberate design choice
    /// (`Expr` has no `TempDecl`/`Assignment`/`Return` of its own to leak),
    /// not an oversight — but nothing pinned it, so a future "fix" that adds
    /// a snapshot/restore around the `Expr` arm to match `Block` would
    /// silently change this behavior with no test to catch it.
    ///
    /// `LambdaBody::Expr` genuinely cannot write `locals`/`annotated`/
    /// `return_ty`/`has_value_return` directly — those are only ever
    /// mutated by `BlockStmt` variants, which an expression body has none
    /// of. But it *can* still reach `local_fn_origins` — a frame-scoped
    /// field — indirectly: a call inside the expression body that passes an
    /// **enclosing** `Temp` local through a `ref` parameter records an
    /// untraced write against that name via `record_ref_param_writes` →
    /// `record_fn_write` → `bump_local_write`, exactly as it would in an
    /// enclosing (non-lambda) call. Because `LambdaBody::Expr` opens no
    /// frame, this write must be **visible after** `infer_lambda` returns —
    /// if a future change wraps the `Expr` arm in a snapshot/restore, this
    /// write would be silently discarded and this assertion would fail.
    #[test]
    fn lambda_expr_body_opens_no_frame_for_ref_param_write() {
        let poke_id = DefinitionId::new(DefinitionTag::Address, 21);
        let x_id = DefinitionId::new(DefinitionTag::LocalVar, 22);

        let mut index = SymbolIndex::default();
        index
            .symbols
            .insert(poke_id, knot_symbol_with_ref_param(poke_id, "poke", "p"));
        index
            .symbols
            .insert(x_id, temp_symbol(x_id, "x", range(0, 1)));

        let call_range = range(20, 24);
        let x_arg_range = range(25, 26);
        let mut resolution_by_range = BTreeMap::new();
        resolution_by_range.insert(range_key(call_range), poke_id);
        resolution_by_range.insert(range_key(x_arg_range), x_id);

        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);
        // Enclosing def state before the lambda walk: `x` already traced to
        // no targets, `untraced: false` — distinguishable from the
        // `untraced: true` a ref-param write sets via `bump_local_write`.
        pass.local_fn_origins.insert(
            "x".to_string(),
            LocalFnOrigins {
                targets: BTreeSet::new(),
                untraced: false,
            },
        );

        // `|| poke(ref x)` — an expression-bodied lambda whose sole
        // expression is a call passing the enclosing `x` through `poke`'s
        // `ref` param.
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: Vec::new(),
            return_type: None,
            body: brink_ir::LambdaBody::Expr(Box::new(Expr::Call(
                HirPath {
                    segments: vec![Name {
                        text: "poke".to_string(),
                        range: call_range,
                    }],
                    range: call_range,
                },
                vec![Expr::Path(HirPath {
                    segments: vec![Name {
                        text: "x".to_string(),
                        range: x_arg_range,
                    }],
                    range: x_arg_range,
                })],
            ))),
        };

        pass.infer_lambda(&lambda);

        assert_eq!(
            pass.local_fn_origins.get("x"),
            Some(&LocalFnOrigins {
                targets: BTreeSet::new(),
                untraced: true,
            }),
            "issue #1816: a `LambdaBody::Expr` opens no frame, so a \
             ref-param write reaching an enclosing local from inside the \
             lambda's own expression must be visible in the enclosing \
             def's `local_fn_origins` after the walk, not swallowed by a \
             (would-be, and wrong) frame restore"
        );
    }

    // ── Issue #1941: a lambda's value-position read of an annotated ──
    // param still typed Unknown — the structurally parallel gap #1938 left
    // for `infer_return`'s fn-return position. `infer_lambda`'s tail
    // (`LambdaBody::Block`) and sole expression (`LambdaBody::Expr`) are
    // both this lambda's own value position, exactly like a `return`, but
    // neither ran the read through `or_own_annotation` — and, unlike a
    // plain `fn`/`flow`, nothing ever seeded `self.annotated` with a
    // lambda's own param annotations in the first place (see
    // `infer_lambda`'s new seeding block for why a `fn`/`flow` gets this for
    // free at `new_pass` time and a lambda did not).

    #[test]
    fn lambda_tail_expr_reading_an_annotated_param_exports_its_declared_type() {
        let index = SymbolIndex::default();
        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let resolution_by_range = BTreeMap::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        // `|t: int| { t }` — a block-bodied lambda whose tail is a bare
        // read of its own annotated param, no annotation-resolving
        // `resolution_by_range` entry at all (mirrors `own_annotation`'s
        // bare single-segment fallback, exercised the same way #1912's
        // fixtures were).
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "t".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: Some(brink_ir::TypeExpr::Named {
                    name: "int".to_string(),
                    range: range(13, 16),
                }),
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Block {
                stmts: Vec::new(),
                tail: Some(Box::new(Expr::Path(HirPath {
                    segments: vec![Name {
                        text: "t".to_string(),
                        range: range(20, 21),
                    }],
                    range: range(20, 21),
                }))),
            },
        };

        let ty = pass.infer_lambda(&lambda);
        assert_eq!(
            ty,
            Ty::Fn(vec![Ty::Int], Box::new(Ty::Int), FnRow::unknown()),
            "issue #1941: `t`'s tail read is a pure value-position read of \
             an annotated param — it must export `int`, not `Unknown`"
        );
    }

    #[test]
    fn lambda_expr_body_reading_an_annotated_param_exports_its_declared_type() {
        let index = SymbolIndex::default();
        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let resolution_by_range = BTreeMap::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        // `|t: int| t` — the expression-bodied twin. `LambdaBody::Expr`
        // opens no frame at all (issue #1816), so this exercises the exact
        // same `or_own_annotation` call from the other arm of the `match`.
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "t".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: Some(brink_ir::TypeExpr::Named {
                    name: "int".to_string(),
                    range: range(13, 16),
                }),
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Expr(Box::new(Expr::Path(HirPath {
                segments: vec![Name {
                    text: "t".to_string(),
                    range: range(20, 21),
                }],
                range: range(20, 21),
            }))),
        };

        let ty = pass.infer_lambda(&lambda);
        assert_eq!(
            ty,
            Ty::Fn(vec![Ty::Int], Box::new(Ty::Int), FnRow::unknown()),
            "issue #1941: an expression-bodied lambda's sole expression is \
             its value position — a bare annotated-param read must export \
             `int`, not `Unknown`"
        );
    }

    /// The seeding block `infer_lambda` gained for issue #1941 must be
    /// scoped to the lambda's own walk exactly like every other
    /// `self.annotated` write in this function — an enclosing local of the
    /// same bare name, annotated differently, must see its own annotation
    /// again once the lambda walk finishes (mirrors
    /// `lambda_local_shadow_frame_boundary_guard`'s "five frame-scoped
    /// fields" guard, one field, one new write site).
    #[test]
    fn lambda_param_annotation_seed_does_not_leak_past_the_walk() {
        let index = SymbolIndex::default();
        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let resolution_by_range = BTreeMap::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);
        // Enclosing frame: a same-named `t` already ascribed `string` (e.g.
        // a prior `~ temp t: string = …`).
        pass.annotated.insert("t".to_string(), Ty::String);

        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 30)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "t".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: Some(brink_ir::TypeExpr::Named {
                    name: "int".to_string(),
                    range: range(13, 16),
                }),
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Expr(Box::new(Expr::Path(HirPath {
                segments: vec![Name {
                    text: "t".to_string(),
                    range: range(20, 21),
                }],
                range: range(20, 21),
            }))),
        };

        let ty = pass.infer_lambda(&lambda);
        assert_eq!(
            ty,
            Ty::Fn(vec![Ty::Int], Box::new(Ty::Int), FnRow::unknown()),
            "the lambda's own `t: int` seed must win inside its own walk"
        );
        assert_eq!(
            pass.annotated.get("t"),
            Some(&Ty::String),
            "issue #1941: the lambda's own param-annotation seed must not \
             leak past the walk — the enclosing frame's same-named `t: \
             string` ascription must be exactly what it was before"
        );
    }

    // ── Issue #1994 (RULED 2026-08-01, closing #1932): a lambda's own ──
    // written annotation now governs its param/return type unconditionally
    // — narrowing #1910's body-derived read-back to the unannotated case —
    // and an incompatible body raises an eager `LambdaAnnotationMismatch`
    // (`E174`) at the lambda itself, never silently discarded.

    #[test]
    fn lambda_return_annotation_governs_over_a_disagreeing_body() {
        let index = SymbolIndex::default();
        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let resolution_by_range = BTreeMap::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        // `||: int { "wrong" }` — a zero-arg lambda whose written return
        // annotation is `int` but whose body always produces a `string`.
        let return_annotation_range = range(8, 11);
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(0, 30)),
            params: Vec::new(),
            return_type: Some(brink_ir::TypeExpr::Named {
                name: "int".to_string(),
                range: return_annotation_range,
            }),
            body: brink_ir::LambdaBody::Expr(Box::new(Expr::String(brink_ir::StringExpr {
                parts: vec![brink_ir::StringPart::Literal("wrong".to_string())],
            }))),
        };

        let ty = pass.infer_lambda(&lambda);
        assert_eq!(
            ty,
            Ty::Fn(Vec::new(), Box::new(Ty::Int), FnRow::unknown()),
            "issue #1994: the written return annotation must govern the \
             lambda's resulting type even though the body disagrees — it \
             must not be silently overridden by the body-derived `string`"
        );
        assert_eq!(
            pass.lambda_annotation_mismatches,
            vec![LambdaAnnotationMismatch {
                range: return_annotation_range,
                param_name: None,
                expected: Ty::Int,
                found: Ty::String,
            }],
            "issue #1994: an eager mismatch must be recorded pointing at \
             the lambda's own written return annotation, not deferred to \
             wherever the lambda is later called"
        );
    }

    #[test]
    fn lambda_param_annotation_governs_over_a_disagreeing_body_write() {
        let x_id = DefinitionId::new(DefinitionTag::LocalVar, 1);
        let mut index = SymbolIndex::default();
        // Issue #1994's own field doc: a lambda's own params are indexed as
        // `SymbolKind::Temp`, the same project-wide keyspace an enclosing
        // `~ temp` gets — `temp_symbol` below mirrors that real convention.
        index
            .symbols
            .insert(x_id, temp_symbol(x_id, "x", range(10, 11)));

        let target_range = range(20, 21);
        let mut resolution_by_range = BTreeMap::new();
        resolution_by_range.insert(range_key(target_range), x_id);

        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        // `|x: int| { x = true; }` — the body's only evidence for `x`
        // pins it to `bool` (a clean, un-conflicted single write — `x` was
        // never observed with any other type first), disagreeing with the
        // lambda's own written `x: int` annotation.
        let annotation_range = range(13, 16);
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 40)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "x".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: Some(brink_ir::TypeExpr::Named {
                    name: "int".to_string(),
                    range: annotation_range,
                }),
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Block {
                stmts: vec![BlockStmt::Assignment(brink_ir::Assignment {
                    ptr: Provenance::synthetic(NodeClass::Stmt, range(18, 26)),
                    target: Expr::Path(HirPath {
                        segments: vec![Name {
                            text: "x".to_string(),
                            range: target_range,
                        }],
                        range: target_range,
                    }),
                    op: AssignOp::Set,
                    value: Expr::Bool(true),
                })],
                tail: None,
            },
        };

        let ty = pass.infer_lambda(&lambda);
        assert_eq!(
            ty,
            Ty::Fn(vec![Ty::Int], Box::new(Ty::Unknown), FnRow::unknown()),
            "issue #1994: the lambda's own written `x: int` annotation must \
             govern the param's resulting type even though its body's only \
             evidence disagrees"
        );
        assert_eq!(
            pass.lambda_annotation_mismatches,
            vec![LambdaAnnotationMismatch {
                range: annotation_range,
                param_name: Some("x".to_string()),
                expected: Ty::Int,
                found: Ty::Bool,
            }],
            "issue #1994: an eager mismatch must be recorded pointing at \
             the lambda's own written param annotation, not silently \
             discarded in favor of the body's own wrong derivation"
        );
    }

    #[test]
    fn lambda_unannotated_param_still_exports_its_body_derived_type() {
        // Regression pin (issue #1994's own scope note: "do not simply
        // revert #1910 — its fix for the unannotated case is correct"):
        // an unannotated param/return must still export whatever its body
        // derives, exactly as #1910 left it — this test would fail if the
        // new annotation-precedence logic were accidentally applied to the
        // unannotated case too (e.g. by treating `None` as "annotation
        // says Unknown" instead of "no annotation to govern with").
        let x_id = DefinitionId::new(DefinitionTag::LocalVar, 1);
        let mut index = SymbolIndex::default();
        index
            .symbols
            .insert(x_id, temp_symbol(x_id, "x", range(10, 11)));

        let target_range = range(20, 21);
        let mut resolution_by_range = BTreeMap::new();
        resolution_by_range.insert(range_key(target_range), x_id);

        let globals = BTreeMap::new();
        let known_sigs = BTreeMap::new();
        let inferable = BTreeSet::new();
        let list_names = BTreeSet::new();
        let struct_names = BTreeSet::new();
        let handle_names = BTreeSet::new();
        let ctx = BodyCtx {
            resolution_by_range: &resolution_by_range,
            index: &index,
            globals: &globals,
            known_sigs: &known_sigs,
            inferable: &inferable,
            list_names: &list_names,
            struct_names: &struct_names,
            handle_names: &handle_names,
            native: false,
        };
        let mut pass = empty_pass(&ctx);

        // `|x| { x = true; }` — no written annotation on `x` at all.
        let lambda = brink_ir::LambdaExpr {
            ptr: Provenance::synthetic(NodeClass::Lambda, range(5, 40)),
            params: vec![brink_ir::Param {
                name: Name {
                    text: "x".to_string(),
                    range: range(10, 11),
                },
                is_ref: false,
                is_divert: false,
                annotation: None,
            }],
            return_type: None,
            body: brink_ir::LambdaBody::Block {
                stmts: vec![BlockStmt::Assignment(brink_ir::Assignment {
                    ptr: Provenance::synthetic(NodeClass::Stmt, range(18, 26)),
                    target: Expr::Path(HirPath {
                        segments: vec![Name {
                            text: "x".to_string(),
                            range: target_range,
                        }],
                        range: target_range,
                    }),
                    op: AssignOp::Set,
                    value: Expr::Bool(true),
                })],
                tail: None,
            },
        };

        let ty = pass.infer_lambda(&lambda);
        assert_eq!(
            ty,
            Ty::Fn(vec![Ty::Bool], Box::new(Ty::Unknown), FnRow::unknown()),
            "issue #1910 (unchanged by #1994): an unannotated param still \
             exports whatever its body derives"
        );
        assert!(
            pass.lambda_annotation_mismatches.is_empty(),
            "an unannotated param has nothing to disagree with — no \
             mismatch should ever be recorded for it"
        );
    }
}
