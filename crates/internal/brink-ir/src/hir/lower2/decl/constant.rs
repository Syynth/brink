//! `ConstDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::super::helpers::name_from_ident;
use super::DeclareSymbols;
use crate::{ConstDecl, DiagnosticCode, Expr, SymbolKind};

impl DeclareSymbols for ast::ConstDecl {
    type Output = ConstDecl;

    fn declare_and_lower(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<ConstDecl> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E006))?;
        let name =
            name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E006))?;
        sink.declare(SymbolKind::Constant, &name.text, name.range);

        let value = if let Some(e) = self.value() {
            e.lower_expr(scope, sink).unwrap_or(Expr::Null)
        } else {
            sink.diagnose(range, DiagnosticCode::E007);
            Expr::Null
        };

        Ok(ConstDecl {
            ptr: ast::AstPtr::new(self),
            name,
            value,
        })
    }
}
