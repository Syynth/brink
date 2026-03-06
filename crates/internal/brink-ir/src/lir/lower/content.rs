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
                .map(|b| lir::InlineBranch {
                    condition: b.condition.as_ref().map(|e| lower_expr(e, ctx)),
                    content: lower_content_parts(&b.content, ctx),
                })
                .collect();
            lir::ContentPart::InlineConditional(lir::InlineCond { branches })
        }
        hir::ContentPart::InlineSequence(seq) => {
            let branches = seq
                .branches
                .iter()
                .map(|parts| lower_content_parts(parts, ctx))
                .collect();
            lir::ContentPart::InlineSequence(lir::InlineSeq {
                kind: seq.kind,
                branches,
            })
        }
    }
}
