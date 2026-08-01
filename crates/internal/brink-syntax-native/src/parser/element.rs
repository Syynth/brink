//! Prose block elements — the built-in screenplay preset's *grammar*
//! (`docs/prose-dialect-spec.md` §8b/§8d, RULED 2026-07-25 across sittings
//! 4–5).
//!
//! Four line shapes plus one body shape:
//!
//! ```text
//! INT. MARKET SQUARE - NIGHT [market] #tense #act1   scene heading  (§8b.2/.3)
//!   The square is empty.                             ↑ header-scoped body
//!
//! @VENDOR #(v.o.)                                    block cue      (§8d.4)
//! (hushed)                                           parenthetical
//! You shouldn't be here after dark.                  dialogue (chain rule)
//!
//! @KID: Says who?                                    compact cue    (§8b.9)
//! ```
//!
//! # What this module is *not*
//!
//! Classification only — nothing here lowers. Element **roles**
//! (attached-forward vs content vs structural), compile-time attachment
//! and the preset's data payloads are issues #1717/#1720. (The conventions
//! schema's `lower:` column this doc used to name is **dissolved** —
//! `docs/decision-log.md` 2026-07-31, "Conventions are annotated
//! handlers": there is a handler or there isn't.)
//!
//! Downstream, `hir::lower_native::element` (issue #1838) now claims a
//! heading whose text a natural-notation `@[element(claims = "…")]`
//! handler matches, rewriting it to one call. Every *other* shape here —
//! and any heading nothing claims — still meets `hir::lower_native`'s
//! loud-`E129` default arm ("parses cleanly but has no HIR lowering yet in
//! this slice"), which is the deliberate staging, not a silent drop.
//!
//! # Two rulings this module implements literally
//!
//! **Header-scoped stitch bodies (§8b.2).** A scene heading's body runs to
//! the next heading or the enclosing close — which *amends charter §4's
//! "braces are the universal body delimiter"* for preset heading-elements
//! in prose-ground only. Consequences were embraced, not overlooked:
//! heading-stitches are **flat siblings** (scenes don't nest, as on a real
//! page), and deeper nesting keeps the general `flow x { … }` spelling,
//! which stays first-class. This restores ink's own header-scoped stitch;
//! it is not an invention.
//!
//! **The lyrics element is dropped (§8b.1)** — Fountain's `~` force-marker
//! collides with the logic-line escape, and the conflict dies with the
//! element. There is deliberately no `LYRICS` shape here.

use crate::SyntaxKind::{
    AT, BACKSLASH, COLON, COMPACT_CUE, CUE, CUE_NAME, DOC_COMMENT_OUTER, DOT, EOF, HASH, IDENT,
    L_BRACE, L_BRACKET, L_PAREN, NEWLINE, PARENTHETICAL, R_BRACE, R_BRACKET, R_PAREN, SCENE_BODY,
    SCENE_HEADING, SCENE_SLUG, SCENE_STITCH, SCENE_TITLE, TEXT,
};

use super::Parser;

// ── Scene headings & header-scoped stitches (§8b.2/.3) ───────────────

/// The declared heading pattern: an `INT.`/`EXT.` prefix at the very start
/// of a body item (§8b.3; the inventory's "`INT.`/`EXT.` prefix pattern").
/// `INT./EXT.` — the combined prefix — starts with the same two tokens, so
/// it is covered by the same guard.
///
/// The prefix is matched by **text**, not by a reserved keyword: `INT` and
/// `EXT` stay ordinary identifiers everywhere else in the language (a
/// `var INT = 1` binding is untouched). Only the exact upper-case spelling
/// followed by a `.` at item position claims a line, which is the
/// explicit-format posture — the preset never guesses from ALL-CAPS shape.
pub(crate) fn at_scene_heading(p: &Parser<'_, '_>) -> bool {
    at_scene_heading_at(p, 0)
}

/// [`at_scene_heading`], checked at non-trivia lookahead offset `n` instead
/// of the current position — the shared core [`at_scene_heading_past_leading_doc`]
/// reuses after skipping past a `///` run.
fn at_scene_heading_at(p: &Parser<'_, '_>, n: usize) -> bool {
    p.nth(n) == IDENT && matches!(p.nth_text(n), "INT" | "EXT") && p.nth(n + 1) == DOT
}

