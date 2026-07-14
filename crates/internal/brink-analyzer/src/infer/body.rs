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

use super::ty::{Ty, unify, unify_all};
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
}

/// The result of walking one definition's body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct BodyResult {
    /// Declared params, in declaration order, with their observed type.
    pub params: Vec<(String, Ty)>,
    /// Every local (params ∪ temps) by bare name — `params` is the ordered
    /// subset callers care about for a `Sig`-shaped view; `locals` is the
    /// full picture for hover/diagnostics consumers (TM-5).
    pub locals: BTreeMap<String, Ty>,
    pub return_ty: Ty,
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
    let annotated: BTreeMap<String, Ty> = def
        .params
        .iter()
        .filter_map(|p| {
            let te = p.annotation.as_ref()?;
            let ty = crate::annotations::resolve(te, ctx.list_names, ctx.struct_names)?;
            Some((p.name.text.clone(), ty))
        })
        .collect();

    let mut pass = InferPass {
        ctx,
        locals: BTreeMap::new(),
        return_ty: Ty::Unknown,
        calls: BTreeSet::new(),
        referenced_globals: BTreeSet::new(),
        annotated,
        value_calls: Vec::new(),
    };
    pass.infer_block(def.body);

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
            .and_then(|te| crate::annotations::resolve(te, ctx.list_names, ctx.struct_names))
            .unwrap_or(pass.return_ty)
    } else {
        pass.return_ty
    };

    BodyResult {
        params: param_types,
        locals: pass.locals,
        return_ty,
        calls: pass.calls,
        referenced_globals: pass.referenced_globals,
        value_calls: pass.value_calls,
    }
}

struct InferPass<'a, 'b> {
    ctx: &'a BodyCtx<'b>,
    locals: BTreeMap<String, Ty>,
    return_ty: Ty,
    calls: BTreeSet<DefinitionId>,
    referenced_globals: BTreeSet<DefinitionId>,
    /// Resolvable annotation/ascription types by local name — params up
    /// front, temps added as their declarations are walked. Consulted only
    /// as a *fallback* at consumption sites where the body-derived type is
    /// still `Unknown` (call-position callee lookup, the end-of-walk
    /// signature overlay) — never joined into the lattice, so E063's
    /// two-independent-derivations comparison stays intact.
    annotated: BTreeMap<String, Ty>,
    value_calls: Vec<ValueCallFact>,
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
    fn observe(&mut self, expr: &Expr, ty: &Ty) {
        if ty.is_unknown() {
            return;
        }
        let Expr::Path(p) = expr else { return };
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
            && let Some(ty) =
                crate::annotations::resolve(te, self.ctx.list_names, self.ctx.struct_names)
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
        }
    }

    fn infer_path(&mut self, p: &HirPath) -> Ty {
        let Some(def) = self.resolve(p.range) else {
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

        self.check_value_call(path.range, &callee, callee_ty, args, arg_tys)
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
    ) -> Ty {
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
    fn infer_intrinsic(
        &mut self,
        name: &str,
        range: TextRange,
        args: &[Expr],
        arg_tys: &[Ty],
    ) -> Ty {
        match name {
            // `len` (stdlib slice 1) and `int` (TM-3-completion conversion
            // intrinsic, #659) both return a fixed `Ty::Int` independent of
            // the argument — merged into one arm per clippy's
            // `match_same_arms` (identical bodies, distinct call sites).
            "len" | "int" => Ty::Int,
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
            "push" => {
                if let (Some(Ty::Array(elem)), Some(item)) = (arg_tys.first(), args.get(1)) {
                    self.observe(item, elem);
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
            "float" => Ty::Float,
            "string" => Ty::String,
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
                self.check_value_call(range, &callee, callee_ty, &args[1..], &arg_tys[1..])
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
            }
            Stmt::Assignment(a) => {
                self.infer_expr(&a.target);
                let ty = self.infer_expr(&a.value);
                self.observe(&a.target, &ty);
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
        }
    }

    fn infer_return(&mut self, value: Option<&Expr>, onwards: &[Expr]) {
        if let Some(v) = value {
            let ty = self.infer_expr(v);
            self.return_ty = unify(&self.return_ty, &ty);
        }
        for e in onwards {
            self.infer_expr(e);
        }
    }

    fn infer_content(&mut self, content: &Content) {
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
            }
            BlockStmt::Assignment(a) => {
                self.infer_expr(&a.target);
                let ty = self.infer_expr(&a.value);
                self.observe(&a.target, &ty);
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
                let iter_ty = self.infer_expr(&f.iterable);
                let elem_ty = match iter_ty {
                    Ty::Array(elem) => *elem,
                    Ty::Map(k, _) => *k,
                    _ => Ty::Unknown,
                };
                self.bind_local(&f.var_name.text, &elem_ty);
                for s in &f.body {
                    self.infer_block_stmt(s);
                }
            }
            BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
            BlockStmt::ExprStmt(e) => {
                self.infer_expr(e);
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
