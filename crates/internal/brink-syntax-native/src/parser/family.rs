//! The annotated-brace family (charter §6): one brace grammar, the
//! annotation position declares the kind. `{?` (choice points) lives in
//! `choice.rs`; this module covers `{if`/`{match` (conditionals) and
//! `{~`/`{&`/`{!`/`{|` (alternations).

use crate::SyntaxKind::{
    self, ALTERNATION_BLOCK, ALTERNATION_MARKER, AMP, AT_L_BRACKET, BANG, COMMA, CONDITIONAL_BLOCK,
    DIVERT, ELSE_BRANCH, ENTRY, EOF, FAT_ARROW, HASH, IF_ARM, KW_ELSE, KW_IF, KW_MATCH, KW_RETURN,
    L_BRACE, MATCH_ARM, MATCH_PATTERN, MINUS, NEWLINE, PIPE, QUESTION, R_BRACE, THREAD, TILDE,
};

use super::Parser;

// ── Family dispatch (peeked from `content.rs`/`block.rs`) ───────────────

pub(crate) fn at_choice_point(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACE) && p.nth(1) == QUESTION
}

pub(crate) fn at_conditional(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACE) && matches!(p.nth(1), KW_IF | KW_MATCH)
}

/// "Alternation markers win" (ruled 2026-07-22, #1258/#1261): a `{` led by
/// any of `!`/`~`/`&`/`|` is ALWAYS claimed by this family, on one-token
/// lookahead, with no further disambiguation against what follows —
/// dispatched ahead of `content::interpolation` in `content_items_until`'s
/// match, so this is a hard precedence rule, not a heuristic. That makes a
/// bare-brace interpolation whose expression happens to start with one of
/// those four tokens (a prefix-`!` expression, or a `|params| body`
/// lambda) unreachable as interpolation — ruled acceptable because the
/// escape hatch is one keystroke: wrap the expression in parens
/// (`{(!x)}`, `{(|x| x)}`), which starts with `L_PAREN` and therefore
/// never matches here, always falling through to plain `{expr}`
/// interpolation instead. `PIPE` is also the token `LAMBDA_EXPR` opens and
/// closes its parameter list with, so a `{|params| body}` is claimed by
/// this family too — and, since `{|}` is a real stopping-sequence marker
/// (charter §116), it parses as an ordinary stopping-sequence (a lambda in
/// content position is written `{(|x| x)}`); see `inline_alternatives`.
pub(crate) fn at_alternation(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACE) && matches!(p.nth(1), TILDE | AMP | BANG | PIPE)
}

// ── Conditional: `{if cond …}` / `{match subject {…}}` ──────────────────

/// Finding #4 (`syntax_kind.rs` doc comment on `CONDITIONAL_BLOCK`): both
/// the inline colon body (`{if cond: … else: …}`, charter §6's literal
/// example) and the braced-arm body (`{if cond { … } else { … }}`, charter
/// §4's "braces are the universal body delimiter") are accepted here —
/// `match` always uses a braced arm list (a colon form doesn't generalize
/// to multiple patterns). Both forms accept the inline `else:`/`else{`
/// boundary on the SAME physical line as the arm it follows (#1254 Gap 1,
/// ruled #1258-adjacent) and a flat `else if <cond> { … }`/`else if
/// <cond>: …` chain (ruled 2026-07-22, #1258): a flat chain lowers to the
/// identical shape an explicit nested `{if …}` would (`else_branch` opens
/// an ordinary `CONDITIONAL_BLOCK` directly, sharing `conditional_body`
/// with the brace-delimited entry point), so there is exactly one tree
/// shape for "else if" regardless of which spelling was used.
pub(crate) fn conditional_block(p: &mut Parser<'_, '_>) {
    p.start_node(CONDITIONAL_BLOCK);
    if !p.enter_depth() {
        p.expect(L_BRACE);
        p.finish_node();
        return;
    }
    p.expect(L_BRACE);
    conditional_body(p);
    p.expect(R_BRACE);
    p.exit_depth();
    p.finish_node();
}

/// The `if`/`match` head plus its arm(s) — everything a `CONDITIONAL_BLOCK`
/// contains between its delimiters. Shared by `conditional_block` (the
/// brace-delimited `{if …}`/`{match …}` entry point) and `else_branch`'s
/// flat `else if` chain arm, which opens a `CONDITIONAL_BLOCK` of its own
/// with NO surrounding `{`/`}` tokens (the flat spelling has none in the
/// source — the outer `CONDITIONAL_BLOCK`'s own braces already cover the
/// whole chain), so the two entry points diverge only in delimiter
/// handling, never in body shape.
fn conditional_body(p: &mut Parser<'_, '_>) {
    if p.eat(KW_IF) {
        head_expression(p);
        // `{if EXPR as NAME: … else: …}` (B1b, issue #1475) — the template
        // condition position of the SAME `as` binding the statement form
        // takes, riding the already-ruled `{if}` spelling rather than a
        // second binding grammar. Scoped to the `IF_ARM` only, never the
        // `ELSE_BRANCH`.
        p.skip_ws();
        if super::binding::at_as_binding(p) {
            super::binding::as_binding(p);
        }
        if_arm(p);
        if at_else_arm(p) {
            else_branch(p);
        }
    } else if p.eat(KW_MATCH) {
        head_expression(p);
        match_arm_list(p);
    } else {
        p.error("expected `if` or `match` after `{`".into());
    }
}

