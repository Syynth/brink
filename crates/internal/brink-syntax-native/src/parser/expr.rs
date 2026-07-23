//! A minimal expression grammar shared by interpolation content,
//! annotation-adjacent positions, choice guards, divert targets, and
//! `var`/`const` initializers.
//!
//! B0.8 Wave A (`docs/decision-log.md` 2026-07-23 "Code-ground sitting")
//! adds the statement layer (`let`/assignment/expression-statements/
//! blocks-as-values, `super::stmt`) over this skeleton, including a new
//! `L_BRACE` atom case below for block-expressions. `if`/`while`/`for`/
//! `until`-statements and UFCS *resolution* (the call shape already
//! parses) remain B0.8 Wave B (`docs/b0-sequencing.md` §B0.8) — this stays
//! the shared expression *skeleton*, not the statement grammar itself.

use crate::SyntaxKind::{
    AMP_AMP, ARG_LIST, BANG, BANG_EQ, BOOLEAN_LIT, CALL_EXPR, COLON_COLON, COMMA, DOT, EQ_EQ,
    FLOAT, FLOAT_LIT, GT, GT_EQ, IDENT, INFIX_EXPR, INTEGER, INTEGER_LIT, KW_FALSE, KW_TRUE,
    L_BRACE, L_PAREN, LAMBDA_EXPR, LAMBDA_PARAMS, LT, LT_EQ, MINUS, PAREN_EXPR, PATH, PATH_EXPR,
    PATH_SEGMENT, PERCENT, PIPE, PLUS, PREFIX_EXPR, QUOTE, R_PAREN, SLASH, STAR, STRING_ESCAPE,
    STRING_LIT, STRING_TEXT,
};

use super::Parser;

/// Precedence levels for Pratt parsing (higher binds tighter). Assignment
/// is not in this table — no statement grammar lives here (see module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Prec {
    None = 0,
    Or = 1,         // ||
    And = 2,        // &&
    Equality = 3,   // ==, !=
    Comparison = 4, // <, >, <=, >=
    Add = 5,        // +, -
    Mul = 6,        // *, /, %
    Prefix = 7,     // -, ! (unary)
}

impl Prec {
    /// The minimum binding power a right-hand side must have to be pulled
    /// into *this* operator's recursive parse, i.e. `self as u8 + 1`.
    ///
    /// Every infix operator in this grammar is left-associative (Pratt
    /// convention: recurse on strictly higher precedence than the operator
    /// just consumed). Reusing `self` unchanged here would instead pull a
    /// second operator at the *same* precedence into the RHS, producing
    /// right-associative parses for symmetric-precedence operators (`-`,
    /// `/`, `%`, `<`, `>`, `<=`, `>=`, `==`, `!=`, `&&`, `||`) — invisible
    /// for `+`/`*` since they're mathematically associative, but silently
    /// wrong for `-`/`/` (`10 - 3 - 2` would group as `10 - (3 - 2)` = 9
    /// instead of `(10 - 3) - 2` = 5). Saturates at `Prefix`, the highest
    /// level: nothing binds tighter, so no further operator is ever pulled
    /// in past a prefix expression either.
    fn next(self) -> Prec {
        match self {
            Prec::None => Prec::Or,
            Prec::Or => Prec::And,
            Prec::And => Prec::Equality,
            Prec::Equality => Prec::Comparison,
            Prec::Comparison => Prec::Add,
            Prec::Add => Prec::Mul,
            Prec::Mul | Prec::Prefix => Prec::Prefix,
        }
    }
}

fn infix_binding_power(kind: crate::SyntaxKind) -> Option<Prec> {
    Some(match kind {
        AMP_AMP => Prec::And,
        EQ_EQ | BANG_EQ => Prec::Equality,
        LT | GT | LT_EQ | GT_EQ => Prec::Comparison,
        PLUS | MINUS => Prec::Add,
        STAR | SLASH | PERCENT => Prec::Mul,
        _ => return None,
    })
}

fn is_prefix_op(kind: crate::SyntaxKind) -> bool {
    matches!(kind, MINUS | BANG)
}

/// Parse an expression using Pratt parsing.
pub(crate) fn expression(p: &mut Parser<'_, '_>) {
    expression_bp(p, Prec::None);
}

fn expression_bp(p: &mut Parser<'_, '_>, min_bp: Prec) {
    if !p.enter_depth() {
        return;
    }

    // Every `atom()` branch below (`INTEGER_LIT`/`FLOAT_LIT`/`BOOLEAN_LIT`)
    // does a raw `bump()` rather than `expect()`/`eat()` (it needs to open
    // the literal-wrapper node first) — so this function, not `atom`
    // itself, is responsible for guaranteeing `pos` is already aligned to
    // a real token, not pending trivia, before that happens.
    p.skip_ws();
    let checkpoint = p.checkpoint();

    if is_prefix_op(p.current()) {
        p.start_node_at(checkpoint, PREFIX_EXPR);
        p.skip_ws();
        p.bump(); // operator
        p.skip_ws();
        expression_bp(p, Prec::Prefix);
        p.finish_node();
    } else if !atom(p) {
        p.exit_depth();
        return;
    }

    loop {
        p.skip_ws();

        // `||` — two adjacent PIPE tokens, not a compound lexer token
        // (mirrors `brink-syntax`'s `||`/`++`/`--` precedent).
        if p.current() == PIPE && p.nth_raw(1) == PIPE {
            if (Prec::Or as u8) < (min_bp as u8) {
                break;
            }
            p.start_node_at(checkpoint, INFIX_EXPR);
            p.bump();
            p.bump();
            p.skip_ws();
            expression_bp(p, Prec::Or.next());
            p.finish_node();
            continue;
        }

        let Some(prec) = infix_binding_power(p.current()) else {
            break;
        };
        if (prec as u8) < (min_bp as u8) {
            break;
        }

        p.start_node_at(checkpoint, INFIX_EXPR);
        p.bump(); // operator
        p.skip_ws();
        expression_bp(p, prec.next());
        p.finish_node();
    }

    p.exit_depth();
}

