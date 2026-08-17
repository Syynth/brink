use crate::SyntaxKind::{
    COMMA, DIVERT, EQ, EQ_EQ, GT, IDENTIFIER, KNOT_BODY, KNOT_DEF, KNOT_HEADER, KNOT_PARAM_DECL,
    KNOT_PARAMS, KW_FUNCTION, KW_REF, L_PAREN, NEWLINE, R_PAREN, STITCH_BODY, STITCH_DEF,
    STITCH_HEADER,
};

use super::Parser;
use super::types::{at_type_annotation, type_annotation};

/// Returns `true` if we're at a knot header (`== ...`).
pub(crate) fn at_knot(p: &Parser<'_, '_>) -> bool {
    p.current() == EQ_EQ
}

/// Returns `true` if we're at a stitch header (`= ...` but not `==` or `=>`).
///
/// Both lookaheads use `nth` (trivia-skipping) rather than `nth_raw` because
/// `current()` already skips trivia without advancing `pos` — so `nth_raw(1)`
/// could see a trivia token rather than the token actually following `=`.
pub(crate) fn at_stitch(p: &Parser<'_, '_>) -> bool {
    p.current() == EQ && p.nth(1) != EQ && p.nth(1) != GT
}

/// Parse a knot definition.
///
/// ```text
/// knot_definition = { knot_header ~ NEWLINE ~ knot_body }
/// ```
pub(crate) fn knot_definition(p: &mut Parser<'_, '_>) {
    p.start_node(KNOT_DEF);
    knot_header(p);
    if p.at(NEWLINE) {
        p.bump();
    }
    knot_body(p);
    p.finish_node();
}

/// Parse a knot header.
///
/// ```text
/// knot_header = { "==" ~ "="* ~ INLINE_WS* ~ ("function" ~ INLINE_WS+)? ~ identifier
///                 ~ INLINE_WS* ~ knot_params? ~ INLINE_WS* ~ ("==" ~ "="*)? }
/// ```
fn knot_header(p: &mut Parser<'_, '_>) {
    p.start_node(KNOT_HEADER);
    // Opening equals: `==` followed by optional extra `=` or `==` tokens
    p.bump(); // first EQ_EQ
    eat_extra_equals(p);
    p.skip_ws();

    // Optional `function` keyword
    if p.current() == KW_FUNCTION {
        p.bump();
        p.skip_ws();
    }

    // Knot name
    p.start_node(IDENTIFIER);
    p.expect_ident_or_keyword();
    p.finish_node();
    p.skip_ws();

    // Optional params
    if p.current() == L_PAREN {
        knot_params(p);
        p.skip_ws();
    }

    // Optional return type annotation (TM-2, docs/typed-mode-spec.md §3):
    // `): type ===` in function-header return position.
    if at_type_annotation(p) {
        type_annotation(p);
        p.skip_ws();
    }

    // Optional trailing equals
    if p.current() == EQ_EQ || p.current() == EQ {
        eat_extra_equals(p);
    }

    p.finish_node();
}

/// Parse knot parameters: `( param, param, ... )`
///
/// ```text
/// knot_params = { "(" ~ knot_param_decl_list? ~ ")" }
/// knot_param_decl = { ("ref" ~)? ~ identifier }
/// ```
fn knot_params(p: &mut Parser<'_, '_>) {
    p.start_node(KNOT_PARAMS);
    p.bump(); // L_PAREN
    p.skip_ws();

    if p.current() != R_PAREN {
        knot_param_decl(p);
        loop {
            p.skip_ws();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws();
            knot_param_decl(p);
        }
    }

    p.skip_ws();
    p.expect(R_PAREN);
    p.finish_node();
}

fn knot_param_decl(p: &mut Parser<'_, '_>) {
    p.start_node(KNOT_PARAM_DECL);
    // `->` before a param declares a divert-type parameter (the caller passes
    // a divert target rather than a value).  C# ref: `FlowDecl` in
    // `InkParser_Knots.cs`.
    if p.current() == DIVERT {
        p.bump();
        p.skip_ws();
    }
    if p.current() == KW_REF {
        p.bump();
        p.skip_ws();
    }
    p.start_node(IDENTIFIER);
    p.expect_ident_or_keyword();
    p.finish_node();
    // Optional param type annotation (TM-2, docs/typed-mode-spec.md §3):
    // `name: type`. `at_type_annotation` only peeks (non-destructive), so
    // the non-annotated case consumes nothing extra here — unchanged CST
    // for every pre-existing (unannotated) param.
    if at_type_annotation(p) {
        type_annotation(p);
    }
    p.finish_node();
}