/// True when a scene heading is at the current position, **or** follows a
/// leading `///` doc-comment run (review finding on #1715): `scene_stitch`'s
/// body loop calls this, not the bare [`at_scene_heading`], as its
/// terminator check.
///
/// Without this, a documented second heading wasn't recognized as ending
/// the current stitch's body: `DOC_COMMENT_OUTER` is not trivia
/// (`SyntaxKind::is_trivia` = `WHITESPACE`/`LINE_COMMENT`/`BLOCK_COMMENT`
/// only), so [`at_scene_heading`]'s `p.at(IDENT)` check failed on the `///`
/// token, the loop did not break, and `block::item` went on to consume the
/// doc run and recurse into a *nested* `scene_stitch` — silently violating
/// §8b.2's "heading-stitches are flat siblings, scenes do not nest" for any
/// heading past the first that carries a doc comment.
///
/// Mirrors `doc_comment::consume_doc_run`'s "one or more `///` lines, each
/// terminated by a `NEWLINE`" shape closely enough for a lookahead (not a
/// consumer): it does not special-case a blank line ending the run early,
/// because over-breaking on that rare shape is harmless here — the loop
/// would stop one iteration sooner than strictly necessary, and the
/// dispatcher it hands off to (`block::item`) still resolves the
/// doc-attachment question exactly the same way it always does.
fn at_scene_heading_past_leading_doc(p: &Parser<'_, '_>) -> bool {
    let mut n = 0;
    while p.nth(n) == DOC_COMMENT_OUTER && p.nth(n + 1) == NEWLINE {
        n += 2;
    }
    at_scene_heading_at(p, n)
}

/// `INT. TITLE [slug] #tags` followed by every item up to the next heading
/// / the enclosing `}` / EOF — one `SCENE_STITCH` wrapping a
/// `SCENE_HEADING` and its braceless `SCENE_BODY` (§8b.2).
///
/// `doc` is a leading `///` run already consumed by `block::item`, threaded
/// through so it attaches as this node's leading `DOC_COMMENT` child, the
/// same way every declaration header does (B0.6b) — a heading declares a
/// stitch (§3.2's structural exception), so it documents like one.
///
/// No `enter_depth` guard here, unlike `braced_item_list`: this rule is not
/// self-recursive. The body loop *stops* at the next heading rather than
/// recursing into it (that is exactly what "flat siblings" means), so the
/// only way to nest deeper is through a `flow x { … }`, whose own
/// `braced_item_list` carries the depth guard.
pub(crate) fn scene_stitch(p: &mut Parser<'_, '_>, doc: Option<rowan::Checkpoint>) {
    super::doc_comment::open_with_doc(p, SCENE_STITCH, doc);
    scene_heading(p);

    p.start_node(SCENE_BODY);
    // A scene body starts a fresh dialogue chain and restores the
    // enclosing one on the way out — a heading is never part of a chain.
    let outer_chain = p.set_cue_chain(false);
    loop {
        p.skip_ws();
        if p.at_eof() || p.at(R_BRACE) || at_scene_heading_past_leading_doc(p) {
            break;
        }
        let before = p.pos();
        super::block::item(p);
        if p.pos() == before {
            p.error_recover("unexpected token inside scene body");
        }
    }
    p.set_cue_chain(outer_chain);
    p.finish_node();

    p.finish_node();
}

/// One heading line. Line order is fixed by §8b.3 — **pattern, `[slug]`,
/// tags** — and the two rejected slug spellings stay rejected: `#x#`
/// (clashes with the tag lexer) and `{x}` (lexes as interpolation;
/// headings get no carve-out, so a `{` on a heading line is just title
/// text as far as this rule is concerned).
fn scene_heading(p: &mut Parser<'_, '_>) {
    // Leading indentation belongs to the enclosing body, not to the
    // heading's display name — flush it before the node opens.
    p.skip_ws();
    p.start_node(SCENE_HEADING);
    scene_title(p);
    if at_scene_slug(p) {
        scene_slug(p);
    }
    super::content::tag_line_tail(p);
    p.finish_node();
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}

/// Everything before the optional `[slug]`/tags: the display name (§3.3).
/// Raw-bumped like `content::text_run_until`, so interior spacing and any
/// run of `.`/`-`/digits in a slugline survives verbatim in one `TEXT`-
/// shaped node.
fn scene_title(p: &mut Parser<'_, '_>) {
    p.start_node(SCENE_TITLE);
    loop {
        let k = p.nth_raw(0);
        if matches!(k, EOF | NEWLINE | HASH | R_BRACE) {
            break;
        }
        if k == L_BRACKET && at_scene_slug(p) {
            break;
        }
        p.bump();
    }
    p.finish_node();
}

