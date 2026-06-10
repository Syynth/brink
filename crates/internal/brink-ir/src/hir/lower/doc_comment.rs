//! Parsing of `///` JSDoc-style doc-comments on declarations.
//!
//! Comments are trivia in the green tree (no grammar involvement), so we walk
//! the tokens immediately preceding a declaration to collect the contiguous
//! block of `///` lines, then parse the `@param` / `@returns` / `@kind` tags
//! into a [`DocBlock`]. Like Rust doc-comments, codegen ignores these —
//! only the analyzer/IDE consume them.

use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use super::context::LowerSink;
use crate::{DiagnosticCode, DocBlock, ExternalKind, TypeRef};

/// Which doc-comment tags are meaningful for a declaration kind. Tags that
/// are well-formed but not allowed by the policy are dropped and reported as
/// inapplicable (E043).
#[derive(Debug, Clone, Copy)]
pub struct DocPolicy {
    /// `@param` / `@returns` — signature tags for callables.
    pub allow_params: bool,
    /// `@kind` — the host-capability category, externals only.
    pub allow_kind: bool,
}

impl DocPolicy {
    /// `EXTERNAL` declarations: all tags.
    pub const EXTERNAL: Self = Self {
        allow_params: true,
        allow_kind: true,
    };
    /// Knots and stitches: signature tags, but no `@kind`.
    pub const CALLABLE: Self = Self {
        allow_params: true,
        allow_kind: false,
    };
    /// `VAR` / `CONST` / `LIST`: free text only.
    pub const VALUE: Self = Self {
        allow_params: false,
        allow_kind: false,
    };
}

/// Problem ranges found while parsing a doc block, for the caller to diagnose.
#[derive(Debug, Default)]
pub struct DocIssues {
    /// Tags that failed to parse (→ E038).
    pub malformed: Vec<TextRange>,
    /// Well-formed tags not applicable to this declaration kind (→ E043).
    pub inapplicable: Vec<TextRange>,
}

impl DocIssues {
    /// Emit the standard diagnostics for the collected ranges.
    pub fn diagnose(self, sink: &mut impl LowerSink) {
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

/// Parse collected `///` lines into a [`DocBlock`], returning the ranges of
/// malformed and policy-inapplicable tags.
fn parse_lines(lines: &[(String, TextRange)], policy: DocPolicy) -> (Option<DocBlock>, DocIssues) {
    let mut doc = DocBlock::default();
    let mut free: Vec<String> = Vec::new();
    let mut issues = DocIssues::default();

    for (line, range) in lines {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('@') {
            let mut it = rest.splitn(2, char::is_whitespace);
            let tag = it.next().unwrap_or("");
            let arg = it.next().unwrap_or("").trim();
            match tag {
                "param" if !policy.allow_params => issues.inapplicable.push(*range),
                "param" => match parse_param(arg) {
                    Some(entry) => doc.params.push(entry),
                    None => issues.malformed.push(*range),
                },
                "returns" | "return" if !policy.allow_params => {
                    issues.inapplicable.push(*range);
                }
                "returns" | "return" => match parse_braced_type(arg) {
                    Some(ty) => doc.returns = Some(ty),
                    None => issues.malformed.push(*range),
                },
                "kind" if !policy.allow_kind => issues.inapplicable.push(*range),
                "kind" => match ExternalKind::from_tag(arg) {
                    Some(kind) => doc.kind = Some(kind),
                    None => issues.malformed.push(*range),
                },
                // `@widget` is recognized but reserved for Tier 3, and unknown
                // tags are ignored leniently — both are no-ops at the MVP.
                _ => {}
            }
        } else if !line.is_empty() {
            free.push(line.to_string());
        }
    }

    if !free.is_empty() {
        doc.doc = Some(free.join("\n"));
    }
    let has_content =
        doc.doc.is_some() || !doc.params.is_empty() || doc.returns.is_some() || doc.kind.is_some();
    (has_content.then_some(doc), issues)
}

/// Parse a `@param` argument: `<name> {<type>}`.
fn parse_param(arg: &str) -> Option<(String, TypeRef)> {
    let mut it = arg.splitn(2, char::is_whitespace);
    let name = it.next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    let ty = parse_braced_type(it.next().unwrap_or("").trim())?;
    Some((name.to_string(), ty))
}

/// Parse a `{<type>}`-braced type reference. `None` if not well-formed.
fn parse_braced_type(s: &str) -> Option<TypeRef> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    if inner.is_empty() {
        return None;
    }
    Some(TypeRef(inner.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
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