/// The content-ground counterpart of `control_flow::head_expression`: a
/// `{if …}`/`{match …}` head is directly followed by its arm-body opener,
/// which may be `{` (`arm_body`/`match_arm_list`), so a trailing
/// `TypeName { … }` construction literal (B5, issue #1464) must not
/// swallow it. The colon form (`{if x: …}`) is unambiguous, but the
/// restriction is set for both — the head is parsed before either opener
/// is visible.
fn head_expression(p: &mut Parser<'_, '_>) {
    let saved = p.set_no_construct_literal(true);
    super::expr::expression(p);
    p.set_no_construct_literal(saved);
}

fn if_arm(p: &mut Parser<'_, '_>) {
    p.start_node(IF_ARM);
    arm_body(p);
    p.finish_node();
}

/// `else` starts a fallback arm in three shapes (mirrors Finding #5's
/// keyword-vs-prose disambiguation pattern, otherwise it's ordinary
/// prose): immediately followed by its body opener (`{` or `:`), or by a
/// flat chained `if` (`else if cond { … }` / `else if cond: …`, ruled
/// 2026-07-22, #1258) — the third shape is what `else_branch` recognizes
/// as "the arm's whole body is another conditional," not a body opener at
/// all.
pub(crate) fn at_else_arm(p: &Parser<'_, '_>) -> bool {
    p.at(KW_ELSE) && matches!(p.nth(1), L_BRACE | SyntaxKind::COLON | KW_IF)
}

