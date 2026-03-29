use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{Block, CondBranch, CondKind, Conditional};

use super::super::block::lower_branch_body;
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::LowerConditional;

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
