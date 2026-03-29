use brink_syntax::ast;

use crate::Stmt;

use super::super::context::{LowerScope, LowerSink};
use super::super::expr::LowerExpr;
use super::sequence::lower_block_sequence;
use super::{LowerConditional, LowerSequence};

// ─── Multiline block promotion ──────────────────────────────────────

/// Try to lower a `MultilineBlock` AST node into a statement.
pub fn lower_multiline_block(
    mb: &ast::MultilineBlock,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Option<Stmt> {
    if let Some(cond) = mb.conditional()
        && let Ok(c) = cond.lower_conditional(scope, sink)
    {
        return Some(Stmt::Conditional(c));
    }

    if let Some(seq) = mb.sequence()
        && seq.multiline_branches().is_some()
    {
        return Some(Stmt::Sequence(lower_block_sequence(&seq, scope, sink)));
    }

    if let Some(branches) = mb.branches_cond()
        && let Ok(c) = branches.lower_conditional(scope, sink)
    {
        return Some(Stmt::Conditional(c));
    }

    None
}

/// Try to promote an `InlineLogic` node to a block-level statement.
pub fn lower_multiline_block_from_inline(
    inline: &ast::InlineLogic,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Option<Stmt> {
    if let Some(ml_cond) = inline.multiline_conditional()
        && let Ok(c) = ml_cond.lower_conditional(scope, sink)
    {
        return Some(Stmt::Conditional(c));
    }

    if let Some(cond) = inline.conditional()
        && (cond.multiline_branches().is_some() || cond.branchless_body().is_some())
        && let Ok(c) = cond.lower_conditional(scope, sink)
    {
        return Some(Stmt::Conditional(c));
    }

    if let Some(seq) = inline.sequence()
        && seq.multiline_branches().is_some()
    {
        return Some(Stmt::Sequence(lower_block_sequence(&seq, scope, sink)));
    }

    None
}

// ─── Inline logic → content parts ───────────────────────────────────

/// Lower inline logic into content parts (value interpolation, inline
/// conditional, or inline sequence).
pub fn lower_inline_logic_into_parts(
    inline: &ast::InlineLogic,
    parts: &mut Vec<crate::ContentPart>,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) {
    if let Some(inner) = inline.inner_expression()
        && let Some(expr) = inner.expr().and_then(|e| e.lower_expr(scope, sink).ok())
    {
        parts.push(crate::ContentPart::Interpolation(expr));
        return;
    }

    if let Some(cond) = inline.conditional()
        && let Ok(ic) = cond.lower_conditional(scope, sink)
    {
        parts.push(crate::ContentPart::InlineConditional(ic));
        return;
    }

    if let Some(seq) = inline.sequence()
        && let Ok(is) = seq.lower_sequence(scope, sink)
    {
        parts.push(crate::ContentPart::InlineSequence(is));
        return;
    }

    if let Some(imp) = inline.implicit_sequence()
        && let Ok(is) = imp.lower_sequence(scope, sink)
    {
        parts.push(crate::ContentPart::InlineSequence(is));
    }
}
