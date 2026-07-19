//! F27: condition-position `Option[T]` has no truthiness — E116
//! (`docs/stdlib-spec.md` §1.6, ruled 2026-07-19, issue #1120).
//!
//! The ruling: Option has **no** truthiness. A condition-position
//! `Option[T]` (an `if`/`while` condition, a `{cond: …}` conditional
//! branch, a choice guard, an `await` condition) is a compile error under
//! `types = strict` and a runtime fault under gradual
//! (`RuntimeError::OptionTruthiness`). Authors write `== none` /
//! `== some(x)` (or, post-B1, the `as`-binding). This supersedes NS-A1's
//! shipped falsy-none truthiness.
//!
//! Strict-mode-only, mirroring `conversions::check`'s gating and
//! classification posture exactly: wired into `strict::check`, judging only
//! **statically classifiable** conditions through the same inference
//! substrate (`structs::classify_expr_ty` — a param/temp's finalized
//! `BodyTypes::locals`, a global's declaration-derived type, a resolved
//! callee's `InferredSig::return_ty`, an index into a known collection),
//! plus the two condition shapes that classification can't see but the
//! Option package owns outright: a direct call to an unresolved
//! Option-returning stdlib intrinsic (`{find(s, "x"): …}` — membership from
//! [`crate::infer::intrinsic_returns_option`], the same table
//! `infer::body::InferPass::infer_intrinsic`'s typing arms implement) and
//! the bare unresolved `none` literal. Whenever the resolved type is
//! `Unknown`/`Conflicted` or the shape isn't handled, the condition stays
//! silently unchecked — "Unknown never disagrees" — and the runtime fault
//! remains the backstop that still catches every case at execution time.
//!
//! `{expr: - val: …}` switch *case* values are compared with `==`, not
//! evaluated for truthiness — neither the scrutinee nor the case values are
//! condition positions, so a switch is only recursed into for nested bodies.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart, Diagnostic,
    DiagnosticCode, ElseBranch, Expr, FileId, HirFile, IfStmt, PrefixOp, ResolutionMap, Stmt,
    SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, Ty};
use crate::structs::{self, MistypeCtx};

/// Strict-mode-only condition-position Option checks over every truthiness
/// condition in the project. Callers only reach this once
/// `strict::config_error` has confirmed `types = strict` + `dialect =
/// brink` (mirrors `conversions::check`'s entry condition).
#[must_use]
pub(crate) fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    // No manifest access, mirroring `conversions::check`'s own note — an
    // Option type never originates from a `handle<K>` manifest vocabulary.
    let globals = crate::infer::collect_globals(files, index, None);
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        for knot in &hir.knots {
            let kind = match knot.ptr {
                brink_ir::ContainerPtr::Knot(_) => SymbolKind::Knot,
                brink_ir::ContainerPtr::Stitch(_) => SymbolKind::Stitch,
            };
            let knot_locals = annotations::def_id_for(index, file, kind, &knot.name.text)
                .and_then(|id| inference.bodies.get(&id))
                .map(|b| &b.locals);
            let ctx = MistypeCtx {
                index,
                globals: &globals,
                signatures: &inference.signatures,
                resolution_by_range: &resolution_by_range,
                locals: knot_locals,
            };
            check_block(&knot.body, file, &ctx, &mut out);
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                let stitch_locals =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                        .and_then(|id| inference.bodies.get(&id))
                        .map(|b| &b.locals);
                let ctx = MistypeCtx {
                    index,
                    globals: &globals,
                    signatures: &inference.signatures,
                    resolution_by_range: &resolution_by_range,
                    locals: stitch_locals.or(knot_locals),
                };
                check_block(&stitch.body, file, &ctx, &mut out);
            }
        }
    }
    out
}

fn check_block(block: &Block, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    for stmt in &block.stmts {
        check_stmt(stmt, file, ctx, out);
    }
}

