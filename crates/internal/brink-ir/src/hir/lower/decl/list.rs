//! `ListDecl` symbol declaration and lowering.

use brink_syntax::ast::{self, AstNode};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::directive::{DirectiveTarget, apply_scope_directives, directives_before};
use super::super::doc_comment::{DocPolicy, parse_doc_comment};
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
        let (doc, issues) = parse_doc_comment(self.syntax(), DocPolicy::VALUE);
        issues.diagnose(sink);
        sink.declare_full(
            SymbolKind::List,
            &list_name_text,
            name.range,
            Vec::new(),
            None,
            doc,
        );

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

        // `#@local` doesn't apply to LISTs (diagnosed if it attaches), but
        // `#@private`/`#@public` do (M-2: LISTs are importable, §2).
        let dirs = directives_before(self.syntax());
        let _ = apply_scope_directives(&dirs, DirectiveTarget::List, sink);
        if let Some(vis) = super::super::directive::visibility_from_directives(&dirs, sink) {
            sink.set_visibility(SymbolKind::List, &list_name_text, vis);
        }
        if let Some((old_name, was_range)) =
            super::super::directive::was_from_directives(&dirs, sink)
        {
            if old_name == list_name_text {
                sink.diagnose(was_range, DiagnosticCode::E095);
            } else {
                sink.set_was(SymbolKind::List, &list_name_text, old_name, was_range);
            }
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