/// True at a trailing `[ident]` slug — recognized **only** at the tail of
/// the heading (nothing but tags or the line end may follow), so a `[` in
/// the middle of a title stays title text rather than silently claiming
/// part of the display name.
fn at_scene_slug(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACKET)
        && p.nth(1) == IDENT
        && p.nth(2) == R_BRACKET
        && matches!(p.nth(3), HASH | NEWLINE | R_BRACE | EOF)
}

fn scene_slug(p: &mut Parser<'_, '_>) {
    p.start_node(SCENE_SLUG);
    p.expect(L_BRACKET);
    p.expect(IDENT);
    p.expect(R_BRACKET);
    p.finish_node();
}

// ── Cues: the block form and the compact form (§8b.9, §8d.4) ─────────

/// `@NAME` at item position. The `@` and the name must be **adjacent** —
/// `@ home tomorrow` keeps `SyntaxKind::AT`'s documented promise that "a
/// lone `@` in prose stays plain text", and `@[…]` is a different token
/// (`AT_L_BRACKET`) entirely, so the annotation channel cannot collide.
pub(crate) fn at_cue(p: &Parser<'_, '_>) -> bool {
    p.at(AT) && p.nth(1) == IDENT && p.nth_adjacent(0)
}

/// Parses **both** ruled cue patterns off one prefix, deciding between
/// them at the `:` (§8b.9 — the compact cue is "a second declared pattern
/// beside the block cue", not a rewrite of it, so each gets its own node
/// kind):
///
/// ```text
/// @VENDOR #(v.o.)        → CUE          (+ tags: the ruled home for
/// (hushed)                               cue extensions, §8d.4)
/// @KID: Says who?        → COMPACT_CUE  (+ the fused CONTENT_LINE)
/// ```
pub(crate) fn cue_line(p: &mut Parser<'_, '_>) {
    // `eat`, not a raw `bump`: it flushes any pending trivia first, so the
    // checkpoint opens on the `@` itself even when the caller left leading
    // whitespace unconsumed.
    p.skip_ws();
    let start = p.checkpoint();
    p.eat(AT);
    cue_name(p);
    if p.at(COLON) {
        p.start_node_at(start, COMPACT_CUE);
        p.bump(); // COLON
        // The fused dialogue line — an ordinary content line, so
        // interpolation, glue, inline markup and trailing tags all work in
        // it exactly as they do under a block cue. It consumes its own
        // terminating NEWLINE.
        super::content::content_line(p);
        p.finish_node();
        return;
    }
    p.start_node_at(start, CUE);
    super::content::tag_line_tail(p);
    p.finish_node();
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}

