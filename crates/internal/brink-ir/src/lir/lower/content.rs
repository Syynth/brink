use brink_format::CountingFlags;

use crate::hir;

use super::context::LowerCtx;
use super::expr::lower_expr;
use super::lir;

/// Lower HIR Content to LIR Content.
pub fn lower_content(content: &hir::Content, ctx: &mut LowerCtx<'_>) -> lir::Content {
    lir::Content {
        parts: lower_content_parts(&content.parts, ctx),
        tags: content
            .tags
            .iter()
            .map(|t| lower_content_parts(&t.parts, ctx))
            .collect(),
    }
}

/// Lower HIR content parts to LIR content parts (public for use by choice tag lowering).
pub fn lower_content_parts_pub(
    parts: &[hir::ContentPart],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::ContentPart> {
    lower_content_parts(parts, ctx)
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
        hir::ContentPart::InlineSequence(seq) => lower_inline_sequence(seq, ctx),
    }
}

/// Lower an inline sequence into a wrapper container and return `EnterSequence`.
fn lower_inline_sequence(seq: &hir::Sequence, ctx: &mut LowerCtx<'_>) -> lir::ContentPart {
    // Count existing pending children to derive a unique sequence index.
    let seq_idx = ctx
        .pending_children
        .iter()
        .filter(|c| c.kind == lir::ContainerKind::Sequence)
        .count();
    let wrapper_id = ctx.alloc_sequence_id(seq_idx);

    let branches = seq
        .branches
        .iter()
        .map(|b| lower_inline_block(b, ctx))
        .collect();

    let display_name = format!("s-{seq_idx}");
    let wrapper = lir::Container {
        id: wrapper_id,
        name: Some(display_name),
        kind: lir::ContainerKind::Sequence,
        params: Vec::new(),
        body: vec![lir::Stmt::Sequence(lir::Sequence {
            kind: seq.kind,
            branches,
        })],
        children: Vec::new(),
        counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
    };
    ctx.pending_children.push(wrapper);

    lir::ContentPart::EnterSequence(wrapper_id)
}

/// Lower a block in inline content context (no choice/gather children possible).
fn lower_inline_block(block: &hir::Block, ctx: &mut LowerCtx<'_>) -> Vec<lir::Stmt> {
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        if let Some(s) = super::stmts::lower_stmt(stmt, ctx) {
            stmts.push(s);
        }
    }
    stmts
}
