//! The annotated-brace family (charter §6): one brace grammar, the
//! annotation position declares the kind. `{?` (choice points) lives in
//! `choice.rs`; this module covers `{if`/`{match` (conditionals) and
//! `{~`/`{&`/`{!`/`{|` (alternations).

use crate::SyntaxKind::{
    self, ALTERNATION_BLOCK, ALTERNATION_MARKER, AMP, BANG, COMMA, CONDITIONAL_BLOCK, ELSE_BRANCH,
    ENTRY, EOF, FAT_ARROW, IF_ARM, KW_ELSE, KW_IF, KW_MATCH, L_BRACE, MATCH_ARM, MATCH_PATTERN,
    MINUS, NEWLINE, PIPE, QUESTION, R_BRACE, TILDE,
};

use super::Parser;

// ── Family dispatch (peeked from `content.rs`/`block.rs`) ───────────────

pub(crate) fn at_choice_point(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACE) && p.nth(1) == QUESTION
}

pub(crate) fn at_conditional(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACE) && matches!(p.nth(1), KW_IF | KW_MATCH)
}

pub(crate) fn at_alternation(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACE) && matches!(p.nth(1), TILDE | AMP | BANG | PIPE)
}

// ── Conditional: `{if cond …}` / `{match subject {…}}` ──────────────────

/// Finding #4 (`syntax_kind.rs` doc comment on `CONDITIONAL_BLOCK`): both
/// the inline colon body (`{if cond: … else: …}`, charter §6's literal
/// example) and the braced-arm body (`{if cond { … } else { … }}`, charter
/// §4's "braces are the universal body delimiter") are accepted here —
/// `match` always uses a braced arm list (a colon form doesn't generalize
/// to multiple patterns).
pub(crate) fn conditional_block(p: &mut Parser<'_, '_>) {
    p.start_node(CONDITIONAL_BLOCK);
    if !p.enter_depth() {
        p.expect(L_BRACE);
        p.finish_node();
        return;
    }
    p.expect(L_BRACE);
    if p.eat(KW_IF) {
        super::expr::expression(p);
        if_arm(p);
        if at_else_arm(p) {
            else_branch(p);
        }
    } else if p.eat(KW_MATCH) {
        super::expr::expression(p);
        match_arm_list(p);
    } else {
        p.error("expected `if` or `match` after `{`".into());
    }
    p.expect(R_BRACE);
    p.exit_depth();
    p.finish_node();
}

fn if_arm(p: &mut Parser<'_, '_>) {
    p.start_node(IF_ARM);
    arm_body(p);
    p.finish_node();
}

/// `else` only starts a fallback arm when immediately followed by its body
/// opener (`{` or `:`) — otherwise it's ordinary prose (mirrors Finding #5's
/// keyword-vs-prose disambiguation pattern).
fn at_else_arm(p: &Parser<'_, '_>) -> bool {
    p.at(KW_ELSE) && matches!(p.nth(1), L_BRACE | SyntaxKind::COLON)
}

fn else_branch(p: &mut Parser<'_, '_>) {
    p.start_node(ELSE_BRANCH);
    p.expect(KW_ELSE);
    arm_body(p);
    p.finish_node();
}

/// `: content...` or `{ item* }` — shared by `IF_ARM`/`ELSE_BRANCH`.
fn arm_body(p: &mut Parser<'_, '_>) {
    if p.eat(SyntaxKind::COLON) {
        colon_body(p);
    } else if p.at(L_BRACE) {
        super::block::braced_item_list(p, crate::SyntaxKind::BLOCK);
    } else {
        p.error("expected `:` or `{` to open the arm body".into());
    }
}

/// Colon-form arm body: body items run until the closing `}` or an
/// `else`-arm boundary. Recursive-descent naturally keeps this
/// brace-depth-safe — nested constructs consume their own matching braces
/// before returning control here.
fn colon_body(p: &mut Parser<'_, '_>) {
    loop {
        p.skip_ws();
        if p.at(R_BRACE) || p.at_eof() || at_else_arm(p) {
            break;
        }
        let before = p.pos();
        super::block::body_line(p);
        if p.pos() == before {
            p.error_recover("unexpected token in conditional body");
        }
    }
}

// ── Match arm list ────────────────────────────────────────────────────

fn match_arm_list(p: &mut Parser<'_, '_>) {
    p.expect(L_BRACE);
    if p.enter_depth() {
        p.skip_ws_and_newlines();
        loop {
            if p.peek_skip_nl() == R_BRACE || p.at_eof() {
                break;
            }
            let before = p.pos();
            match_arm(p);
            if p.pos() == before {
                p.error_recover("unexpected token in match arm list");
                p.skip_ws_and_newlines();
                continue;
            }
            p.skip_ws_and_newlines();
            p.eat(COMMA);
            p.skip_ws_and_newlines();
        }
        p.exit_depth();
    }
    p.expect(R_BRACE);
}

