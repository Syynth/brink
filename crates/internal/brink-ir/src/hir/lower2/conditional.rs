//! Conditional and sequence normalization traits.
//!
//! Defines [`IntoConditional`] and [`IntoSequence`] — normalization traits
//! that collapse multiple AST representations into their common HIR types.

use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{
    Block, CondBranch, CondKind, Conditional, DiagnosticCode, Expr, Sequence, SequenceType, Stmt,
};

use super::block::{lower_branch_body, wrap_content_as_block};
use super::context::{LowerScope, LowerSink, Lowered};
use super::expr::LowerExpr;

// ─── IntoConditional ────────────────────────────────────────────────

/// Normalization trait: multiple AST representations → [`Conditional`].
pub trait LowerConditional {
    fn lower_conditional(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Conditional>;
}

// ── ConditionalWithExpr ─────────────────────────────────────────────

impl LowerConditional for ast::ConditionalWithExpr {
    fn lower_conditional(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Conditional> {
        let ptr = SyntaxNodePtr::from_node(self.syntax());
        let range = self.syntax().text_range();
        let condition = self
            .condition()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E020))
            .and_then(|e| e.lower_expr(scope, sink))?;

        Ok(lower_conditional_with_expr(
            self, &condition, ptr, scope, sink,
        ))
    }
}

