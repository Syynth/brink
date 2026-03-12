use crate::SyntaxKind::{EOF, HASH, L_BRACE, NEWLINE, TAG, TAG_LINE, TAGS};

use super::Parser;

/// Parse a tag-only line: `tags NEWLINE`.
///
/// ```text
/// tag_line = { tags ~ NEWLINE }
/// ```
pub(crate) fn tag_line(p: &mut Parser<'_, '_>) {
    p.start_node(TAG_LINE);
    tags(p);
    if p.at(NEWLINE) {
        p.bump();
    } else if !p.at_eof() {
        p.error("expected newline after tags".into());
    }
    p.finish_node();
}

/// Parse one or more tags: `#text #text ...`
///
/// ```text
/// tags = { tag+ }
/// tag  = { "#" ~ TAG_CHAR* }
/// TAG_CHAR = { !(NEWLINE | "#") ~ ANY }
/// ```
pub(crate) fn tags(p: &mut Parser<'_, '_>) {
    p.start_node(TAGS);

    while p.current() == HASH {
        tag(p);
    }

    p.finish_node();
}

/// Parse a single tag: `# text-until-# or newline`.
fn tag(p: &mut Parser<'_, '_>) {
    p.start_node(TAG);
    p.skip_ws();
    p.bump_assert(HASH); // the `#`

    // Consume everything until the next `#` or NEWLINE.
    // `{...}` blocks are parsed as inline logic for dynamic tag content.
    loop {
        if p.at_eof() {
            break;
        }
        match p.nth_raw(0) {
            HASH | NEWLINE | EOF => break,
            L_BRACE => {
                super::inline::inline_logic(p);
            }
            _ => p.bump(),
        }
    }

    p.finish_node();
}
