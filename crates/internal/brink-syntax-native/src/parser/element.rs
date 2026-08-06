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
    AT, BACKSLASH, BANG, BANG_DISPATCH, COLON, COMPACT_CUE, CUE, CUE_NAME, DISPATCH_NAME,
    DOC_COMMENT_OUTER, DOT, EOF, HASH, IDENT, L_BRACE, L_BRACKET, L_PAREN, NEWLINE, PARENTHETICAL,
    R_BRACE, R_BRACKET, R_PAREN, SCENE_BODY, SCENE_HEADING, SCENE_SLUG, SCENE_STITCH, SCENE_TITLE,
    TEXT,
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
///
/// **`\#` escapes the title-boundary role of `#` (issue #1738), mirroring
/// `content::tag()`'s and `cue_name()`'s identical fix.** `#` is one of the
/// four members of the ruled, final inline escape set (§8d.6), and before
/// this fix `scene_title` gave it zero escape treatment — an unconditional
/// `HASH` stop with no backslash awareness, the exact pre-fix shape
/// `tag()`/`cue_name()` had. Same `backslash_count`-parity carve-out, same
/// "backslash not stripped from the literal text" precedent: this scan
/// already tests `nth_raw(0)` directly (no `cur`/`raw` adjacency hazard
/// like `tag()`'s), so the parity check is safe to apply unconditionally.
/// Pinned by `a_scene_title_with_an_escaped_hash_does_not_end_the_title_early`.
///
/// **Superseded in part by issue #2045:** the raw CST node built here is
/// still an unstripped, lossless copy of the source — that part of the
/// precedent holds. But `ast::SceneTitle::text()` (`ast/nodes.rs`) is a
/// *later* materialization point that now strips a recognized escape's
/// backslash from the title's rendered display text, in parity with
/// `markup::escape`; `try_claim`/`try_dispatch`'s natural-notation matching
/// deliberately keeps reading the raw, unstripped `SyntaxNode` text instead
/// (its byte offsets are load-bearing for capture-group provenance, #1838)
/// — so "not stripped" is still true of the CST node and of that one
/// pattern-matching reader, but no longer true of every reader.
fn scene_title(p: &mut Parser<'_, '_>) {
    p.start_node(SCENE_TITLE);
    let mut backslash_count: u32 = 0;
    loop {
        let k = p.nth_raw(0);
        if matches!(k, EOF | NEWLINE | R_BRACE) {
            break;
        }
        if k == HASH && backslash_count & 1 == 0 {
            break;
        }
        if k == L_BRACKET && at_scene_slug(p) {
            break;
        }
        if k == BACKSLASH {
            backslash_count += 1;
        } else {
            backslash_count = 0;
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

/// Spec'd in `docs/prose-dialect-spec.md` §4.7a.
///
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
/// `}` only stops the scan once depth is back to zero. An `L_BRACE` is
/// excluded from the counter when it is preceded by an *odd* number of
/// consecutive raw `BACKSLASH`es (#1852) — `\{` is the literal-brace escape
/// (#1716/PR #1732), but `\\{` is an escaped backslash followed by a real,
/// depth-counted brace, so counting consecutive backslashes (not just the
/// immediately preceding token) is required to tell the two apart.
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
/// one member `tag()`'s does not, `HASH`, checked *before* the depth guard,
/// exactly like `NEWLINE`/`EOF` — so a `#` still cuts a name short even
/// while a brace is open. `COLON`, unlike `HASH`, is now depth-guarded the
/// same way `R_BRACE` is (#1851): a colon inside an unclosed `{` is part of
/// an interpolation, not the cue's terminator, so `@NAME {a:b} c.` no
/// longer stops at the `:` — it scans through to the balanced `}` like
/// `tag()` does.
///
/// **CONFIRMED (issue #1883, item 1): existing #1787 reasoning still
/// applies — `HASH` staying a hard, depth-blind reset is intentional, not
/// a residual gap to close.** `COLON`/`R_BRACE` are raw, ungrammared
/// punctuation this scan locally balances as "just text within this scan";
/// `HASH` is never that — an unescaped `HASH` always begins its own `TAG`
/// node, a real, tokenized CST boundary (the exact reasoning §4.7's
/// per-tag-scope ruling already states for why a fresh `HASH` must reset
/// `depth` to zero between sibling tags applies just as directly here).
/// Gating `HASH` by `depth == 0` the way `COLON` is would let an unescaped
/// `#` merge into the name's own text whenever a brace happens to be
/// open — turning an always-starts-a-new-`TAG` token into
/// sometimes-just-name-text depending on unrelated brace balance, which
/// would blur that same absolute boundary from the other direction. So
/// `@NAME {a#b} c.` still fails to parse: the name ends at `a`, `#b`
/// becomes a sibling `TAG`, and the still-open `{`'s matching `}` becomes
/// a stray top-level token once the name's own scan is long over. Pinned
/// by `a_hash_inside_an_open_brace_still_ends_a_cue_name_early`. See
/// `docs/prose-dialect-spec.md` §4.7b for the durable spec-level home.
///
/// **CONFIRMED (issue #1883, item 2): `\}`'s unconditional significance to
/// the depth check (mirroring `tag()`, above) is intentional, not a
/// residual asymmetry to close.** `\{`'s backslash-parity carve-out exists
/// because `\{` is one of the ruled, final four-character inline escape
/// set (§8d.6: `\< \{ \# \\`) — #1716/PR #1732 ruled it the literal-brace
/// escape. `}` is not a member of that set, so there is no equivalent
/// "`\}` is a literal, non-metacharacter close-brace" ruling to protect —
/// an `R_BRACE` preceded by a `BACKSLASH` is exactly what it looks like,
/// an ordinary backslash followed by an ordinary, structurally
/// significant `}`, so it keeps ending the name exactly like an unescaped
/// `}` would, at depth zero. Pinned by
/// `a_cue_names_own_unescaped_closing_brace_remains_the_terminator_even_when_preceded_by_a_backslash`.
/// See `docs/prose-dialect-spec.md` §4.7b for the durable spec-level home.
///
/// **`\#` escapes the name-boundary role of `#` (issue #1738), mirroring
/// `tag()`'s identical fix** — an unescaped `HASH` still cuts the name short
/// (the paragraph above, and #1883 — resolved, see §4.7b — are both about
/// *that* case and are unchanged by this), but `#` is one of the four
/// members of the ruled, final inline escape set (§8d.6), and `cue_name()`
/// gave it zero escape treatment before this fix: a `\#` inside a name
/// still ended it at the `#`, same defect `tag()` had. Same
/// `backslash_count`-parity carve-out,
/// same "backslash not stripped from the literal text" precedent as `\{`
/// just above. Pinned by
/// `a_cue_name_with_an_escaped_hash_does_not_end_the_name_early`.
///
/// **Superseded in part by issue #2045:** the raw CST node built here is
/// still an unstripped, lossless copy of the source — that part of the
/// precedent holds. But `ast::CueName::text()` (`ast/nodes.rs`) is a
/// *later* materialization point that now strips a recognized escape's
/// backslash the same way `ast::Tag::text()` does, so "not stripped" is
/// still true of the CST node but no longer true of every reader.
fn cue_name(p: &mut Parser<'_, '_>) {
    p.start_node(CUE_NAME);
    let mut depth: u32 = 0;
    let mut backslash_count: u32 = 0;
    loop {
        let raw = p.nth_raw(0);
        if matches!(raw, EOF | NEWLINE) {
            break;
        }
        // See `tag()`'s identical carve-out (issue #1738) for why this is
        // safe to check directly against `raw`: `cue_name()` already checks
        // every stop kind against `raw` (unlike `tag()`, which mixes `cur`
        // and `raw` — see that function's own doc for why), so
        // `backslash_count`'s parity is always accurate exactly when this
        // fires, with no additional adjacency care needed here.
        if raw == HASH && backslash_count & 1 == 0 {
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

// ── `!name` sigil dispatch (§3.5b, issue #2004) ──────────────────────

/// `!name` at item position — the self-announcing annotation-element
/// dispatch sigil. The `!` and the name must be **adjacent**, the same
/// adjacency discipline [`at_cue`] applies to `@NAME`: a bare `!` followed
/// by a gap (`! Wait, listen.`) is ordinary prose punctuation, never a
/// malformed dispatch attempt.
///
/// Recognition only — whether `name` actually names a declared
/// `@[element(args = "…")]` handler, and whether that handler's pattern
/// matches the remainder, is a lowering-time question
/// (`hir::lower_native::element::try_dispatch`), not this function's. An
/// unresolved `!name` line still parses cleanly (see [`bang_dispatch`]'s
/// doc for why that composes with `\!`, the ruled line-start escape).
pub(crate) fn at_bang_dispatch(p: &Parser<'_, '_>) -> bool {
    p.at(BANG) && p.nth(1) == IDENT && p.nth_adjacent(0)
}

/// Parses a confirmed [`at_bang_dispatch`] position into one `BANG_DISPATCH`:
/// the `!`, a `DISPATCH_NAME` holding the dispatching identifier, and the
/// remainder as a fused `CONTENT_LINE` via `content::content_line` —
/// the exact same fused-line technique [`cue_line`]'s `COMPACT_CUE` arm uses
/// for `@NAME: text`, so interpolation, glue, inline markup and trailing tags
/// all parse in the remainder exactly as they would in any other content
/// line.
///
/// Reusing `content_line` here, rather than a bespoke raw-text scan, is
/// deliberate: `hir::lower_native::element::try_dispatch` requires the
/// remainder to be **wholly literal** (no interpolation etc.) before a
/// portable-regex pattern can match it — exactly the same requirement
/// natural-notation claiming already enforces
/// (`hir::lower_native::element::candidate`) — so a dynamic remainder still
/// parses, and is diagnosed loudly downstream (`E129`, "parses cleanly but
/// has no HIR lowering yet") rather than being rejected here at the grammar
/// level.
///
/// Composes with `\!` (§8d.6, the ruled line-start escape,
/// `markup::at_line_start_escape`) by construction, not by a special case
/// here: `\!` lexes as `BACKSLASH` `BANG`, never a bare `BANG`, so
/// `body_line`'s own dispatch on `p.current()` never reaches this function
/// for an escaped `!` — it falls to the ordinary content-line default arm,
/// exactly like it did before this sigil existed.
pub(crate) fn bang_dispatch(p: &mut Parser<'_, '_>) {
    p.skip_ws();
    p.start_node(BANG_DISPATCH);
    p.bump(); // `!`
    p.start_node(DISPATCH_NAME);
    p.bump(); // the dispatching IDENT
    p.finish_node();
    super::content::content_line(p);
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
