//! Inline markup — the XML-shaped span layer inside prose content
//! (`docs/prose-dialect-spec.md` §4, RULED 2026-07-25, issue #1716).
//!
//! ```text
//! He hands you <item id="lantern">the old lantern</item>.   span, attrs
//! The bell tolls again. <pause/>                            point marker (§8b.11)
//! <center>LATER</center>                                    markup, not an element (§8d.3)
//! Hello \<world\>                                           escaped literals (§8d.6)
//! ```
//!
//! # Blunt lexing (§4.1), no new lexer tokens
//!
//! `GLUE` (`<>`) and `THREAD` (`<-`, splice) are already distinct compound
//! tokens at the lexer (`lexer/punctuation.rs`), so a bare `LT` reaching
//! this module can never be either. Recognition is a **parser**-level
//! adjacency check over already-existing tokens, the same discipline
//! `element::at_cue` uses for `@NAME`: `LT` immediately (no trivia) followed
//! by `IDENT` opens a span ([`at_span_open`]); `LT SLASH IDENT` immediately
//! closes one ([`at_span_close`]). A `<` that doesn't qualify either way —
//! `5 < 10`, a lone `<3` — falls through to ordinary `TEXT`, unchanged.
//!
//! # Hyphenated tag names (§4.1, RULED 2026-08-01, issue #1996)
//!
//! A tag name may contain `-`, but **only as an internal separator between
//! two `IDENT` segments** — `<fade-in>` is legal, a leading or trailing
//! hyphen is not (`<-x>`, `<x->`). This is a **parser**-level widening of
//! the tag-name shape only ([`tag_name_len`]/[`tag_name_text`]), scoped to
//! span-tag position — it does not touch `IDENT` lexing itself
//! (`lexer/ident.rs`), so identifiers everywhere else in the language are
//! unaffected.
//!
//! The leading-hyphen ban isn't just a style choice; it's partly a
//! consequence already forced by the lexer. An **open** tag's name can
//! never start with `-` in the first place: `<-` is already claimed by
//! `THREAD` (splice) at the lexer (`lexer/punctuation.rs`), so `<-x>` never
//! even reaches [`at_span_open`] as a `LT` — it lexes as `THREAD IDENT`.
//! A **close** tag's `</-x>` *is* lexically distinguishable (`LT SLASH
//! MINUS IDENT`, since `SLASH` breaks the `THREAD` pattern) — but
//! [`at_span_close`] requires its own first name token to be `IDENT`, not
//! `MINUS`, so it is rejected there for the same reason, keeping the rule
//! symmetric across open/close by construction rather than by a
//! special-cased check.
//!
//! A trailing hyphen is representable at the token level (a lone `-` not
//! immediately followed by `>`/`=` lexes as `MINUS`) but is deliberately
//! **not** folded into the name: [`tag_name_len`] only extends a name
//! across a `MINUS` that is itself followed by another adjacent `IDENT`.
//! `<x->` stops the name at `x`, leaving the `-` (or, when it sits directly
//! before `>`, the whole `->` — greedily lexed as one `DIVERT` token)
//! unconsumed; [`span`]'s own `p.expect(GT)` then reports a clear parse
//! error instead of silently accepting the dangling hyphen as part of the
//! name.
//!
//! A continuation segment (the word after a `-`) may also be a reserved
//! keyword ([`is_name_segment`]) — native keywords are reserved everywhere
//! in *code*, but a tag name is freeform prose vocabulary (§4.2), and
//! `fade-in` is this very ruling's own worked example even though `in` is
//! `KW_IN`. This leniency is deliberately narrow: only a segment reached
//! after an already-confirmed `-` gets it; the tag's opening segment still
//! goes through [`at_span_open`]/[`at_span_close`]'s existing `IDENT`-only
//! check unchanged, so a tag literally *named* a bare keyword (`<in>`,
//! hyphen or not) is still not representable — a pre-existing limitation
//! this issue doesn't ask to widen.
//!
//! # Nesting doctrine (§4.3) — enforced by sharing one scanning engine
//!
//! A tag must close in the same fragment scope it opened in. [`span`] does
//! not implement its own content scanner — it calls back into
//! `content::content_items_until_impl` with the **same** `stop` set its own
//! caller was given, plus an `expected_close` name. That single point is
//! what makes the doctrine unrepresentable-to-violate rather than merely
//! checked: if the enclosing fragment's own boundary (`NEWLINE`, an
//! enclosing `}`/`]`, EOF, or any other caller-supplied stop token) is
//! reached before the matching close tag, the recursive scan returns
//! without having consumed it, and [`span`] reports the tag unclosed — it
//! has no way to "reach out" and consume a close tag sitting past that
//! boundary, because the boundary is exactly what stops the shared scanner.
//! `<b>hello {name}</b>` closes inside the outer scope it opened in ✓.
//! `{tired: <i>yawn</i>|Ready.}` closes inside the *inner* scope (the
//! branch) it opened in ✓. `<b>hi {tired: there</b>|friend}` opens outside
//! the conditional and tries to close inside its branch — the branch's own
//! scan (stop set includes the branch boundary) reaches that boundary
//! first, `</b>` is never seen there, and `<b>` is reported unclosed ✗.
//!
//! Spans are also **line-scoped** (§4.3, mechanically forced): every
//! content-line-level caller's `stop` set already includes `NEWLINE`, so a
//! span can never survive past the end of its line either.
//!
//! # Escape set is final (§8d.6)
//!
//! `\<` `\{` `\#` `\\` — and only those four, **inline** (anywhere in a
//! line). A `BACKSLASH` before anything else is a compile error
//! ([`escape`]), never silently swallowed as a literal backslash.
//!
//! §8d.6 also rules a second, disjoint pair as **line-start** escapes —
//! `\!` `\@` ([`at_line_start_escape`]/[`line_start_escape`], issue #1744) —
//! protecting a literal leading `!`/`@` from the sigils those characters
//! carry there (`@NAME` cue dispatch, the reserved `!name` annotation
//! sigil). "Line-start" means the first item `content::content_line`/
//! `content_line_else_boundary` is asked to scan — which is the true start
//! of a physical line for an ordinary content line, but is also right
//! after a compact cue's `@NAME:` prefix (`element::cue_line`'s
//! `COMPACT_CUE` arm calls `content_line` directly for the fused dialogue
//! line), since that call reuses the same entry point. A `\!`/`\@`
//! anywhere else in a line is not part of the inline four and hits the
//! same compile error as any other unrecognized backslash.

