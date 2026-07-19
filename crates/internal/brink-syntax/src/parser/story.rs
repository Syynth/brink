use crate::SyntaxKind::{
    ANNOTATION_LINE, AT_L_BRACKET, AUTHOR_WARNING, EMPTY_LINE, HASH, IDENT, KW_TODO, L_BRACE,
    L_PAREN, MINUS, NEWLINE, PLUS, R_BRACE, R_BRACKET, R_PAREN, SOURCE_FILE, STAR,
    STRAY_CLOSING_BRACE, TILDE, WHITESPACE,
};

use super::Parser;

/// Parse the entire source file.
pub(crate) fn source_file(p: &mut Parser<'_, '_>) {
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
fn top_level_statement(p: &mut Parser<'_, '_>) {
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
    // TM-4b (docs/typed-mode-spec.md §6): `STRUCT Name = #{ … }` is a
    // contextual top-level declaration (`STRUCT` stays a plain `IDENT`
    // everywhere else) — checked after the hard-keyword declarations above,
    // same position T1b's contextual block keywords would occupy if they
    // were ever top-level.
    if super::declaration::at_struct_decl(p) {
        super::declaration::struct_declaration(p);
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
pub(crate) fn line(p: &mut Parser<'_, '_>) {
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
        AT_L_BRACKET => {
            annotation_line(p);
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
fn is_multiline_block(p: &Parser<'_, '_>) -> bool {
    let mut i = 1; // skip past L_BRACE at nth_raw(0)
    loop {
        match p.nth_raw(i) {
            WHITESPACE => i += 1,
            NEWLINE => return true,
            _ => return false,
        }
    }
}

/// Parse an annotation line (NS-A2, the `@[effects(…)]` assertion final
/// form — `docs/stdlib-spec.md` §9.2):
///
/// ```text
/// annotation_line = { "@[" ~ IDENT ~ ("(" ~ arg_tokens ~ ")")? ~ "]" ~ NEWLINE }
/// ```
///
/// The argument tokens are kept flat inside the node (the raw text between
/// the balanced parens is re-read by `brink-ir`'s directive recognizer, the
/// same string-level contract the `#@…` tag channel uses). Superset-parsed
/// under every dialect; `strict-ink` rejection (E051) and argument-grammar
/// validation live in `brink-ir`/`brink-analyzer`.
fn annotation_line(p: &mut Parser<'_, '_>) {
    p.start_node(ANNOTATION_LINE);
    p.bump(); // AT_L_BRACKET
    p.skip_ws();
    if p.at(IDENT) {
        p.bump();
    } else {
        p.error("expected an annotation name after `@[`".into());
    }
    p.skip_ws();
    if p.at(L_PAREN) {
        p.bump();
        let mut depth = 1usize;
        while depth > 0 && !p.at_eof() && p.nth_raw(0) != NEWLINE {
            match p.nth_raw(0) {
                L_PAREN => depth += 1,
                R_PAREN => depth -= 1,
                _ => {}
            }
            p.bump();
        }
        if depth > 0 {
            p.error("unclosed `(` in annotation arguments".into());
        }
    }
    p.skip_ws();
    if p.at(R_BRACKET) {
        p.bump();
    } else {
        p.error("expected `]` to close the annotation".into());
    }
    // Anything else on the line is unexpected — consume to newline so the
    // parser stays line-synchronized (matches `author_warning`'s recovery).
    let mut trailing = false;
    while !p.at_eof() && p.nth_raw(0) != NEWLINE {
        if !matches!(p.nth_raw(0), WHITESPACE) {
            trailing = true;
        }
        p.bump();
    }
    if trailing {
        p.error("unexpected text after `]` on an annotation line".into());
    }
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Parse `TODO: text\n`.
fn author_warning(p: &mut Parser<'_, '_>) {
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
fn stray_closing_brace(p: &mut Parser<'_, '_>) {
    p.start_node(STRAY_CLOSING_BRACE);
    p.skip_ws();
    p.bump(); // R_BRACE
    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}
