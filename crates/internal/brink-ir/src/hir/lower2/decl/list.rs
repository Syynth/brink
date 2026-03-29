//! `ListDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::{make_name, name_from_ident};
use super::DeclareSymbols;
use crate::{DiagnosticCode, ListDecl, ListMember, SymbolKind};

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