use crate::SyntaxKind::{
    AT, BACKSLASH, BANG, EQ, ERROR, GT, HASH, IDENT, L_BRACE, LT, MINUS, QUOTE, SLASH, SPAN,
    SPAN_ATTR, SPAN_ATTR_VALUE, SPAN_NAME, STRING_ESCAPE, STRING_TEXT,
};

use super::Parser;

// ── Recognition (blunt lexing, §4.1) ─────────────────────────────────

/// True at `<ident` with zero gap — a span's open tag.
pub(crate) fn at_span_open(p: &Parser<'_, '_>) -> bool {
    p.at(LT) && p.nth(1) == IDENT && p.nth_adjacent(0)
}

/// True at `</ident` with zero gap — a span's close tag. Does not require
/// the trailing `>` to be present; that is a normal `expect` at the actual
/// consumption site ([`span`]/[`consume_stray_close`]), not part of
/// recognition (mirrors [`at_span_open`], which likewise doesn't demand the
/// open tag's own `>`/attrs be well-formed to be *recognized* as an
/// attempt).
pub(crate) fn at_span_close(p: &Parser<'_, '_>) -> bool {
    p.at(LT) && p.nth(1) == SLASH && p.nth(2) == IDENT && p.nth_adjacent(0) && p.nth_adjacent(1)
}

