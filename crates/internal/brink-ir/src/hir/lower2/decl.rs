//! Declaration lowering phase.
//!
//! The [`DeclareSymbols`] trait is implemented on AST declaration nodes.
//! Each impl registers the declared symbol in the [`LowerSink`] and
//! produces the corresponding HIR declaration node.

use brink_syntax::ast::{self, AstNode};

use super::context::{LowerScope, LowerSink, Lowered};
use super::expr::LowerExpr;
use super::helpers::{make_name, name_from_ident};
use crate::{
    ConstDecl, DiagnosticCode, Expr, ExternalDecl, ListDecl, ListMember, ParamInfo, SymbolKind,
    VarDecl,
};

// ─── Trait definition ───────────────────────────────────────────────

/// Extension trait for AST declaration nodes that register symbols and
/// produce HIR declaration types.
pub trait DeclareSymbols {
    type Output;

    fn declare_and_lower(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Self::Output>;
}

// ─── VAR ────────────────────────────────────────────────────────────

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

// ─── CONST ──────────────────────────────────────────────────────────

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

// ─── LIST ───────────────────────────────────────────────────────────

impl DeclareSymbols for ast::ListDecl {
    type Output = ListDecl;

    fn declare_and_lower(
        &self,
        _scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<ListDecl> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E008))?;
        let name =
            name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E008))?;
        let list_name_text = name.text.clone();
        sink.declare(SymbolKind::List, &list_name_text, name.range);

        let members: Vec<ListMember> = self
            .definition()
            .map(|def| {
                def.members()
                    .filter_map(|m| lower_list_member(&m, range, sink))
                    .collect()
            })
            .unwrap_or_default();

        for member in &members {
            let qualified = format!("{list_name_text}.{}", member.name.text);
            sink.declare(SymbolKind::ListItem, &qualified, member.name.range);
        }

        Ok(ListDecl {
            ptr: ast::AstPtr::new(self),
            name,
            members,
        })
    }
}

fn lower_list_member(
    m: &ast::ListMember,
    _parent_range: rowan::TextRange,
    sink: &mut impl LowerSink,
) -> Option<ListMember> {
    let range = m.syntax().text_range();
    if let Some(on) = m.on_member() {
        let name_text = on.name().or_else(|| {
            sink.diagnose(range, DiagnosticCode::E009);
            None
        })?;
        #[expect(clippy::cast_possible_truncation, reason = "list values fit in i32")]
        return Some(ListMember {
            name: make_name(name_text, range),
            value: on.value().map(|v| v as i32),
            is_active: true,
        });
    }
    if let Some(off) = m.off_member() {
        let name_text = off.name().or_else(|| {
            sink.diagnose(range, DiagnosticCode::E009);
            None
        })?;
        #[expect(clippy::cast_possible_truncation, reason = "list values fit in i32")]
        return Some(ListMember {
            name: make_name(name_text, range),
            value: off.value().map(|v| v as i32),
            is_active: false,
        });
    }
    sink.diagnose(range, DiagnosticCode::E009);
    None
}

// ─── EXTERNAL ───────────────────────────────────────────────────────

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
