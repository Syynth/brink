use crate::SyntaxKind::{
    self, AMP_AMP, BANG, BANG_EQ, BANG_QUESTION, BOOLEAN_LIT, CARET, COMMA, DIVERT,
    DIVERT_TARGET_EXPR, DOT, EOF, EQ_EQ, FLOAT, FLOAT_LIT, FUNCTION_CALL, GT, GT_EQ, IDENT,
    IDENTIFIER, INFIX_EXPR, INTEGER, INTEGER_LIT, KW_AND, KW_FALSE, KW_HAS, KW_HASNT, KW_MOD,
    KW_NOT, KW_OR, KW_TRUE, L_BRACE, L_PAREN, LIST_EXPR, LT, LT_EQ, MINUS, MINUS_EQ, NEWLINE,
    PAREN_EXPR, PERCENT, PIPE_PIPE, PLUS, PLUS_EQ, POSTFIX_EXPR, PREFIX_EXPR, QUESTION, QUOTE,
    R_PAREN, SLASH, STAR, STRING_LIT,
};

use super::Parser;

/// Precedence levels for Pratt parsing (higher binds tighter).
///
/// Bare `=` is NOT in the Pratt table -- assignment is handled by `logic_line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Prec {
    None = 0,
    Assign = 1,     // +=, -=
    Or = 2,         // or, ||
    And = 3,        // and, &&
    Equality = 4,   // ==, !=
    Comparison = 5, // <, >, <=, >=
    HasOps = 6,     // has, hasnt, ?, !?
    Add = 7,        // +, -
    Mul = 8,        // *, /, %, mod
    Intersect = 9,  // ^ (list intersection in ink, not power)
    Prefix = 10,    // -, !, not (unary)
                    // Postfix (++, --) is handled without a Prec value — bumped directly in the loop.
}

/// Return the infix precedence and whether the operator is right-associative.
fn infix_binding_power(kind: SyntaxKind) -> Option<(Prec, bool)> {
    Some(match kind {
        // Assignment compound ops
        PLUS_EQ | MINUS_EQ => (Prec::Assign, true),
        // Logical or
        PIPE_PIPE | KW_OR => (Prec::Or, false),
        // Logical and
        AMP_AMP | KW_AND => (Prec::And, false),
        // Equality
        EQ_EQ | BANG_EQ => (Prec::Equality, false),
        // Comparison
        LT | GT | LT_EQ | GT_EQ => (Prec::Comparison, false),
        // Has / hasnt
        QUESTION | BANG_QUESTION | KW_HAS | KW_HASNT => (Prec::HasOps, false),
        // Additive
        PLUS | MINUS => (Prec::Add, false),
        // Multiplicative
        STAR | SLASH | PERCENT | KW_MOD => (Prec::Mul, false),
        // List intersection (right-associative)
        CARET => (Prec::Intersect, true),
        _ => return None,
    })
}

/// Returns `true` if `kind` is a prefix operator.
fn is_prefix_op(kind: SyntaxKind) -> bool {
    matches!(kind, MINUS | BANG | KW_NOT)
}

/// Parse an expression using Pratt parsing.
pub(crate) fn expression(p: &mut Parser<'_>) {
    expression_bp(p, Prec::None);
}

/// Pratt expression parser core.
fn expression_bp(p: &mut Parser<'_>, min_bp: Prec) {
    let checkpoint = p.checkpoint();

    // -- Prefix --
    if is_prefix_op(p.current()) {
        // For MINUS, only treat as prefix if not followed by `>` (that would be DIVERT,
        // but the lexer handles this already by emitting DIVERT for `->`)
        p.start_node_at(checkpoint, PREFIX_EXPR);
        p.skip_ws();
        p.bump(); // operator
        p.skip_ws();
        expression_bp(p, Prec::Prefix);
        p.finish_node();
    } else {
        // -- Atom --
        if !atom(p) {
            return;
        }
    }

    loop {
        p.skip_ws();

        // -- Postfix --
        // Both `++` and `--` are two adjacent tokens (no whitespace between them).
        // The lexer emits individual PLUS/MINUS tokens; the parser detects pairs.
        if p.current() == PLUS && p.nth_raw(1) == PLUS {
            p.start_node_at(checkpoint, POSTFIX_EXPR);
            p.bump(); // first +
            p.bump(); // second +
            p.finish_node();
            continue;
        }
        if p.current() == MINUS && p.nth_raw(1) == MINUS {
            p.start_node_at(checkpoint, POSTFIX_EXPR);
            p.bump(); // first -
            p.bump(); // second -
            p.finish_node();
            continue;
        }

        // -- Infix --
        let Some((prec, right_assoc)) = infix_binding_power(p.current()) else {
            break;
        };

        // For MINUS, ensure it's not `->` (DIVERT) -- already handled by lexer
        // For PLUS, ensure it's not `+=` -- already handled by lexer

        if prec < min_bp {
            break;
        }
        if prec == min_bp && !right_assoc {
            break;
        }

        p.start_node_at(checkpoint, INFIX_EXPR);
        p.skip_ws();
        p.bump(); // operator
        p.skip_ws();
        expression_bp(p, prec);
        p.finish_node();
    }
}

