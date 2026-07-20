//! `@[name(args)]` — the annotation-line channel, paren-clause grammar
//! (charter §11 / NS-A2 lineage, `docs/directive-annotations-spec.md`
//! §5b's `@[effects(pure, silent, reads(gold, hp))]`).

use crate::SyntaxKind::{
    ANNOTATION_ARG, ANNOTATION_ARGS, ANNOTATION_LINE, AT_L_BRACKET, COMMA, FLOAT, FLOAT_LIT,
    IDENT, INTEGER, INTEGER_LIT, L_PAREN, NEWLINE, QUOTE, R_BRACKET, R_PAREN,
};

use super::Parser;

pub(crate) fn annotation_line(p: &mut Parser<'_, '_>) {
    p.start_node(ANNOTATION_LINE);
    p.expect(AT_L_BRACKET);
    p.expect(IDENT);
    if p.at(L_PAREN) {
        annotation_args(p);
    }
    p.expect(R_BRACKET);
    // Anything else on the line is unexpected — consume to newline so the
    // parser stays line-synchronized (mirrors `brink-syntax`'s
    // `annotation_line` recovery).
    let mut trailing = false;
    while !p.at_eof() && p.nth_raw(0) != NEWLINE {
        if !matches!(p.nth_raw(0), crate::SyntaxKind::WHITESPACE) {
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

fn annotation_args(p: &mut Parser<'_, '_>) {
    p.start_node(ANNOTATION_ARGS);
    p.expect(L_PAREN);
    if p.enter_depth() {
        p.skip_ws_and_newlines();
        while p.peek_skip_nl() != R_PAREN && !p.at_eof() {
            let before = p.pos();
            annotation_arg(p);
            if p.pos() == before {
                p.error_recover("unexpected token in annotation arguments");
                p.skip_ws_and_newlines();
                continue;
            }
            p.skip_ws_and_newlines();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws_and_newlines();
        }
        p.exit_depth();
    }
    p.expect(R_PAREN);
    p.finish_node();
}

/// One argument: a bare `IDENT`, `IDENT(ANNOTATION_ARGS)` (the nested
/// paren-clause form), or a literal (`INTEGER`/`FLOAT`/`STRING` — the
/// future `@[maxlen(80)]`-shaped tenants `directive-annotations-spec.md`
/// §6 names).
fn annotation_arg(p: &mut Parser<'_, '_>) {
    match p.current() {
        IDENT => {
            p.start_node(ANNOTATION_ARG);
            p.expect(IDENT);
            if p.at(L_PAREN) {
                annotation_args(p);
            }
            p.finish_node();
        }
        INTEGER => {
            p.start_node(ANNOTATION_ARG);
            p.start_node(INTEGER_LIT);
            p.expect(INTEGER);
            p.finish_node();
            p.finish_node();
        }
        FLOAT => {
            p.start_node(ANNOTATION_ARG);
            p.start_node(FLOAT_LIT);
            p.expect(FLOAT);
            p.finish_node();
            p.finish_node();
        }
        QUOTE => {
            p.start_node(ANNOTATION_ARG);
            super::expr::string_lit(p);
            p.finish_node();
        }
        _ => {}
    }
}