/// Unified handler for all `ConditionalWithExpr` shapes: branchless body,
/// inline branches, multiline branches, or bare condition.
fn lower_conditional_with_expr(
    cond: &ast::ConditionalWithExpr,
    condition: &Expr,
    ptr: SyntaxNodePtr,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Conditional {
    let mut branches = Vec::new();

    // Branchless body: `{x: content}`
    if let Some(body) = cond.branchless_body() {
        use super::block::LowerBlock;
        let block = body.lower_block(scope, sink);
        branches.push(CondBranch {
            condition: Some(condition.clone()),
            body: block,
        });
        if let Some(else_branch) = body.else_branch()
            && let Some(ml_branch) = else_branch.branch()
        {
            let else_body = ml_branch.body().map_or_else(Block::default, |body| {
                lower_branch_body(body.syntax(), scope, sink)
            });
            branches.push(CondBranch {
                condition: None,
                body: else_body,
            });
        }
        return Conditional {
            ptr,
            kind: CondKind::InitialCondition,
            branches,
        };
    }

    // Inline branches: `{x: a | b}`
    if let Some(inline_branches) = cond.inline_branches() {
        let mut first = true;
        for b in inline_branches.branches() {
            let cond_expr = if first {
                first = false;
                Some(condition.clone())
            } else {
                None
            };
            branches.push(CondBranch {
                condition: cond_expr,
                body: wrap_content_as_block(b.syntax(), scope, sink),
            });
        }
        return Conditional {
            ptr,
            kind: CondKind::InitialCondition,
            branches,
        };
    }

    // Multiline branches: `{x: - 1: ... - 2: ... }`
    if let Some(ml_branches) = cond.multiline_branches() {
        let all_have_conditions = ml_branches
            .branches()
            .all(|b| b.is_else() || b.condition().is_some());

        for b in ml_branches.branches() {
            let cond_expr = if b.is_else() {
                None
            } else {
                b.condition().and_then(|e| e.lower_expr(scope, sink).ok())
            };
            let body = b.body().map_or_else(Block::default, |body| {
                lower_branch_body(body.syntax(), scope, sink)
            });
            branches.push(CondBranch {
                condition: cond_expr,
                body,
            });
        }

        let kind = if all_have_conditions {
            CondKind::Switch(condition.clone())
        } else {
            if let Some(first_no_cond) = branches.iter_mut().find(|b| b.condition.is_none()) {
                first_no_cond.condition = Some(condition.clone());
            }
            CondKind::InitialCondition
        };

        return Conditional {
            ptr,
            kind,
            branches,
        };
    }

    // Fallback: bare condition, no body
    branches.push(CondBranch {
        condition: Some(condition.clone()),
        body: Block::default(),
    });
    Conditional {
        ptr,
        kind: CondKind::InitialCondition,
        branches,
    }
}

// ── MultilineConditional ────────────────────────────────────────────

impl LowerConditional for ast::MultilineConditional {
    fn lower_conditional(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Conditional> {
        Ok(lower_if_else_branches(
            self.branches(),
            SyntaxNodePtr::from_node(self.syntax()),
            scope,
            sink,
        ))
    }
}

// ── MultilineBranchesCond ───────────────────────────────────────────

impl LowerConditional for ast::MultilineBranchesCond {
    fn lower_conditional(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Conditional> {
        Ok(lower_if_else_branches(
            self.branches(),
            SyntaxNodePtr::from_node(self.syntax()),
            scope,
            sink,
        ))
    }
}

/// Shared: lower a sequence of `MultilineBranchCond` into an if-else chain.
fn lower_if_else_branches(
    branches: impl Iterator<Item = ast::MultilineBranchCond>,
    ptr: SyntaxNodePtr,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Conditional {
    let branches = branches
        .map(|b| {
            let condition = if b.is_else() {
                None
            } else {
                b.condition().and_then(|e| e.lower_expr(scope, sink).ok())
            };
            let body = b.body().map_or_else(Block::default, |body| {
                lower_branch_body(body.syntax(), scope, sink)
            });
            CondBranch { condition, body }
        })
        .collect();
    Conditional {
        ptr,
        kind: CondKind::IfElse,
        branches,
    }
}

// ─── IntoSequence ───────────────────────────────────────────────────

/// Normalization trait: multiple AST representations → [`Sequence`].
pub trait LowerSequence {
    fn lower_sequence(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Sequence>;
}

// ── SequenceWithAnnotation (inline or block) ────────────────────────

impl LowerSequence for ast::SequenceWithAnnotation {
    fn lower_sequence(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Sequence> {
        let kind = lower_sequence_type(self);

        let branches = if let Some(inline_branches) = self.inline_branches() {
            inline_branches
                .branches()
                .map(|b| wrap_content_as_block(b.syntax(), scope, sink))
                .collect()
        } else if let Some(ml_branches) = self.multiline_branches() {
            ml_branches
                .branches()
                .map(|b| {
                    b.body().map_or_else(Block::default, |body| {
                        lower_branch_body(body.syntax(), scope, sink)
                    })
                })
                .collect()
        } else {
            return Err(sink.diagnose(self.syntax().text_range(), DiagnosticCode::E021));
        };

        Ok(Sequence {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            kind,
            branches,
        })
    }
}

// ── ImplicitSequence ────────────────────────────────────────────────

impl LowerSequence for ast::ImplicitSequence {
    fn lower_sequence(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Sequence> {
        let branches: Vec<Block> = self
            .branches()
            .map(|b| wrap_content_as_block(b.syntax(), scope, sink))
            .collect();
        Ok(Sequence {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            kind: SequenceType::STOPPING,
            branches,
        })
    }
}

// ─── Block-level sequence ───────────────────────────────────────────

/// Lower a `SequenceWithAnnotation` as a block-level sequence (leading
/// `EndOfLine` per branch). Used by multiline block promotion.
pub fn lower_block_sequence(
    seq: &ast::SequenceWithAnnotation,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Sequence {
    let kind = lower_sequence_type(seq);
    let branches = seq.multiline_branches().map_or_else(Vec::new, |ml| {
        ml.branches()
            .map(|b| {
                let mut block = b.body().map_or_else(Block::default, |body| {
                    lower_branch_body(body.syntax(), scope, sink)
                });
                block.stmts.insert(0, Stmt::EndOfLine);
                block
            })
            .collect()
    });
    Sequence {
        ptr: SyntaxNodePtr::from_node(seq.syntax()),
        kind,
        branches,
    }
}

fn lower_sequence_type(seq: &ast::SequenceWithAnnotation) -> SequenceType {
    let mut kind = SequenceType::empty();

    if let Some(sym) = seq.symbol_annotation() {
        if sym.amp_token().is_some() {
            kind |= SequenceType::CYCLE;
        }
        if sym.bang_token().is_some() {
            kind |= SequenceType::ONCE;
        }
        if sym.tilde_token().is_some() {
            kind |= SequenceType::SHUFFLE;
        }
        if sym.dollar_token().is_some() {
            kind |= SequenceType::STOPPING;
        }
    }

    if let Some(word) = seq.word_annotation() {
        if word.stopping_kw().is_some() {
            kind |= SequenceType::STOPPING;
        }
        if word.cycle_kw().is_some() {
            kind |= SequenceType::CYCLE;
        }
        if word.shuffle_kw().is_some() {
            kind |= SequenceType::SHUFFLE;
        }
        if word.once_kw().is_some() {
            kind |= SequenceType::ONCE;
        }
    }

    if kind.is_empty() {
        SequenceType::STOPPING
    } else {
        kind
    }
}

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
