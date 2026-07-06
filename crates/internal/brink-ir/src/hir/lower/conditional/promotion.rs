use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::Stmt;

use super::super::context::{LowerScope, LowerSink};
use super::super::expr::LowerExpr;
use super::sequence::lower_block_sequence;
use super::{LowerConditional, LowerSequence};

// ─── Multiline block promotion ──────────────────────────────────────

/// Try to lower a `MultilineBlock` AST node into a statement.
///
/// The returned `Conditional`/`Sequence`'s `ptr` is rewritten to `mb`'s own
/// range (not the inner `conditional`/`branches_cond`/`sequence` node's
/// range that `lower_conditional`/`lower_block_sequence` stamp) — `mb` is
/// the node that actually owns the enclosing `{`/`}` braces. Editor line
/// classification (`brink-ide::line_context`'s conditional-scaffold pass,
/// #413) walks this range to classify the braces and any `- cond:`/`-
/// else:` branch headers as `Logic`; without this correction, the opening
/// brace line falls outside every statement's range and is left `Blank`.
pub fn lower_multiline_block(
    mb: &ast::MultilineBlock,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Option<Stmt> {
    let mb_ptr = SyntaxNodePtr::from_node(mb.syntax());

    if let Some(cond) = mb.conditional()
        && let Ok(mut c) = cond.lower_conditional(scope, sink)
    {
        c.ptr = mb_ptr;
        return Some(Stmt::Conditional(c));
    }

    if let Some(seq) = mb.sequence()
        && seq.multiline_branches().is_some()
    {
        let mut s = lower_block_sequence(&seq, scope, sink);
        s.ptr = mb_ptr;
        return Some(Stmt::Sequence(s));
    }

    if let Some(branches) = mb.branches_cond()
        && let Ok(mut c) = branches.lower_conditional(scope, sink)
    {
        c.ptr = mb_ptr;
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
    // Same brace-inclusion correction as `lower_multiline_block` above:
    // `inline` (`{`...`}`) always encloses the inner conditional/sequence
    // node it wraps, so its own range is the true scaffold anchor.
    let inline_ptr = SyntaxNodePtr::from_node(inline.syntax());

    if let Some(ml_cond) = inline.multiline_conditional()
        && let Ok(mut c) = ml_cond.lower_conditional(scope, sink)
    {
        c.ptr = inline_ptr;
        return Some(Stmt::Conditional(c));
    }

    if let Some(cond) = inline.conditional()
        && (cond.multiline_branches().is_some() || cond.branchless_body().is_some())
        && let Ok(mut c) = cond.lower_conditional(scope, sink)
    {
        c.ptr = inline_ptr;
        return Some(Stmt::Conditional(c));
    }

    if let Some(seq) = inline.sequence()
        && seq.multiline_branches().is_some()
    {
        let mut s = lower_block_sequence(&seq, scope, sink);
        s.ptr = inline_ptr;
        return Some(Stmt::Sequence(s));
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
