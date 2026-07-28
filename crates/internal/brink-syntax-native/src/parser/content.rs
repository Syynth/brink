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
    self, CONTENT_LINE, DIVERT, DOC_COMMENT_INNER, DOC_COMMENT_OUTER, EOF, GLUE, GLUE_NODE, GT,
    HASH, IDENT, INTERPOLATION, KW_ELSE, L_BRACE, L_PAREN, LABEL, NEWLINE, R_BRACE, R_PAREN, TAG,
    TAG_LINE, TEXT, TILDE,
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
    content_items_until_impl(p, stop, false);
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
    content_items_until_impl(p, stop, true);
}

fn content_items_until_impl(p: &mut Parser<'_, '_>, stop: &[SyntaxKind], stop_at_else_arm: bool) {
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
        if !starts_text_run(cur, stop) {
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
fn starts_text_run(cur: SyntaxKind, stop: &[SyntaxKind]) -> bool {
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

fn tag(p: &mut Parser<'_, '_>, extra_stop: &[SyntaxKind]) {
    p.start_node(TAG);
    p.expect(HASH);
    while !matches!(p.current(), NEWLINE | EOF | HASH | R_BRACE)
        && !extra_stop.contains(&p.current())
    {
        p.bump();
    }
    p.finish_node();
}
