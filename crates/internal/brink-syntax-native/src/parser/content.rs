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
    self, BACKSLASH, CONTENT_LINE, DIVERT, DOC_COMMENT_INNER, DOC_COMMENT_OUTER, EOF, GLUE,
    GLUE_NODE, GT, HASH, IDENT, INTERPOLATION, KW_ELSE, L_BRACE, L_PAREN, LABEL, LT, NEWLINE,
    R_BRACE, R_PAREN, TAG, TAG_LINE, TEXT, TILDE,
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
    // `\!` / `\@` — the line-start escape set (§8d.6, issue #1744),
    // checked once, right here, before the general item scanner: a
    // literal leading `!`/`@` would otherwise collide with the `@NAME`
    // cue sigil or the `!name` annotation-element dispatch sigil (§3.5b,
    // issue #2004, `element::at_bang_dispatch`). "Right here" is the
    // first item *this function* scans — the true start of a physical
    // line for a normal content line, but also right after a compact
    // cue's `@NAME:` prefix (`element::cue_line`'s `COMPACT_CUE` arm) or a
    // `!name` dispatch's own name (`element::bang_dispatch`), since both
    // call this same function for their fused remainder. Anywhere else in
    // the line, `\!`/`\@` fall through to `content_items_until`'s generic
    // `BACKSLASH` handling and remain the ordinary compile error
    // (`markup::escape`'s four-char
    // inline set).
    if super::markup::at_line_start_escape(p) {
        super::markup::line_start_escape(p);
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

/// `content_line`'s twin for `family::colon_body`'s per-line dispatch
/// (`family::colon_body_line`, #1254 Gap 1): identical in every respect
/// except the scan also stops at a same-line else-arm boundary
/// (`family::at_else_arm`), so `{if cond: … else: …}` written as one
/// physical line hands control back to `colon_body` instead of the
/// trailing `else: …` getting swallowed into this line's `TEXT`.
pub(crate) fn content_line_else_boundary(p: &mut Parser<'_, '_>) {
    p.start_node(CONTENT_LINE);
    if at_content_label(p) {
        label(p);
        p.skip_ws();
    }
    // Same line-start escape check as `content_line` above — kept in
    // sync per this function's own "identical in every respect" doc.
    if super::markup::at_line_start_escape(p) {
        super::markup::line_start_escape(p);
    }
    content_items_until_else_boundary(p, &[NEWLINE, R_BRACE, HASH]);
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
    content_items_until_impl(p, stop, false, None);
}

/// [`content_items_until`]'s twin for `family::colon_body`'s per-line
/// dispatch (#1254 Gap 1): identical, except the scan also stops at a
/// same-line else-arm boundary (`family::at_else_arm`) — the two are kept
/// as separate entry points (rather than adding an `else`-boundary kind to
/// every caller's `stop` slice) because `at_else_arm` needs two-token
/// lookahead (`KW_ELSE` immediately followed by `{`/`:`/`if`), not the
/// plain single-token membership test `stop` gives every other kind, and
/// this behavior must stay opt-in: an ordinary content line's bare word
/// "else" (unfollowed by one of those three tokens) is just prose and
/// must keep its leading whitespace like any other `TEXT`-run word, never
/// treated as a structural boundary the way a real else-arm opener is.
pub(crate) fn content_items_until_else_boundary(p: &mut Parser<'_, '_>, stop: &[SyntaxKind]) {
    content_items_until_impl(p, stop, true, None);
}

/// The shared engine's actual body. `expected_close` is `Some(name)` only
/// when this call is [`super::markup::span`] scanning a span's own body
/// looking for `</name>` — every other caller passes `None`. Forwarding the
/// exact same `stop`/`stop_at_else_arm` a span's caller was given, one
/// level down, into the recursive call `markup::span` makes for its body,
/// is the entire nesting-doctrine enforcement mechanism (see
/// `markup`'s module doc); this function does not need to know that, it
/// only needs to treat a close tag matching `expected_close` as a stop
/// condition like any other.
///
/// `pub(crate)`, not private: `markup::span` (a sibling module) is the one
/// other caller, recursing back in for a span's body.
pub(crate) fn content_items_until_impl(
    p: &mut Parser<'_, '_>,
    stop: &[SyntaxKind],
    stop_at_else_arm: bool,
    expected_close: Option<&str>,
) {
    loop {
        let cur = p.current();
        // Inter-interpolation whitespace fix (#1264): same bug class as
        // the significant-whitespace policy just below, but that policy
        // only folds leading whitespace into a FOLLOWING TEXT run —
        // `starts_text_run` returns `false` for every `L_BRACE`
        // unconditionally, so pending trivia ahead of a bare `{expr}`
        // interpolation still gets `skip_ws`'d bare, landing outside any
        // node. `{a} {b}`'s separating space is exactly that: trivia
        // between the closing `}` of one interpolation and the opening
        // `{` of the next. When the upcoming `L_BRACE` is going to resolve
        // to a genuine bare interpolation (not the choice/conditional/
        // alternation family, and not a caller-flagged body-opener —
        // `at_bare_interpolation` mirrors the match arms below exactly),
        // fold the pending trivia into a `TEXT` node via `text_run_until`
        // first. `text_run_until` breaks at the very next `L_BRACE`, so
        // this only ever wraps the trivia itself; the interpolation is
        // then parsed normally on the next loop iteration with no pending
        // trivia left, so nothing downstream (`is_body_open_brace`
        // included) sees a different parser state than before this fix.
        if cur == L_BRACE && p.nth_raw(0).is_trivia() && at_bare_interpolation(p, stop) {
            text_run_until(p, stop, stop_at_else_arm);
            continue;
        }
        // Same whitespace-significance fix as the interpolation one just
        // above, for `\` (escape) and `<`/`</` (span open/close): each is
        // unconditionally special in `starts_text_run` (so the outer
        // `skip_ws()` a few lines down would otherwise discard pending
        // trivia bare, landing outside any node — `Hello <b>` losing the
        // space before `<b>`, or `\< \{`'s inter-escape space vanishing).
        // Fold pending trivia into a `TEXT` node first; the item is then
        // parsed fresh on the next iteration with nothing pending.
        if cur == BACKSLASH && p.nth_raw(0).is_trivia() {
            text_run_until(p, stop, stop_at_else_arm);
            continue;
        }
        if cur == LT
            && p.nth_raw(0).is_trivia()
            && (super::markup::at_span_open(p) || super::markup::at_span_close(p))
        {
            text_run_until(p, stop, stop_at_else_arm);
            continue;
        }
        if stop_at_else_arm && cur == KW_ELSE && super::family::at_else_arm(p) {
            // A genuine else-arm opener always trims its own leading
            // trivia, the same policy every other structural item this
            // loop recognizes follows (the annotated-brace family, glue,
            // …) — only a `TEXT` run keeps significant leading whitespace.
            p.skip_ws();
            break;
        }
        // Leading-trivia policy (significant-whitespace fix): only a prose
        // TEXT run keeps its leading whitespace. When the next item is a
        // text run, DON'T `skip_ws` here — `text_run_until` (which bumps
        // RAW tokens) folds the leading whitespace into the `TEXT` node, so
        // significant inter-token prose whitespace survives as narrative
        // rather than being discarded as bare trivia hung off the enclosing
        // content node. That whitespace is exactly the space after a `]`
        // bracket close (`CHOICE_INNER_CONTENT`) or a `<>` glue marker: the
        // `'A wager!'` / `<> "But surely…"` shapes whose space this loop
        // used to eat, diverging from the ink twin (`'A wager!'I returned.`).
        //
        // For every OTHER item (the annotated-brace family, bare `{expr}`
        // interpolation, glue, diverts, doc-comment tokens) and for every
        // stop/break token, consume leading trivia first, exactly as before:
        // `is_body_open_brace`'s RAW lookahead and each item node's own
        // leading `bump`/`expect` must land on the real token, not on
        // pending whitespace. `current()` already skips trivia read-only, so
        // the dispatch decision below is identical either way — only WHERE
        // the whitespace lands in the tree changes. Line-leading indentation
        // is never captured here: every caller (`block`, `colon_body`,
        // `entry`, `inline_alternatives`, `choice`, `content_line`) already
        // skips it before this loop is entered.
        if !starts_text_run(p, cur, stop) {
            p.skip_ws();
        }
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
        // A close tag always breaks the loop too — whether it's the one
        // *this* frame is looking for (`expected_close`, handed back to
        // `markup::span` to consume) or someone else's/a stray one (handled
        // right here, loudly, with forward progress guaranteed — see
        // `markup::consume_stray_close`'s doc). Checked before the generic
        // `stop`/`L_BRACE` handling below since `LT` is never itself a
        // caller-supplied stop kind.
        if cur == LT && super::markup::at_span_close(p) {
            // Compare the close tag's FULL (possibly hyphenated) name, not
            // just its first token (`p.nth_text(2)`) — `expected_close` may
            // itself be hyphenated (§4.1, issue #1996), and comparing only
            // the leading segment would wrongly treat e.g. `</fade>` as the
            // expected close for a `<fade-in>` open tag.
            if expected_close.is_some_and(|name| super::markup::at_span_close_named(p, name)) {
                break;
            }
            super::markup::consume_stray_close(p);
            continue;
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
            LT if super::markup::at_span_open(p) => {
                super::markup::span(p, stop, stop_at_else_arm);
            }
            BACKSLASH => super::markup::escape(p),
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
            _ => text_run_until(p, stop, stop_at_else_arm),
        }
    }
}

/// True when the next non-trivia token (`cur`, already trivia-skipped by
/// `current()`) begins a prose `TEXT` run — i.e. it would fall to
/// `content_items_until`'s `_` dispatch arm rather than a structural item
/// (`{…}` family/interpolation, glue, divert, doc-comment) or a stop/break
/// token (`EOF`/`HASH`/a caller-supplied stop). When it does, the outer
/// loop must NOT `skip_ws` first, so `text_run_until` can fold the leading
/// whitespace into the `TEXT` node and preserve significant inter-token
/// prose whitespace. Every branch this returns `false` for either breaks
/// the loop or dispatches to an item whose own `bump`/`expect` (or the
/// explicit `skip_ws` the loop still performs) consumes the leading trivia,
/// so structural lookahead and node-boundary placement are unchanged.
fn starts_text_run(p: &Parser<'_, '_>, cur: SyntaxKind, stop: &[SyntaxKind]) -> bool {
    // `BACKSLASH` is always special (an escape or a compile error, §8d.6 —
    // never plain text), and a qualifying `<` (an actual span open/close,
    // `markup::at_span_open`/`at_span_close`) is a structural item like
    // every other one this loop recognizes. A `<` that does NOT qualify
    // (`5 < 10`, a lone `<3`) falls through and stays ordinary text,
    // unchanged.
    if cur == BACKSLASH {
        return false;
    }
    if cur == LT && (super::markup::at_span_open(p) || super::markup::at_span_close(p)) {
        return false;
    }
    !matches!(
        cur,
        EOF | HASH | L_BRACE | GLUE | DIVERT | DOC_COMMENT_OUTER | DOC_COMMENT_INNER
    ) && !stop.contains(&cur)
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
    // `family::is_multiline` uses for the alternation family. Looked up via
    // `raw_trivia_offset` rather than the literal `+ 1` the brace's own
    // width would suggest: the inter-interpolation whitespace fix above
    // calls this function while trivia ahead of the `{` may still be
    // pending (not yet `skip_ws`'d), so the brace itself might not sit at
    // raw offset 0. When there's no pending trivia (every other call
    // site — trivia is always flushed before this is reached) the offset
    // is 0 and this is exactly the old `peek_is_newline(p, 1)`.
    super::family::peek_is_newline(p, raw_trivia_offset(p) + 1)
}

/// Number of raw trivia tokens (`WHITESPACE`/comments) sitting at the
/// parser's current raw position before the next real token — `0` when
/// none are pending. Lets [`is_body_open_brace`] give the same answer
/// whether or not its caller has already `skip_ws`'d.
fn raw_trivia_offset(p: &Parser<'_, '_>) -> usize {
    let mut i = 0;
    while p.nth_raw(i).is_trivia() {
        i += 1;
    }
    i
}

/// True when the `L_BRACE` at the parser's (possibly trivia-pending)
/// position is going to resolve to a bare `{expr}` interpolation — neither
/// the choice/conditional/alternation family nor (when the caller lists
/// `L_BRACE` as a stop kind) a body-opener. Mirrors exactly the condition
/// under which `content_items_until`'s dispatch match reaches
/// [`interpolation`]; used solely to gate the inter-interpolation
/// whitespace fix, which must never fire for the family/body-open cases —
/// those still need their leading trivia `skip_ws`'d bare, unchanged.
fn at_bare_interpolation(p: &Parser<'_, '_>, stop: &[SyntaxKind]) -> bool {
    let is_family = super::family::at_choice_point(p)
        || super::family::at_conditional(p)
        || super::family::at_alternation(p);
    let is_body_opener = stop.contains(&L_BRACE) && is_body_open_brace(p);
    !is_family && !is_body_opener
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
/// caller-supplied stop kind, an else-arm boundary when `stop_at_else_arm`
/// (#1254 Gap 1), or EOF), including any interior whitespace/plain-comments
/// — those are literal prose here, not trivia to discard (so trailing
/// whitespace already absorbed into an in-progress run before one of these
/// breaking constructs — e.g. the space before a trailing `#tag`/`->`/
/// `else:` — stays part of this `TEXT` node; only a FRESH item's leading
/// whitespace is ever trimmed, by the outer loop, before this function is
/// even called). `HASH`/`DIVERT`/doc-comment tokens always break a text run
/// (tags and diverts are recognized structurally by every caller; a
/// doc-comment token must never fold into visible prose), even if the
/// caller didn't ask for it — mirrors `content_items_until`'s own
/// unconditional-break agreement for `HASH`; without this, a `->` (or a
/// stray `//!`) reached mid-run would get bumped as literal text before the
/// outer loop ever saw it, and its dedicated match arm there would be dead
/// code. The else-arm boundary needs the same treatment for the same
/// reason: `family::at_else_arm`'s two-token lookahead only fires reliably
/// when checked from the OUTER loop between items, so a genuine
/// `else:`/`else{`/`else if` reached mid-run must hand control back here
/// too, or it would get swallowed as literal text before that check ever
/// runs (`colon_form_else_on_the_same_line_is_recognized_as_an_else_arm`).
fn text_run_until(p: &mut Parser<'_, '_>, stop: &[SyntaxKind], stop_at_else_arm: bool) {
    p.start_node(TEXT);
    loop {
        let k = p.nth_raw(0);
        if k == EOF
            || k == L_BRACE
            || k == GLUE
            || k == HASH
            || k == DIVERT
            || k == BACKSLASH
            || (k == LT && (super::markup::at_span_open(p) || super::markup::at_span_close(p)))
            || k == DOC_COMMENT_OUTER
            || k == DOC_COMMENT_INNER
            || stop.contains(&k)
        {
            break;
        }
        if stop_at_else_arm && k == KW_ELSE && super::family::at_else_arm(p) {
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
        tag(p, &[]);
    }
}

/// [`tag_line_tail`]'s twin for a *declaration header* line
/// (`decl::header_tags`, §8b.4's `flow x #tag { … }`): identical, except
/// each tag's text also stops at the body opener that follows it on the
/// same line — a bare `{`, or a `~`/`>` body-dialect selector (charter §4).
/// Without the extra stops the last tag's free-text run would swallow the
/// brace and the header would lose its body.
///
/// The two entry points are kept separate rather than always stopping at
/// `L_BRACE`/`TILDE`/`GT`, because on a *content* line those characters are
/// ordinary tag text with no body opener anywhere in sight, and narrowing
/// the general tag grammar to serve the header case would silently change
/// what an existing `#tag` means.
pub(crate) fn header_tag_tail(p: &mut Parser<'_, '_>) {
    while p.at(HASH) {
        tag(p, &[L_BRACE, TILDE, GT]);
    }
}

/// #1728: a tag's body is raw text (`tag_text_runs_raw_to_end_of_line`) —
/// it never re-parses `{…}` as a real interpolation/alternation/choice
/// node — but it still must not mistake a `}` that merely *echoes* a `{`
/// the tag's own text already contains for its own terminator. `depth`
/// counts literal, unpaired `{`s bumped so far: a `}` only stops the scan
/// when depth is back to zero, i.e. it isn't balanced by an earlier `{`
/// within this same tag. This is pure raw-character balancing, not
/// interpolation awareness — `#tag {gold` (never closed) still runs to
/// end of line exactly as before.
///
/// The real tradeoff, stated plainly (review of #1728, not "no
/// regression"): a *balanced* brace inside a tag no longer terminates it
/// early (the bug this fixes), but a genuinely *unbalanced*, unescaped `{`
/// left open inside a tag now eats the enclosing single-line block's own
/// same-line `}` closer instead of stopping there — the mirror image of
/// the original bug, inherent to depth-based balancing over raw text with
/// no real grammar to bound it. Pinned by
/// `an_unbalanced_open_brace_in_a_tag_eats_the_enclosing_blocks_own_closer`.
///
/// `\{` is different: #1716/PR #1732 made it the literal-brace escape, so
/// it is text, not a metacharacter, and counting it as a depth-opener
/// would be the surprising reading — a `\{` with no real interpolation
/// nearby would then let a later unescaped `}` swallow the enclosing
/// closer, converting previously clean source into a parse error. So an
/// `L_BRACE` is excluded from the counter when it is preceded by an *odd*
/// number of consecutive raw `BACKSLASH`es (#1852: `\\{` is an escaped
/// backslash followed by a real, depth-counted brace, not an escaped
/// brace — an even backslash count means the backslashes escape each
/// other and the brace stands unescaped) (an `R_BRACE` is still
/// unconditionally significant to the depth check either way — `\}` alone,
/// with no preceding unescaped `{`, already terminated a tag before this
/// fix and still does). Pinned by
/// `a_tag_with_an_escaped_open_brace_does_not_swallow_the_enclosing_blocks_own_closer`.
///
/// **`\#` escapes the tag-boundary role of `#` (issue #1738).** A bare
/// unescaped `HASH` always ends a tag — that is the `TAG_LINE`/trailing-tag
/// grammar's own separator (`#a#b` is two sibling tags,
/// `tags_with_no_space_between_are_two_separate_tag_nodes`), and this
/// function must keep honoring that for the common case. But `#` is one of
/// the four members of the ruled, final inline escape set (§8d.6: `\< \{ \#
/// \\`), and before this fix `tag()` gave it **zero** escape treatment: a
/// `\#` inside a tag's own text still split the tag in two at the `#`,
/// leaving a dangling, meaningless backslash in the first half — the exact
/// "escape/markup layer coverage inconsistent across prose scanners" gap
/// #1738 tracks (the *ordinary* content-line scanner already turns `\#` into
/// a literal `#` via `markup::escape`'s `ESCAPE` node; `tag()`'s free-text
/// scan didn't). The fix mirrors the *existing* `\{` carve-out immediately
/// below, not `markup::escape`'s node-producing shape: an `HASH` is only
/// treated as the tag's terminator when NOT preceded by an odd number of
/// consecutive raw `BACKSLASH`es — same `backslash_count` parity tracking,
/// same "even means the backslashes escape each other" reading (#1852). Like
/// `\{`, the backslash is **not stripped** from this raw `TAG` node's own
/// CST text — it stays a lossless, unstripped copy of the source, exactly
/// like every other raw-text scanner in this file — this stays
/// self-consistent with `\{`'s established "structural role only" treatment
/// inside these two raw-text scanners, not a claim that `tag()` now runs the
/// full `markup::escape` layer (it still doesn't: `<ident>` stays inert
/// literal text here too, per the separate, already-ruled #1783 "markup is
/// literal in a `#` tag" decision — untouched by this fix).
///
/// **Superseded in part by issue #2045:** the CST-level claim above still
/// holds, but `ast::Tag::text()` (`ast/nodes.rs`) is a *later* materialization
/// point that now strips a recognized escape's backslash from the tag's
/// rendered text (parity with `markup::escape`'s stripping for ordinary
/// content), and `hir::lower_native::body::lower_tag` was changed to funnel
/// through it instead of hand-rolling its own HASH-skip + concatenation —
/// so "not stripped" is only true of the raw CST node, not of every reader
/// of a tag's text. Pinned by
/// `a_tag_with_an_escaped_hash_does_not_end_the_tag_early` (raw CST) and
/// `a_tags_text_accessor_strips_a_recognized_escapes_backslash` (the
/// stripping accessor).
///
/// **RULED (review of #1777, issue #1787): `depth` is scoped per-tag, not
/// per-line, and that is the intended contract, not a gap.** `tag_line_tail`
/// calls this function fresh for each `HASH` it sees, so `depth` always
/// starts at zero for a new tag regardless of what an earlier sibling tag
/// on the same line left unbalanced. In `#a {x #b}` (two trailing tags —
/// `content_line`'s own doc comment: "Trailing `#tag`s are folded in
/// before the line ends", so this shape only ever arises between
/// *sibling* tags, never between a tag and following prose), tag `a`'s
/// scan is cut short by the (unescaped — see the `\#` note just above) `HASH`
/// starting `b` — before the brace-depth check ever runs, exactly
/// like `NEWLINE`/`EOF` — so `a`'s in-progress depth of 1 is simply
/// discarded, not carried into `b`'s scan. `b` starts its own scan at
/// depth zero and immediately meets the `}`, stopping there without
/// consuming it, so that brace is left for whatever the line's own
/// enclosing block expects it to close. Carrying depth across the `HASH`
/// boundary instead (a per-line scope) would make one tag's own unbalanced
/// text reach *through* a syntactically distinct sibling tag and swallow
/// that sibling's — or the enclosing block's — own closer, a strictly
/// worse and less local failure mode than today's already-accepted
/// per-tag tradeoff above: a `HASH` is a real, tokenized boundary (each one
/// starts its own `TAG` node), unlike the raw, grammar-blind `{`/`}`
/// characters this scan balances, so treating it as anything other than a
/// hard reset would blur a structural boundary the CST already treats as
/// absolute. Pinned by
/// `a_tags_own_unbalanced_brace_does_not_leak_depth_into_a_sibling_tag`.
fn tag(p: &mut Parser<'_, '_>, extra_stop: &[SyntaxKind]) {
    p.start_node(TAG);
    p.expect(HASH);
    let mut depth: u32 = 0;
    let mut backslash_count: u32 = 0;
    loop {
        let cur = p.current();
        if matches!(cur, NEWLINE | EOF) || extra_stop.contains(&cur) {
            break;
        }
        if cur == R_BRACE && depth == 0 {
            break;
        }
        // `HASH` keeps `cur`'s existing "peek past pending trivia, don't
        // consume it" early-exit shape (same as `NEWLINE`/`EOF` above) for
        // the common unescaped case — a trailing space before a sibling
        // tag's `#` must stay outside this tag's own text exactly as
        // before this fix (`a_tags_own_unbalanced_brace_does_not_leak_depth_into_a_sibling_tag`
        // pins the precise whitespace placement). The escape check itself,
        // though, must not trust `backslash_count` at this point unless
        // `p.nth_raw(0)` is *actually* `HASH` with no pending trivia ahead
        // of it — `cur` can report `HASH` while raw position still sits on
        // intervening whitespace (peeked through), and `backslash_count`
        // would then still hold a stale value from before that whitespace
        // is ever bumped/reset. Requiring `p.nth_raw(0) == HASH` too closes
        // that gap: a non-adjacent `\ #` (space between them) never reads
        // as escaped, matching the adjacency discipline `markup::escape`/
        // `at_line_start_escape` already enforce elsewhere.
        if cur == HASH && !(p.nth_raw(0) == HASH && backslash_count & 1 == 1) {
            break;
        }
        // `nth_raw(0)`, not `cur`: `cur` (`current()`) looks past pending
        // trivia to the next real token, so it can report the same
        // upcoming `{`/`}` on several iterations in a row while this loop
        // bumps the whitespace ahead of it one raw token at a time.
        // `nth_raw(0)` is the exact raw token this iteration is about to
        // bump, so depth changes exactly once per brace, never double- or
        // zero-counted. Tracked in lockstep with `backslash_count`, which
        // counts consecutive backslashes before a brace or other token —
        // escapes are only ever adjacent raw tokens, never separated by
        // trivia. An L_BRACE is only excluded from depth counting if
        // preceded by an odd number of backslashes (the last one escapes
        // it); an even number means the backslashes themselves are escaped
        // (#1852: `\\{` should count the brace, not escape it).
        let raw = p.nth_raw(0);
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
