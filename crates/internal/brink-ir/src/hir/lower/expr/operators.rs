//! Operator expression lowering: prefix, infix, postfix, and parenthesized.

use brink_syntax::ast::{self, AstNode};

use crate::provenance::NodeClass;
use crate::{DiagnosticCode, Expr, InfixExpr};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::{lower_infix_op, lower_postfix_op, lower_prefix_op};
use super::LowerExpr;

// ─── Operators ──────────────────────────────────────────────────────

impl LowerExpr for ast::PrefixExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let op = lower_prefix_op(self).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E016))?;
        let operand = self
            .operand()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        Ok(Expr::Prefix(op, Box::new(operand)))
    }
}

impl LowerExpr for ast::InfixExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let lhs = self
            .lhs()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let op = lower_infix_op(self).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E016))?;
        let rhs = self
            .rhs()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        // The whole `lhs op rhs` node's own range — the identity key a
        // side table addresses this node by (issue #1517).
        let ptr = scope.prov(NodeClass::Infix, self.syntax());
        Ok(Expr::Infix(InfixExpr::new(ptr, lhs, op, rhs)))
    }
}

impl LowerExpr for ast::PostfixExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let operand = self
            .operand()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let op =
            lower_postfix_op(self).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E016))?;
        Ok(Expr::Postfix(Box::new(operand), op))
    }
}

impl LowerExpr for ast::ParenExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        self.inner()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))
    }
}
