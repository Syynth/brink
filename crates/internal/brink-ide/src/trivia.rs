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
#[expect(
    clippy::struct_excessive_bools,
    reason = "a per-line fact record — each flag is an independent trivia \
              classification, not a state machine to encode as an enum"
)]
pub struct LineTrivia {
    /// The line is a `//` line comment, or lies inside a `/* ... */` block
    /// comment.
    pub comment: bool,
    /// The line lies inside a `/* ... */` block comment.
    pub block_comment: bool,
    /// The line is a standalone tag line (`# ...`) — and not a comment.
    pub tag: bool,
    /// The line is a `TODO:` author note (`AUTHOR_WARNING`, #3050). Ink
    /// only — the native surface has no `TODO` construct, so
    /// [`line_trivia_native`] never sets it.
    pub todo: bool,
}

/// Compute per-line trivia from the source text and syntax tree.
///
/// `line_count` fixes the output length (the caller owns the trailing-newline
/// convention). Line comments and tag lines come from a source scan; block
/// comments from `BLOCK_COMMENT` tokens in the CST — a block comment wins
/// over a tag sigil on the same line.
#[must_use]
pub fn line_trivia(source: &str, root: &SyntaxNode, line_count: usize) -> Vec<LineTrivia> {
    let mut trivia = line_trivia_from_source(source, line_count);

    // ── Block comments (`/* ... */`) from the syntax tree ──
    let idx = LineIndex::new(source);
    for token in root.descendants_with_tokens() {
        if let Some(token) = token.as_token()
            && token.kind() == brink_syntax::SyntaxKind::BLOCK_COMMENT
        {
            mark_block_comment(&mut trivia, &idx, token.text_range());
        }
    }

    // ── `TODO:` author notes (#3050) from the syntax tree ──
    // Single-line by grammar (`author_warning` consumes through NEWLINE);
    // mark the node's start line.
    for node in root.descendants() {
        if node.kind() == brink_syntax::SyntaxKind::AUTHOR_WARNING {
            let line = idx.line_col(node.text_range().start()).0 as usize;
            if let Some(t) = trivia.get_mut(line) {
                t.todo = true;
            }
        }
    }

    trivia
}

/// The native (`.brink`) sibling of [`line_trivia`] (issue #2291) — same
/// source-scan pass for `//` line comments and `#` tags (native shares
/// ink's lexical convention for both, per the lexer/`TAG`/`TAG_LINE`
/// `SyntaxKind`s), but block comments are read from
/// `brink_syntax_native::SyntaxKind::BLOCK_COMMENT` tokens in the *native*
/// CST rather than ink's. Never feed this a `brink_syntax::SyntaxNode` root
/// parsed from native text (ink's grammar over native source) — see
/// `IdeSession::syntax_root`'s doc comment.
#[must_use]
pub fn line_trivia_native(
    source: &str,
    root: &brink_syntax_native::SyntaxNode,
    line_count: usize,
) -> Vec<LineTrivia> {
    let mut trivia = line_trivia_from_source(source, line_count);

    let idx = LineIndex::new(source);
    for token in root.descendants_with_tokens() {
        if let Some(token) = token.as_token()
            && token.kind() == brink_syntax_native::SyntaxKind::BLOCK_COMMENT
        {
            mark_block_comment(&mut trivia, &idx, token.text_range());
        }
    }

    trivia
}

/// Shared source-scan pass: `//` line comments and standalone `#` tag
/// lines. Frontend-agnostic — both dialects share this lexical convention.
fn line_trivia_from_source(source: &str, line_count: usize) -> Vec<LineTrivia> {
    let mut trivia = vec![LineTrivia::default(); line_count];

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

    trivia
}

/// Mark every line a block-comment token's range spans as `comment` +
/// `block_comment`. Shared by [`line_trivia`] and [`line_trivia_native`] —
/// takes only a `TextRange`, so it has no dependency on which CST the token
/// came from.
fn mark_block_comment(trivia: &mut [LineTrivia], idx: &LineIndex, range: rowan::TextRange) {
    let start_line = idx.line_col(range.start()).0 as usize;
    let end_line = idx.line_col(range.end()).0 as usize;
    for line in start_line..=end_line {
        if let Some(t) = trivia.get_mut(line) {
            t.comment = true;
            t.block_comment = true;
        }
    }
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

    // ── #2291: `line_trivia_native` must read block comments from the real
    // native CST, not garble them by ink-parsing native source text. ──────

    fn native_trivia_for(source: &str) -> Vec<LineTrivia> {
        let parse = brink_syntax_native::parse(source);
        line_trivia_native(source, &parse.syntax(), source.lines().count().max(1))
    }

    #[test]
    fn native_line_comment_and_tag_line() {
        let t = native_trivia_for("// comment\n# a tag\nplain\n");
        assert!(t[0].comment && !t[0].tag && !t[0].block_comment);
        assert!(t[1].tag && !t[1].comment);
        assert_eq!(t[2], LineTrivia::default());
    }

    #[test]
    fn native_block_comment_spans_lines() {
        // A `.brink` file with a `flow` body — proves `line_trivia_native`
        // finds the block comment by walking the *native* CST's own
        // `BLOCK_COMMENT` tokens end to end, not merely by sharing the
        // source-scan pass with `line_trivia`.
        let src = "flow main() {\n/* one\ntwo */\nafter\n}\n";
        let t = native_trivia_for(src);
        assert!(t[1].comment && t[1].block_comment, "{t:?}");
        assert!(t[2].comment && t[2].block_comment, "{t:?}");
        assert!(!t[3].comment && !t[3].block_comment, "{t:?}");
    }
}
