//! Parsing of `///` JSDoc-style doc-comments on `EXTERNAL` declarations.
//!
//! Comments are trivia in the green tree (no grammar involvement), so we walk
//! the tokens immediately preceding a declaration to collect the contiguous
//! block of `///` lines, then parse the `@param` / `@returns` / `@kind` tags
//! into an [`ExternalDoc`]. Like Rust doc-comments, codegen ignores these —
//! only the analyzer/IDE consume them.

use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use crate::{ExternalDoc, ExternalKind, TypeRef};

/// Parse the `///` doc-comment block preceding `node` into an [`ExternalDoc`].
///
/// Returns the doc (if any tags or text were found) plus the source ranges of
/// any malformed tags, for the caller to diagnose.
#[must_use]
pub fn parse_external_doc(node: &SyntaxNode) -> (Option<ExternalDoc>, Vec<TextRange>) {
    let lines = collect_doc_lines(node);
    if lines.is_empty() {
        return (None, Vec::new());
    }
    parse_lines(&lines)
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

/// Parse collected `///` lines into an [`ExternalDoc`], returning the ranges of
/// malformed tags.
fn parse_lines(lines: &[(String, TextRange)]) -> (Option<ExternalDoc>, Vec<TextRange>) {
    let mut doc = ExternalDoc::default();
    let mut free: Vec<String> = Vec::new();
    let mut malformed: Vec<TextRange> = Vec::new();

    for (line, range) in lines {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('@') {
            let mut it = rest.splitn(2, char::is_whitespace);
            let tag = it.next().unwrap_or("");
            let arg = it.next().unwrap_or("").trim();
            match tag {
                "param" => match parse_param(arg) {
                    Some(entry) => doc.params.push(entry),
                    None => malformed.push(*range),
                },
                "returns" | "return" => match parse_braced_type(arg) {
                    Some(ty) => doc.returns = Some(ty),
                    None => malformed.push(*range),
                },
                "kind" => match ExternalKind::from_tag(arg) {
                    Some(kind) => doc.kind = Some(kind),
                    None => malformed.push(*range),
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
    (has_content.then_some(doc), malformed)
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

    /// Parse `src`, find the first `EXTERNAL_DECL`, and parse its doc.
    fn doc_of(src: &str) -> (Option<ExternalDoc>, Vec<TextRange>) {
        let parsed = parse(src);
        let node = parsed
            .syntax()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::EXTERNAL_DECL)
            .expect("source should contain an EXTERNAL declaration");
        parse_external_doc(&node)
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
        let (doc, malformed) = doc_of(src);
        let doc = doc.expect("doc present");
        assert!(malformed.is_empty(), "no malformed tags: {malformed:?}");
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
        let (doc, malformed) = doc_of("EXTERNAL plain(x)\n");
        assert!(doc.is_none());
        assert!(malformed.is_empty());
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
        let (doc, malformed) = doc_of(src);
        let doc = doc.expect("doc present");
        assert_eq!(malformed.len(), 1, "the bad @param is reported");
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
        let (doc, malformed) = doc_of(src);
        let doc = doc.expect("doc present");
        assert!(
            malformed.is_empty(),
            "unknown/widget tags ignored, not malformed"
        );
        assert_eq!(
            doc.params,
            vec![("c".to_string(), TypeRef("color".to_string()))]
        );
    }
}
