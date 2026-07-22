//! Prose content: text runs, `{expr}` interpolation, `<>` glue, diverts in
//! content position, and `#`-tags (charter §5/§6/§11).
//!
//! [`content_items_until`] is the shared engine: a loop that recognizes
//! interpolation/glue/the annotated-brace family/diverts and folds
//! everything else into `TEXT` runs, stopping at a caller-chosen set of
//! terminators. Reused by `CONTENT_LINE` (stops at newline), choice-line
//! anatomy (stops at `[`/`]`), and inline alternation bodies (stops at
//! `|`).

use crate::SyntaxKind::{
    self, CONTENT_LINE, DIVERT, DOC_COMMENT_INNER, DOC_COMMENT_OUTER, EOF, GLUE, GLUE_NODE, HASH,
    IDENT, INTERPOLATION, L_BRACE, L_PAREN, LABEL, NEWLINE, R_BRACE, R_PAREN, TAG, TAG_LINE, TEXT,
};

use super::Parser;

/// A single line of prose content, terminated by `NEWLINE` or EOF. Never
/// consumes a bare `R_BRACE` (that closes the enclosing body, never the
/// content line itself). May open with a `(label)` — G-1 (RULED
/// 2026-07-20, "label any content line"): extends the choice-line `(name)`
/// label syntax (charter §11) to any content line, giving ink's `-
/// (label)` mid-flow re-entry / backward-loop divert target a native
/// spelling. Trailing `#tag`s are folded in before the line ends.
pub(crate) fn content_line(p: &mut Parser<'_, '_>) {
    p.start_node(CONTENT_LINE);
    if at_content_label(p) {
        label(p);
        p.skip_ws();
    }
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

/// `(ident)` at the very start of a content line — G-1's label prefix.
/// Guarded by the same positive-lookahead discipline `decl.rs`'s Finding
/// #5 uses for keyword-headed declarations (see that module's doc
/// comment): only the exact `L_PAREN IDENT R_PAREN` shape commits to a
/// `LABEL`; anything else (an empty `()`, a multi-word parenthetical, an
/// unclosed paren) falls through to ordinary prose text, same as before
/// this fix. This does not fully resolve the syntax's inherent ambiguity
/// with a single-word parenthetical remark opening a line (`(Sighing) I
/// trudge on.` is indistinguishable from a real label by construction) —
/// that is the exact tradeoff the choice-line `(name)` syntax already
/// accepts (charter §11); noted as a finding rather than papered over.
fn at_content_label(p: &Parser<'_, '_>) -> bool {
    p.at(L_PAREN) && p.nth(1) == IDENT && p.nth(2) == R_PAREN
}

/// `(name)` — a label. Shared by `content_line` (G-1) and `choice.rs`'s
/// choice-line label (charter §11, "kept in the one label syntax"): one
/// grammar rule, two call sites.
pub(crate) fn label(p: &mut Parser<'_, '_>) {
    p.start_node(LABEL);
    p.expect(L_PAREN);
    p.expect(IDENT);
    p.expect(R_PAREN);
    p.finish_node();
}

/// The shared prose-scanning loop: recognizes interpolation/glue/the
/// annotated-brace family/diverts, folds everything else into `TEXT` runs,
/// and stops as soon as the current (trivia-skipped) token is EOF or
/// appears in `stop`. Does not consume the stopping token.
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
        if cur == EOF || cur == HASH {
            break;
        }
        if cur == L_BRACE {
            // G-2: an `L_BRACE` a caller lists as a stop kind (choice-text
            // scanning, where a trailing brace may open a nested-content
            // `CHOICE_BODY`) only actually terminates the run when it is a
            // genuine body-opener — see `is_body_open_brace`. Every other
            // `L_BRACE` shape (the annotated-brace family, or bare
            // `{expr}` interpolation) keeps scanning and falls through to
            // the match below, exactly like a plain `content_line` call
            // (which never lists `L_BRACE` in `stop`, so the generic
            // `stop.contains` branch below never fires for it either —
            // this `if` deliberately does NOT fall through to that branch,
            // it's the one caller-supplied stop kind `L_BRACE` needs
            // special-casing for).
            if stop.contains(&L_BRACE) && is_body_open_brace(p) {
                break;
            }
        } else if stop.contains(&cur) {
            break;
        }
        match cur {
            L_BRACE if super::family::at_choice_point(p) => super::choice::choice_point(p),
            L_BRACE if super::family::at_conditional(p) => super::family::conditional_block(p),
            L_BRACE if super::family::at_alternation(p) => super::family::alternation_block(p),
            L_BRACE => interpolation(p),
            GLUE => glue_node(p),
            // N-1: a divert anywhere in a content run is a real DIVERT
            // node (charter §11: diverts are "kept verbatim" including in
            // content position — the Fogg exhibit spells
            // `* [The wager.] -> know_about_wager` this way).
            // `block::body_line` only recognized `->` as a divert when it
            // was a line's first token; this loop had no case for it at
            // all, so a `->` following prose text on the same line folded
            // into a literal `TEXT` run instead (`text_run_until` below is
            // updated to break on `DIVERT` too, so it hands control back
            // here rather than swallowing the arrow as text).
            DIVERT => super::divert::divert_in_content(p),
            // B0.6b: a doc-comment token (`///` / `//!`) that reaches the
            // content scanner is, by construction, not in an attachment
            // position — a leading `///` is consumed by `block::item` before
            // dispatch, and an inner `//!` by `braced_item_list` right after
            // its `{`. Anything left over (a `//!` after real content, or
            // either form appearing after prose on the same line) is bumped
            // BARE here — no `TEXT` node — so it stays invisible narrative,
            // matching the trivia-fallback the unattached-`///` path already
            // gives (`doc_comment.rs`). Content lowering iterates node
            // children only, so a bare token produces no visible output. The
            // invariant: a doc-comment token must NEVER become story prose.
            DOC_COMMENT_OUTER | DOC_COMMENT_INNER => p.bump(),
            _ => text_run_until(p, stop),
        }
    }
}