fn else_branch(p: &mut Parser<'_, '_>) {
    p.start_node(ELSE_BRANCH);
    p.expect(KW_ELSE);
    if p.at(KW_IF) {
        // Flat `else if` chain (#1258): the arm's entire body is a nested
        // conditional, with no `{`/`}`/`:` of its own — parse it as a
        // brace-less `CONDITIONAL_BLOCK` sharing `conditional_body` with
        // the ordinary entry point, so this chains identically to writing
        // `else { {if cond { … }} }` by hand (same node kinds, same
        // `is_if`/`condition`/`if_arm`/`else_arm` accessors), just without
        // the extra delimiter tokens the flat spelling never had.
        p.start_node(CONDITIONAL_BLOCK);
        if p.enter_depth() {
            conditional_body(p);
            p.exit_depth();
        }
        p.finish_node();
    } else {
        arm_body(p);
    }
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
/// before returning control here. The `at_else_arm` check here catches an
/// `else` that starts its OWN physical line; `colon_body_line`'s prose
/// fallback (`content::content_line_else_boundary`) catches one that
/// trails other content on the SAME line (#1254 Gap 1) — between the two,
/// `{if cond: … else: …}` written as a single physical line (charter §6's
/// literal example) now produces a real `ELSE_BRANCH` either way.
fn colon_body(p: &mut Parser<'_, '_>) {
    loop {
        p.skip_ws();
        if p.at(R_BRACE) || p.at_eof() || at_else_arm(p) {
            break;
        }
        let before = p.pos();
        colon_body_line(p);
        if p.pos() == before {
            p.error_recover("unexpected token in conditional body");
        }
    }
}

/// `colon_body`'s per-physical-line dispatch — identical to
/// `block::body_line`'s dispatch table, except the prose fallback calls
/// `content::content_line_else_boundary` instead of plain `content_line`,
/// so a same-line `else:`/`else{`/`else if` boundary trailing other
/// content on this line stops the scan and hands control back to
/// `colon_body` instead of being swallowed into this line's `TEXT` (#1254
/// Gap 1). Every OTHER line shape (tags, annotations, diverts, return, an
/// out-of-choice `<-` thread, the brace family) behaves exactly like
/// `body_line`, since none of them can ever swallow a trailing `else` the
/// way an unbounded prose scan can. Keep this in sync with `body_line` if
/// its dispatch table grows.
fn colon_body_line(p: &mut Parser<'_, '_>) {
    match p.current() {
        NEWLINE => {
            p.bump();
        }
        HASH => super::content::tag_line(p),
        AT_L_BRACKET => super::annotation::annotation_line(p),
        DIVERT => super::divert::divert_or_tunnel(p),
        KW_RETURN => super::divert::return_stmt(p),
        // `~ stmt` — the content-ground logic-line escape (charter §8.2,
        // RULED 2026-07-23, issue #1991). Kept in sync with `body_line`'s
        // own `TILDE` arm per this function's doc.
        TILDE => super::stmt::logic_line(p),
        // A bare `<-` outside a choice point warns (ruling #1263) — it must
        // reach `splice_outside_choice_point` here too, not fall through to
        // the prose scan and be silently swallowed as TEXT.
        THREAD => super::choice::splice_outside_choice_point(p),
        L_BRACE if at_choice_point(p) => super::choice::choice_point(p),
        L_BRACE if at_conditional(p) => conditional_block(p),
        L_BRACE if at_alternation(p) => alternation_block(p),
        EOF => {}
        _ => super::content::content_line_else_boundary(p),
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
    let has_branches = if is_multiline(p) {
        multiline_entries(p)
    } else {
        inline_alternatives(p)
    };
    // Item 4 (ruled #1258/#1261, brink-syntax parity —
    // `sequence_stopping_empty_emits_error`/`sequence_symbol_empty_emits_error`):
    // a marker with zero branches (`{~}`, `{&\n}`) is malformed, not a
    // silently-accepted degenerate case.
    if !has_branches {
        p.error("empty alternation: `{~}`/`{&}`/`{!}`/`{|}` need at least one branch".into());
    }
    p.expect(R_BRACE);
    p.exit_depth();
    p.finish_node();
}

/// Consumes the marker token and returns its kind (`TILDE`/`AMP`/`BANG`/
/// `PIPE`). The returned kind is currently unused by callers (the pipe form
/// no longer needs marker-specific handling — see `inline_alternatives`),
/// but is kept for symmetry and future marker-specific decisions.
fn alternation_marker(p: &mut Parser<'_, '_>) -> SyntaxKind {
    p.start_node(ALTERNATION_MARKER);
    // Caller (`at_alternation`) already verified the current token is one
    // of TILDE/AMP/BANG/PIPE.
    p.skip_ws();
    let kind = p.current();
    p.bump();
    p.finish_node();
    kind
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

/// Returns whether at least one `-`-prefixed `ENTRY` was parsed (item 4's
/// empty-alternation check — a body with only blank lines/comments and no
/// real `-` entry is exactly as empty as `{~}`).
fn multiline_entries(p: &mut Parser<'_, '_>) -> bool {
    let mut has_entry = false;
    loop {
        p.skip_ws();
        if p.at(R_BRACE) || p.at_eof() {
            break;
        }
        if p.at(MINUS) {
            entry(p);
            has_entry = true;
        } else {
            let before = p.pos();
            super::block::body_line(p);
            if p.pos() == before {
                p.error_recover("unexpected token before the first `-` entry");
            }
        }
    }
    has_entry
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
///
/// Returns whether at least one branch was seen (item 4's empty-alternation
/// check — a completely empty inline body, `{~}`, makes zero loop
/// iterations).
///
/// `PIPE` collides with lambda syntax — `{|x| x}` is byte-for-byte a valid
/// one-param lambda — but the pipe form is **always** a stopping-sequence,
/// never a "malformed lambda" (ruled 2026-07-22, superseding the earlier
/// pipe clause of "alternation markers win"): `{| }` is a real
/// stopping-sequence marker (charter §116), so `{|x| x}`, `{|heads|tails}`,
/// and `{|heads| tails}` are all valid two-branch stopping-sequences —
/// whitespace after the separator is ordinary branch content, not a lambda
/// signal. There is no way to distinguish a lambda from a spaced two-branch
/// alternation syntactically, so the marker wins uniformly and a lambda in
/// content position is spelled with parens (`{(|x| x)}`), the same escape
/// `!` uses (`{(!x)}`). An earlier revision special-cased "one separator
/// with a trailing space" as malformed; that over-fired on valid spaced
/// alternations and was removed.
fn inline_alternatives(p: &mut Parser<'_, '_>) -> bool {
    let mut has_any = false;
    loop {
        p.skip_ws();
        match p.current() {
            R_BRACE | EOF | NEWLINE => break,
            PIPE => {
                has_any = true;
                p.bump();
            }
            // `content_items_until` always stops at `HASH` too (see its
            // doc comment) — handle trailing tags explicitly so this loop
            // still makes progress instead of spinning.
            SyntaxKind::HASH => {
                has_any = true;
                super::content::tag_line_tail(p);
            }
            _ => {
                has_any = true;
                super::content::content_items_until(p, &[PIPE, R_BRACE, NEWLINE]);
            }
        }
    }
    has_any
}
