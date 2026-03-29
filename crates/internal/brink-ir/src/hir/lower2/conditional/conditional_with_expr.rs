use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{Block, CondBranch, CondKind, Conditional, DiagnosticCode, Expr};

use super::super::block::{lower_branch_body, wrap_content_as_block};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::LowerConditional;

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
        use super::super::block::LowerBlock;
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