fn match_arm(p: &mut Parser<'_, '_>) {
    p.start_node(MATCH_ARM);
    p.start_node(MATCH_PATTERN);
    super::expr::expression(p);
    p.finish_node();
    p.expect(FAT_ARROW);
    if p.at(L_BRACE) {
        super::block::braced_item_list(p, crate::SyntaxKind::BLOCK);
    } else {
        super::expr::expression(p);
    }
    p.finish_node();
}

// ── Alternation: `{~ …}` / `{& …}` / `{! …}` / `{| …}` ───────────────────

pub(crate) fn alternation_block(p: &mut Parser<'_, '_>) {
    p.start_node(ALTERNATION_BLOCK);
    if !p.enter_depth() {
        p.expect(L_BRACE);
        p.finish_node();
        return;
    }
    p.expect(L_BRACE);
    alternation_marker(p);
    if is_multiline(p) {
        multiline_entries(p);
    } else {
        inline_alternatives(p);
    }
    p.expect(R_BRACE);
    p.exit_depth();
    p.finish_node();
}

fn alternation_marker(p: &mut Parser<'_, '_>) {
    p.start_node(ALTERNATION_MARKER);
    // Caller (`at_alternation`) already verified the current token is one
    // of TILDE/AMP/BANG/PIPE.
    p.skip_ws();
    p.bump();
    p.finish_node();
}

/// A multiline annotated block is a marker immediately followed by a
/// newline (whitespace/comments aside) — mirrors `brink-syntax`'s
/// `is_multiline_block` check.
fn is_multiline(p: &Parser<'_, '_>) -> bool {
    peek_is_newline(p, 0)
}

/// True when, starting `i` raw tokens ahead of the parser's current
/// position, the next non-trivia raw token is a `NEWLINE`. The shared scan
/// behind [`is_multiline`] (`i = 0`: is the token right here a newline,
/// trivia aside) and `content::is_body_open_brace` (`i = 1`: is the token
/// right after an as-yet-unconsumed `L_BRACE` a newline) — both are the
/// same "does this brace open a multiline block" signal, checked from two
/// different parser positions (family.rs consumes the brace + marker
/// before asking; content.rs asks before consuming anything, since G-2
/// needs the answer to decide whether to consume the brace as
/// interpolation at all).
pub(crate) fn peek_is_newline(p: &Parser<'_, '_>, mut i: usize) -> bool {
    loop {
        match p.nth_raw(i) {
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                i += 1;
            }
            NEWLINE => return true,
            _ => return false,
        }
    }
}

fn multiline_entries(p: &mut Parser<'_, '_>) {
    loop {
        p.skip_ws();
        if p.at(R_BRACE) || p.at_eof() {
            break;
        }
        if p.at(MINUS) {
            entry(p);
        } else {
            let before = p.pos();
            super::block::body_line(p);
            if p.pos() == before {
                p.error_recover("unexpected token before the first `-` entry");
            }
        }
    }
}

/// One `-`-prefixed entry/arm (charter §6). Runs until the next `-` or the
/// closing `}`.
fn entry(p: &mut Parser<'_, '_>) {
    p.start_node(ENTRY);
    p.expect(MINUS);
    loop {
        p.skip_ws();
        if p.at(R_BRACE) || p.at(MINUS) || p.at_eof() {
            break;
        }
        let before = p.pos();
        super::block::body_line(p);
        if p.pos() == before {
            p.error_recover("unexpected token in entry");
        }
    }
    p.finish_node();
}

/// Single-line, pipe-separated alternatives (`{~ red|blue|green}`). The
/// marker char may itself be `|` (stopping-sequence) — only the *first*
/// `|` right after the marker is that marker; every later bare `|` is a
/// separator, disambiguated positionally, not lexically.
fn inline_alternatives(p: &mut Parser<'_, '_>) {
    loop {
        p.skip_ws();
        match p.current() {
            R_BRACE | EOF | NEWLINE => break,
            PIPE => {
                p.bump();
            }
            // `content_items_until` always stops at `HASH` too (see its
            // doc comment) — handle trailing tags explicitly so this loop
            // still makes progress instead of spinning.
            SyntaxKind::HASH => super::content::tag_line_tail(p),
            _ => super::content::content_items_until(p, &[PIPE, R_BRACE, NEWLINE]),
        }
    }
}
