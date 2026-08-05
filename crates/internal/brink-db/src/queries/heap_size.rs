//! `heap_size` estimators for the queries #537's measurement flagged as the
//! heaviest Arc-hidden payloads (issue #538, decision log 2026-07-15 "FG-5
//! memory bounding"): [`super::signature_query`], [`super::def_body_query`],
//! [`super::solve_scc_query`], [`super::infer_body_query`], and
//! [`super::lowered_query`]. Wired via `#[salsa::tracked(heap_size = ...)]`
//! so `crate::memory::snapshot`'s `heap_bytes` column reads `Some(_)` for
//! these five instead of the honest-`None` every query reported before this
//! pass (this crate's `memory.rs` module docs explain why `None` was the
//! right default until specific queries earned an estimator).
//!
//! [`super::local_signature_query`] (issue #530) shares [`signature_heap_size`]
//! rather than earning its own estimator: its output is the identical
//! `Option<Arc<Sig>>` shape `signature_query` returns, just resolved via a
//! per-file path, so the same walk applies unchanged.
//!
//! ## Scope: best-effort, not byte-exact
//!
//! These walk `Vec`/`String`/`BTreeMap` allocations — the dominant heap
//! cost per #537's data — through the structurally "big" containers (knot
//! bodies, choice text, content/tag text, the `Ty`/`TypeExpr` type trees).
//! They deliberately do **not** recurse into `Expr`'s own nested
//! `Box`/`Vec` children (binary-op operands, call arguments, literal
//! elements): `Expr` has a couple dozen variants and doing that by hand
//! precisely would either wildcard-match (silently going stale as variants
//! are added — exactly the failure mode this crate's `memory.rs` docs
//! warn a hand-rolled estimator courts) or require a full second AST
//! walker maintained in lockstep with `brink_ir::hir`. `Expr` fields are
//! still counted at their *inline* stack size wherever they sit inside an
//! already-walked `Vec`/`Box` (e.g. `parts: Vec<ContentPart>` bills
//! `size_of::<ContentPart>()` per element regardless of which variant, so
//! an `Interpolation(Expr)` element's own inline size is included) — the
//! gap is only the *further* heap allocations an `Expr` itself might own
//! (a nested `Box<Expr>`, a `Path`'s segment names). Every match below is
//! exhaustive (no `_` fallback) specifically so a new AST variant is a
//! compile error here, not a silent undercount.
//!
//! Undercounting is the deliberate failure direction — an estimator that
//! sometimes reports less than the true heap footprint is honest; one that
//! silently drifts stale-inflated would mislead a reader worse than the
//! `None` this replaces.

use std::mem::{size_of, size_of_val};
use std::sync::Arc;

use brink_analyzer::{
    BodyTypes, DirectCallArgMismatch, FieldAssignMismatch, InferredSig, LambdaAnnotationMismatch,
    LambdaEscapeSlot, Sig, Ty, TypedAssignMismatch, UfcsCallArgs, ValueCallFact,
};
use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Choice, ChoiceSet, CondBranch, Conditional, Content, ContentPart,
    DeclaredSymbol, Diagnostic, DivertPath, DivertTarget, ElseBranch, HirFile, IfStmt, Knot, Name,
    Param, ParamInfo, Path, Sequence, SequenceBranch, Stmt, Tag, TempDecl, TypeExpr, VarDecl,
};
#[cfg(test)]
use brink_ir::{Expr, ForStmt, NodeClass, Provenance};

#[cfg(test)]
use super::lower_file;
use super::{DefBody, LoweredFile, SolvedScc};

// ─── Generic container helpers ─────────────────────────────────────────

fn vec_heap<T>(v: &[T]) -> usize {
    // `len`, not `capacity` — a `&Vec<T>` deref loses the capacity, only the
    // slice's length survives, so this undercounts by whatever spare
    // capacity a `Vec` is carrying at snapshot time. Same undercount
    // direction as every other approximation in this module.
    size_of_val(v)
}

fn string_heap(s: &String) -> usize {
    s.capacity()
}

fn name_heap(n: &Name) -> usize {
    string_heap(&n.text)
}

fn path_heap(p: &Path) -> usize {
    vec_heap(&p.segments) + p.segments.iter().map(name_heap).sum::<usize>()
}

fn divert_path_heap(dp: &DivertPath) -> usize {
    match dp {
        DivertPath::Path(p) => path_heap(p),
        DivertPath::Done | DivertPath::End => 0,
    }
}

fn divert_target_heap(dt: &DivertTarget) -> usize {
    divert_path_heap(&dt.path) + vec_heap(&dt.args)
}

// ─── `Ty` / `TypeExpr` (small, closed, fully recursive) ────────────────

