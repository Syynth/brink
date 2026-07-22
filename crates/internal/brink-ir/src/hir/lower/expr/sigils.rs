//! T1b superset expression lowering: sigil literals + indexing
//! (docs/t1b-surface-spec.md §3-4).
//!
//! Structural AST→HIR lowering only — this is a plain, dialect-agnostic
//! prefix of the pipeline shared by both dialects (docs/t1b-surface-spec.md
//! §1: "strict-mode checking of the oracle corpus shares every parse and
//! lowering prefix with normal compilation"). Whether the construct is
//! *allowed* is a `brink-analyzer` dialect-gate concern (E051/E052), decided
//! after HIR lowering completes.

use brink_syntax::ast::{self, AstNode};

use crate::provenance::NodeClass;

use crate::hir::types::{ArrayLiteral, FnLiteral, IndexExpr, MapLiteral, RefArgExpr};
use crate::{DiagnosticCode, Expr};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::lower_path;
use super::LowerExpr;

impl LowerExpr for ast::ArrayLiteral {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let mut elements = Vec::new();
        for el in self.elements() {
            elements.push(el.lower_expr(scope, sink)?);
        }
        Ok(Expr::ArrayLiteral(ArrayLiteral {
            ptr: scope.prov(NodeClass::ArrayLiteral, self.syntax()),
            elements,
        }))
    }
}

impl LowerExpr for ast::MapLiteral {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let mut entries = Vec::new();
        for entry in self.entries() {
            let range = entry.syntax().text_range();
            let key = entry
                .key()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
                .and_then(|e| e.lower_expr(scope, sink))?;
            let value = entry
                .value()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
                .and_then(|e| e.lower_expr(scope, sink))?;
            entries.push((key, value));
        }
        Ok(Expr::MapLiteral(MapLiteral {
            ptr: scope.prov(NodeClass::MapLiteral, self.syntax()),
            entries,
        }))
    }
}

impl LowerExpr for ast::FnLiteral {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let ast_target = self
            .target()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let target = lower_path(&ast_target);
        // The target is a Function reference (`project_manifest` walks
        // `FnLiteral.target` and always records `arg_count: None` for it —
        // `#fn` binds a *prefix* of the param row, so full-arity checking
        // doesn't apply the way it does for a direct call).
        let mut args = Vec::new();
        for a in self.args() {
            args.push(a.lower_expr(scope, sink)?);
        }
        Ok(Expr::FnLiteral(FnLiteral {
            ptr: scope.prov(NodeClass::FnLiteral, self.syntax()),
            target,
            args,
        }))
    }
}

impl LowerExpr for ast::RefExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let operand = self
            .operand()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        Ok(Expr::RefArg(RefArgExpr {
            ptr: scope.prov(NodeClass::RefArg, self.syntax()),
            operand: Box::new(operand),
        }))
    }
}

/// `start..end` / `start..=end` — range literal (NS-A5,
/// docs/stdlib-spec.md §7, F7). Both bounds are ordinary expressions;
/// a missing bound is E015 (same shape as the sibling extension exprs).
impl LowerExpr for ast::RangeExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let start = self
            .start()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let end = self
            .end()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        Ok(Expr::Range(crate::RangeExpr {
            ptr: scope.prov(NodeClass::Range, self.syntax()),
            start: Box::new(start),
            end: Box::new(end),
            inclusive: self.is_inclusive(),
        }))
    }
}

impl LowerExpr for ast::IndexExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let base = self
            .base()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let index = self
            .index()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        Ok(Expr::Index(IndexExpr {
            ptr: scope.prov(NodeClass::Index, self.syntax()),
            base: Box::new(base),
            index: Box::new(index),
        }))
    }
}
