use crate::SyntaxKind::{
    COMMA, CONST_DECL, EQ, EXTERNAL_DECL, FILE_PATH, FUNCTION_PARAM_LIST, IDENT, IDENTIFIER,
    INCLUDE_STMT, INTEGER, KW_CONST, KW_EXTERNAL, KW_INCLUDE, KW_LIST, KW_VAR, L_PAREN, LIST_DECL,
    LIST_DEF, LIST_MEMBER, LIST_MEMBER_OFF, LIST_MEMBER_ON, NEWLINE, R_PAREN, VAR_DECL,
};

use super::Parser;

/// Parse `INCLUDE filepath\n`.
///
/// ```text
/// include_statement = { "INCLUDE" ~ INLINE_WS+ ~ file_path }
/// file_path = { (!NEWLINE ~ ANY)+ }
/// ```
pub(crate) fn include_statement(p: &mut Parser<'_>) {
    p.start_node(INCLUDE_STMT);
    p.bump(); // KW_INCLUDE
    p.skip_ws();

    // file_path: everything until newline
    p.start_node(FILE_PATH);
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
pub(crate) fn external_declaration(p: &mut Parser<'_>) {
    p.start_node(EXTERNAL_DECL);
    p.bump(); // KW_EXTERNAL
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
    p.skip_ws();
    p.expect(L_PAREN);
    p.skip_ws();

    if p.current() == IDENT {
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
fn function_param_list(p: &mut Parser<'_>) {
    p.start_node(FUNCTION_PARAM_LIST);
    p.start_node(IDENTIFIER);
    p.bump(); // first identifier
    p.finish_node();
    loop {
        p.skip_ws();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws();
        p.start_node(IDENTIFIER);
        p.expect(IDENT);
        p.finish_node();
    }
    p.finish_node();
}

/// Parse `VAR ident = expr\n`.
///
/// ```text
/// var_declaration = { "VAR" ~ INLINE_WS+ ~ identifier ~ INLINE_WS* ~ "=" ~ INLINE_WS* ~ expression ~ NEWLINE }
/// ```
pub(crate) fn var_declaration(p: &mut Parser<'_>) {
    p.start_node(VAR_DECL);
    p.bump(); // KW_VAR
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
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
pub(crate) fn const_declaration(p: &mut Parser<'_>) {
    p.start_node(CONST_DECL);
    p.bump(); // KW_CONST
    p.skip_ws();
    p.start_node(IDENTIFIER);
    p.expect(IDENT);
    p.finish_node();
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
pub(crate) fn list_declaration(p: &mut Parser<'_>) {
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

fn list_definition(p: &mut Parser<'_>) {
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

fn list_member(p: &mut Parser<'_>) {
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

/// Returns `true` if the current token starts a declaration.
pub(crate) fn at_declaration(p: &Parser<'_>) -> bool {
    matches!(
        p.current(),
        KW_INCLUDE | KW_EXTERNAL | KW_VAR | KW_CONST | KW_LIST
    )
}

/// Returns `true` if the current token starts a declaration that can appear
/// inside a knot/stitch body. C# treats `VAR`, `CONST`, and `LIST` as
/// valid at all statement levels — they don't terminate the body.
pub(crate) fn at_inline_declaration(p: &Parser<'_>) -> bool {
    matches!(p.current(), KW_VAR | KW_CONST | KW_LIST)
}

/// Dispatch to the correct declaration parser.
pub(crate) fn declaration(p: &mut Parser<'_>) {
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
