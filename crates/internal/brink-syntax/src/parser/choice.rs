use crate::SyntaxKind::{
    BACKSLASH, BLOCK_COMMENT, CHOICE, CHOICE_BRACKET_CONTENT, CHOICE_BULLETS, CHOICE_CONDITION,
    CHOICE_INNER_CONTENT, CHOICE_START_CONTENT, DIVERT, EOF, ESCAPE, GLUE, GLUE_NODE, HASH, IDENT,
    IDENTIFIER, L_BRACE, L_BRACKET, L_PAREN, LABEL, LINE_COMMENT, MINUS, NEWLINE, PIPE, PLUS,
    R_BRACE, R_BRACKET, R_PAREN, STAR, TEXT, THREAD, TUNNEL_ONWARDS,
};

use super::Parser;

/// Parse a choice line.
///
/// ```text
/// choice = {
///     choice_bullets
///   ~ label?
///   ~ (NEWLINE ~ &(!NEWLINE ~ ANY))?
///   ~ choice_condition*
///   ~ choice_start_content?
///   ~ choice_bracket_content?
///   ~ choice_inner_content?
///   ~ divert?
///   ~ tags?
///   ~ NEWLINE
/// }
/// ```
pub(crate) fn choice(p: &mut Parser<'_>) {
    p.start_node(CHOICE);

    // choice_bullets: (WS* ~ ("*" | "+"))+
    choice_bullets(p);
    p.skip_ws();

    // Optional label: (ident)
    if p.current() == L_PAREN && p.nth(1) == IDENT && p.nth(2) == R_PAREN {
        label(p);
        p.skip_ws();

        // Optional newline after label (if next line has content)
        if p.current() == NEWLINE && !matches!(p.nth(1), NEWLINE | EOF) {
            p.bump(); // NEWLINE
        }
    }

    // Optional conditions: { expr }*
    while p.current() == L_BRACE {
        choice_condition(p);
        p.skip_ws();
    }

    // choice_start_content: content elements before [
    if at_choice_content(p) && p.current() != L_BRACKET {
        choice_start_content(p);
    }

    // choice_bracket_content: [ content ]
    if p.current() == L_BRACKET {
        choice_bracket_content(p);
    }

    // choice_inner_content: content elements after ]
    if at_choice_content(p) {
        choice_inner_content(p);
    }

    // Optional trailing divert
    p.skip_ws();
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
        p.error("expected newline after choice".into());
    }

    p.finish_node();
}

/// Parse choice bullets: one or more `*` or `+` (with optional whitespace).
fn choice_bullets(p: &mut Parser<'_>) {
    p.start_node(CHOICE_BULLETS);
    while matches!(p.current(), STAR | PLUS) {
        p.bump();
        p.skip_ws();
    }
    p.finish_node();
}

/// Parse a label: `( ident )`.
/// Used by both choices and gathers.
pub(crate) fn label(p: &mut Parser<'_>) {
    p.start_node(LABEL);
    p.bump(); // L_PAREN
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    p.skip_ws();
    p.expect(R_PAREN);
    p.finish_node();
}

/// Parse choice condition: `{ expr }`.
fn choice_condition(p: &mut Parser<'_>) {
    p.start_node(CHOICE_CONDITION);
    p.bump(); // L_BRACE
    p.skip_ws();
    super::expression::expression(p);
    p.skip_ws();
    p.expect(R_BRACE);
    p.finish_node();
}

/// Parse content before the bracket.
fn choice_start_content(p: &mut Parser<'_>) {
    p.start_node(CHOICE_START_CONTENT);
    choice_content_elements(p);
    p.finish_node();
}

/// Parse bracketed content: `[ content ]`.
fn choice_bracket_content(p: &mut Parser<'_>) {
    p.start_node(CHOICE_BRACKET_CONTENT);
    p.bump(); // L_BRACKET
    while p.current() != R_BRACKET && !matches!(p.current(), NEWLINE | EOF) {
        let before = p.pos();
        choice_content_element(p);
        if p.pos() == before {
            // No progress — consume the stuck token to avoid infinite loop
            p.bump();
        }
    }
    if p.current() == R_BRACKET {
        p.bump();
    } else {
        p.error("expected `]`".into());
    }
    p.finish_node();
}

/// Parse content after the bracket.
fn choice_inner_content(p: &mut Parser<'_>) {
    p.start_node(CHOICE_INNER_CONTENT);
    choice_content_elements(p);
    p.finish_node();
}

/// Parse choice content elements until a stop character.
fn choice_content_elements(p: &mut Parser<'_>) {
    while at_choice_content(p) && p.current() != L_BRACKET {
        let before = p.pos();
        choice_content_element(p);
        // Safety: if no progress was made, break to avoid infinite loop.
        // This can happen when a token (e.g. `]`) is accepted by
        // `at_choice_content` but not consumed by `choice_content_element`.
        if p.pos() == before {
            break;
        }
    }
}

/// Parse a single choice content element.
fn choice_content_element(p: &mut Parser<'_>) {
    match p.current() {
        L_BRACE => {
            super::inline::inline_logic(p);
        }
        GLUE => {
            p.start_node(GLUE_NODE);
            p.bump();
            p.finish_node();
        }
        BACKSLASH if !matches!(p.nth(1), NEWLINE | EOF) => {
            p.start_node(ESCAPE);
            p.bump(); // backslash
            p.bump(); // escaped char
            p.finish_node();
        }
        _ => {
            choice_text(p);
        }
    }
}

/// Parse a run of choice text characters.
fn choice_text(p: &mut Parser<'_>) {
    p.start_node(TEXT);
    loop {
        if p.at_eof() {
            break;
        }
        match p.nth_raw(0) {
            NEWLINE | L_BRACKET | R_BRACKET | L_BRACE | R_BRACE | GLUE | DIVERT
            | TUNNEL_ONWARDS | LINE_COMMENT | BLOCK_COMMENT | HASH | BACKSLASH | EOF | THREAD => {
                break;
            }
            _ => p.bump(),
        }
    }
    p.finish_node();
}

/// Returns `true` if we're at a choice content character.
fn at_choice_content(p: &Parser<'_>) -> bool {
    !matches!(
        p.current(),
        NEWLINE | EOF | HASH | DIVERT | TUNNEL_ONWARDS | THREAD | PIPE | MINUS | R_BRACE
    )
}
