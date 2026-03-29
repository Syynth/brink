//! `VarDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::super::helpers::name_from_ident;
use super::DeclareSymbols;
use crate::{DiagnosticCode, Expr, SymbolKind, VarDecl};

impl DeclareSymbols for ast::VarDecl {
    type Output = VarDecl;

    fn declare_and_lower(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<VarDecl> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E004))?;
        let name =
            name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E004))?;
        sink.declare(SymbolKind::Variable, &name.text, name.range);

        let value = if let Some(e) = self.value() {
            e.lower_expr(scope, sink).unwrap_or(Expr::Null)
        } else {
            sink.diagnose(range, DiagnosticCode::E005);
            Expr::Null
        };

        Ok(VarDecl {
            ptr: ast::AstPtr::new(self),
            name,
            value,
        })
    }
}
