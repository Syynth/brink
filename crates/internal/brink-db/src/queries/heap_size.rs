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

use brink_analyzer::{BodyTypes, InferredSig, Sig, Ty, ValueCallFact};
use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Choice, ChoiceSet, CondBranch, Conditional, Content, ContentPart,
    DeclaredSymbol, Diagnostic, DivertPath, DivertTarget, ElseBranch, HirFile, IfStmt, Knot, Name,
    Param, ParamInfo, Path, Sequence, Stmt, Tag, TempDecl, TypeExpr, VarDecl,
};

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
        | Ty::Divert
        | Ty::Range { .. }
        | Ty::Unknown
        | Ty::Conflicted => 0,
        Ty::List(name) | Ty::Struct(name) | Ty::Handle(name) => string_heap(name),
        Ty::Array(inner) | Ty::Option(inner) => size_of::<Ty>() + ty_heap(inner),
        Ty::Map(key, value) => size_of::<Ty>() * 2 + ty_heap(key) + ty_heap(value),
        Ty::Fn(params, ret) => {
            vec_heap(params)
                + params.iter().map(ty_heap).sum::<usize>()
                + size_of::<Ty>()
                + ty_heap(ret)
        }
    }
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
            })
            .sum::<usize>()
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
    block_heap(&b.body)
}

fn sequence_heap(s: &Sequence) -> usize {
    vec_heap(&s.branches) + s.branches.iter().map(block_heap).sum::<usize>()
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
        Stmt::Assignment(_) | Stmt::ExprStmt(_) | Stmt::Await(_) | Stmt::EndOfLine => 0,
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
        BlockStmt::While(w) => block_stmts_heap(&w.body),
        BlockStmt::For(f) => name_heap(&f.var_name) + block_stmts_heap(&f.body),
        BlockStmt::Assignment(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Await(_) => 0,
    }
}

fn if_stmt_heap(i: &IfStmt) -> usize {
    block_stmts_heap(&i.body) + i.else_branch.as_ref().map_or(0, else_branch_heap)
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
        + sig.fn_type.as_ref().map_or(0, ty_heap)
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
}

pub(crate) fn lowered_file_heap_size(value: &LoweredFile) -> usize {
    hir_file_heap(&value.hir)
        + manifest_heap(&value.manifest)
        + diagnostics_heap(&value.diagnostics)
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
            fn_type: None,
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
            fn_type: None,
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
            ty_heap(&Ty::Fn(vec![Ty::Int, Ty::String], Box::new(Ty::Bool)))
                > ty_heap(&Ty::Fn(vec![], Box::new(Ty::Bool)))
        );
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
        };
        let mut big_stmts = Vec::new();
        for _ in 0..50 {
            big_stmts.push(Stmt::EndOfLine);
        }
        let big = DefBody {
            file,
            params: Vec::new(),
            return_annotation: None,
            body: Block {
                label: None,
                stmts: big_stmts,
                container_id: None,
            },
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

    /// Uses the real parse -> lower pipeline (same `lower_file` helper
    /// [`super::lower_file`] `lowered_query` calls) rather than
    /// hand-built `HirFile`/`Knot` values — several HIR node types (e.g.
    /// `ContainerPtr`) carry real `AstPtr`s with no meaningful `Default`,
    /// so a genuine small-vs-large source pair is both easier to build and
    /// closer to "known payloads" than a synthetic struct literal.
    #[test]
    fn lowered_file_heap_size_grows_with_story_length() {
        let small_src = "== knot\nHi.\n-> END\n";
        let big_src = format!(
            "== knot\n{}-> END\n",
            "A repeated line of dialogue content for size comparison.\n".repeat(50)
        );

        let small_parse = brink_syntax::parse(small_src);
        let big_parse = brink_syntax::parse(&big_src);
        let small_file = lower_file(brink_ir::FileId(0), &small_parse);
        let big_file = lower_file(brink_ir::FileId(0), &big_parse);

        let small_size = lowered_file_heap_size(&small_file);
        let big_size = lowered_file_heap_size(&big_file);
        assert!(
            big_size > small_size,
            "expected the 50x-longer story to report more heap: small={small_size} big={big_size}"
        );
    }
}
