//! `ExternalDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::name_from_ident;
use super::DeclareSymbols;
use crate::{DiagnosticCode, ExternalDecl, ParamInfo, SymbolKind};

impl DeclareSymbols for ast::ExternalDecl {
    type Output = ExternalDecl;

    fn declare_and_lower(
        &self,
        _scope: &LowerScope,
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

        sink.declare_with(
            SymbolKind::External,
            &name.text,
            name.range,
            param_infos,
            None,
        );

        #[expect(
            clippy::cast_possible_truncation,
            reason = "external params won't exceed 255"
        )]
        let param_count = self.param_list().map_or(0, |pl| pl.params().count() as u8);

        Ok(ExternalDecl {
            ptr: ast::AstPtr::new(self),
            name,
            param_count,
        })
    }
}