/// The name run after the `@` sigil. Raw-bumped up to `:`/tags/line end,
/// so a multi-word character name (`@MARKET VENDOR`) is one name rather
/// than a name plus stray text.
///
/// #1786: confirmed to share `content::tag()`'s pre-#1728 shape — an
/// unconditional stop at the first raw `R_BRACE` mistook a `}` that merely
/// *echoed* a `{` already inside the name (e.g. `@NAME {gold} coins.`
/// inside a `flow f() { … }` body) for the enclosing block's own closer,
/// ending the name — and the flow's `BLOCK` — early. Fixed the same way
/// `tag()` was: `depth` counts literal, unpaired `{`s bumped so far, and a
/// `}` only stops the scan once depth is back to zero. An `L_BRACE`
/// immediately preceded by a raw `BACKSLASH` is excluded from the counter
/// — `\{` is the literal-brace escape (#1716/PR #1732), not a
/// metacharacter, so counting it as a depth-opener would let a later
/// unescaped `}` swallow the enclosing closer on perfectly ordinary
/// escaped text.
///
/// Same tradeoff as `tag()`, stated the same way: a *balanced* brace in a
/// cue name no longer terminates it early (the bug this fixes), but a
/// genuinely *unbalanced*, unescaped `{` left open in a name now eats the
/// enclosing single-line block's own same-line `}` closer instead of
/// stopping there — inherent to depth-based balancing over raw text with
/// no real grammar to bound it. Pinned by
/// `an_unbalanced_open_brace_in_a_cue_name_eats_the_enclosing_blocks_own_closer`.
///
/// This is not full parity with `tag()`, though: `cue_name`'s stop set has
/// two members `tag()`'s does not, `COLON` and `HASH`, and both are
/// checked *before* the depth guard, exactly like `NEWLINE`/`EOF` — so a
/// `:` or `#` still cuts a name short even while a brace is open,
/// unconditionally consuming neither into the balanced scan. A name like
/// `@NAME {a:b} c.` still stops at the `:` inside the unclosed `{`, well
/// short of the cascade this fix removes for a `}`-only case.
fn cue_name(p: &mut Parser<'_, '_>) {
    p.start_node(CUE_NAME);
    let mut depth: u32 = 0;
    let mut backslash_count: u32 = 0;
    loop {
        let raw = p.nth_raw(0);
        if matches!(raw, EOF | NEWLINE | HASH) {
            break;
        }
        // COLON is a stop only at depth zero: a colon inside braces (e.g.
        // `{a:b}`) is part of an interpolation, not the cue's terminator.
        if raw == COLON && depth == 0 {
            break;
        }
        if raw == R_BRACE && depth == 0 {
            break;
        }
        // Track consecutive backslashes before a brace to detect escaped
        // braces correctly (#1852: `\\{` should count the brace, not escape
        // it). An L_BRACE is only excluded from depth counting if preceded
        // by an odd number of backslashes; an even number means the
        // backslashes themselves are escaped. Use bitwise AND to check for
        // even (lowest bit is 0).
        match raw {
            L_BRACE if backslash_count & 1 == 0 => {
                depth += 1;
                backslash_count = 0;
            }
            R_BRACE => {
                depth = depth.saturating_sub(1);
                backslash_count = 0;
            }
            BACKSLASH => backslash_count += 1,
            _ => {
                backslash_count = 0;
            }
        }
        p.bump();
    }
    p.finish_node();
}

// ── Parentheticals (chain-gated) ─────────────────────────────────────

/// True at a whole-line `( … )` **inside a live cue chain** — the
/// inventory's "`(…)` line, chain: after cue or dialogue".
///
/// The chain gate is load-bearing, not decoration. A bare `(name)` line is
/// already a shipped construct: G-1's content-line label
/// (`content::at_content_label`), used as a backward-divert target — the
/// `tests/tier1-brink-respell/labeled-mid-flow-gather` fixture has two of
/// them. Requiring a live chain means those lines, and every other
/// `(label)` outside dialogue, parse exactly as they did before this rule
/// existed; only a `(…)` line that follows a cue can become a
/// parenthetical. That is also why the residual ambiguity
/// `content::at_content_label` documents ("`(Sighing) I trudge on.` is
/// indistinguishable from a real label by construction") does not widen
/// here: a parenthetical must fill its whole line.
pub(crate) fn at_parenthetical(p: &Parser<'_, '_>) -> bool {
    if !p.in_cue_chain() || p.nth_raw(0) != L_PAREN {
        return false;
    }
    let Some(close) = closing_paren_offset(p) else {
        return false;
    };
    let mut after = close + 1;
    while p.nth_raw(after).is_trivia() {
        after += 1;
    }
    matches!(p.nth_raw(after), NEWLINE | EOF | HASH | R_BRACE)
}

/// Raw offset of the `)` closing the `(` at offset 0, or `None` if the
/// line ends first. Bounded by the physical line — it never scans past a
/// `NEWLINE`/EOF, so this cannot become an unbounded walk of the token
/// stream.
fn closing_paren_offset(p: &Parser<'_, '_>) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = 0usize;
    loop {
        match p.nth_raw(i) {
            EOF | NEWLINE => return None,
            L_PAREN => depth += 1,
            R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// `(hushed)` — the delivery line. Trailing tags are accepted for the same
/// reason every other prose line accepts them; the parenthetical's own
/// text is a single raw run, since a delivery is literal text, never
/// interpolated content.
pub(crate) fn parenthetical(p: &mut Parser<'_, '_>) {
    p.start_node(PARENTHETICAL);
    p.expect(L_PAREN);
    p.start_node(TEXT);
    let mut depth = 1usize;
    loop {
        match p.nth_raw(0) {
            EOF | NEWLINE => break,
            L_PAREN => depth += 1,
            R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        p.bump();
    }
    p.finish_node();
    p.expect(R_PAREN);
    super::content::tag_line_tail(p);
    p.finish_node();
    if p.at(NEWLINE) {
        p.skip_ws();
        p.bump();
    }
}
