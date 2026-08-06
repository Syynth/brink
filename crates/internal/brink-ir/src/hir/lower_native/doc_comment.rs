//! `///`/`//!` doc-comment attachment for the native surface (B0.6b,
//! `docs/decision-log.md` 2026-07-20). Unlike the OLD ink frontend's
//! trivia-walk re-derivation (`hir::lower::doc_comment::collect_doc_lines`),
//! attachment here is already decided structurally by the parser: a
//! `DOC_COMMENT` CST node sits as the leading child of the declaration/
//! container it documents (`brink_syntax_native::parser::doc_comment`). This
//! module's only job is turning that already-attached node's lines into a
//! [`DocBlock`], via the shared, format-agnostic
//! [`crate::hir::doc_block::parse_lines`] tag parser both frontends share.

use brink_syntax_native::ast;

use crate::DocBlock;
use crate::hir::FileId;
use crate::hir::doc_block::{DocIssues, DocPolicy, parse_lines};
use crate::{Diagnostic, DiagnosticCode};

fn diag(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// Turn an already-attached `DOC_COMMENT` node (either the leading outer
/// form or a container's inner form — both share the same line shape, see
/// `ast::DocComment::lines`) into a [`DocBlock`], diagnosing malformed
/// (E038) / policy-inapplicable (E043) tags directly into `diags`. Native
/// has no `LowerSink` indirection (`hir::lower`'s frontend-specific
/// machinery) — every native lowering pass threads a plain
/// `&mut Vec<Diagnostic>`, so issues are emitted straight into it.
///
/// `None` in, `None` out: a declaration/container with no attached doc
/// comment produces no `DocBlock`, same as the OLD frontend's
/// `parse_doc_comment` on an empty `collect_doc_lines` walk.
pub(super) fn lower_doc_comment(
    file_id: FileId,
    doc: Option<ast::DocComment>,
    policy: DocPolicy,
    diags: &mut Vec<Diagnostic>,
) -> Option<DocBlock> {
    let doc = doc?;
    let lines = doc.lines();
    if lines.is_empty() {
        return None;
    }
    let (block, issues) = parse_lines(&lines, policy);
    emit_issues(file_id, issues, diags);
    block
}

fn emit_issues(file_id: FileId, issues: DocIssues, diags: &mut Vec<Diagnostic>) {
    for range in issues.malformed {
        diags.push(diag(file_id, range, DiagnosticCode::E038));
    }
    for range in issues.inapplicable {
        diags.push(diag(file_id, range, DiagnosticCode::E043));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_syntax_native::ast::AstNode as _;
    use brink_syntax_native::parser::parse;

    /// Parse `src`, find the first node of `kind`, pull its (outer-form)
    /// `.doc()` accessor via `get_doc`, and lower it.
    fn doc_of(
        src: &str,
        kind: brink_syntax_native::SyntaxKind,
        get_doc: impl Fn(&brink_syntax_native::SyntaxNode) -> Option<ast::DocComment>,
        policy: DocPolicy,
    ) -> (Option<DocBlock>, Vec<Diagnostic>) {
        let parsed = parse(src);
        let node = parsed
            .syntax()
            .descendants()
            .find(|n| n.kind() == kind)
            .expect("source should contain the requested declaration");
        let mut diags = Vec::new();
        let block = lower_doc_comment(FileId(0), get_doc(&node), policy, &mut diags);
        (block, diags)
    }

    fn flow_doc(src: &str) -> (Option<DocBlock>, Vec<Diagnostic>) {
        doc_of(
            src,
            brink_syntax_native::SyntaxKind::FLOW_DECL,
            |n| ast::FlowDecl::cast(n.clone()).and_then(|f| f.doc()),
            DocPolicy::CALLABLE,
        )
    }

    #[test]
    fn documented_flow_decl_produces_doc_block_with_param() {
        let src = "/// Greets someone.\n/// @param name {string}\nflow greet(name) {\n}\n";
        let (doc, diags) = flow_doc(src);
        let doc = doc.expect("doc present");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(doc.doc.as_deref(), Some("Greets someone."));
        assert_eq!(
            doc.params,
            vec![("name".to_string(), crate::TypeRef("string".to_string()))]
        );
    }

    #[test]
    fn undocumented_flow_decl_has_no_doc() {
        let (doc, diags) = flow_doc("flow greet() {\n}\n");
        assert!(doc.is_none());
        assert!(diags.is_empty());
    }

    #[test]
    fn malformed_param_reports_e038() {
        let src = "/// @param name\nflow greet(name) {\n}\n";
        let (doc, diags) = flow_doc(src);
        // No free text, no valid tags recorded -> no DocBlock, but the
        // malformed tag is still diagnosed (matches the OLD frontend's
        // `malformed_param_is_reported` precedent).
        assert!(doc.is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E038);
    }

    #[test]
    fn kind_tag_on_flow_is_inapplicable_e043() {
        let src = "/// A flow.\n/// @kind query\nflow greet() {\n}\n";
        let (doc, diags) = flow_doc(src);
        let doc = doc.expect("doc present");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E043);
        assert!(doc.kind.is_none(), "inapplicable tag is dropped");
        assert_eq!(doc.doc.as_deref(), Some("A flow."));
    }

    #[test]
    fn inner_doc_populates_via_block_doc_accessor() {
        let src = "flow greet() {\n//! Describes this flow from within.\nHi!\n}\n";
        let parsed = parse(src);
        let flow = parsed
            .syntax()
            .descendants()
            .find(|n| n.kind() == brink_syntax_native::SyntaxKind::FLOW_DECL)
            .and_then(ast::FlowDecl::cast)
            .expect("flow decl");
        let body = flow
            .body()
            .and_then(|b| match b {
                ast::Body::Prose(block) => Some(block),
                ast::Body::Code(_) => None,
            })
            .expect("flow's default body is prose-ground");
        let mut diags = Vec::new();
        let doc = lower_doc_comment(FileId(0), body.doc(), DocPolicy::CALLABLE, &mut diags);
        let doc = doc.expect("inner doc lowers");
        assert!(diags.is_empty());
        assert_eq!(doc.doc.as_deref(), Some("Describes this flow from within."));
    }
}
