//! `VarDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{DirectiveTarget, apply_scope_directives, directives_before};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::expr::LowerExpr;
use super::super::helpers::name_from_ident;
use super::super::types::lower_type_annotation;
use super::DeclareSymbols;
use crate::provenance::NodeClass;
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

        // `#@local` and `#@private`/`#@public` directive line(s) immediately
        // above the declaration.
        let dirs = directives_before(self.syntax());
        let is_local = apply_scope_directives(&dirs, DirectiveTarget::Var, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(SymbolKind::Variable, &name.text, vis);
        }
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            if old_name == name.text {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                sink.set_was(SymbolKind::Variable, &name.text, old_name, was_range);
            }
        }

        let annotation = self
            .type_annotation()
            .and_then(|ta| lower_type_annotation(&ta));

        Ok(VarDecl {
            ptr: scope.prov(NodeClass::VarDecl, self.syntax()),
            name,
            value,
            is_local,
            annotation,
        })
    }
}
