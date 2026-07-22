//! Parsing of `///` JSDoc-style doc-comments on declarations.
//!
//! Comments are trivia in the green tree (no grammar involvement), so we walk
//! the tokens immediately preceding a declaration to collect the contiguous
//! block of `///` lines, then hand them to the shared, format-agnostic
//! `hir::doc_block::parse_lines` (B0.6b, `docs/decision-log.md` 2026-07-20:
//! factored out so both the OLD parser's trivia-walk attachment here and the
//! native frontend's CST-node attachment,
//! `hir::lower_native::doc_comment`, share the identical `@param`/
//! `@returns`/`@kind` tag parser). Like Rust doc-comments, codegen ignores
//! these — only the analyzer/IDE consume them.

use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use super::context::LowerSink;
use crate::hir::doc_block::{DocIssues, parse_lines};
use crate::{DiagnosticCode, DocBlock};

// Re-exported so every existing `super::super::doc_comment::{DocPolicy,
// parse_doc_comment}` call site (`lower/structure/{knot,stitch}.rs`,
// `lower/decl/{constant,var,external,list,struct_decl}.rs`) keeps working
// unmodified — `DocPolicy` now lives in the shared module.
pub use crate::hir::doc_block::DocPolicy;

impl DocIssues {
    /// Emit the standard diagnostics for the collected ranges. An inherent
    /// impl living here rather than alongside `DocIssues`'s definition
    /// (`hir::doc_block`) because it needs `LowerSink`, which is
    /// `hir::lower`-frontend-specific machinery the shared module has no
    /// business depending on — native's own diagnostics don't go through
    /// `LowerSink` at all (`hir::lower_native::doc_comment::emit_issues`
    /// pushes directly into a `Vec<Diagnostic>`). Same crate, so the
    /// cross-module inherent impl is legal (Rust's orphan rule only
    /// restricts cross-*crate* impls).
    pub(crate) fn diagnose(self, sink: &mut impl LowerSink) {
        for range in self.malformed {
            sink.diagnose(range, DiagnosticCode::E038);
        }
        for range in self.inapplicable {
            sink.diagnose(range, DiagnosticCode::E043);
        }
    }
}

/// Parse the `///` doc-comment block preceding `node` into a [`DocBlock`].
///
/// Editor note: per the decision log, a doc block is structurally part of the
/// declaration it precedes — folding, structural moves, and view slices must
/// keep them together.
///
/// Returns the doc (if any tags or text were found) plus the source ranges of
/// any malformed or policy-inapplicable tags, for the caller to diagnose.
#[must_use]
pub fn parse_doc_comment(node: &SyntaxNode, policy: DocPolicy) -> (Option<DocBlock>, DocIssues) {
    let lines = collect_doc_lines(node);
    if lines.is_empty() {
        return (None, DocIssues::default());
    }
    parse_lines(&lines, policy)
}

