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

/// Lower a single content part (public for use by stmts.rs content combining).
pub fn lower_content_part_pub(part: &hir::ContentPart, ctx: &mut LowerCtx<'_>) -> lir::ContentPart {
    lower_content_part(part, ctx)
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
                    let mut bc = 0;
                    let mut bg = 0;
                    let body = super::stmts::lower_block(
                        &b.body,
                        ctx,
                        &super::plan::ContainerPlan::empty(),
                        &mut bc,
                        &mut bg,
                    );
                    lir::CondBranch { condition, body }
                })
                .collect();
            lir::ContentPart::InlineConditional(lir::Conditional { branches })
        }
        hir::ContentPart::InlineSequence(seq) => {
            let branches = seq
                .branches
                .iter()
                .map(|b| {
                    let mut bc = 0;
                    let mut bg = 0;
                    super::stmts::lower_block(
                        b,
                        ctx,
                        &super::plan::ContainerPlan::empty(),
                        &mut bc,
                        &mut bg,
                    )
                })
                .collect();
            lir::ContentPart::InlineSequence(lir::Sequence {
                kind: seq.kind,
                branches,
            })
        }
    }
}
