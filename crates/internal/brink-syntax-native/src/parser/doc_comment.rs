//! Doc-comment attachment (B0.6b, `docs/decision-log.md` 2026-07-20):
//! promotes `///`/`//!` from trivia to a first-class `DOC_COMMENT` CST node,
//! attached structurally by the parser rather than re-derived later by a
//! trivia walk.
//!
//! Two independent attachment sites:
//!
//! - **Leading (outer, `///`).** [`maybe_consume_leading_run`] is called by
//!   `block::item` before its declaration-head dispatch. It always consumes
//!   a `DOC_COMMENT_OUTER` run it finds (as bare tokens, no node yet — see
//!   below) and returns a [`rowan::Checkpoint`] marking where the run
//!   started. The caller then either wraps that checkpoint into a
//!   `DOC_COMMENT` node as the declaration's leading child (via
//!   [`open_with_doc`], when a declaration head actually follows), or — the
//!   B0.6b judgment call for an *unattached* leading doc (nothing
//!   declaration-shaped follows) — does nothing further. In the unattached
//!   case the run's tokens are already sitting bare in the tree exactly
//!   where ordinary trivia would, with no diagnostic: a deliberate
//!   trivia-fallback, not an error, flagged in the issue for a later ruling
//!   on whether an `unused_doc_comment`-style warning belongs here instead.
//!
//! - **Inner (`//!`).** [`maybe_consume_inner_run`] is called right after a
//!   `BLOCK`'s opening `{` (and at the very start of `SOURCE_FILE`) — since
//!   the enclosing container node is already open at that point, no
//!   checkpoint trick is needed: the run is wrapped in a `DOC_COMMENT` node
//!   directly, becoming the container's leading child unconditionally
//!   (there is no "unattached" case for the inner form — it always
//!   documents whatever container it opens).
use crate::SyntaxKind::{
    self, DOC_COMMENT, DOC_COMMENT_INNER, DOC_COMMENT_OUTER, NEWLINE, WHITESPACE,
};

use super::Parser;

/// If the current token is a leading (`///`) doc-comment token, consume the
/// full contiguous run (see [`consume_doc_run`] for what "contiguous"
/// means) as bare tokens and return a checkpoint marking where the run
/// started, for the caller to retroactively wrap (via [`open_with_doc`])
/// if — and only if — a declaration head turns out to follow. Returns
/// `None` without consuming anything if the current token isn't
/// `DOC_COMMENT_OUTER`.
pub(crate) fn maybe_consume_leading_run(p: &mut Parser<'_, '_>) -> Option<rowan::Checkpoint> {
    if p.current() != DOC_COMMENT_OUTER {
        return None;
    }
    let checkpoint = p.checkpoint();
    consume_doc_run(p, DOC_COMMENT_OUTER);
    Some(checkpoint)
}

/// If the current token is an inner (`//!`) doc-comment token, consume the
/// run and wrap it in a `DOC_COMMENT` node as the next child of whatever
/// node is currently open (a `BLOCK` right after its `{`, or `SOURCE_FILE`
/// at its very start — both callers hold the enclosing node open already,
/// so the wrap is unconditional). No-op if the current token isn't
/// `DOC_COMMENT_INNER`.
pub(crate) fn maybe_consume_inner_run(p: &mut Parser<'_, '_>) {
    if p.current() != DOC_COMMENT_INNER {
        return;
    }
    p.start_node(DOC_COMMENT);
    consume_doc_run(p, DOC_COMMENT_INNER);
    p.finish_node();
}

/// Open a declaration node, attaching a previously-consumed leading doc run
/// (see [`maybe_consume_leading_run`]) as its leading `DOC_COMMENT` child
/// when `doc` is `Some`. Every native decl-parsing function
/// (`parser::decl`) calls this in place of a bare `p.start_node(kind)`.
pub(crate) fn open_with_doc(
    p: &mut Parser<'_, '_>,
    kind: SyntaxKind,
    doc: Option<rowan::Checkpoint>,
) {
    match doc {
        Some(checkpoint) => {
            // Wrap the already-bumped run (bare tokens sitting since
            // `checkpoint`) into its own DOC_COMMENT node first...
            p.start_node_at(checkpoint, DOC_COMMENT);
            p.finish_node();
            // ...then reopen the SAME checkpoint as the declaration node —
            // rowan's checkpoint/start_node_at contract wraps *everything*
            // emitted since `checkpoint`, node or bare token alike, so this
            // second wrap picks up the just-closed DOC_COMMENT node as its
            // first child, with the rest of the declaration's own tokens
            // following as later children once the caller resumes bumping.
            p.start_node_at(checkpoint, kind);
        }
        None => p.start_node(kind),
    }
}

/// Consume a contiguous run of `doc_kind` comment lines, starting at the
/// current position (the caller has already confirmed the first token is
/// `doc_kind`). "Contiguous": each line may be indented (leading
/// `WHITESPACE`, bumped bare) and is separated from the next by exactly one
/// `NEWLINE`; a blank line — a `NEWLINE` followed (past any `WHITESPACE`)
/// by another `NEWLINE` — ends the run, and that second `NEWLINE` is left
/// unconsumed for normal dispatch to handle. A plain (non-doc) comment, a
/// different-kind doc comment, or any other token also ends the run without
/// being consumed. Every token that IS consumed is bumped bare (no node) —
/// callers decide afterward whether to retroactively wrap the run in a
/// `DOC_COMMENT` node.
fn consume_doc_run(p: &mut Parser<'_, '_>, doc_kind: SyntaxKind) {
    loop {
        while p.nth_raw(0) == WHITESPACE {
            p.bump();
        }
        if p.nth_raw(0) != doc_kind {
            break;
        }
        p.bump(); // the comment token itself (already runs to end-of-line)
        if p.nth_raw(0) != NEWLINE {
            break; // EOF, or (structurally impossible) no newline follows
        }
        // Blank-line lookahead: does another NEWLINE (past any pure
        // indentation) immediately follow the one we're about to consume?
        let mut ahead = 1;
        while p.nth_raw(ahead) == WHITESPACE {
            ahead += 1;
        }
        let blank_follows = p.nth_raw(ahead) == NEWLINE;
        p.bump(); // this line's terminating NEWLINE
        if blank_follows {
            break;
        }
    }
}