fn check_stmt(stmt: &Stmt, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    match stmt {
        Stmt::Content(c) => check_content(c, file, ctx, out),
        Stmt::Conditional(c) => check_conditional(c, file, ctx, out),
        Stmt::ChoiceSet(cs) => check_choice_set(cs, file, ctx, out),
        Stmt::LabeledBlock(b) => check_block(b, file, ctx, out),
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                check_block(branch, file, ctx, out);
            }
        }
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                check_block_stmt(bs, file, ctx, out);
            }
        }
        // `~ await <cond>`: the runtime re-evaluates the condition for
        // truthiness to decide when to wake — condition position.
        Stmt::Await(a) => {
            if let Some(cond) = &a.condition {
                check_condition(cond, a.ptr.text_range(), file, ctx, out);
            }
        }
        Stmt::Divert(_)
        | Stmt::TunnelCall(_)
        | Stmt::ThreadStart(_)
        | Stmt::TempDecl(_)
        | Stmt::Assignment(_)
        | Stmt::Return(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine => {}
    }
}

fn check_content(c: &Content, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    for part in &c.parts {
        match part {
            ContentPart::InlineConditional(cond) => check_conditional(cond, file, ctx, out),
            ContentPart::InlineSequence(s) => {
                for branch in &s.branches {
                    check_block(branch, file, ctx, out);
                }
            }
            ContentPart::Interpolation(_)
            | ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring => {}
        }
    }
}

fn check_conditional(
    c: &Conditional,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    // A switch's case values are `==`-compared against the scrutinee, never
    // truthiness-evaluated (see the module doc) — only branch *bodies* are
    // recursed for a `CondKind::Switch`.
    let conditions_are_truthiness = !matches!(c.kind, CondKind::Switch(_));
    for branch in &c.branches {
        if conditions_are_truthiness && let Some(cond) = &branch.condition {
            check_condition(cond, c.ptr.text_range(), file, ctx, out);
        }
        check_block(&branch.body, file, ctx, out);
    }
}

fn check_choice_set(cs: &ChoiceSet, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    for choice in &cs.choices {
        check_choice(choice, file, ctx, out);
    }
    check_block(&cs.continuation, file, ctx, out);
}

fn check_choice(choice: &Choice, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    if let Some(cond) = &choice.condition {
        check_condition(cond, choice.ptr.text_range(), file, ctx, out);
    }
    if let Some(c) = &choice.start_content {
        check_content(c, file, ctx, out);
    }
    if let Some(c) = &choice.bracket_content {
        check_content(c, file, ctx, out);
    }
    if let Some(c) = &choice.inner_content {
        check_content(c, file, ctx, out);
    }
    check_block(&choice.body, file, ctx, out);
}

fn check_block_stmt(bs: &BlockStmt, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    match bs {
        BlockStmt::If(i) => check_if(i, file, ctx, out),
        BlockStmt::While(w) => {
            // A plain `while` and a `while await` both truthiness-evaluate
            // their condition (the wake contract: waking IS condition-true).
            check_condition(&w.condition, w.ptr.text_range(), file, ctx, out);
            for s in &w.body {
                check_block_stmt(s, file, ctx, out);
            }
        }
        BlockStmt::For(f) => {
            for s in &f.body {
                check_block_stmt(s, file, ctx, out);
            }
        }
        BlockStmt::Await(a) => {
            if let Some(cond) = &a.condition {
                check_condition(cond, a.ptr.text_range(), file, ctx, out);
            }
        }
        BlockStmt::TempDecl(_)
        | BlockStmt::Assignment(_)
        | BlockStmt::Return(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_) => {}
    }
}

fn check_if(i: &IfStmt, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    check_condition(&i.condition, i.ptr.text_range(), file, ctx, out);
    for s in &i.body {
        check_block_stmt(s, file, ctx, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => check_if(inner, file, ctx, out),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                check_block_stmt(s, file, ctx, out);
            }
        }
        None => {}
    }
}

/// Check one truthiness condition. `fallback_range` anchors the diagnostic
/// when the condition expression carries no own range (most `Expr` shapes
/// don't) — the enclosing construct's span, same posture as
/// `await_purity`'s E105 anchor.
///
/// `not <cond>` recurses: the VM's `Not` opcode truthiness-evaluates its
/// operand through the same `is_truthy` path, so `{not r: …}` over an
/// Option `r` is the identical fault shape.
fn check_condition(
    cond: &Expr,
    fallback_range: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    if let Expr::Prefix(PrefixOp::Not, inner) = cond {
        check_condition(inner, fallback_range, file, ctx, out);
        return;
    }
    if !condition_is_option(cond, ctx) {
        return;
    }
    out.push(Diagnostic {
        file,
        range: expr_anchor(cond).unwrap_or(fallback_range),
        message: format!(
            "{}: an `Option[T]` has no truthiness (F27, docs/stdlib-spec.md §1.6) — test \
             `== none` / `== some(x)` explicitly",
            DiagnosticCode::E116.title(),
        ),
        code: DiagnosticCode::E116,
    });
}