/// True when the `L_BRACE` at the parser's current (unconsumed) position
/// is a genuine body-opener (e.g. a choice's `CHOICE_BODY`) rather than
/// bare `{expr}` interpolation — G-2's disambiguation. Only meaningful
/// when the caller listed `L_BRACE` in its stop set (choice-text scanning,
/// `choice.rs`'s `CHOICE_START_CONTENT`/`CHOICE_INNER_CONTENT`); a plain
/// `content_line` never does, so this is never consulted there — a `{` mid
/// content-line has no competing "body" grammar that could start there, so
/// it's uniformly family-or-interpolation, unchanged from before this fix.
fn is_body_open_brace(p: &Parser<'_, '_>) -> bool {
    if super::family::at_choice_point(p)
        || super::family::at_conditional(p)
        || super::family::at_alternation(p)
    {
        return false;
    }
    // A bare `{` is a body-opener exactly when it's the multiline shape
    // (immediately followed by a NEWLINE, trivia aside) — the same signal
    // `family::is_multiline` uses for the alternation family, checked one
    // token earlier here since the brace itself hasn't been consumed yet.
    super::family::peek_is_newline(p, 1)
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
/// construct (`{`, `<>`, `->` (N-1), a doc-comment token (B0.6b), a
/// caller-supplied stop kind, or EOF), including any interior
/// whitespace/plain-comments — those are literal prose here, not trivia to
/// discard. `HASH`/`DIVERT`/doc-comment tokens always break a text run
/// (tags and diverts are recognized structurally by every caller; a
/// doc-comment token must never fold into visible prose), even if the
/// caller didn't ask for it — mirrors `content_items_until`'s own
/// unconditional-break agreement for `HASH`; without this, a `->` (or a
/// stray `//!`) reached mid-run would get bumped as literal text before the
/// outer loop ever saw it, and its dedicated match arm there would be dead
/// code.
fn text_run_until(p: &mut Parser<'_, '_>, stop: &[SyntaxKind]) {
    p.start_node(TEXT);
    loop {
        let k = p.nth_raw(0);
        if k == EOF
            || k == L_BRACE
            || k == GLUE
            || k == HASH
            || k == DIVERT
            || k == DOC_COMMENT_OUTER
            || k == DOC_COMMENT_INNER
            || stop.contains(&k)
        {
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