/// Walk backward from `node`'s first token, collecting the contiguous block of
/// `///` comment lines that immediately precede it (stopping at a blank-line
/// gap, a non-doc comment, or real content). Returned in source order, each as
/// `(text-after-"///", comment-range)`.
fn collect_doc_lines(node: &SyntaxNode) -> Vec<(String, TextRange)> {
    let Some(first) = node.first_token() else {
        return Vec::new();
    };
    let mut out: Vec<(String, TextRange)> = Vec::new();
    let mut newlines = 0u32;
    let mut tok = first.prev_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::WHITESPACE => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines >= 2 {
                    break; // blank line — end of the contiguous doc block
                }
            }
            SyntaxKind::LINE_COMMENT => {
                if let Some(body) = t.text().strip_prefix("///") {
                    newlines = 0;
                    out.push((body.trim_start().to_string(), t.text_range()));
                } else {
                    break; // a plain `//` comment breaks the doc block
                }
            }
            _ => break, // real content
        }
        tok = t.prev_token();
    }
    out.reverse(); // walked backward; restore source order
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExternalKind, TypeRef};
    use brink_syntax::parse;

    /// Parse `src`, find the first node of `kind`, and parse its doc.
    fn doc_of_kind(
        src: &str,
        kind: SyntaxKind,
        policy: DocPolicy,
    ) -> (Option<DocBlock>, DocIssues) {
        let parsed = parse(src);
        let node = parsed
            .syntax()
            .descendants()
            .find(|n| n.kind() == kind)
            .expect("source should contain the requested declaration");
        parse_doc_comment(&node, policy)
    }

    /// Parse `src`, find the first `EXTERNAL_DECL`, and parse its doc.
    fn doc_of(src: &str) -> (Option<DocBlock>, DocIssues) {
        doc_of_kind(src, SyntaxKind::EXTERNAL_DECL, DocPolicy::EXTERNAL)
    }

    #[test]
    fn parses_full_doc_block() {
        let src = "\
/// Whether the player holds an item.
/// @param item {item_id}
/// @returns {bool}
/// @kind query
EXTERNAL holds(item)
";
        let (doc, issues) = doc_of(src);
        let doc = doc.expect("doc present");
        assert!(
            issues.malformed.is_empty(),
            "no malformed tags: {:?}",
            issues.malformed
        );
        assert_eq!(
            doc.doc.as_deref(),
            Some("Whether the player holds an item.")
        );
        assert_eq!(
            doc.params,
            vec![("item".to_string(), TypeRef("item_id".to_string()))]
        );
        assert_eq!(doc.returns, Some(TypeRef("bool".to_string())));
        assert_eq!(doc.kind, Some(ExternalKind::Query));
    }

    #[test]
    fn no_doc_when_no_comments() {
        let (doc, issues) = doc_of("EXTERNAL plain(x)\n");
        assert!(doc.is_none());
        assert!(issues.malformed.is_empty());
    }

    #[test]
    fn blank_line_breaks_the_block() {
        // The comment is separated from the decl by a blank line — not attached.
        let src = "\
/// orphaned doc

EXTERNAL holds(item)
";
        let (doc, _) = doc_of(src);
        assert!(
            doc.is_none(),
            "doc separated by a blank line is not attached"
        );
    }

    #[test]
    fn plain_comment_breaks_the_block() {
        // A plain `//` line between the `///` block and the decl breaks it.
        let src = "\
/// kept
// not a doc line
EXTERNAL holds(item)
";
        let (doc, _) = doc_of(src);
        assert!(doc.is_none(), "a plain // comment terminates the doc block");
    }

    #[test]
    fn malformed_param_is_reported() {
        let src = "\
/// @param item
/// @returns {bool}
EXTERNAL holds(item)
";
        let (doc, issues) = doc_of(src);
        let doc = doc.expect("doc present");
        assert_eq!(issues.malformed.len(), 1, "the bad @param is reported");
        assert!(doc.params.is_empty(), "malformed param not recorded");
        assert_eq!(doc.returns, Some(TypeRef("bool".to_string())));
    }

    #[test]
    fn unknown_tags_and_widget_are_ignored() {
        let src = "\
/// @widget color_picker
/// @nonsense whatever
/// @param c {color}
EXTERNAL tint(c)
";
        let (doc, issues) = doc_of(src);
        let doc = doc.expect("doc present");
        assert!(
            issues.malformed.is_empty(),
            "unknown/widget tags ignored, not malformed"
        );
        assert_eq!(
            doc.params,
            vec![("c".to_string(), TypeRef("color".to_string()))]
        );
    }

    #[test]
    fn knot_doc_with_signature_tags() {
        let src = "\
/// Damage roll for an attack.
/// @param weapon {item_id}
/// @returns {int}
== function damage(weapon) ==
~ return 1
";
        let (doc, issues) = doc_of_kind(src, SyntaxKind::KNOT_DEF, DocPolicy::CALLABLE);
        let doc = doc.expect("doc present");
        assert!(issues.malformed.is_empty());
        assert!(issues.inapplicable.is_empty());
        assert_eq!(doc.doc.as_deref(), Some("Damage roll for an attack."));
        assert_eq!(
            doc.params,
            vec![("weapon".to_string(), TypeRef("item_id".to_string()))]
        );
        assert_eq!(doc.returns, Some(TypeRef("int".to_string())));
    }

    #[test]
    fn kind_tag_on_knot_is_inapplicable() {
        let src = "\
/// A knot.
/// @kind query
== hub ==
text
";
        let (doc, issues) = doc_of_kind(src, SyntaxKind::KNOT_DEF, DocPolicy::CALLABLE);
        let doc = doc.expect("doc present");
        assert_eq!(issues.inapplicable.len(), 1, "@kind reported inapplicable");
        assert!(doc.kind.is_none(), "inapplicable tag is dropped");
        assert_eq!(doc.doc.as_deref(), Some("A knot."));
    }

    #[test]
    fn nested_stitch_doc_is_reachable() {
        let src = "\
== hub ==
intro text
/// The market square.
= market
stall text
";
        let (doc, issues) = doc_of_kind(src, SyntaxKind::STITCH_DEF, DocPolicy::CALLABLE);
        let doc = doc.expect("doc present");
        assert!(issues.malformed.is_empty());
        assert_eq!(doc.doc.as_deref(), Some("The market square."));
    }

    #[test]
    fn var_doc_free_text_and_inapplicable_param() {
        let src = "\
/// Player health.
/// @param x {int}
VAR health = 100
";
        let (doc, issues) = doc_of_kind(src, SyntaxKind::VAR_DECL, DocPolicy::VALUE);
        let doc = doc.expect("doc present");
        assert_eq!(issues.inapplicable.len(), 1, "@param on VAR inapplicable");
        assert!(doc.params.is_empty(), "inapplicable tag is dropped");
        assert_eq!(doc.doc.as_deref(), Some("Player health."));
    }

    #[test]
    fn const_and_list_docs_are_reachable() {
        let src = "\
/// Movement speed.
CONST SPEED = 0.5
";
        let (doc, _) = doc_of_kind(src, SyntaxKind::CONST_DECL, DocPolicy::VALUE);
        assert_eq!(
            doc.expect("doc present").doc.as_deref(),
            Some("Movement speed.")
        );

        let src = "\
/// Mood states.
LIST mood = happy, sad
";
        let (doc, _) = doc_of_kind(src, SyntaxKind::LIST_DECL, DocPolicy::VALUE);
        assert_eq!(
            doc.expect("doc present").doc.as_deref(),
            Some("Mood states.")
        );
    }
}