/// Whether `cond`'s type is statically known to be `Option[T]` — the
/// inference-substrate classification first, then the two shapes it can't
/// see (see the module doc): an unresolved (builtin, not author-shadowed)
/// call to an Option-returning intrinsic, and the bare unresolved `none`
/// literal.
fn condition_is_option(cond: &Expr, ctx: &MistypeCtx<'_>) -> bool {
    match cond {
        Expr::Call(path, _) => {
            if let [seg] = path.segments.as_slice()
                && !ctx.resolution_by_range.contains_key(&range_key(path.range))
            {
                return crate::infer::intrinsic_returns_option(&seg.text);
            }
            matches!(structs::classify_expr_ty(cond, ctx), Some(Ty::Option(_)))
        }
        Expr::Path(p) => {
            if let [seg] = p.segments.as_slice()
                && seg.text == "none"
                && !ctx.resolution_by_range.contains_key(&range_key(p.range))
            {
                return true;
            }
            matches!(structs::classify_expr_ty(cond, ctx), Some(Ty::Option(_)))
        }
        _ => matches!(structs::classify_expr_ty(cond, ctx), Some(Ty::Option(_))),
    }
}

/// A best-effort own-range for a condition expression, for diagnostic
/// anchoring — the shapes that carry a source range (a path, a call's
/// callee path, and the roots reachable through unary/index/field
/// wrappers). `None` falls back to the enclosing construct's span.
fn expr_anchor(expr: &Expr) -> Option<TextRange> {
    match expr {
        Expr::Path(p) => Some(p.range),
        Expr::Call(path, _) => Some(path.range),
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => expr_anchor(inner),
        Expr::Index(idx) => expr_anchor(&idx.base),
        Expr::FieldAccess(fa) => expr_anchor(&fa.base),
        _ => None,
    }
}

fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `conversions::resolution_index`.
fn resolution_index(
    resolutions: &ResolutionMap,
    file: FileId,
) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| (range_key(r.range), r.target))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;

    /// Real resolutions + a whole-project [`InferenceResult`] — mirrors
    /// `conversions::tests::build_with_inference`.
    fn check_all(src: &str) -> Vec<Diagnostic> {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    #[test]
    fn option_temp_in_inline_conditional_guard_is_e116() {
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r: found.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn direct_option_intrinsic_call_in_condition_is_e116() {
        let diags = check_all("=== main ===\n{find(\"ab\", \"b\"): found.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn option_temp_in_choice_guard_is_e116() {
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n* {r} [go] Went.\n- -> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn option_temp_in_block_if_condition_is_e116() {
        let diags = check_all(
            "=== main ===\n~ {\n    temp r = find(\"ab\", \"b\")\n    if r {\n        \
             return\n    }\n}\nHi.\n-> END\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn negated_option_condition_is_e116() {
        // `not r` truthiness-evaluates `r` through the same VM path.
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n{not r: absent.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn explicit_none_comparison_is_clean() {
        let diags = check_all(
            "=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r == none: absent.}\n\
             {r == some(1): at one.}\n-> END\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_truthiness_idiom_stays_clean() {
        // The `{visited_knot: …}` idiom survives — F27 bans Option only.
        let diags = check_all("=== main ===\n~ temp n = 3\n{n: nonzero.}\n-> END\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unknown_typed_condition_stays_silent() {
        // An unclassifiable condition (an unused param) never flags —
        // "Unknown never disagrees"; the runtime fault is the backstop.
        let diags = check_all("=== main(r) ===\n{r: yes.}\n-> END\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn switch_case_values_are_not_condition_positions() {
        // `{n: - 1: one - else: other}` case values are `==`-compared, not
        // truthiness-evaluated — never flagged, even with Option around.
        let diags = check_all(
            "=== main ===\n~ temp n = 2\n{n:\n- 1: one\n- 2: two\n- else: other\n}\n-> END\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn option_returning_user_function_call_in_condition_is_e116() {
        let diags = check_all(
            "=== function probe() ===\n~ return find(\"ab\", \"b\")\n\
             === main ===\n{probe(): found.}\n-> END\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn await_condition_of_option_type_is_e116() {
        let diags = check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n~ await r\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }
}
