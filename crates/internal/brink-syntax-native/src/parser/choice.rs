//! `{? … }` — explicit choice points (charter §5). All choices live inside
//! a point; there is no bare knot-level `*`/`+` anymore (that ambiguity
//! died with the gather).

use crate::SyntaxKind::{
    CHOICE, CHOICE_BODY, CHOICE_BRACKET_CONTENT, CHOICE_BULLET, CHOICE_GUARD, CHOICE_INNER_CONTENT,
    CHOICE_POINT, CHOICE_START_CONTENT, DOT, ELSE_BRANCH, EOF, HASH, IDENT, KW_ELSE, KW_IF,
    L_BRACE, L_BRACKET, L_PAREN, NEWLINE, PLUS, QUESTION, R_BRACE, R_BRACKET, R_PAREN, SPLICE,
    STAR, THREAD,
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
        super::content::label(p);
    }

    choice_text(p);

    // #1264 / #1195: `content_items_until` (`content.rs`) always breaks on
    // `HASH` regardless of the caller's stop set, so a choice line's
    // trailing `#tag`s stop `choice_text`'s scan cleanly — but unlike
    // `content_line`, nothing here used to consume them. They were left
    // for `choice_point`'s outer loop, whose `match` has no `HASH` arm, so
    // it fell into `error_recover` and wrapped the tag in `ERROR` nodes.
    // Mirror `content_line`'s tail call so trailing tags fold into `TAG`
    // nodes here too, as direct children of `CHOICE` (siblings of
    // `CHOICE_START_CONTENT`/the bracket anatomy), matching how
    // `content_line` attaches its own trailing tags.
    if p.at(HASH) {
        super::content::tag_line_tail(p);
    }

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

/// `{if cond}` — and, grammatically, `{if cond as name}`.
///
/// Guard-`as` is a ruled, implemented part of the language
/// (`docs/decision-log.md` 2026-07-26, "Choice-guard `as` un-deferred:
/// capture-at-presentation, by-value (COW)"; issue #1508 landed the
/// lowering — `brink-ir`'s `E146` "not yet supported" diagnostic is
/// retired). The parser accepts it exactly the same way regardless; only
/// `brink-ir::hir::lower_native::choice`'s handling of a present binding
/// changed.
fn choice_guard(p: &mut Parser<'_, '_>) {
    p.start_node(CHOICE_GUARD);
    p.expect(L_BRACE);
    p.expect(KW_IF);
    super::expr::expression(p);
    p.skip_ws();
    if super::binding::at_as_binding(p) {
        super::binding::as_binding(p);
    }
    p.expect(R_BRACE);
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

/// `<-` outside a choice point (issue #1263, ruled #1260 on #1256): charter
/// §11 narrows threads to scoped splices inside choice points, so `<-` has
/// no structural meaning here — but it can also be literal dialogue
/// punctuation (a shrug, a dash), so this must never be a hard error the
/// way an unrecognized token inside a `CHOICE_POINT` is. Only [`splice`]
/// (called from `choice_point`'s own loop) ever builds a `SPLICE` node.
///
/// Called from `block::body_line` when a line starts with `THREAD` outside
/// any choice point. Emits a warning-severity diagnostic, then falls
/// through to `content::content_line` exactly as before this fix existed —
/// the CST shape is untouched, `<-` still folds into an ordinary `TEXT`
/// run inside `CONTENT_LINE`, lossless round-trip preserved.
pub(crate) fn splice_outside_choice_point(p: &mut Parser<'_, '_>) {
    let message = if looks_like_flow_reference(p) {
        "`<-` outside a choice point has no effect here; it is treated as \
         ordinary text, not a splice. The tokens after `<-` look like a \
         knot/flow reference (`<- name` / `<- name(args)`) — if this was \
         meant as a splice, move it inside a `{? … }` choice point (charter \
         §11); splices are only recognized there."
    } else {
        "`<-` outside a choice point has no effect here; it is treated as \
         ordinary text, not a splice. Splices (`<- flow(args)`) are only \
         recognized inside a `{? … }` choice point (charter §11)."
    };
    p.warning(message.to_owned());
    super::content::content_line(p);
}

/// Read-only lookahead for [`splice_outside_choice_point`]'s confidence
/// signal: does the line, right after `<-`, look like a real divert-target
/// reference — an `IDENT`-led dotted path, optionally called, with nothing
/// else trailing on the line? This crate has no symbol table (it performs
/// no resolution, `lib.rs`'s doc comment), so "resolves to a real
/// knot/flow reference" is judged by shape alone, the same shape
/// `expr::path`/`divert::divert_target` accept. Never mutates `p`.
fn looks_like_flow_reference(p: &Parser<'_, '_>) -> bool {
    // p.nth(0) is THREAD (the caller's dispatch token); start just after it.
    let mut n = 1;
    if p.nth(n) != IDENT {
        return false;
    }
    n += 1;
    while p.nth(n) == DOT {
        n += 1;
        if p.nth(n) != IDENT {
            return false;
        }
        n += 1;
    }
    if p.nth(n) == L_PAREN {
        let mut depth: u32 = 0;
        loop {
            match p.nth(n) {
                L_PAREN => {
                    depth += 1;
                    n += 1;
                }
                R_PAREN => {
                    depth -= 1;
                    n += 1;
                    if depth == 0 {
                        break;
                    }
                }
                NEWLINE | EOF => return false,
                _ => n += 1,
            }
        }
    }
    matches!(p.nth(n), NEWLINE | EOF | R_BRACE)
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
