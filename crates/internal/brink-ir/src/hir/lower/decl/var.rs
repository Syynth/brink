//! `VarDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{DirectiveTarget, apply_scope_directives, directives_before};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::expr::LowerExpr;
use super::super::helpers::name_from_ident;
use super::super::types::lower_type_annotation;
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
        let (doc, issues) = parse_doc_comment(self.syntax(), DocPolicy::VALUE);
        issues.diagnose(sink);
        sink.declare_full(
            SymbolKind::Variable,
            &name.text,
            name.range,
            Vec::new(),
            None,
            doc,
        );

        let value = if let Some(e) = self.value() {
            e.lower_expr(scope, sink).unwrap_or(Expr::Null)
        } else {
            sink.diagnose(range, DiagnosticCode::E005);
            Expr::Null
        };

        // `#@local` directive line(s) immediately above the declaration.
        let dirs = directives_before(self.syntax());
        let is_local = apply_scope_directives(&dirs, DirectiveTarget::Var, sink);

        let annotation = self
            .type_annotation()
            .and_then(|ta| lower_type_annotation(&ta));

        Ok(VarDecl {
            ptr: ast::AstPtr::new(self),
            name,
            value,
            is_local,
            annotation,
        })
    }
}
