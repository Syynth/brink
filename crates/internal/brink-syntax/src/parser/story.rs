use crate::SyntaxKind::{
    AUTHOR_WARNING, EMPTY_LINE, HASH, KW_TODO, L_BRACE, MINUS, NEWLINE, PLUS, R_BRACE, SOURCE_FILE,
    STAR, STRAY_CLOSING_BRACE, TILDE, WHITESPACE,
};

use super::Parser;

/// Parse the entire source file.
pub(crate) fn source_file(p: &mut Parser<'_>) {
    p.start_node(SOURCE_FILE);

    while !p.at_eof() {
        p.skip_ws();

        if p.at_eof() {
            break;
        }

        let before = p.pos();
        top_level_statement(p);
        if p.pos() == before {
            // No progress — skip the stuck token to avoid infinite loop
            p.error_recover("unexpected token");
        }
    }

    p.finish_node();
}

/// Dispatch a single top-level statement.
fn top_level_statement(p: &mut Parser<'_>) {
    if super::knot::at_knot(p) {
        super::knot::knot_definition(p);
        return;
    }
    if super::knot::at_stitch(p) {
        super::knot::stitch_definition(p);
        return;
    }
    if super::declaration::at_declaration(p) {
        super::declaration::declaration(p);
        return;
    }
    line(p);
}

/// Parse a single line (used by both top-level and knot/stitch bodies).
///
/// ```text
/// line = { empty_line | author_warning | logic_line | multiline_block
///        | choice | gather_line | stray_closing_brace | tag_line | content_line }
/// ```
pub(crate) fn line(p: &mut Parser<'_>) {
    match p.current() {
        NEWLINE => {
            p.start_node(EMPTY_LINE);
            p.bump();
            p.finish_node();
        }
        HASH => {
            super::tag::tag_line(p);
        }
        KW_TODO => {
            author_warning(p);
        }
        R_BRACE => {
            stray_closing_brace(p);
        }
        TILDE => {
            super::logic::logic_line(p);
        }
        STAR | PLUS => {
            super::choice::choice(p);
        }
        MINUS => {
            super::gather::gather_line(p);
        }
        L_BRACE if is_multiline_block(p) => {
            super::inline::multiline_block(p);
            // Consume trailing newline after `}`
            if p.at(NEWLINE) {
                p.bump();
            }
        }
        _ => {
            super::content::content_line(p);
        }
    }
}

/// Check if `{` starts a multiline block (followed by NEWLINE after optional whitespace).
/// We use `nth_raw` to look at raw tokens including whitespace.
fn is_multiline_block(p: &Parser<'_>) -> bool {
    let mut i = 1; // skip past L_BRACE at nth_raw(0)
    loop {
        match p.nth_raw(i) {
            WHITESPACE => i += 1,
            NEWLINE => return true,
            _ => return false,
        }
    }
}

/// Parse `TODO: text\n`.
fn author_warning(p: &mut Parser<'_>) {
    p.start_node(AUTHOR_WARNING);
    p.bump(); // KW_TODO
    // Consume everything until newline
    while !p.at_eof() && p.nth_raw(0) != NEWLINE {
        p.bump();
    }
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Parse a stray `}` on its own line.
fn stray_closing_brace(p: &mut Parser<'_>) {
    p.start_node(STRAY_CLOSING_BRACE);
    p.skip_ws();
    p.bump(); // R_BRACE
    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}
