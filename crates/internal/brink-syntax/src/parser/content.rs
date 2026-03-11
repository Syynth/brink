use crate::SyntaxKind::{
    BACKSLASH, BLOCK_COMMENT, CONTENT_LINE, DIVERT, EOF, ESCAPE, GLUE, GLUE_NODE, HASH, L_BRACE,
    LINE_COMMENT, MIXED_CONTENT, NEWLINE, PIPE, R_BRACE, TEXT, THREAD, TUNNEL_ONWARDS,
};

use super::Parser;

/// Parse a content line: mixed content and/or divert, optional tags, then newline.
///
/// ```text
/// content_line = { mixed_content ~ divert? ~ tags? ~ NEWLINE }
/// ```
///
/// A content line may start with mixed content, a divert, or both (text then divert).
pub(crate) fn content_line(p: &mut Parser<'_, '_>) {
    p.start_node(CONTENT_LINE);

    // If we start at a divert, skip mixed_content and go straight to divert
    if super::divert::at_divert(p) {
        super::divert::divert(p);
        // After a tunnel call `-> target ->`, the `->->` remains as a separate
        // divert token. Parse it as a second divert on the same content line.
        if super::divert::at_divert(p) {
            super::divert::divert(p);
        }
    } else {
        mixed_content(p);
        // After mixed content, check for a trailing divert
        if super::divert::at_divert(p) {
            super::divert::divert(p);
            // Same tunnel-call trailing `->->` handling
            if super::divert::at_divert(p) {
                super::divert::divert(p);
            }
        }
    }

    if p.current() == HASH {
        super::tag::tags(p);
    }
    if p.at(NEWLINE) {
        p.bump();
    } else if !p.at_eof() {
        p.error("expected newline at end of content line".into());
    }
    p.finish_node();
}

/// Parse mixed content: a sequence of content elements (text, glue, inline
/// logic, escapes).
///
/// ```text
/// mixed_content = { content_element+ }
/// content_element = { inline_logic | glue | content_escape | text_content }
/// ```
pub(crate) fn mixed_content(p: &mut Parser<'_, '_>) {
    p.start_node(MIXED_CONTENT);

    loop {
        match p.current() {
            // Stop characters for mixed content
            NEWLINE | EOF | HASH | DIVERT | TUNNEL_ONWARDS | THREAD => break,

            // Glue `<>`
            GLUE => {
                p.skip_ws(); // flush trivia before the GLUE token
                p.start_node(GLUE_NODE);
                p.bump();
                p.finish_node();
            }

            // Content escape: `\` followed by non-newline
            BACKSLASH if !matches!(p.nth(1), NEWLINE | EOF) => {
                p.start_node(ESCAPE);
                p.bump(); // backslash
                p.bump(); // escaped char
                p.finish_node();
            }

            // Inline logic `{ ... }` -- dispatch to inline parser
            L_BRACE => {
                // If there's trivia (whitespace) between the previous element
                // and this `{`, flush it as a TEXT node first. `p.current()`
                // skips trivia, so whitespace-only runs between `}` and `{`
                // would otherwise be silently dropped.
                if p.nth_raw(0) == L_BRACE {
                    super::inline::inline_logic(p);
                } else {
                    text_content(p);
                }
            }

            // Everything else is text content
            _ => {
                let before = p.pos();
                text_content(p);
                if p.pos() == before {
                    break;
                }
            }
        }
    }

    p.finish_node();
}

/// Parse a run of text tokens into a `TEXT` node.
///
/// The lexer emits fine-grained tokens for text (IDENT, WHITESPACE, DOT, etc.).
/// The parser collects runs of non-structural tokens until hitting a stop
/// character for the current text context.
///
/// Stop characters for `text_content` (from pest `TEXT_CHAR`):
/// NEWLINE, `{`, `}`, `<>`, `->`, `->->`, `<-`, `//`, `/*`, `#`, `|`, `\`
fn text_content(p: &mut Parser<'_, '_>) {
    p.start_node(TEXT);

    loop {
        if p.at_eof() {
            break;
        }
        match p.nth_raw(0) {
            // Stop characters
            NEWLINE | L_BRACE | R_BRACE | GLUE | DIVERT | TUNNEL_ONWARDS | THREAD
            | LINE_COMMENT | BLOCK_COMMENT | HASH | PIPE | BACKSLASH | EOF => break,
            // Everything else is text
            _ => p.bump(),
        }
    }

    p.finish_node();
}