/// [`at_span_close`], plus the close tag's name matches `name` exactly —
/// comparing the **full**, possibly hyphenated, tag name
/// ([`tag_name_text`]), never just its first token. Only meaningful where
/// [`at_span_close`] already holds.
///
/// `pub(crate)`, not private: `content::content_items_until_impl` also
/// calls this directly, to decide whether a close tag it reaches is the one
/// its own `expected_close` frame is looking for (see that function's
/// doc) — comparing only the close tag's leading `IDENT` segment there
/// would wrongly treat e.g. `</fade>` as matching a `<fade-in>` open tag
/// (both start with the `fade` segment).
pub(crate) fn at_span_close_named(p: &Parser<'_, '_>, name: &str) -> bool {
    at_span_close(p) && tag_name_text(p, 2, tag_name_len(p, 2)) == name
}

/// Length, in (non-trivia) lookahead tokens, of a tag name starting at
/// offset `start` — `p.nth(start)` must already be a confirmed `IDENT`
/// (every call site checks this before calling: [`at_span_open`]/
/// [`at_span_close`]'s own recognition, or [`span`]'s freshly-bumped `<`).
/// A name is `IDENT (MINUS IDENT)*`, all mutually adjacent (no whitespace/
/// comment gap anywhere) — see the module doc's "Hyphenated tag names"
/// section for why a leading hyphen can't reach here and a trailing one is
/// deliberately left unconsumed. Always returns an odd count (1, 3, 5, …).
fn tag_name_len(p: &Parser<'_, '_>, start: usize) -> usize {
    let mut len = 1;
    while p.nth_adjacent(start + len - 1)
        && p.nth(start + len) == MINUS
        && p.nth_adjacent(start + len)
        && is_name_segment(p.nth(start + len + 1))
    {
        len += 2;
    }
    len
}

/// True for a token kind that can stand as a hyphen-continuation segment of
/// a tag name (the word *after* a `-`) — `IDENT`, or any reserved keyword.
/// Native keywords are reserved everywhere in ordinary code (Rust-style,
/// per `SyntaxKind::is_keyword`'s doc), but a tag name is freeform prose
/// vocabulary (§4.2), not code: `<fade-in>` must work even though `in` is
/// `KW_IN` in expression position. Deliberately narrower than "any keyword
/// anywhere in a tag name" — only a *continuation* segment (reached after
/// an already-confirmed `-`) gets this leniency; the tag's own opening
/// segment still goes through `at_span_open`/`at_span_close`'s existing
/// `IDENT`-only check, unchanged.
fn is_name_segment(k: crate::SyntaxKind) -> bool {
    k == IDENT || k.is_keyword()
}

/// The concatenated source text of a `len`-token tag name starting at
/// lookahead offset `start` (as computed by [`tag_name_len`]) —
/// `"fade"` + `"-"` + `"in"` = `"fade-in"`.
fn tag_name_text(p: &Parser<'_, '_>, start: usize, len: usize) -> String {
    (0..len).map(|i| p.nth_text(start + i)).collect()
}

/// True at `/>` with zero gap — a self-closing tag (the point-marker shape,
/// §8b.11: `<pause/>`, `<sfx name="bell"/>`).
fn at_self_close(p: &Parser<'_, '_>) -> bool {
    p.at(SLASH) && p.nth(1) == GT && p.nth_adjacent(0)
}

// ── Spans ──────────────────────────────────────────────────────────

