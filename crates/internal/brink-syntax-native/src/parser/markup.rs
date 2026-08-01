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
//! `\!` `\@`, legal only as the very first item of a content line
//! ([`at_line_start_escape`]/[`line_start_escape`], issue #1744) —
//! protecting a literal leading `!`/`@` from the sigils those characters
//! carry there (`@NAME` cue dispatch, the reserved `!name` annotation
//! sigil). A `\!`/`\@` anywhere else in a line is not part of the inline
//! four and hits the same compile error as any other unrecognized
//! backslash.

use crate::SyntaxKind::{
    AT, BACKSLASH, BANG, EQ, ERROR, GT, HASH, IDENT, L_BRACE, LT, QUOTE, SLASH, SPAN, SPAN_ATTR,
    SPAN_ATTR_VALUE, SPAN_NAME, STRING_ESCAPE, STRING_TEXT,
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

/// [`at_span_close`], plus the close tag's name matches `name` exactly.
/// Only meaningful where [`at_span_close`] already holds.
fn at_span_close_named(p: &Parser<'_, '_>, name: &str) -> bool {
    at_span_close(p) && p.nth_text(2) == name
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
    let name = p.nth_text(0).to_string();
    p.start_node(SPAN_NAME);
    p.bump(); // the name IDENT
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
        p.bump(); // `<`
        p.bump(); // `/`
        p.bump(); // the close name IDENT
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
    p.bump(); // `<`
    p.bump(); // `/`
    p.bump(); // name
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
// its doc comment). `\!` and `\@` are legal only as the very first item
// of a content line (`content::content_line`/`content_line_else_boundary`,
// right after the optional `(label)` prefix), where a bare `!`/`@` would
// otherwise carry sigil meaning: `@NAME` opens a `CUE`
// (`element::at_cue`), and a bare line-start `!` is reserved for the
// `!name` annotation-element dispatch (§3.5b) — implemented or not, an
// author must be able to write a literal leading `!`/`@` without
// colliding with either. Anywhere else in a line, `\!`/`\@` are not in
// the inline set and remain the ordinary "backslash before anything
// else" compile error [`escape`] already gives.

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
    p.start_node(crate::SyntaxKind::ESCAPE);
    p.bump(); // `\`
    p.bump(); // `!` or `@`
    p.finish_node();
}