fn ty_heap(ty: &Ty) -> usize {
    match ty {
        Ty::Int
        | Ty::Float
        | Ty::Bool
        | Ty::String
        // issue #1846: `Ty::Content` carries no payload of its own (unlike
        // `Ty::List`/`Struct`/`Handle`'s nominal name) — zero heap cost,
        // same as every other unit-payload leaf here.
        | Ty::Content
        | Ty::Divert
        | Ty::Range { .. }
        | Ty::Tower(_)
        | Ty::Unknown
        | Ty::Conflicted => 0,
        Ty::List(name) | Ty::Struct(name) | Ty::Handle(name) => string_heap(name),
        Ty::Array(inner) | Ty::Option(inner) | Ty::Weighted(inner) => {
            size_of::<Ty>() + ty_heap(inner)
        }
        Ty::Map(key, value) => size_of::<Ty>() * 2 + ty_heap(key) + ty_heap(value),
        Ty::Fn(params, ret, row) => {
            vec_heap(params)
                + params.iter().map(ty_heap).sum::<usize>()
                + size_of::<Ty>()
                + ty_heap(ret)
                + fn_row_heap(row)
        }
    }
}

/// The effect row riding `Ty::Fn` (issue #1680). The unknown top element is
/// a niche-optimized `None` and costs nothing; a concrete target set costs
/// the boxed `BTreeSet` header plus one id per member. Same
/// per-element-only approximation direction as every other estimate here —
/// the tree's internal node padding is not modeled.
fn fn_row_heap(row: &brink_analyzer::FnRow) -> usize {
    row.targets().map_or(0, |targets| {
        size_of::<std::collections::BTreeSet<DefinitionId>>()
            + targets.len() * size_of::<DefinitionId>()
    })
}

fn type_expr_heap(te: &TypeExpr) -> usize {
    match te {
        TypeExpr::Named { name, .. } => string_heap(name),
        TypeExpr::Generic { name, args, .. } => {
            string_heap(name) + vec_heap(args) + args.iter().map(type_expr_heap).sum::<usize>()
        }
        TypeExpr::Fn { params, ret, .. } => {
            vec_heap(params)
                + params.iter().map(type_expr_heap).sum::<usize>()
                + size_of::<TypeExpr>()
                + type_expr_heap(ret)
        }
    }
}

fn opt_type_expr_heap(te: Option<&TypeExpr>) -> usize {
    te.map_or(0, type_expr_heap)
}

// ─── HIR body tree (`Block`/`Stmt` and the T1b `BlockStmt` superset) ────

fn content_parts_heap(parts: &[ContentPart]) -> usize {
    vec_heap(parts)
        + parts
            .iter()
            .map(|part| match part {
                ContentPart::Text(s) => string_heap(s),
                ContentPart::Glue | ContentPart::Spring | ContentPart::Interpolation(_) => 0,
                ContentPart::InlineConditional(c) => conditional_heap(c),
                ContentPart::InlineSequence(s) => sequence_heap(s),
                ContentPart::Span(span) => span_heap(span),
            })
            .sum::<usize>()
}

fn span_heap(span: &brink_ir::SpanPart) -> usize {
    string_heap(&span.name)
        + vec_heap(&span.attrs)
        + span
            .attrs
            .iter()
            .map(|(k, v)| string_heap(k) + string_heap(v))
            .sum::<usize>()
        + content_parts_heap(&span.children)
}

fn tag_heap(t: &Tag) -> usize {
    content_parts_heap(&t.parts)
}

fn content_heap(c: &Content) -> usize {
    content_parts_heap(&c.parts) + vec_heap(&c.tags) + c.tags.iter().map(tag_heap).sum::<usize>()
}

fn conditional_heap(c: &Conditional) -> usize {
    vec_heap(&c.branches) + c.branches.iter().map(cond_branch_heap).sum::<usize>()
}

fn cond_branch_heap(b: &CondBranch) -> usize {
    // The `as` binding's `Name` is heap-allocated like every other
    // (B1b, issue #1475) — see `block_stmt_heap`'s `ForStmt::val_name`.
    b.binding.as_ref().map_or(0, name_heap) + block_heap(&b.body)
}

fn sequence_heap(s: &Sequence) -> usize {
    vec_heap(&s.branches) + s.branches.iter().map(seq_branch_heap).sum::<usize>()
}

fn seq_branch_heap(b: &SequenceBranch) -> usize {
    block_heap(&b.body)
}

fn choice_heap(c: &Choice) -> usize {
    c.label.as_ref().map_or(0, name_heap)
        + [&c.start_content, &c.bracket_content, &c.inner_content]
            .iter()
            .map(|opt| opt.as_ref().map_or(0, content_heap))
            .sum::<usize>()
        + vec_heap(&c.tags)
        + c.tags.iter().map(tag_heap).sum::<usize>()
        + block_heap(&c.body)
}

fn choice_set_heap(cs: &ChoiceSet) -> usize {
    vec_heap(&cs.choices)
        + cs.choices.iter().map(choice_heap).sum::<usize>()
        + block_heap(&cs.continuation)
}

fn temp_decl_heap(td: &TempDecl) -> usize {
    name_heap(&td.name) + opt_type_expr_heap(td.annotation.as_ref())
}

