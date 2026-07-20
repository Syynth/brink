//! `ExternalDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{DirectiveTarget, apply_scope_directives, directives_before};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
use super::super::helpers::name_from_ident;
use super::DeclareSymbols;
use crate::provenance::NodeClass;
use crate::{DiagnosticCode, ExternalDecl, ParamInfo, SymbolKind};

impl DeclareSymbols for ast::ExternalDecl {
    type Output = ExternalDecl;

    fn declare_and_lower(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<ExternalDecl> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E010))?;
        let name =
            name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E010))?;

        let param_infos: Vec<ParamInfo> = self
            .param_list()
            .into_iter()
            .flat_map(|pl| pl.params().collect::<Vec<_>>())
            .filter_map(|p| {
                p.name().map(|n| ParamInfo {
                    name: n,
                    is_ref: false,
                    is_divert: false,
                })
            })
            .collect();

        // Parse the inline `///` doc-comment block (if any) and report any
        // malformed tags. Codegen ignores the doc — it's tooling metadata.
        let (doc, issues) = parse_doc_comment(self.syntax(), DocPolicy::EXTERNAL);
        issues.diagnose(sink);

        sink.declare_full(
            SymbolKind::External,
            &name.text,
            name.range,
            param_infos.clone(),
            None,
            doc.clone(),
        );

        #[expect(
            clippy::cast_possible_truncation,
            reason = "external params won't exceed 255"
        )]
        let param_count = self.param_list().map_or(0, |pl| pl.params().count() as u8);

        // `#@local` doesn't apply to EXTERNALs (diagnosed if it attaches);
        // `#@private`/`#@public` are recorded (M-2) rather than silently
        // dropped.
        let dirs = directives_before(self.syntax());
        let _ = apply_scope_directives(&dirs, DirectiveTarget::External, sink);
        let mut visibility = None;
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(SymbolKind::External, &name.text, vis);
            visibility = Some(vis);
        }
        let mut was = None;
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            if old_name == name.text {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                sink.set_was(SymbolKind::External, &name.text, old_name.clone(), was_range);
                was = Some((old_name, was_range));
            }
        }

        Ok(ExternalDecl {
            ptr: scope.prov(NodeClass::ExternalDecl, self.syntax()),
            name,
            param_count,
            params: param_infos,
            doc,
            visibility,
            was,
        })
    }
}
