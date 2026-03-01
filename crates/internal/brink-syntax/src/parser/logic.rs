use crate::SyntaxKind::{
    ASSIGNMENT, BLOCK_LOGIC_LINE, EOF, EQ, IDENT, IDENTIFIER, KW_RETURN, KW_TEMP, LOGIC_LINE,
    MINUS_EQ, NEWLINE, PLUS_EQ, RETURN_STMT, TEMP_DECL,
};

use super::Parser;

/// Parse a logic line: `~ statement NEWLINE`.
///
/// ```text
/// logic_line = { "~" ~ (return_statement | temp_declaration | assignment | expression) ~ NEWLINE }
/// ```
pub(crate) fn logic_line(p: &mut Parser<'_>) {
    p.start_node(LOGIC_LINE);
    p.bump(); // TILDE
    p.skip_ws();

    match p.current() {
        KW_RETURN => return_statement(p),
        KW_TEMP => temp_declaration(p),
        IDENT if is_assignment_ahead(p) => assignment(p),
        _ => {
            // Bare expression
            super::expression::expression(p);
        }
    }

    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    } else if !p.at_eof() {
        p.error("expected newline after logic line".into());
    }
    p.finish_node();
}

/// Parse a logic line inside a block (no trailing NEWLINE consumption).
/// Used by multiline branches and branchless bodies.
///
/// ```text
/// block_logic_line = { "~" ~ (return_statement | temp_declaration | assignment | expression) }
/// ```
pub(crate) fn block_logic_line(p: &mut Parser<'_>) {
    p.start_node(BLOCK_LOGIC_LINE);
    p.bump(); // TILDE
    p.skip_ws();

    match p.current() {
        KW_RETURN => return_statement(p),
        KW_TEMP => temp_declaration(p),
        IDENT if is_assignment_ahead(p) => assignment(p),
        _ => {
            super::expression::expression(p);
        }
    }

    p.skip_ws();
    p.finish_node();
}

/// Check if the identifier is followed by an assignment operator (=, +=, -=).
/// We must not confuse `=` in `== knot ==` or bare `=` in stitch headers.
fn is_assignment_ahead(p: &Parser<'_>) -> bool {
    let next = p.nth(1);
    matches!(next, EQ | PLUS_EQ | MINUS_EQ) && !(next == EQ && p.nth(2) == EQ)
}

/// Parse `return expr?`.
fn return_statement(p: &mut Parser<'_>) {
    p.start_node(RETURN_STMT);
    p.bump(); // KW_RETURN
    p.skip_ws();
    // Optional expression
    if !matches!(p.current(), NEWLINE | EOF) {
        super::expression::expression(p);
    }
    p.finish_node();
}

/// Parse `temp ident = expr`.
fn temp_declaration(p: &mut Parser<'_>) {
    p.start_node(TEMP_DECL);
    p.bump(); // KW_TEMP
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    p.skip_ws();
    assignment_op(p);
    p.skip_ws();
    super::expression::expression(p);
    p.finish_node();
}

/// Parse `ident op= expr`.
fn assignment(p: &mut Parser<'_>) {
    p.start_node(ASSIGNMENT);
    super::divert::path(p);
    p.skip_ws();
    assignment_op(p);
    p.skip_ws();
    super::expression::expression(p);
    p.finish_node();
}

/// Consume an assignment operator: `=`, `+=`, or `-=`.
/// Bare `=` must not be `==`.
fn assignment_op(p: &mut Parser<'_>) {
    match p.current() {
        PLUS_EQ | MINUS_EQ | EQ => {
            p.bump();
        }
        _ => {
            p.error("expected assignment operator".into());
        }
    }
}