/// Returns `true` if an atom was parsed (and `false`, with an error
/// recorded, if the current token can't start an expression at all).
fn atom(p: &mut Parser<'_, '_>) -> bool {
    match p.current() {
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
        KW_TRUE | KW_FALSE => {
            p.start_node(BOOLEAN_LIT);
            p.bump();
            p.finish_node();
            true
        }
        QUOTE => {
            string_lit(p);
            true
        }
        L_PAREN => {
            paren_expr(p);
            true
        }
        PIPE => {
            lambda_expr(p);
            true
        }
        IDENT => {
            path_or_call(p);
            true
        }
        // A statement-block used as a value (blocks-as-values ruled,
        // B0.8 Wave A, `docs/decision-log.md` 2026-07-23 "Code-ground
        // sitting"): `let x = { let y = 1; y + 1 };` — the block-expr
        // layer lives in `super::stmt`, not here (this crate's expression
        // *skeleton* stays the shared Pratt core; the statement layer
        // rides on top of it, per `parser/stmt.rs`'s module doc).
        L_BRACE => {
            super::stmt::stmt_block(p);
            true
        }
        _ => {
            p.error(format!("expected an expression, found {:?}", p.current()));
            false
        }
    }
}

fn paren_expr(p: &mut Parser<'_, '_>) {
    p.start_node(PAREN_EXPR);
    p.expect(L_PAREN);
    expression(p);
    p.expect(R_PAREN);
    p.finish_node();
}

/// `|x, y| expr` — lambda pipes, tokenized and structurally parsed;
/// lowering is explicitly deferred (charter §7/§8, b0-sequencing §B0.5:
/// "B0.5 tokenizes pipes; B0.8 does not lower them").
fn lambda_expr(p: &mut Parser<'_, '_>) {
    p.start_node(LAMBDA_EXPR);
    lambda_params(p);
    p.skip_ws();
    expression(p);
    p.finish_node();
}

fn lambda_params(p: &mut Parser<'_, '_>) {
    p.start_node(LAMBDA_PARAMS);
    p.expect(PIPE);
    while !p.at(PIPE) && !p.at_eof() {
        let before = p.pos();
        p.expect(IDENT);
        if p.pos() == before {
            p.error_recover("unexpected token in lambda parameter list");
            continue;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    p.expect(PIPE);
    p.finish_node();
}

fn path_or_call(p: &mut Parser<'_, '_>) {
    let checkpoint = p.checkpoint();
    path(p);
    p.skip_ws();
    if p.at(L_PAREN) {
        p.start_node_at(checkpoint, CALL_EXPR);
        arg_list(p);
        p.finish_node();
    } else {
        p.start_node_at(checkpoint, PATH_EXPR);
        p.finish_node();
    }
}

/// A dotted/`::`-separated name path (charter §13.2): `::` crosses module
/// walls, `.` walks everything inside.
pub(crate) fn path(p: &mut Parser<'_, '_>) {
    p.start_node(PATH);
    path_segment(p);
    while p.eat(DOT) || p.eat(COLON_COLON) {
        path_segment(p);
    }
    p.finish_node();
}

fn path_segment(p: &mut Parser<'_, '_>) {
    p.start_node(PATH_SEGMENT);
    p.expect(IDENT);
    p.finish_node();
}

/// `(expr, expr, …)`.
pub(crate) fn arg_list(p: &mut Parser<'_, '_>) {
    p.start_node(ARG_LIST);
    p.expect(L_PAREN);
    p.skip_ws_and_newlines();
    while p.peek_skip_nl() != R_PAREN && !p.at_eof() {
        let before = p.pos();
        expression(p);
        if p.pos() == before {
            p.error_recover("unexpected token in argument list");
            p.skip_ws_and_newlines();
            continue;
        }
        p.skip_ws_and_newlines();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws_and_newlines();
    }
    p.expect(R_PAREN);
    p.finish_node();
}

/// A quoted string literal. May contain `{expr}` interpolation, reusing
/// the same `INTERPOLATION` node prose content uses (charter's bare-brace
/// rule is universal: "and nothing else, ever").
pub(crate) fn string_lit(p: &mut Parser<'_, '_>) {
    p.start_node(STRING_LIT);
    p.expect(QUOTE);
    loop {
        match p.current() {
            STRING_TEXT | STRING_ESCAPE => p.bump(),
            crate::SyntaxKind::L_BRACE => super::content::interpolation(p),
            _ => break,
        }
    }
    p.expect(QUOTE);
    p.finish_node();
}