/// Parse one `SPAN`, starting at a confirmed [`at_span_open`] position.
/// `stop`/`stop_at_else_arm` are forwarded unchanged into the recursive
/// content scan for the span's body — see the module doc's nesting-doctrine
/// section for why that single forwarding is the whole enforcement
/// mechanism.
pub(crate) fn span(p: &mut Parser<'_, '_>, stop: &[crate::SyntaxKind], stop_at_else_arm: bool) {
    p.start_node(SPAN);
    p.bump(); // `<`
    let name_len = tag_name_len(p, 0);
    let name = tag_name_text(p, 0, name_len);
    p.start_node(SPAN_NAME);
    for _ in 0..name_len {
        p.bump(); // the name: one `IDENT`, or `IDENT (MINUS IDENT)*` if hyphenated
    }
    p.finish_node();

    span_attrs(p);

    if at_self_close(p) {
        p.bump(); // `/`
        p.bump(); // `>`
        p.finish_node();
        return;
    }
    p.expect(GT);

    // MAX_DEPTH guard (CLAUDE.md: "guard against unbounded growth") — spans
    // recurse into the shared content scanner exactly like `interpolation`
    // recurses into `expr::expression`; a pathologically deep
    // `<a><a><a>…` input must not blow the stack. When the depth budget is
    // exhausted, `enter_depth` has already recorded its own diagnostic;
    // skip scanning the (unbounded, unsafe-to-recurse-into) body rather
    // than descend further, and fall through to the close-tag check below,
    // which will correctly — if redundantly — report this span unclosed
    // too (forward progress is unaffected: the open tag's tokens are
    // already consumed above).
    if p.enter_depth() {
        super::content::content_items_until_impl(p, stop, stop_at_else_arm, Some(name.as_str()));
        p.exit_depth();
    }

    if at_span_close_named(p, &name) {
        // `tag_name_len` reads via lookahead only (doesn't move `p`), so it
        // must run before `<`/`/` are bumped — offset 2 is the close name's
        // first token relative to the *current* position, exactly what
        // `at_span_close`/`at_span_close_named` just checked against.
        let close_len = tag_name_len(p, 2);
        p.bump(); // `<`
        p.bump(); // `/`
        for _ in 0..close_len {
            p.bump(); // the close name (mirrors the open name above)
        }
        p.expect(GT);
    } else {
        p.error(format!(
            "unclosed tag `<{name}>`: expected a matching `</{name}>` in the same line/scope"
        ));
    }
    p.finish_node();
}

/// Zero or more `name="value"` attributes on a span's open tag. Guarded by
/// the same positive-lookahead discipline `content::at_content_label` uses
/// (`IDENT` *and* the following token is `EQ`) so a bare trailing word in a
/// malformed tag doesn't get misread as the start of an attribute.
fn span_attrs(p: &mut Parser<'_, '_>) {
    loop {
        p.skip_ws();
        if p.at(IDENT) && p.nth(1) == EQ {
            p.start_node(SPAN_ATTR);
            p.bump(); // name
            p.skip_ws();
            p.expect(EQ);
            p.skip_ws();
            span_attr_value(p);
            p.finish_node();
        } else {
            break;
        }
    }
}

/// `"value"` — static text only (§4.1's worked examples are all static;
/// see [`crate::SyntaxKind::SPAN_ATTR_VALUE`]'s doc for why this
/// deliberately does not reuse `expr::string_lit`'s `{expr}`-interpolation
/// support).
fn span_attr_value(p: &mut Parser<'_, '_>) {
    p.start_node(SPAN_ATTR_VALUE);
    p.expect(QUOTE);
    while matches!(p.current(), STRING_TEXT | STRING_ESCAPE) {
        p.bump();
    }
    p.expect(QUOTE);
    p.finish_node();
}

/// A close tag reached that does not match what the current (possibly
/// absent) span frame is looking for — either a genuinely stray close with
/// no live open anywhere, or one belonging to an ancestor that hasn't
/// finished yet (a nesting-doctrine violation in progress). Reported loudly
/// and consumed as a single `ERROR`-wrapped unit so scanning can keep
/// making forward progress; the ancestor (if any) still gets its own
/// "unclosed tag" diagnostic when its own scan runs out of input.
pub(crate) fn consume_stray_close(p: &mut Parser<'_, '_>) {
    p.error("unexpected closing tag: no matching open tag here".to_owned());
    p.start_node(ERROR);
    // Read via lookahead (offset 2, the name's first token relative to the
    // still-unconsumed `<`) before bumping anything — mirrors `span`'s own
    // close-tag consumption.
    let name_len = tag_name_len(p, 2);
    p.bump(); // `<`
    p.bump(); // `/`
    for _ in 0..name_len {
        p.bump(); // name (possibly hyphenated)
    }
    if p.at(GT) {
        p.bump();
    }
    p.finish_node();
}