fn stmt_heap(s: &Stmt) -> usize {
    match s {
        Stmt::Content(c) => content_heap(c),
        Stmt::Divert(d) => divert_target_heap(&d.target),
        Stmt::TunnelCall(tc) => {
            vec_heap(&tc.targets) + tc.targets.iter().map(divert_target_heap).sum::<usize>()
        }
        Stmt::ThreadStart(ts) => divert_target_heap(&ts.target),
        Stmt::TempDecl(td) => temp_decl_heap(td),
        Stmt::Return(r) => vec_heap(&r.onwards_args),
        Stmt::ChoiceSet(cs) => size_of::<ChoiceSet>() + choice_set_heap(cs),
        Stmt::LabeledBlock(b) => size_of::<Block>() + block_heap(b),
        Stmt::Conditional(c) => conditional_heap(c),
        Stmt::Sequence(s) => sequence_heap(s),
        Stmt::LogicBlock(lb) => block_stmts_heap(&lb.stmts),
        // Issue #2108: `AttachElement`'s `Expr` payload is uncounted for
        // the same reason `Assignment`/`ExprStmt`/`Await`'s are — this
        // walker does not recurse into generic `Expr` trees at all (see
        // e.g. `Stmt::Return` above, which counts `onwards_args` but not
        // `value`); `EndElementRun` carries no payload regardless.
        Stmt::Assignment(_)
        | Stmt::ExprStmt(_)
        | Stmt::Await(_)
        | Stmt::EndOfLine
        | Stmt::AttachElement(_)
        | Stmt::EndElementRun => 0,
    }
}

fn block_heap(b: &Block) -> usize {
    b.label.as_ref().map_or(0, name_heap)
        + vec_heap(&b.stmts)
        + b.stmts.iter().map(stmt_heap).sum::<usize>()
}

fn block_stmts_heap(stmts: &[BlockStmt]) -> usize {
    vec_heap(stmts) + stmts.iter().map(block_stmt_heap).sum::<usize>()
}

fn block_stmt_heap(bs: &BlockStmt) -> usize {
    match bs {
        BlockStmt::TempDecl(td) => temp_decl_heap(td),
        BlockStmt::Return(r) => vec_heap(&r.onwards_args),
        BlockStmt::If(i) => if_stmt_heap(i),
        BlockStmt::While(w) => w.binding.as_ref().map_or(0, name_heap) + block_stmts_heap(&w.body),
        BlockStmt::For(f) => {
            name_heap(&f.var_name)
                + f.val_name.as_ref().map_or(0, name_heap)
                + block_stmts_heap(&f.body)
        }
        BlockStmt::Assignment(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Await(_) => 0,
    }
}

fn if_stmt_heap(i: &IfStmt) -> usize {
    i.binding.as_ref().map_or(0, name_heap)
        + block_stmts_heap(&i.body)
        + i.else_branch.as_ref().map_or(0, else_branch_heap)
}

fn else_branch_heap(e: &ElseBranch) -> usize {
    match e {
        ElseBranch::ElseIf(b) => size_of::<IfStmt>() + if_stmt_heap(b),
        ElseBranch::Else(stmts) => block_stmts_heap(stmts),
    }
}

fn param_heap(p: &Param) -> usize {
    name_heap(&p.name) + opt_type_expr_heap(p.annotation.as_ref())
}

fn param_info_heap(p: &ParamInfo) -> usize {
    string_heap(&p.name)
}

// ─── `signature_query` — `Option<Arc<Sig>>` ─────────────────────────────

fn sig_heap(sig: &Sig) -> usize {
    string_heap(&sig.name)
        + vec_heap(&sig.params)
        + sig.params.iter().map(param_info_heap).sum::<usize>()
        + sig.value_ty.as_ref().map_or(0, ty_heap)
        + vec_heap(&sig.param_annotations)
        + sig
            .param_annotations
            .iter()
            .filter_map(Option::as_ref)
            .map(ty_heap)
            .sum::<usize>()
        + sig.return_annotation.as_ref().map_or(0, ty_heap)
}

#[expect(
    clippy::ref_option,
    reason = "salsa's heap_size fn contract is `fn(&Self::Output<'_>) -> usize` — \
              `Self::Output` for signature_query is `Option<Arc<Sig>>`, so the \
              parameter type is fixed by the macro, not a style choice"
)]
pub(crate) fn signature_heap_size(value: &Option<Arc<Sig>>) -> usize {
    value
        .as_ref()
        .map_or(0, |sig| size_of::<Sig>() + sig_heap(sig))
}

// ─── `def_body_query` — `Option<Arc<DefBody>>` ──────────────────────────

fn def_body_heap(body: &DefBody) -> usize {
    vec_heap(&body.params)
        + body.params.iter().map(param_heap).sum::<usize>()
        + opt_type_expr_heap(body.return_annotation.as_ref())
        + block_heap(&body.body)
}

#[expect(
    clippy::ref_option,
    reason = "salsa's heap_size fn contract is `fn(&Self::Output<'_>) -> usize` — \
              `Self::Output` for def_body_query is `Option<Arc<DefBody>>`, so the \
              parameter type is fixed by the macro, not a style choice"
)]
pub(crate) fn def_body_heap_size(value: &Option<Arc<DefBody>>) -> usize {
    value
        .as_ref()
        .map_or(0, |body| size_of::<DefBody>() + def_body_heap(body))
}

// ─── `solve_scc_query` — `Arc<SolvedScc>` ───────────────────────────────

