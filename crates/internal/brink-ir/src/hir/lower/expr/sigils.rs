//! T1b superset expression lowering: sigil literals + indexing
//! (docs/t1b-surface-spec.md §3-4).
//!
//! Structural AST→HIR lowering only — this is a plain, dialect-agnostic
//! prefix of the pipeline shared by both dialects (docs/t1b-surface-spec.md
//! §1: "strict-mode checking of the oracle corpus shares every parse and
//! lowering prefix with normal compilation"). Whether the construct is
//! *allowed* is a `brink-analyzer` dialect-gate concern (E051/E052), decided
//! after HIR lowering completes.

use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::hir::types::{ArrayLiteral, IndexExpr, MapLiteral};
use crate::{DiagnosticCode, Expr};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerExpr;

impl LowerExpr for ast::ArrayLiteral {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let mut elements = Vec::new();
        for el in self.elements() {
            elements.push(el.lower_expr(scope, sink)?);
        }
        Ok(Expr::ArrayLiteral(ArrayLiteral {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
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
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            entries,
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
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            base: Box::new(base),
            index: Box::new(index),
        }))
    }
}