// ── Escapes (§8d.6, the set is final) ────────────────────────────────

/// True when `k` is one of the four escapable tokens `\<` `\{` `\#` `\\`
/// produce literals from. **Do not extend this** — §8d.6 rules the set
/// final.
fn is_escapable(k: crate::SyntaxKind) -> bool {
    matches!(k, LT | L_BRACE | HASH | BACKSLASH)
}

/// `BACKSLASH` plus exactly one of `< { # \`, immediately adjacent (no
/// trivia) — anything else is a compile error, per §8d.6 ("backslash before
/// anything else is a compile error"), recovered the same
/// single-token-`ERROR`-wrap way [`consume_stray_close`] does.
pub(crate) fn escape(p: &mut Parser<'_, '_>) {
    if is_escapable(p.nth(1)) && p.nth_adjacent(0) {
        p.start_node(crate::SyntaxKind::ESCAPE);
        p.bump(); // `\`
        p.bump(); // the escaped token
        p.finish_node();
    } else {
        p.error_recover(
            "invalid escape sequence: `\\` must be immediately followed by one of `<`, `{`, `#`, `\\`",
        );
    }
}

// ── Line-start escapes `\!` `\@` (§8d.6, issue #1744) ─────────────────
//
// A second, disjoint escape set from [`is_escapable`]'s four inline
// escapes — **not** an extension of it (§8d.6 rules that set final; see
// its doc comment). `\!` and `\@` are legal as the first item
// `content::content_line`/`content_line_else_boundary` scans (right after
// the optional `(label)` prefix) — the true start of a physical line for
// an ordinary content line, but also right after a compact cue's `@NAME:`
// prefix, since `element::cue_line`'s `COMPACT_CUE` arm calls
// `content_line` directly for the fused dialogue line and that call is
// this same entry point. A bare `!`/`@` there would otherwise carry sigil
// meaning: `@NAME` opens a `CUE` (`element::at_cue`), and a bare
// line-start `!` is reserved for the `!name` annotation-element dispatch
// (§3.5b) — implemented or not, an author must be able to write a literal
// leading `!`/`@` without colliding with either. Anywhere else in a line,
// `\!`/`\@` are not in the inline set and remain the ordinary "backslash
// before anything else" compile error [`escape`] already gives.

/// True at `\!` or `\@`, immediately adjacent (no trivia) — the two
/// line-start escapes. Only meaningful when checked at a content line's
/// own start; the caller is responsible for that positioning (mirrors
/// [`at_span_open`]'s "recognition only, not a claim about context").
pub(crate) fn at_line_start_escape(p: &Parser<'_, '_>) -> bool {
    p.at(BACKSLASH) && matches!(p.nth(1), BANG | AT) && p.nth_adjacent(0)
}

/// Consume a confirmed [`at_line_start_escape`] position into an `ESCAPE`
/// node holding the literal `!`/`@`. Reuses [`escape`]'s exact node shape
/// (`BACKSLASH` + the one escaped token) so lowering
/// (`hir::lower_native::body::push_escape`) needs no change — it already
/// takes any `ESCAPE` node's second token verbatim as the literal it
/// produces.
pub(crate) fn line_start_escape(p: &mut Parser<'_, '_>) {
    // Flush pending trivia *before* opening the node — a raw `bump()`
    // emits whatever sits at `p.pos` including trivia (see `eat`'s doc
    // comment on why every raw-bump call site needs this). Doing it here,
    // ahead of `start_node`, keeps that trivia a sibling of `ESCAPE`
    // rather than swallowed inside it; flushing after `start_node` would
    // still land the trivia inside the node.
    p.skip_ws();
    p.start_node(crate::SyntaxKind::ESCAPE);
    p.bump(); // `\`
    p.bump(); // `!` or `@`
    p.finish_node();
}
