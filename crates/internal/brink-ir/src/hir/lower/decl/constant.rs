//! `ConstDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{DirectiveTarget, apply_scope_directives, directives_before};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::expr::LowerExpr;
use super::super::helpers::name_from_ident;
use super::super::types::lower_type_annotation;
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
        let (doc, issues) = parse_doc_comment(self.syntax(), DocPolicy::VALUE);
        issues.diagnose(sink);
        sink.declare_full(
            SymbolKind::Constant,
            &name.text,
            name.range,
            Vec::new(),
            None,
            doc,
        );

        let value = if let Some(e) = self.value() {
            e.lower_expr(scope, sink).unwrap_or(Expr::Null)
        } else {
            sink.diagnose(range, DiagnosticCode::E007);
            Expr::Null
        };

        // `#@local` doesn't apply to CONSTs (diagnosed if it attaches), but
        // `#@private`/`#@public` do (M-2: CONSTs are importable, §2).
        let dirs = directives_before(self.syntax());
        let _ = apply_scope_directives(&dirs, DirectiveTarget::Const, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(SymbolKind::Constant, &name.text, vis);
        }

        let annotation = self
            .type_annotation()
            .and_then(|ta| lower_type_annotation(&ta));

        Ok(ConstDecl {
            ptr: ast::AstPtr::new(self),
            name,
            value,
            annotation,
        })
    }
}
