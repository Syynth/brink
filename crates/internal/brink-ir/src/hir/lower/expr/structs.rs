//! TM-4b struct expression lowering: construction literals + field access
//! (docs/typed-mode-spec.md §6).
//!
//! Structural AST→HIR lowering only — dialect-agnostic, mirrors
//! `expr::sigils`. Whether the construct is *allowed* is
//! `brink-analyzer::dialect_gate`'s job (E051); whether a construction's
//! fields are complete/well-typed is `brink-analyzer`'s strict-mode
//! construction check, not this module's.

use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::hir::types::{FieldAccessExpr, StructLiteral};
use crate::{DiagnosticCode, Expr, RefKind};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::{make_name, name_from_ident};
use super::LowerExpr;

impl LowerExpr for ast::StructLiteral {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let shape_name_text = ident
            .name()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let shape_range = ident.syntax().text_range();
        let shape = make_name(shape_name_text.clone(), shape_range);

        sink.add_unresolved(
            &shape_name_text,
            shape_range,
            RefKind::Struct,
            &scope.to_scope(),
            None,
        );

        let mut fields = Vec::new();
        for f in self.fields() {
            let f_range = f.syntax().text_range();
            let f_ident = f
                .identifier()
                .ok_or_else(|| sink.diagnose(f_range, DiagnosticCode::E017))?;
            let f_name = name_from_ident(&f_ident)
                .ok_or_else(|| sink.diagnose(f_range, DiagnosticCode::E017))?;
            let value = f
                .value()
                .ok_or_else(|| sink.diagnose(f_range, DiagnosticCode::E015))
                .and_then(|v| v.lower_expr(scope, sink))?;
            fields.push((f_name, value));
        }

        Ok(Expr::StructLiteral(StructLiteral {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            shape,
            fields,
        }))
    }
}

impl LowerExpr for ast::FieldAccessExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let base = self
            .base()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let field_ident = self
            .field()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let field = name_from_ident(&field_ident)
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        Ok(Expr::FieldAccess(FieldAccessExpr {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            base: Box::new(base),
            field,
        }))
    }
}
