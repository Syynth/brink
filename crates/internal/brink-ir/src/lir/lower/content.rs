use crate::hir;

use super::context::LowerCtx;
use super::expr::lower_expr;
use super::lir;

/// Lower HIR Content to LIR Content.
pub fn lower_content(content: &hir::Content, ctx: &mut LowerCtx<'_>) -> lir::Content {
    lir::Content {
        parts: lower_content_parts(&content.parts, ctx),
        tags: content.tags.iter().map(|t| t.text.clone()).collect(),
    }
}

fn lower_content_parts(
    parts: &[hir::ContentPart],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::ContentPart> {
    parts.iter().map(|p| lower_content_part(p, ctx)).collect()
}

fn lower_content_part(part: &hir::ContentPart, ctx: &mut LowerCtx<'_>) -> lir::ContentPart {
    match part {
        hir::ContentPart::Text(t) => lir::ContentPart::Text(t.clone()),
        hir::ContentPart::Glue => lir::ContentPart::Glue,
        hir::ContentPart::Interpolation(expr) => {
            lir::ContentPart::Interpolation(lower_expr(expr, ctx))
        }
        hir::ContentPart::InlineConditional(cond) => {
            let branches = cond
                .branches
                .iter()
                .map(|b| {
                    let condition = b.condition.as_ref().map(|e| lower_expr(e, ctx));
                    let body = lower_inline_block(&b.body, ctx);
                    lir::CondBranch { condition, body }
                })
                .collect();
            lir::ContentPart::InlineConditional(lir::Conditional {
                kind: lir::CondKind::InitialCondition,
                branches,
            })
        }
        hir::ContentPart::InlineSequence(seq) => {
            let branches = seq
                .branches
                .iter()
                .map(|b| lower_inline_block(b, ctx))
                .collect();
            lir::ContentPart::InlineSequence(lir::Sequence {
                kind: seq.kind,
                branches,
            })
        }
    }
}

/// Lower a block in inline content context (no choice/gather children possible).
fn lower_inline_block(block: &hir::Block, ctx: &mut LowerCtx<'_>) -> Vec<lir::Stmt> {
    let empty_plan = super::plan::ContainerPlan::empty();
    let mut cc = 0;
    let mut gc = 0;
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        if let Some(s) = super::stmts::lower_stmt(stmt, ctx, &empty_plan, &mut cc, &mut gc) {
            stmts.push(s);
        }
    }
    stmts
}
