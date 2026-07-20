//! Prose content: text runs, `{expr}` interpolation, `<>` glue, and
//! `#`-tags (charter §5/§6/§11).
//!
//! [`content_items_until`] is the shared engine: a loop that recognizes
//! interpolation/glue/the annotated-brace family and folds everything else
//! into `TEXT` runs, stopping at a caller-chosen set of terminators. Reused
//! by `CONTENT_LINE` (stops at newline), choice-line anatomy (stops at
//! `[`/`]`), and inline alternation bodies (stops at `|`).

use crate::SyntaxKind::{
    self, CONTENT_LINE, EOF, GLUE, GLUE_NODE, HASH, INTERPOLATION, L_BRACE, NEWLINE, R_BRACE, TAG,
    TAG_LINE, TEXT,
};

use super::Parser;

/// A single line of prose content, terminated by `NEWLINE` or EOF. Never
/// consumes a bare `R_BRACE` (that closes the enclosing body, never the
/// content line itself). Trailing `#tag`s are folded in before the line
/// ends.
pub(crate) fn content_line(p: &mut Parser<'_, '_>) {
    p.start_node(CONTENT_LINE);
    content_items_until(p, &[NEWLINE, R_BRACE, HASH]);
    if p.at(HASH) {
        tag_line_tail(p);
    }
    p.finish_node();
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}

/// The shared prose-scanning loop: recognizes interpolation/glue/the
/// annotated-brace family, folds everything else into `TEXT` runs, and
/// stops as soon as the current (trivia-skipped) token is EOF or appears
/// in `stop`. Does not consume the stopping token.
pub(crate) fn content_items_until(p: &mut Parser<'_, '_>, stop: &[SyntaxKind]) {
    loop {
        p.skip_ws();
        let cur = p.current();
        // `HASH` always stops this loop, whether or not the caller asked
        // for it: `text_run_until` below always treats `HASH` as breaking
        // (tags are recognized structurally by every caller), and if this
        // outer loop didn't agree, a `HASH` the caller forgot to list would
        // fall through to `text_run_until` and make zero progress —
        // `content_line` and every choice-anatomy caller rely on this
        // agreement to stay infinite-loop-safe.
        if cur == EOF || cur == HASH || stop.contains(&cur) {
            break;
        }
        match cur {
            L_BRACE if super::family::at_choice_point(p) => super::choice::choice_point(p),
            L_BRACE if super::family::at_conditional(p) => super::family::conditional_block(p),
            L_BRACE if super::family::at_alternation(p) => super::family::alternation_block(p),
            L_BRACE => interpolation(p),
            GLUE => glue_node(p),
            _ => text_run_until(p, stop),
        }
    }
}

/// `{expr}` — bare-brace interpolation, and nothing else, ever (charter
/// §6).
pub(crate) fn interpolation(p: &mut Parser<'_, '_>) {
    p.start_node(INTERPOLATION);
    if p.enter_depth() {
        p.expect(L_BRACE);
        super::expr::expression(p);
        p.expect(R_BRACE);
        p.exit_depth();
    } else {
        p.expect(L_BRACE);
    }
    p.finish_node();
}

fn glue_node(p: &mut Parser<'_, '_>) {
    p.start_node(GLUE_NODE);
    p.expect(GLUE);
    p.finish_node();
}

/// A run of literal text: every raw token up to the next breaking
/// construct (`{`, `<>`, a caller-supplied stop kind, or EOF), including
/// any interior whitespace/comments — those are literal prose here, not
/// trivia to discard. `HASH` always breaks a text run (tags are recognized
/// structurally by every caller), even if the caller didn't ask for it.
fn text_run_until(p: &mut Parser<'_, '_>, stop: &[SyntaxKind]) {
    p.start_node(TEXT);
    loop {
        let k = p.nth_raw(0);
        if k == EOF || k == L_BRACE || k == GLUE || k == HASH || stop.contains(&k) {
            break;
        }
        p.bump();
    }
    p.finish_node();
}

/// `# tag text # another tag\n` — a standalone tag-line body item (charter
/// §11: tags kept).
pub(crate) fn tag_line(p: &mut Parser<'_, '_>) {
    p.start_node(TAG_LINE);
    tag_line_tail(p);
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
    p.finish_node();
}

/// One or more `#`-tags in a row, without a `TAG_LINE` wrapper — used by
/// `tag_line`, `content_line`'s trailing-tags tail, and any other prose
/// loop (e.g. inline alternation bodies) that hits a `HASH` it must
/// consume to keep making forward progress.
pub(crate) fn tag_line_tail(p: &mut Parser<'_, '_>) {
    while p.at(HASH) {
        tag(p);
    }
}

fn tag(p: &mut Parser<'_, '_>) {
    p.start_node(TAG);
    p.expect(HASH);
    while !matches!(p.current(), NEWLINE | EOF | HASH | R_BRACE) {
        p.bump();
    }
    p.finish_node();
}