/// Parse an atom. Returns `false` if no atom was found.
fn atom(p: &mut Parser<'_>) -> bool {
    match p.current() {
        // Parenthesized expression or list expression
        // list_expr = ( dotted_id, dotted_id, ... ) -- needs lookahead
        // paren_expr = ( expression )
        // function_call = ident ( args ) -- handled below under IDENT
        L_PAREN => {
            // Disambiguate: list_expr if `(` is followed by `)` or by ident with comma/dot
            // before any operator. We use a simple heuristic: if after `(` we see
            // IDENT DOT or IDENT COMMA or `)`, it's a list_expr.
            if looks_like_list_expr(p) {
                list_expr(p);
            } else {
                paren_expr(p);
            }
            true
        }

        // Divert target expression: `-> target` (in expression context)
        DIVERT => {
            divert_target_expr(p);
            true
        }

        // Identifier -- could be function_call or plain identifier
        IDENT => {
            // function_call = ident ( args )
            if p.nth(1) == L_PAREN {
                function_call(p);
            } else {
                // dotted_identifier
                super::divert::dotted_identifier(p);
            }
            true
        }

        // Literals
        INTEGER => {
            p.start_node(INTEGER_LIT);
            p.bump();
            p.finish_node();
            true
        }
        FLOAT => {
            p.start_node(FLOAT_LIT);
            p.bump();
            p.finish_node();
            true
        }
        QUOTE => {
            string_literal(p);
            true
        }
        KW_TRUE | KW_FALSE => {
            p.start_node(BOOLEAN_LIT);
            p.bump();
            p.finish_node();
            true
        }

        _ => false,
    }
}

/// Heuristic: does `(` start a list expression rather than a paren expression?
///
/// `list_expr` = `(` `dotted_id` (`,` `dotted_id`)* `)`  or  `(` `)`
/// We look for: `( )` or `( IDENT [DOT IDENT]* , ...`
fn looks_like_list_expr(p: &Parser<'_>) -> bool {
    // `()` is an empty list
    if p.nth(1) == R_PAREN {
        return true;
    }

    // Scan forward from nth(1) past dotted_identifier to see if next is , or )
    let mut i = 1;
    if p.nth(i) != IDENT {
        return false;
    }
    i += 1;
    // Skip dots
    while p.nth(i) == DOT && p.nth(i + 1) == IDENT {
        i += 2;
    }
    matches!(p.nth(i), COMMA | R_PAREN)
}

/// Parse `( expr )`.
fn paren_expr(p: &mut Parser<'_>) {
    p.start_node(PAREN_EXPR);
    p.bump(); // (
    p.skip_ws();
    expression(p);
    p.skip_ws();
    p.expect(R_PAREN);
    p.finish_node();
}

/// Parse `( dotted_id, dotted_id, ... )`.
fn list_expr(p: &mut Parser<'_>) {
    p.start_node(LIST_EXPR);
    p.bump(); // (
    p.skip_ws();
    if p.current() != R_PAREN {
        super::divert::dotted_identifier(p);
        loop {
            p.skip_ws();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws();
            super::divert::dotted_identifier(p);
        }
    }
    p.skip_ws();
    p.expect(R_PAREN);
    p.finish_node();
}

/// Parse `-> dotted_identifier` (in expression context, for divert target values).
fn divert_target_expr(p: &mut Parser<'_>) {
    p.start_node(DIVERT_TARGET_EXPR);
    p.bump(); // DIVERT `->`
    p.skip_ws();
    super::divert::dotted_identifier(p);
    p.finish_node();
}

/// Parse `ident ( arg_list? )`.
fn function_call(p: &mut Parser<'_>) {
    p.start_node(FUNCTION_CALL);
    // The identifier node
    p.start_node(IDENTIFIER);
    p.bump(); // IDENT
    p.finish_node();
    p.skip_ws();
    p.bump(); // (
    p.skip_ws();
    if p.current() != R_PAREN {
        super::divert::arg_list(p);
    }
    p.skip_ws();
    p.expect(R_PAREN);
    p.finish_node();
}

/// Parse a string literal: `"` `string_part`* `"`.
fn string_literal(p: &mut Parser<'_>) {
    p.start_node(STRING_LIT);
    p.bump(); // opening QUOTE

    loop {
        match p.nth_raw(0) {
            QUOTE => {
                p.bump(); // closing QUOTE
                break;
            }
            L_BRACE => {
                // Inline logic inside string interpolation
                super::inline::inline_logic(p);
            }
            NEWLINE | EOF => {
                p.error("unterminated string literal".into());
                break;
            }
            _ => {
                // STRING_TEXT, STRING_ESCAPE, or any other token inside the string
                p.bump();
            }
        }
    }

    p.finish_node();
}
