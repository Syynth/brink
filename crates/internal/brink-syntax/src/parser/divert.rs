use crate::SyntaxKind::{
    ARG_LIST, COMMA, DIVERT, DIVERT_CHAIN, DIVERT_NODE, DIVERT_TARGET_WITH_ARGS, DOT,
    DOTTED_IDENTIFIER, IDENT, KW_DONE, KW_END, L_PAREN, R_PAREN, THREAD, THREAD_START,
    TUNNEL_CALL_NODE, TUNNEL_ONWARDS, TUNNEL_ONWARDS_NODE,
};

use super::Parser;

/// Returns `true` if the current token starts a divert construct.
pub(crate) fn at_divert(p: &Parser<'_>) -> bool {
    matches!(p.current(), DIVERT | TUNNEL_ONWARDS | THREAD)
}

/// Parse a divert node: `thread_start` | `tunnel_onwards` | `divert_chain`.
///
/// ```text
/// divert = { thread_start | tunnel_onwards | divert_chain }
/// ```
pub(crate) fn divert(p: &mut Parser<'_>) {
    p.start_node(DIVERT_NODE);
    match p.current() {
        THREAD => thread_start(p),
        TUNNEL_ONWARDS => tunnel_onwards(p),
        DIVERT => divert_chain(p),
        _ => {
            p.error("expected divert".into());
        }
    }
    p.finish_node();
}

/// Parse `<- target(args?)`.
///
/// ```text
/// thread_start = { "<-" ~ dotted_identifier ~ ("(" ~ arg_list? ~ ")")? }
/// ```
fn thread_start(p: &mut Parser<'_>) {
    p.start_node(THREAD_START);
    p.bump(); // THREAD token `<-`
    p.skip_ws();
    dotted_identifier(p);
    p.skip_ws();
    if p.current() == L_PAREN {
        p.bump(); // (
        p.skip_ws();
        if p.current() != R_PAREN {
            arg_list(p);
        }
        p.skip_ws();
        p.expect(R_PAREN);
    }
    p.finish_node();
}

/// Parse `->->` optionally followed by a divert chain.
///
/// ```text
/// tunnel_onwards = { "->->" ~ divert_chain? }
/// ```
fn tunnel_onwards(p: &mut Parser<'_>) {
    p.start_node(TUNNEL_ONWARDS_NODE);
    p.bump(); // TUNNEL_ONWARDS token `->->`
    p.skip_ws();
    if p.current() == DIVERT {
        divert_chain(p);
    }
    p.finish_node();
}

/// Parse `-> target(args?) (-> target(args?))*`.
///
/// Detects tunnel call syntax: if the chain ends with a trailing `->` after
/// at least one target, wraps in `TUNNEL_CALL_NODE` instead of `DIVERT_CHAIN`.
///
/// ```text
/// divert_chain = { "->" ~ divert_target_with_args? ~ ("->" ~ divert_target_with_args?)* }
/// tunnel_call  = trailing `->` after targets -> `TUNNEL_CALL_NODE`
/// ```
fn divert_chain(p: &mut Parser<'_>) {
    let checkpoint = p.checkpoint();
    p.start_node(DIVERT_CHAIN);
    p.bump(); // DIVERT token `->`
    p.skip_ws();

    let mut has_target = false;
    let mut trailing_arrow = false;

    // First target (optional -- bare `->` is valid, e.g. `-> DONE`)
    if at_divert_target(p) {
        divert_target_with_args(p);
        has_target = true;
        trailing_arrow = false;
    }

    // Chained diverts
    loop {
        p.skip_ws();
        if p.current() != DIVERT {
            break;
        }
        p.bump(); // `->`
        trailing_arrow = true;
        p.skip_ws();
        if at_divert_target(p) {
            divert_target_with_args(p);
            has_target = true;
            trailing_arrow = false;
        }
    }

    p.finish_node(); // closes DIVERT_CHAIN

    // If we had at least one target and ended with a trailing `->`,
    // this is a tunnel call — wrap the DIVERT_CHAIN in TUNNEL_CALL_NODE.
    if has_target && (trailing_arrow || p.current() == TUNNEL_ONWARDS) {
        p.start_node_at(checkpoint, TUNNEL_CALL_NODE);
        p.finish_node();
    }
}

/// Returns `true` if we're at a divert target (DONE, END, or identifier).
fn at_divert_target(p: &Parser<'_>) -> bool {
    matches!(p.current(), KW_DONE | KW_END | IDENT)
}

/// Parse a divert target with optional arguments.
///
/// ```text
/// divert_target_with_args = { divert_path ~ ("(" ~ arg_list? ~ ")")? }
/// divert_path = { "DONE" | "END" | dotted_identifier }
/// ```
fn divert_target_with_args(p: &mut Parser<'_>) {
    p.start_node(DIVERT_TARGET_WITH_ARGS);

    // divert_path: DONE, END, or dotted_identifier
    match p.current() {
        KW_DONE | KW_END => {
            p.bump();
        }
        IDENT => {
            dotted_identifier(p);
        }
        _ => {
            p.error("expected divert target".into());
        }
    }

    // Optional arguments
    p.skip_ws();
    if p.current() == L_PAREN {
        p.bump(); // (
        p.skip_ws();
        if p.current() != R_PAREN {
            arg_list(p);
        }
        p.skip_ws();
        p.expect(R_PAREN);
    }

    p.finish_node();
}

/// Parse a dotted identifier: `ident.ident.ident`.
pub(crate) fn dotted_identifier(p: &mut Parser<'_>) {
    p.start_node(DOTTED_IDENTIFIER);
    p.expect(IDENT);
    while p.current() == DOT && p.nth(1) == IDENT {
        p.bump(); // DOT
        p.bump(); // IDENT
    }
    p.finish_node();
}

/// Parse an argument list: `expr (, expr)*`.
pub(crate) fn arg_list(p: &mut Parser<'_>) {
    p.start_node(ARG_LIST);
    super::expression::expression(p);
    loop {
        p.skip_ws();
        if !p.eat(COMMA) {
            break;
        }
        p.skip_ws();
        super::expression::expression(p);
    }
    p.finish_node();
}
