//! `{? … }` — explicit choice points (charter §5). All choices live inside
//! a point; there is no bare knot-level `*`/`+` anymore (that ambiguity
//! died with the gather).

use crate::SyntaxKind::{
    CHOICE, CHOICE_BODY, CHOICE_BRACKET_CONTENT, CHOICE_BULLET, CHOICE_GUARD,
    CHOICE_INNER_CONTENT, CHOICE_POINT, CHOICE_START_CONTENT, ELSE_BRANCH, EOF, KW_ELSE, KW_IF,
    L_BRACE, L_BRACKET, L_PAREN, LABEL, NEWLINE, PLUS, QUESTION, R_BRACE, R_BRACKET, R_PAREN,
    SPLICE, STAR, THREAD,
};

use super::Parser;

/// `{? … }`.
pub(crate) fn choice_point(p: &mut Parser<'_, '_>) {
    p.start_node(CHOICE_POINT);
    if !p.enter_depth() {
        p.expect(L_BRACE);
        p.finish_node();
        return;
    }
    p.expect(L_BRACE);
    p.expect(QUESTION);

    loop {
        p.skip_ws();
        match p.current() {
            R_BRACE | EOF => break,
            NEWLINE => {
                p.bump();
            }
            STAR | PLUS => choice(p),
            THREAD => splice(p),
            KW_ELSE if p.nth(1) == L_BRACE => else_branch(p),
            _ => {
                let before = p.pos();
                p.error_recover("expected a choice line, splice, or `else` in a choice point");
                if p.pos() == before {
                    break;
                }
            }
        }
    }

    p.expect(R_BRACE);
    p.exit_depth();
    p.finish_node();
}

/// One `*`/`+` choice line: bullet, optional `{if cond}` guard, optional
/// `(label)`, the `[]` display-split anatomy (kept as-is, charter §5), and
/// an optional braced nested-content body.
fn choice(p: &mut Parser<'_, '_>) {
    p.start_node(CHOICE);
    p.start_node(CHOICE_BULLET);
    p.bump(); // STAR or PLUS, caller-verified
    p.finish_node();

    p.skip_ws();
    if p.at(L_BRACE) && p.nth(1) == KW_IF {
        choice_guard(p);
    }
    p.skip_ws();
    if p.at(L_PAREN) {
        label(p);
    }

    choice_text(p);

    p.skip_ws();
    if p.at(L_BRACE) {
        super::block::braced_item_list(p, CHOICE_BODY);
    }
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
    p.finish_node();
}

fn choice_guard(p: &mut Parser<'_, '_>) {
    p.start_node(CHOICE_GUARD);
    p.expect(L_BRACE);
    p.expect(KW_IF);
    super::expr::expression(p);
    p.expect(R_BRACE);
    p.finish_node();
}

fn label(p: &mut Parser<'_, '_>) {
    p.start_node(LABEL);
    p.expect(L_PAREN);
    p.expect(crate::SyntaxKind::IDENT);
    p.expect(R_PAREN);
    p.finish_node();
}

/// The `text[bracket]inner` display-split anatomy (kept as-is, charter
/// §5). `[` opens the bracket only when it isn't already consumed as a
/// `LABEL`/`CHOICE_GUARD`; terminated by `NEWLINE`/EOF/`R_BRACE`, or by a
/// trailing `{` that opens the choice's nested-content body.
fn choice_text(p: &mut Parser<'_, '_>) {
    p.start_node(CHOICE_START_CONTENT);
    super::content::content_items_until(p, &[L_BRACKET, NEWLINE, R_BRACE, L_BRACE]);
    p.finish_node();

    if p.at(L_BRACKET) {
        p.start_node(CHOICE_BRACKET_CONTENT);
        p.bump(); // [
        super::content::content_items_until(p, &[R_BRACKET, NEWLINE, R_BRACE]);
        p.expect(R_BRACKET);
        p.finish_node();

        p.start_node(CHOICE_INNER_CONTENT);
        super::content::content_items_until(p, &[NEWLINE, R_BRACE, L_BRACE]);
        p.finish_node();
    }
}

/// `<- flow(args)` — a splice inside a choice point (charter §5): harvests
/// another flow's choices into this point.
fn splice(p: &mut Parser<'_, '_>) {
    p.start_node(SPLICE);
    p.expect(THREAD);
    super::expr::path(p);
    if p.at(L_PAREN) {
        super::expr::arg_list(p);
    }
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
    p.finish_node();
}

/// `else { … }` — a choice point's fallback branch (charter §11), always
/// braced (no colon form — unlike `IF_ARM`/`ELSE_BRANCH` in the
/// conditional family).
fn else_branch(p: &mut Parser<'_, '_>) {
    p.start_node(ELSE_BRANCH);
    p.expect(KW_ELSE);
    p.skip_ws();
    if p.at(L_BRACE) {
        super::block::braced_item_list(p, CHOICE_BODY);
    } else {
        p.error("expected `{` after a choice point's `else`".into());
    }
    p.finish_node();
}