/// Parse the body of a knot (lines + stitches until the next knot or EOF).
///
/// VAR, CONST, and LIST declarations are parsed inline (C# allows them at
/// any statement level). Only INCLUDE and EXTERNAL terminate the body.
fn knot_body(p: &mut Parser<'_, '_>) {
    p.start_node(KNOT_BODY);
    loop {
        p.skip_ws();
        if p.at_eof() || at_knot(p) {
            break;
        }
        // INCLUDE/EXTERNAL terminate the body (but not VAR/CONST/LIST)
        if super::declaration::at_declaration(p) && !super::declaration::at_inline_declaration(p) {
            break;
        }
        let before = p.pos();
        if at_stitch(p) {
            stitch_definition(p);
        } else if super::declaration::at_inline_declaration(p) {
            super::declaration::declaration(p);
        } else {
            super::story::line(p);
        }
        if p.pos() == before {
            p.error_recover("unexpected token in knot body");
        }
    }
    p.finish_node();
}

/// Parse a stitch definition.
///
/// ```text
/// stitch_definition = { stitch_header ~ NEWLINE ~ stitch_body }
/// ```
pub(crate) fn stitch_definition(p: &mut Parser<'_, '_>) {
    p.start_node(STITCH_DEF);
    stitch_header(p);
    if p.at(NEWLINE) {
        p.bump();
    }
    stitch_body(p);
    p.finish_node();
}

/// Parse a stitch header.
///
/// ```text
/// stitch_header = { "=" ~ !("=" | ">") ~ INLINE_WS* ~ identifier ~ INLINE_WS* ~ knot_params?
///                   ~ INLINE_WS* ~ type_annotation? }
/// ```
///
/// Two things here previously read differently from what the code below
/// does (#2695) — verified by driving the parser, not by inferring from this
/// comment:
///
/// - Whitespace after `=` is **optional** (`INLINE_WS*`, not `+`): the body
///   is `p.bump()` then `p.skip_ws()`, and `skip_ws` consumes zero or more
///   trivia tokens. `=c` (no space before the name) parses as a stitch.
/// - The `!("=" | ">")` lookahead is **trivia-skipping**, via `at_stitch`'s
///   use of `p.nth(1)` (see its doc comment). So whitespace between `=` and
///   the excluded character doesn't rescue it: `= > j` and `= = k` are
///   excluded exactly like the tight `=>`/`==` forms are, not just those.
///
/// Whether stitch headers *should* require whitespace after `=` is a
/// parser-semantics question this comment does not decide — `at_stitch` is
/// unchanged here. The in-tree corpus (`tests/tier{1,2,3}/**/story.ink`)
/// contains no whitespace-free stitch header to settle it either way.
fn stitch_header(p: &mut Parser<'_, '_>) {
    p.start_node(STITCH_HEADER);
    p.bump(); // EQ (we already checked it's not `==` or `=>`)
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect_ident_or_keyword();
    p.finish_node();
    p.skip_ws();

    if p.current() == L_PAREN {
        knot_params(p);
        p.skip_ws();
    }

    // Optional return type annotation (NG-C, issue #1489, widened to
    // stitches by #1509): `= name(params): type`. Same TM-2 grammar
    // position as `knot_header`'s — a stitch header just has no trailing
    // `===` to eat afterward.
    if at_type_annotation(p) {
        type_annotation(p);
    }

    p.finish_node();
}

/// Parse the body of a stitch (lines until next stitch, knot, or EOF).
///
/// Same inline-declaration logic as `knot_body`.
fn stitch_body(p: &mut Parser<'_, '_>) {
    p.start_node(STITCH_BODY);
    loop {
        p.skip_ws();
        if p.at_eof() || at_knot(p) || at_stitch(p) {
            break;
        }
        if super::declaration::at_declaration(p) && !super::declaration::at_inline_declaration(p) {
            break;
        }
        let before = p.pos();
        if super::declaration::at_inline_declaration(p) {
            super::declaration::declaration(p);
        } else {
            super::story::line(p);
        }
        if p.pos() == before {
            p.error_recover("unexpected token in stitch body");
        }
    }
    p.finish_node();
}

/// Consume any mix of `EQ_EQ` and `EQ` tokens (for knot header equals runs).
fn eat_extra_equals(p: &mut Parser<'_, '_>) {
    while p.current() == EQ_EQ || p.current() == EQ {
        p.bump();
    }
}
