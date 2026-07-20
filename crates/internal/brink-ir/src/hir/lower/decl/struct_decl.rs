//! `StructDecl` symbol declaration and lowering (TM-4b,
//! docs/typed-mode-spec.md §6).
//!
//! Structural AST→HIR lowering only, mirroring `decl::list`'s shape — a
//! plain, dialect-agnostic prefix of the pipeline shared by both dialects.
//! Whether the construct is *allowed* is `brink-analyzer::dialect_gate`'s
//! job (E051), decided after HIR lowering completes.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::helpers::name_from_ident;
use super::super::types::lower_type_expr;
use super::DeclareSymbols;
use crate::provenance::NodeClass;
use crate::{DiagnosticCode, StructDecl, StructFieldDecl, SymbolKind};

impl DeclareSymbols for ast::StructDecl {
    type Output = StructDecl;

    fn declare_and_lower(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<StructDecl> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E008))?;
        let name =
            name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E008))?;
        let (doc, issues) = parse_doc_comment(self.syntax(), DocPolicy::VALUE);
        issues.diagnose(sink);
        sink.declare_full(
            SymbolKind::Struct,
            &name.text,
            name.range,
            Vec::new(),
            None,
            doc.clone(),
        );

        // `#@private`/`#@public` visibility (M-2: STRUCTs are importable, §2).
        let dirs = super::super::directive::directives_before(self.syntax());
        let mut visibility = None;
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(crate::SymbolKind::Struct, &name.text, vis);
            visibility = Some(vis);
        }

        let fields: Vec<StructFieldDecl> = self
            .fields()
            .filter_map(|f| lower_struct_field_decl(&f, range, sink))
            .collect();

        Ok(StructDecl {
            ptr: scope.prov(NodeClass::StructDecl, self.syntax()),
            name,
            fields,
            doc,
            visibility,
        })
    }
}

fn lower_struct_field_decl(
    f: &ast::StructFieldDecl,
    _parent_range: rowan::TextRange,
    sink: &mut impl LowerSink,
) -> Option<StructFieldDecl> {
    let range = f.syntax().text_range();
    let ident = f.identifier().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E008);
        None
    })?;
    let name = name_from_ident(&ident).or_else(|| {
        sink.diagnose(range, DiagnosticCode::E008);
        None
    })?;
    let ty_ast = f.type_expr().or_else(|| {
        sink.diagnose(range, DiagnosticCode::E015);
        None
    })?;
    let ty = lower_type_expr(&ty_ast).or_else(|| {
        sink.diagnose(range, DiagnosticCode::E015);
        None
    })?;
    Some(StructFieldDecl { name, ty })
}
