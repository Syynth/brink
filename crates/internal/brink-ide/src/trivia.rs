//! Trivia facet — per-line comment/tag classification (#463).
//!
//! Comments are CST/source-text facts, not HIR structure: the HIR never
//! sees them. This facet computes them standalone so the structural
//! line view (`line_context`) can *compose* them instead of interleaving
//! source scans with its structural passes — the layered-architecture
//! split (`docs/editor-hir-overlay-spec.md` §1a).

use brink_syntax::SyntaxNode;

use crate::LineIndex;

/// Trivia facts for one source line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineTrivia {
    /// The line is a `//` line comment, or lies inside a `/* ... */` block
    /// comment.
    pub comment: bool,
    /// The line lies inside a `/* ... */` block comment.
    pub block_comment: bool,
    /// The line is a standalone tag line (`# ...`) — and not a comment.
    pub tag: bool,
}

/// Compute per-line trivia from the source text and syntax tree.
///
/// `line_count` fixes the output length (the caller owns the trailing-newline
/// convention). Line comments and tag lines come from a source scan; block
/// comments from `BLOCK_COMMENT` tokens in the CST — a block comment wins
/// over a tag sigil on the same line.
#[must_use]
pub fn line_trivia(source: &str, root: &SyntaxNode, line_count: usize) -> Vec<LineTrivia> {
    let mut trivia = vec![LineTrivia::default(); line_count];

    // ── Line comments and standalone tag lines (source scan) ──
    for (i, line) in source.lines().enumerate() {
        if i >= trivia.len() {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            trivia[i].comment = true;
        } else if trimmed.starts_with('#') && !trimmed.is_empty() {
            trivia[i].tag = true;
        }
    }

    // ── Block comments (`/* ... */`) from the syntax tree ──
    let idx = LineIndex::new(source);
    for token in root.descendants_with_tokens() {
        if let Some(token) = token.as_token()
            && token.kind() == brink_syntax::SyntaxKind::BLOCK_COMMENT
        {
            let range = token.text_range();
            let start_line = idx.line_col(range.start()).0 as usize;
            let end_line = idx.line_col(range.end()).0 as usize;
            for line in start_line..=end_line {
                if let Some(t) = trivia.get_mut(line) {
                    t.comment = true;
                    t.block_comment = true;
                }
            }
        }
    }

    trivia
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trivia_for(source: &str) -> Vec<LineTrivia> {
        let parse = brink_syntax::parse(source);
        line_trivia(source, &parse.syntax(), source.lines().count().max(1))
    }

    #[test]
    fn line_comment_and_tag_line() {
        let t = trivia_for("// comment\n# a tag\nplain\n");
        assert!(t[0].comment && !t[0].tag && !t[0].block_comment);
        assert!(t[1].tag && !t[1].comment);
        assert_eq!(t[2], LineTrivia::default());
    }

    #[test]
    fn block_comment_spans_lines_and_wins_over_tag_sigil() {
        let t = trivia_for("/* one\n# looks like a tag\ntwo */\nafter\n");
        for (line, lt) in t.iter().enumerate().take(3) {
            assert!(lt.comment && lt.block_comment, "line {line}");
        }
        assert!(!t[3].comment && !t[3].block_comment);
    }
}
