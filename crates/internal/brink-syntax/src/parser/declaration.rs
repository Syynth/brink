use crate::SyntaxKind::{
    COLON, COMMA, CONST_DECL, EQ, EXTERNAL_DECL, FILE_PATH, FUNCTION_PARAM_LIST, HASH, IDENT,
    IDENTIFIER, INCLUDE_STMT, INTEGER, KW_CONST, KW_EXTERNAL, KW_INCLUDE, KW_LIST, KW_VAR, L_BRACE,
    L_PAREN, LIST_DECL, LIST_DEF, LIST_MEMBER, LIST_MEMBER_OFF, LIST_MEMBER_ON, NEWLINE, R_BRACE,
    R_PAREN, STRUCT_DECL, STRUCT_FIELD_DECL, VAR_DECL,
};

use super::Parser;
use super::expression::skip_balanced;
use super::types::{at_type_annotation, type_annotation, type_expr};

/// Parse `INCLUDE filepath\n`.
///
/// ```text
/// include_statement = { "INCLUDE" ~ INLINE_WS+ ~ file_path }
/// file_path = { (!NEWLINE ~ ANY)+ }
/// ```
pub(crate) fn include_statement(p: &mut Parser<'_, '_>) {
    p.start_node(INCLUDE_STMT);
    p.bump(); // KW_INCLUDE
    p.skip_ws();

    // file_path: everything until newline
    p.start_node(FILE_PATH);
    if p.at_eof() || p.nth_raw(0) == NEWLINE {
        p.error("expected file path".into());
    }
    while !p.at_eof() && p.nth_raw(0) != NEWLINE {
        p.bump();
    }
    p.finish_node();

    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Parse `EXTERNAL ident(params)\n`.
///
/// ```text
/// external_declaration = { "EXTERNAL" ~ INLINE_WS+ ~ identifier ~ "(" ~ function_param_list? ~ ")" ~ NEWLINE }
/// ```
pub(crate) fn external_declaration(p: &mut Parser<'_, '_>) {
    p.start_node(EXTERNAL_DECL);
    p.bump(); // KW_EXTERNAL
    p.skip_ws();
    // Ink keywords are contextual — an external may be named after an operator
    // keyword (e.g. `EXTERNAL has(item)`). C# `IdentifierWithMetadata` imposes
    // no reserved-word check, so `has`/`and`/`mod`/etc. are valid names here.
    p.start_node(IDENTIFIER);
    p.expect_ident_or_keyword();
    p.finish_node();
    p.skip_ws();
    p.expect(L_PAREN);
    p.skip_ws();

    if p.at_ident_or_keyword() {
        function_param_list(p);
    }

    p.skip_ws();
    p.expect(R_PAREN);
    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Parse `function_param_list = { identifier ~ ("," ~ identifier)* }`.
fn function_param_list(p: &mut Parser<'_, '_>) {
    p.start_node(FUNCTION_PARAM_LIST);
    p.start_node(IDENTIFIER);
    p.bump(); // first identifier (IDENT or contextual keyword)
    p.finish_node();
    loop {
        p.skip_ws();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws();
        p.start_node(IDENTIFIER);
        p.expect_ident_or_keyword();
        p.finish_node();
    }
    p.finish_node();
}

/// Parse `VAR ident = expr\n`.
///
/// ```text
/// var_declaration = { "VAR" ~ INLINE_WS+ ~ identifier ~ INLINE_WS* ~ "=" ~ INLINE_WS* ~ expression ~ NEWLINE }
/// ```
pub(crate) fn var_declaration(p: &mut Parser<'_, '_>) {
    p.start_node(VAR_DECL);
    p.bump(); // KW_VAR
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    // Optional type annotation (TM-2, docs/typed-mode-spec.md §3):
    // `VAR name: type = expr`.
    if at_type_annotation(p) {
        type_annotation(p);
    }
    p.skip_ws();
    p.expect(EQ);
    p.skip_ws();
    super::expression::expression(p);
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Parse `CONST ident = expr\n`.
///
/// ```text
/// const_declaration = { "CONST" ~ INLINE_WS+ ~ identifier ~ type_annotation? ~ INLINE_WS* ~ "=" ~ INLINE_WS* ~ expression ~ NEWLINE }
/// ```
pub(crate) fn const_declaration(p: &mut Parser<'_, '_>) {
    p.start_node(CONST_DECL);
    p.bump(); // KW_CONST
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    // Optional type annotation (TM-2, docs/typed-mode-spec.md §3, "optional
    // anywhere"): `CONST name: type = expr`.
    if at_type_annotation(p) {
        type_annotation(p);
    }
    p.skip_ws();
    p.expect(EQ);
    p.skip_ws();
    super::expression::expression(p);
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Parse `LIST ident = list_def\n`.
///
/// ```text
/// list_declaration = { "LIST" ~ INLINE_WS+ ~ identifier ~ INLINE_WS* ~ "=" ~ INLINE_WS* ~ list_definition ~ NEWLINE }
/// list_definition = { list_member ~ ("," ~ list_member)* }
/// list_member = { list_member_on | list_member_off }
/// list_member_on = { "(" ~ identifier ~ ("=" ~ integer)? ~ ")" }
/// list_member_off = { identifier ~ ("=" ~ integer)? }
/// ```
pub(crate) fn list_declaration(p: &mut Parser<'_, '_>) {
    p.start_node(LIST_DECL);
    p.bump(); // KW_LIST
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    p.skip_ws();
    p.expect(EQ);
    p.skip_ws();
    list_definition(p);
    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

fn list_definition(p: &mut Parser<'_, '_>) {
    p.start_node(LIST_DEF);
    list_member(p);
    loop {
        p.skip_ws();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws();
        list_member(p);
    }
    p.finish_node();
}

fn list_member(p: &mut Parser<'_, '_>) {
    p.start_node(LIST_MEMBER);
    if p.current() == L_PAREN {
        // list_member_on: (ident) or (ident = int)
        p.start_node(LIST_MEMBER_ON);
        p.bump(); // (
        p.skip_ws();
        // Ink keywords are contextual — they may appear as list member names.
        // C# reference: InkParser_Logic.cs treats list item names as plain identifiers.
        p.expect_ident_or_keyword();
        p.skip_ws();
        if p.eat(EQ) {
            p.skip_ws();
            p.expect(INTEGER);
        }
        p.skip_ws();
        p.expect(R_PAREN);
        p.finish_node();
    } else {
        // list_member_off: ident or ident = int
        p.start_node(LIST_MEMBER_OFF);
        p.expect_ident_or_keyword();
        p.skip_ws();
        if p.eat(EQ) {
            p.skip_ws();
            p.expect(INTEGER);
        }
        p.finish_node();
    }
    p.finish_node();
}

/// Parse `STRUCT Name = #{ field: type, … }\n` (TM-4b,
/// docs/typed-mode-spec.md §6, decl syntax amended PR #622).
///
/// The body takes the same braced `#{…}` shape as the construction literal,
/// with types in value position — single-line legal, trailing comma
/// allowed. Top-level only (unlike `VAR`/`CONST`/`LIST`, which C# allows at
/// any statement level) — a struct shape is a project-wide type
/// declaration, not knot-body state.
///
/// ```text
/// struct_declaration = { "STRUCT" ~ IDENT ~ "=" ~ "#" ~ "{" ~ struct_field_decl_list? ~ "}" ~ NEWLINE }
/// struct_field_decl_list = { struct_field_decl ~ ("," ~ struct_field_decl)* ~ ","? }
/// struct_field_decl = { IDENT ~ ":" ~ type_expr }
/// ```
pub(crate) fn struct_declaration(p: &mut Parser<'_, '_>) {
    p.start_node(STRUCT_DECL);
    p.bump(); // "STRUCT" (IDENT, contextual — see `at_struct_decl`)
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    p.skip_ws();
    p.expect(EQ);
    p.skip_ws();
    p.expect(HASH);
    p.skip_ws();
    p.bump_assert(L_BRACE);
    if p.at_depth_limit() {
        p.error("nesting depth limit exceeded".into());
        skip_balanced(p, L_BRACE, R_BRACE);
        p.finish_node();
        return;
    }
    p.depth += 1;
    skip_struct_body_trivia(p);
    if p.current() != R_BRACE {
        struct_field_decl(p);
        loop {
            skip_struct_body_trivia(p);
            if !p.eat(COMMA) {
                break;
            }
            skip_struct_body_trivia(p);
            if p.current() == R_BRACE {
                break; // trailing comma
            }
            struct_field_decl(p);
        }
    }
    skip_struct_body_trivia(p);
    p.expect(R_BRACE);
    p.depth -= 1;
    p.skip_ws();
    if p.at(NEWLINE) {
        p.bump();
    }
    p.finish_node();
}

/// Consume whitespace/comment trivia interleaved with `NEWLINE`s — unlike
/// every other brace-delimited construct in this grammar (`#{…}` map
/// literals, `~ { … }` blocks), a `STRUCT` declaration's body is legal
/// across multiple physical lines (docs/typed-mode-spec.md §6: "single-line
/// form is legal for short structs"; multi-line is the common case). Plain
/// `Parser::skip_ws` never eats `NEWLINE` (it terminates lines/delimits
/// blocks everywhere else in the grammar) — this is the one place a
/// `NEWLINE` between two struct-body tokens is itself just layout. Every
/// `NEWLINE` consumed here still lands as a token in the `STRUCT_DECL`
/// subtree, so the source round-trips losslessly.
fn skip_struct_body_trivia(p: &mut Parser<'_, '_>) {
    loop {
        p.skip_ws();
        if p.current() == NEWLINE {
            p.bump();
        } else {
            break;
        }
    }
}

/// `field : type_expr` — one field of a `STRUCT` declaration's body.
fn struct_field_decl(p: &mut Parser<'_, '_>) {
    p.start_node(STRUCT_FIELD_DECL);
    p.start_node(IDENTIFIER);
    p.expect_ident_or_keyword();
    p.finish_node();
    p.skip_ws();
    p.expect(COLON);
    p.skip_ws();
    type_expr(p);
    p.finish_node();
}

/// Returns `true` if the current token starts a `STRUCT` declaration
/// (TM-4b) — a contextual (soft) keyword check like T1b's `if`/`while`/
/// `for`: `STRUCT` stays a plain `IDENT` everywhere else, so an existing
/// knot/variable/function named `STRUCT` is byte-for-byte unaffected. Full
/// four-token lookahead (`STRUCT` `IDENT` `=` `#{`) disambiguates from any
/// other use of the bare word, mirroring how `at_struct_literal` needs no
/// such lookahead (its shape is unambiguous in expression position).
pub(crate) fn at_struct_decl(p: &Parser<'_, '_>) -> bool {
    p.at_kw_text("STRUCT")
        && p.nth(1) == IDENT
        && p.nth(2) == EQ
        && p.nth(3) == HASH
        && p.nth(4) == L_BRACE
}

/// Returns `true` if the current token starts a declaration.
pub(crate) fn at_declaration(p: &Parser<'_, '_>) -> bool {
    matches!(
        p.current(),
        KW_INCLUDE | KW_EXTERNAL | KW_VAR | KW_CONST | KW_LIST
    )
}

/// Returns `true` if the current token starts a declaration that can appear
/// inside a knot/stitch body. C# treats `VAR`, `CONST`, and `LIST` as
/// valid at all statement levels — they don't terminate the body.
pub(crate) fn at_inline_declaration(p: &Parser<'_, '_>) -> bool {
    matches!(p.current(), KW_VAR | KW_CONST | KW_LIST)
}

/// Dispatch to the correct declaration parser.
pub(crate) fn declaration(p: &mut Parser<'_, '_>) {
    match p.current() {
        KW_INCLUDE => include_statement(p),
        KW_EXTERNAL => external_declaration(p),
        KW_VAR => var_declaration(p),
        KW_CONST => const_declaration(p),
        KW_LIST => list_declaration(p),
        _ => {
            p.error("expected declaration".into());
        }
    }
}