fn inferred_sig_heap(s: &InferredSig) -> usize {
    vec_heap(&s.params) + s.params.iter().map(ty_heap).sum::<usize>() + ty_heap(&s.return_ty)
}

fn value_call_fact_heap(f: &ValueCallFact) -> usize {
    string_heap(&f.callee)
}

/// Issue #1864: a `DirectCallArgMismatch`'s own heap contribution — its
/// `callee` string plus its two `Ty`s (`expected`/`found`), same shape as
/// [`value_call_fact_heap`] extended for the two extra `Ty` fields a direct-
/// call fact carries that a `ValueCallFact` doesn't (its `kind` enum holds
/// those inline instead).
fn direct_call_arg_mismatch_heap(m: &DirectCallArgMismatch) -> usize {
    string_heap(&m.callee) + ty_heap(&m.expected) + ty_heap(&m.found)
}

/// Issue #1877: a `TypedAssignMismatch`'s own heap contribution — mirrors
/// [`direct_call_arg_mismatch_heap`]'s shape (its `target` field plays the
/// same role `callee` does there).
fn typed_assign_mismatch_heap(m: &TypedAssignMismatch) -> usize {
    string_heap(&m.target) + ty_heap(&m.expected) + ty_heap(&m.found)
}

/// Issue #1900: a `FieldAssignMismatch`'s own heap contribution — mirrors
/// [`typed_assign_mismatch_heap`]'s shape (its `root` field plays the same
/// role `target` does there), plus its own `path` `Vec<Name>` (the
/// unresolved field chain) and each segment's own name heap cost.
fn field_assign_mismatch_heap(m: &FieldAssignMismatch) -> usize {
    string_heap(&m.root)
        + ty_heap(&m.root_ty)
        + vec_heap(&m.path)
        + m.path.iter().map(name_heap).sum::<usize>()
        + ty_heap(&m.found)
}

/// Issue #1994: a `LambdaAnnotationMismatch`'s own heap contribution — its
/// optional `param_name` (`None` for a return-slot mismatch, so it costs
/// nothing) plus its two `Ty`s (`expected`/`found`), same shape as
/// [`typed_assign_mismatch_heap`] extended for the optional string.
fn lambda_annotation_mismatch_heap(m: &LambdaAnnotationMismatch) -> usize {
    m.param_name.as_ref().map_or(0, string_heap) + ty_heap(&m.expected) + ty_heap(&m.found)
}

/// Issue #1770: a `LambdaEscapeSlot`'s own heap contribution — its `ty`
/// (same `ty_heap` every other fact's own `Ty` field uses) plus its
/// `slot_label` `String` (`range`/`annotated` own no heap data of their
/// own: two `u32`s and a `bool`, same posture as `array_remove_calls`'
/// `TextRange`).
fn lambda_escape_slot_heap(s: &LambdaEscapeSlot) -> usize {
    ty_heap(&s.ty) + string_heap(&s.slot_label)
}

/// Issue #1881: a `UfcsCallArgs`'s own heap contribution — its `args` `Vec`
/// plus each element's own `Ty` heap cost, same shape `body_types_heap`
/// already applies to `b.params`/`b.locals`. `TextRange` owns no heap data
/// of its own (two `u32`s inline), same posture as `array_remove_calls`.
fn ufcs_call_args_heap(f: &UfcsCallArgs) -> usize {
    vec_heap(&f.args) + f.args.iter().map(ty_heap).sum::<usize>()
}

fn body_types_heap(b: &BodyTypes) -> usize {
    let params_heap: usize = b
        .params
        .iter()
        .map(|(name, ty)| string_heap(name) + ty_heap(ty))
        .sum();
    // `BTreeMap` exposes no `capacity`; approximate one `(key, value)` slot
    // per entry — an undercount of the tree's own node overhead, same
    // direction as every other approximation in this module.
    let locals_heap: usize = b
        .locals
        .iter()
        .map(|(name, ty)| size_of::<(String, Ty)>() + string_heap(name) + ty_heap(ty))
        .sum();
    vec_heap(&b.params)
        + params_heap
        + locals_heap
        + ty_heap(&b.return_ty)
        + vec_heap(&b.value_calls)
        + b.value_calls
            .iter()
            .map(value_call_fact_heap)
            .sum::<usize>()
        // Issue #1532: `TextRange` owns no heap data of its own (two `u32`s
        // inline), so the `Vec`'s own buffer is the whole contribution —
        // same posture as `divert_target_heap`'s `vec_heap(&dt.args)`.
        + vec_heap(&b.array_remove_calls)
        + vec_heap(&b.direct_call_arg_mismatches)
        + b.direct_call_arg_mismatches
            .iter()
            .map(direct_call_arg_mismatch_heap)
            .sum::<usize>()
        + vec_heap(&b.typed_assign_mismatches)
        + b.typed_assign_mismatches
            .iter()
            .map(typed_assign_mismatch_heap)
            .sum::<usize>()
        + vec_heap(&b.field_assign_mismatches)
        + b.field_assign_mismatches
            .iter()
            .map(field_assign_mismatch_heap)
            .sum::<usize>()
        + vec_heap(&b.lambda_annotation_mismatches)
        + b.lambda_annotation_mismatches
            .iter()
            .map(lambda_annotation_mismatch_heap)
            .sum::<usize>()
        + vec_heap(&b.ufcs_call_args)
        + b.ufcs_call_args
            .iter()
            .map(ufcs_call_args_heap)
            .sum::<usize>()
        + vec_heap(&b.lambda_escapes)
        + b.lambda_escapes
            .iter()
            .map(lambda_escape_slot_heap)
            .sum::<usize>()
}

