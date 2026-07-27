use brink_syntax::ast::{self, AstNode};

use crate::Provenance;
use crate::provenance::NodeClass;

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
            scope.prov(NodeClass::Conditional, self.syntax()),
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
            scope.prov(NodeClass::Conditional, self.syntax()),
            scope,
            sink,
        ))
    }
}

/// Shared: lower a sequence of `MultilineBranchCond` into an if-else chain.
fn lower_if_else_branches(
    branches: impl Iterator<Item = ast::MultilineBranchCond>,
    ptr: Provenance,
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
            let branch_ptr = scope.prov(NodeClass::ConditionalBranch, b.syntax());
            let body = b.body().map_or_else(Block::default, |body| {
                lower_branch_body(body.syntax(), scope, sink)
            });
            CondBranch {
                ptr: branch_ptr,
                condition,
                binding: None,
                body,
                container_id: None,
            }
        })
        .collect();
    Conditional {
        ptr,
        kind: CondKind::IfElse,
        branches,
    }
}
