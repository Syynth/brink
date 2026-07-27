use brink_syntax::ast::{self, AstNode};

use crate::Provenance;
use crate::provenance::NodeClass;

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
        let ptr = scope.prov(NodeClass::Conditional, self.syntax());
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
    ptr: Provenance,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Conditional {
    let mut branches = Vec::new();

    // Branchless body: `{x: content}`
    if let Some(body) = cond.branchless_body() {
        return lower_branchless_body(&body, condition, ptr, scope, sink);
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
                ptr: scope.prov(NodeClass::ConditionalBranch, b.syntax()),
                condition: cond_expr,
                binding: None,
                body: wrap_content_as_block(b.syntax(), scope, sink),
                container_id: None,
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
                ptr: scope.prov(NodeClass::ConditionalBranch, b.syntax()),
                condition: cond_expr,
                binding: None,
                body,
                container_id: None,
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

    // Fallback: bare condition, no body — no branch node exists at all, so
    // the whole conditional's own span is the narrowest available.
    branches.push(CondBranch {
        ptr,
        condition: Some(condition.clone()),
        binding: None,
        body: Block::default(),
        container_id: None,
    });
    Conditional {
        ptr,
        kind: CondKind::InitialCondition,
        branches,
    }
}

/// `{x: content}` / `{x: content | else_body}` (branchless-body form): the
/// implicit first arm has no dedicated branch node, so its span is its own
/// body's; the `else` arm (if any) is a real `MultilineBranchCond`.
fn lower_branchless_body(
    body: &ast::BranchlessCondBody,
    condition: &Expr,
    ptr: Provenance,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Conditional {
    use super::super::block::LowerBlock;

    let mut branches = Vec::new();
    let branch_ptr = scope.prov(NodeClass::ConditionalBranch, body.syntax());
    let block = body.lower_block(scope, sink).unwrap_or_default();
    branches.push(CondBranch {
        ptr: branch_ptr,
        condition: Some(condition.clone()),
        binding: None,
        body: block,
        container_id: None,
    });
    if let Some(else_branch) = body.else_branch()
        && let Some(ml_branch) = else_branch.branch()
    {
        let else_ptr = scope.prov(NodeClass::ConditionalBranch, ml_branch.syntax());
        let else_body = ml_branch.body().map_or_else(Block::default, |body| {
            lower_branch_body(body.syntax(), scope, sink)
        });
        branches.push(CondBranch {
            ptr: else_ptr,
            condition: None,
            binding: None,
            body: else_body,
            container_id: None,
        });
    }
    Conditional {
        ptr,
        kind: CondKind::InitialCondition,
        branches,
    }
}