pub(crate) fn solve_scc_heap_size(value: &Arc<SolvedScc>) -> usize {
    let sig_heap: usize = value
        .signatures
        .values()
        .map(|sig| size_of::<(DefinitionId, InferredSig)>() + inferred_sig_heap(sig))
        .sum();
    let body_heap: usize = value
        .bodies
        .values()
        .map(|body| size_of::<(DefinitionId, BodyTypes)>() + body_types_heap(body))
        .sum();
    size_of::<SolvedScc>() + sig_heap + body_heap
}

// ─── `infer_body_query` — `Option<Arc<BodyTypes>>` ──────────────────────

#[expect(
    clippy::ref_option,
    reason = "salsa's heap_size fn contract is `fn(&Self::Output<'_>) -> usize` — \
              `Self::Output` for infer_body_query is `Option<Arc<BodyTypes>>`, so \
              the parameter type is fixed by the macro, not a style choice"
)]
pub(crate) fn infer_body_heap_size(value: &Option<Arc<BodyTypes>>) -> usize {
    value
        .as_ref()
        .map_or(0, |body| size_of::<BodyTypes>() + body_types_heap(body))
}

// ─── `lowered_query` — `LoweredFile` (not Arc-wrapped, `returns(ref)`) ──

fn var_decl_heap(v: &VarDecl) -> usize {
    name_heap(&v.name) + opt_type_expr_heap(v.annotation.as_ref())
}

fn knot_heap(k: &Knot) -> usize {
    name_heap(&k.name)
        + vec_heap(&k.params)
        + k.params.iter().map(param_heap).sum::<usize>()
        + block_heap(&k.body)
        + vec_heap(&k.stitches)
        + k.stitches
            .iter()
            .map(|s| {
                name_heap(&s.name)
                    + vec_heap(&s.params)
                    + s.params.iter().map(param_heap).sum::<usize>()
                    + block_heap(&s.body)
                    + opt_type_expr_heap(s.return_type.as_ref())
            })
            .sum::<usize>()
        + opt_type_expr_heap(k.return_type.as_ref())
}

/// Shallow byte count for a `Vec<DeclaredSymbol>`-shaped manifest field:
/// element count times the struct's own stack shape, no recursion into
/// each `DeclaredSymbol`'s owned `String`s. The manifest and diagnostics
/// tail is real but secondary weight next to knot bodies (per #537's
/// data, per-def/per-file HIR payloads dominate) — a deliberately coarser
/// tier than the block/content walk above.
fn declared_symbols_heap(v: &[DeclaredSymbol]) -> usize {
    vec_heap(v)
}

fn manifest_heap(m: &brink_ir::SymbolManifest) -> usize {
    declared_symbols_heap(&m.knots)
        + declared_symbols_heap(&m.stitches)
        + declared_symbols_heap(&m.variables)
        + declared_symbols_heap(&m.constants)
        + declared_symbols_heap(&m.lists)
        + declared_symbols_heap(&m.structs)
        + declared_symbols_heap(&m.externals)
        + declared_symbols_heap(&m.labels)
        + declared_symbols_heap(&m.list_items)
        + vec_heap(&m.locals)
        + vec_heap(&m.unresolved)
        + m.docs.len() * size_of::<(brink_ir::SymbolKind, String)>()
}

fn diagnostics_heap(diags: &[Diagnostic]) -> usize {
    vec_heap(diags)
}

fn hir_file_heap(hir: &HirFile) -> usize {
    block_heap(&hir.root_content)
        + vec_heap(&hir.knots)
        + hir.knots.iter().map(knot_heap).sum::<usize>()
        + vec_heap(&hir.variables)
        + hir.variables.iter().map(var_decl_heap).sum::<usize>()
        + vec_heap(&hir.constants)
        + vec_heap(&hir.lists)
        + vec_heap(&hir.structs)
        + vec_heap(&hir.externals)
        + vec_heap(&hir.includes)
        + vec_heap(&hir.imports)
        + vec_heap(&hir.visibility)
        + vec_heap(&hir.was_directives)
        + vec_heap(&hir.allow_scopes)
        + hir
            .allow_scopes
            .iter()
            .map(|s| vec_heap(&s.codes))
            .sum::<usize>()
        // Natural-notation element matches (issue #1838): the vec itself
        // plus each record's own owned strings — the handler's name and
        // every capture's name/text, none of which the flat `vec_heap`
        // above can see.
        + vec_heap(&hir.element_matches)
        + hir
            .element_matches
            .iter()
            .map(|m| {
                m.handler.text.capacity()
                    + vec_heap(&m.captures)
                    + m.captures
                        .iter()
                        .map(|c| c.name.capacity() + c.text.capacity())
                        .sum::<usize>()
            })
            .sum::<usize>()
        // Declared claiming handlers (issue #1844, extended by #1863's
        // `params`/`pattern` fields): the vec itself plus each record's
        // own owned strings — the handler's name, its pattern source, and
        // every parameter name (`annotation` is a `Copy` `TextRange`).
        + vec_heap(&hir.claim_handlers)
        + hir
            .claim_handlers
            .iter()
            .map(|h| {
                h.name.text.capacity()
                    + h.pattern.capacity()
                    + vec_heap(&h.params)
                    + h.params.iter().map(String::capacity).sum::<usize>()
            })
            .sum::<usize>()
}

