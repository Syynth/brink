use crate::SyntaxKind::{
    DIVERT, EOF, GATHER, GATHER_DASHES, HASH, IDENT, L_PAREN, MINUS, NEWLINE, PLUS, R_PAREN, STAR,
    THREAD, TUNNEL_ONWARDS,
};

use super::Parser;

/// Parse a gather line.
///
/// ```text
/// gather_line = { gather_dashes ~ label? ~ (choice | mixed_content?) ~ divert? ~ tags? ~ NEWLINE }
/// ```
///
/// When `*` or `+` follows the dashes (and optional label), the gather embeds
/// an inline choice — e.g. `- * hello` is a gather with a choice on the same line.
pub(crate) fn gather_line(p: &mut Parser<'_, '_>) {
    p.start_node(GATHER);

    // gather_dashes: (WS* ~ "-" ~ !">")+
    gather_dashes(p);
    p.skip_ws();

    // Optional label: (ident)
    if p.current() == L_PAREN && p.nth(1) == IDENT && p.nth(2) == R_PAREN {
        super::choice::label(p);
        p.skip_ws();
    }

    // Inline choice on the same line (e.g. `- * hello`)
    if matches!(p.current(), STAR | PLUS) {
        super::choice::choice(p);
    } else {
        // Optional mixed content
        if !matches!(
            p.current(),
            NEWLINE | EOF | HASH | DIVERT | TUNNEL_ONWARDS | THREAD
        ) {
            super::content::mixed_content(p);
        }

        // Optional divert at the end of a gather line
        if super::divert::at_divert(p) {
            super::divert::divert(p);
        }

        // Optional tags
        if p.current() == HASH {
            super::tag::tags(p);
        }

        // Trailing newline
        if p.at(NEWLINE) {
            p.bump();
        } else if !p.at_eof() {
            p.error("expected newline after gather".into());
        }
    }

    p.finish_node();
}

/// Parse gather dashes: one or more `-` (with optional whitespace).
fn gather_dashes(p: &mut Parser<'_, '_>) {
    p.start_node(GATHER_DASHES);
    while p.current() == MINUS {
        p.bump();
        p.skip_ws();
    }
    p.finish_node();
}