/// `value`'s type follows `raw_lowered_query`/`lowered_query`'s own Output
/// type (issue #2289 review finding: both now return `Arc<LoweredFile>`,
/// not `LoweredFile`, so this estimator's own signature must match) —
/// deref-transparent, so the body below is unchanged from before that
/// change.
pub(crate) fn lowered_file_heap_size(value: &Arc<LoweredFile>) -> usize {
    hir_file_heap(&value.hir)
        + manifest_heap(&value.manifest)
        + diagnostics_heap(&value.diagnostics)
        + diagnostics_heap(&value.admission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_heap_size_is_zero_for_none() {
        assert_eq!(signature_heap_size(&None), 0);
    }

    #[test]
    fn signature_heap_size_grows_with_name_length() {
        let base = Sig {
            name: "a".to_string(),
            kind: brink_ir::SymbolKind::Knot,
            params: Vec::new(),
            value_type: None,
            value_ty: None,
            is_local: false,
            param_annotations: Vec::new(),
            return_annotation: None,
        };
        let mut long = base.clone();
        long.name = "a".repeat(1000);

        let small = signature_heap_size(&Some(Arc::new(base)));
        let big = signature_heap_size(&Some(Arc::new(long)));
        assert!(
            big > small + 900,
            "expected the 1000-byte name to dominate: small={small} big={big}"
        );
    }

    #[test]
    fn signature_heap_size_grows_with_param_annotations() {
        let mut sig = Sig {
            name: String::new(),
            kind: brink_ir::SymbolKind::Knot,
            params: Vec::new(),
            value_type: None,
            value_ty: None,
            is_local: false,
            param_annotations: Vec::new(),
            return_annotation: None,
        };
        let bare = signature_heap_size(&Some(Arc::new(sig.clone())));

        sig.param_annotations = vec![Some(Ty::Handle("VeryLongHandleKindName".to_string()))];
        let annotated = signature_heap_size(&Some(Arc::new(sig)));
        assert!(annotated > bare);
    }

    #[test]
    fn ty_heap_zero_for_leaf_variants() {
        assert_eq!(ty_heap(&Ty::Int), 0);
        assert_eq!(ty_heap(&Ty::Unknown), 0);
        assert_eq!(ty_heap(&Ty::Conflicted), 0);
    }

    #[test]
    fn ty_heap_nonzero_for_nominal_and_nested_variants() {
        assert!(ty_heap(&Ty::List("Inventory".to_string())) > 0);
        assert!(ty_heap(&Ty::Array(Box::new(Ty::Int))) > 0);
        assert!(
            ty_heap(&Ty::Fn(
                vec![Ty::Int, Ty::String],
                Box::new(Ty::Bool),
                brink_analyzer::FnRow::unknown()
            )) > ty_heap(&Ty::Fn(
                vec![],
                Box::new(Ty::Bool),
                brink_analyzer::FnRow::unknown()
            ))
        );
    }

    #[test]
    fn ty_heap_accounts_for_a_concrete_fn_row() {
        // `fn_row_heap` (issue #1680) must actually contribute: a `Ty::Fn`
        // carrying a concrete creation-target row estimates strictly larger
        // than the same shape carrying the unknown top element, which is a
        // niche-optimized `None` and costs nothing. Deleting the
        // `+ fn_row_heap(row)` term in `ty_heap` must turn this red.
        let unknown = Ty::Fn(
            vec![Ty::Int],
            Box::new(Ty::Bool),
            brink_analyzer::FnRow::unknown(),
        );
        let traced = Ty::Fn(
            vec![Ty::Int],
            Box::new(Ty::Bool),
            brink_analyzer::FnRow::of_target(DefinitionId::new(
                brink_format::DefinitionTag::Address,
                1,
            )),
        );
        assert!(ty_heap(&traced) > ty_heap(&unknown));
    }

    #[test]
    fn def_body_heap_size_is_zero_for_none() {
        assert_eq!(def_body_heap_size(&None), 0);
    }

    #[test]
    fn def_body_heap_size_grows_with_body_statement_count() {
        let file = brink_ir::FileId(0);
        let small = DefBody {
            file,
            params: Vec::new(),
            return_annotation: None,
            body: Block::default(),
            native: false,
        };
        let mut big_stmts = Vec::new();
        for _ in 0..50 {
            big_stmts.push(Stmt::EndOfLine);
        }
        let big = DefBody {
            file,
            params: Vec::new(),
            return_annotation: None,
            body: Block::from_stmts(big_stmts),
            native: false,
        };

        let small_size = def_body_heap_size(&Some(Arc::new(small)));
        let big_size = def_body_heap_size(&Some(Arc::new(big)));
        assert!(big_size > small_size);
    }

    #[test]
    fn solve_scc_heap_size_grows_with_member_count() {
        let empty = solve_scc_heap_size(&Arc::new(SolvedScc::default()));

        let mut scc = SolvedScc::default();
        scc.bodies.insert(
            DefinitionId::new(brink_format::DefinitionTag::Address, 1),
            BodyTypes {
                params: vec![("x".to_string(), Ty::Int)],
                locals: std::collections::BTreeMap::new(),
                return_ty: Ty::Bool,
                has_value_return: true,
                value_calls: Vec::new(),
                direct_call_arg_mismatches: Vec::new(),
                typed_assign_mismatches: Vec::new(),
                field_assign_mismatches: Vec::new(),
                lambda_annotation_mismatches: Vec::new(),
                ufcs_call_args: Vec::new(),
                array_remove_calls: Vec::new(),
                lambda_escapes: Vec::new(),
            },
        );

        let populated = solve_scc_heap_size(&Arc::new(scc));
        assert!(populated > empty);
    }

    #[test]
    fn infer_body_heap_size_is_zero_for_none() {
        assert_eq!(infer_body_heap_size(&None), 0);
    }

    #[test]
    fn infer_body_heap_size_grows_with_locals() {
        let empty = infer_body_heap_size(&Some(Arc::new(BodyTypes::default())));

        let mut populated = BodyTypes::default();
        populated
            .locals
            .insert("very_long_local_variable_name".to_string(), Ty::Int);

        let populated_size = infer_body_heap_size(&Some(Arc::new(populated)));
        assert!(populated_size > empty);
    }

    /// Issue #1864: `direct_call_arg_mismatches` is a consumer
    /// `body_types_heap` must walk too — same growth-proof shape as
    /// [`infer_body_heap_size_grows_with_locals`], guarding against the
    /// accumulator silently under-reporting a populated `Vec` the way
    /// house rule 20b warns a structurally-ignoring consumer can.
    #[test]
    fn infer_body_heap_size_grows_with_direct_call_arg_mismatches() {
        let empty = infer_body_heap_size(&Some(Arc::new(BodyTypes::default())));

        let mut populated = BodyTypes::default();
        populated
            .direct_call_arg_mismatches
            .push(DirectCallArgMismatch {
                range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
                callee: "very_long_callee_name".to_string(),
                index: 0,
                expected: Ty::Int,
                found: Ty::String,
            });

        let populated_size = infer_body_heap_size(&Some(Arc::new(populated)));
        assert!(populated_size > empty);
    }

    /// Issue #1877: `typed_assign_mismatches` is a consumer
    /// `body_types_heap` must walk too — same growth-proof shape as
    /// [`infer_body_heap_size_grows_with_direct_call_arg_mismatches`].
    #[test]
    fn infer_body_heap_size_grows_with_typed_assign_mismatches() {
        let empty = infer_body_heap_size(&Some(Arc::new(BodyTypes::default())));

        let mut populated = BodyTypes::default();
        populated.typed_assign_mismatches.push(TypedAssignMismatch {
            range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
            target: "very_long_target_name".to_string(),
            expected: Ty::Int,
            found: Ty::String,
        });

        let populated_size = infer_body_heap_size(&Some(Arc::new(populated)));
        assert!(populated_size > empty);
    }

    /// Issue #1994: `lambda_annotation_mismatches` is a consumer
    /// `body_types_heap` must walk too — same growth-proof shape as
    /// [`infer_body_heap_size_grows_with_direct_call_arg_mismatches`], the
    /// house rule 20b guard against a structurally-ignoring accumulator.
    #[test]
    fn infer_body_heap_size_grows_with_lambda_annotation_mismatches() {
        let empty = infer_body_heap_size(&Some(Arc::new(BodyTypes::default())));

        let mut populated = BodyTypes::default();
        populated
            .lambda_annotation_mismatches
            .push(LambdaAnnotationMismatch {
                range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
                param_name: Some("very_long_param_name".to_string()),
                expected: Ty::Int,
                found: Ty::String,
            });

        let populated_size = infer_body_heap_size(&Some(Arc::new(populated)));
        assert!(populated_size > empty);
    }

    /// Issue #1881: `ufcs_call_args` is a consumer `body_types_heap` must
    /// walk too — same growth-proof shape as
    /// [`infer_body_heap_size_grows_with_direct_call_arg_mismatches`].
    #[test]
    fn infer_body_heap_size_grows_with_ufcs_call_args() {
        let empty = infer_body_heap_size(&Some(Arc::new(BodyTypes::default())));

        let mut populated = BodyTypes::default();
        populated.ufcs_call_args.push(UfcsCallArgs {
            range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
            args: vec![Ty::Int, Ty::String],
        });

        let populated_size = infer_body_heap_size(&Some(Arc::new(populated)));
        assert!(populated_size > empty);
    }

    /// Issue #1770: `lambda_escapes` is a consumer `body_types_heap` must
    /// walk too — same growth-proof shape as
    /// [`infer_body_heap_size_grows_with_lambda_annotation_mismatches`], the
    /// house rule 20b guard against a structurally-ignoring accumulator.
    #[test]
    fn infer_body_heap_size_grows_with_lambda_escapes() {
        let empty = infer_body_heap_size(&Some(Arc::new(BodyTypes::default())));

        let mut populated = BodyTypes::default();
        populated.lambda_escapes.push(LambdaEscapeSlot {
            range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
            ty: Ty::Unknown,
            annotated: false,
            slot_label: "lambda parameter `very_long_param_name`".to_string(),
        });

        let populated_size = infer_body_heap_size(&Some(Arc::new(populated)));
        assert!(populated_size > empty);
    }

    /// Uses the real parse -> lower pipeline (same `lower_file` helper
    /// [`super::lower_file`] `lowered_query` calls) rather than
    /// hand-built `HirFile`/`Knot` values — every HIR node carries a
    /// `Provenance` with no meaningful `Default`, so a genuine
    /// small-vs-large source pair is both easier to build and closer to
    /// "known payloads" than a synthetic struct literal.
    #[test]
    fn lowered_file_heap_size_grows_with_story_length() {
        let small_src = "== knot\nHi.\n-> END\n";
        let big_src = format!(
            "== knot\n{}-> END\n",
            "A repeated line of dialogue content for size comparison.\n".repeat(50)
        );

        let small_parse = brink_syntax::parse(small_src);
        let big_parse = brink_syntax::parse(&big_src);
        let small_file = Arc::new(lower_file(brink_ir::FileId(0), &small_parse));
        let big_file = Arc::new(lower_file(brink_ir::FileId(0), &big_parse));

        let small_size = lowered_file_heap_size(&small_file);
        let big_size = lowered_file_heap_size(&big_file);
        assert!(
            big_size > small_size,
            "expected the 50x-longer story to report more heap: small={small_size} big={big_size}"
        );
    }

    fn dummy_provenance() -> Provenance {
        Provenance::synthetic(
            NodeClass::For,
            rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
        )
    }

    fn dummy_name(text: &str) -> Name {
        Name {
            text: text.to_string(),
            range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(1)),
        }
    }

    /// Regression for the native `@[allow(Exxx, …)]` suppression-scope HIR
    /// field (`HirFile::allow_scopes`, issue #1161): `hir_file_heap` must
    /// count it — same precedent as `was_directives` (a `Vec<TextRange>`
    /// added by an earlier annotation PR) that this field sits right next
    /// to. Uses the real native lower (`hir::lower_native::lower`) rather
    /// than a hand-built `HirFile`, since `allow_scopes` is only ever
    /// populated by that frontend.
    #[test]
    fn hir_file_heap_counts_allow_scopes() {
        let empty_src = "var gold = 0\n";
        let scoped_src = "@[allow(E014)]\nvar gold = 0\n";

        let empty_parse = brink_syntax_native::parse(empty_src);
        let scoped_parse = brink_syntax_native::parse(scoped_src);
        assert!(empty_parse.errors().is_empty());
        assert!(scoped_parse.errors().is_empty());

        let (empty_hir, _, empty_diags) =
            brink_ir::hir::lower_native::lower(brink_ir::FileId(0), &empty_parse.tree());
        let (scoped_hir, _, scoped_diags) =
            brink_ir::hir::lower_native::lower(brink_ir::FileId(0), &scoped_parse.tree());
        assert!(empty_diags.is_empty());
        assert!(scoped_diags.is_empty());
        assert!(empty_hir.allow_scopes.is_empty());
        assert_eq!(scoped_hir.allow_scopes.len(), 1);

        let empty_size = hir_file_heap(&empty_hir);
        let scoped_size = hir_file_heap(&scoped_hir);
        assert!(
            scoped_size > empty_size,
            "expected the allow scope's codes Vec to grow the heap estimate: \
             empty={empty_size} scoped={scoped_size}"
        );
    }

    /// Regression for the two-binding `for k, v in m` HIR field
    /// (`ForStmt.val_name`, #1461): `block_stmt_heap` must count it, or the
    /// heap estimate silently undercounts every two-binding loop.
    #[test]
    fn block_stmt_heap_counts_for_val_name() {
        let single_binding = BlockStmt::For(ForStmt {
            ptr: dummy_provenance(),
            var_name: dummy_name("k"),
            val_name: None,
            iterable: Expr::Int(0),
            body: Vec::new(),
        });
        let two_binding = BlockStmt::For(ForStmt {
            ptr: dummy_provenance(),
            var_name: dummy_name("k"),
            val_name: Some(dummy_name("very_long_value_binding_name")),
            iterable: Expr::Int(0),
            body: Vec::new(),
        });

        let single_size = block_stmt_heap(&single_binding);
        let two_size = block_stmt_heap(&two_binding);
        assert!(
            two_size > single_size,
            "expected val_name to grow the heap estimate: single={single_size} two={two_size}"
        );
    }
}
