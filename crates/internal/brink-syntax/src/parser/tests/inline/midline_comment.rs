//! #2976: mid-line comments in an inline-alternative branch (`{a|b}`,
//! `{cond: a|b}`, sequences, and multiline branch bodies) must not
//! fragment the alternative. Third sibling of the zero-progress-break +
//! comment-stop class: #2366/#2958 fixed content lines
//! (`content::mixed_content`), #2960/#2974 fixed choice text
//! (`choice::choice_content_elements`/`choice_content_element`), this
//! fixes `inline::branch_content` (the shared parser for
//! `IMPLICIT_SEQUENCE`/`INLINE_BRANCHES_SEQ`/`INLINE_BRANCHES_COND`
//! branches) and `branchless_cond_body`'s `multiline_branch_text` call
//! site. A matching retry was also added to `multiline_branch_body`'s
//! `multiline_branch_text` call site for symmetry, but that one is
//! unreachable in practice -- see the "Comment in a multiline branch
//! body" section below.
//!
//! `{ a /* c */ x | b }` used to produce 5 parse errors: the comment was
//! hoisted out of `IMPLICIT_SEQUENCE`, the `|` became an `ERROR` node, and
//! the closing `}` became `STRAY_CLOSING_BRACE` (`branch_content`'s
//! catch-all broke on zero progress with no retry). Fixed the same way as
//! both precedents: reuse `Parser::skip_comment_tokens`, retry only when
//! it actually advances the position.

use crate::parser::tests::check;
use crate::{SyntaxKind, parse};

fn text_nodes(root: &crate::SyntaxNode) -> String {
    root.descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect()
}

// ── The issue's own probe case ───────────────────────────────────────

#[test]
fn block_comment_mid_branch_stays_one_implicit_sequence() {
    let src = "{ a /* c */ x | b }\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");

    let sequences: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE)
        .collect();
    assert_eq!(
        sequences.len(),
        1,
        "expected exactly one IMPLICIT_SEQUENCE: {sequences:?}"
    );
    let branches: Vec<_> = sequences[0]
        .children()
        .filter(|n| n.kind() == SyntaxKind::BRANCH_CONTENT)
        .collect();
    assert_eq!(branches.len(), 2, "expected both branches intact");

    let comment_inside_first_branch = branches[0]
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        comment_inside_first_branch,
        "BLOCK_COMMENT should be nested inside the first BRANCH_CONTENT, \
         not hoisted out of IMPLICIT_SEQUENCE"
    );

    // No ERROR / STRAY_CLOSING_BRACE nodes -- the `|` and closing `}` must
    // parse as ordinary structural tokens, not recovery nodes.
    assert!(
        p.syntax()
            .descendants()
            .all(|n| n.kind() != SyntaxKind::ERROR && n.kind() != SyntaxKind::STRAY_CLOSING_BRACE),
        "no ERROR/STRAY_CLOSING_BRACE nodes expected: {:#?}",
        p.syntax()
    );
}

/// Byte-for-byte: only the comment span is elided, whitespace on both
/// sides survives (matches #2958/#2974's precedent -- double space where
/// the comment used to sit).
#[test]
fn block_comment_elision_preserves_surrounding_whitespace() {
    let p = parse("{ a /* c */ x | b }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        text_nodes(&p.syntax()),
        "a  x  b ",
        "comment span alone should be elided, both adjoining spaces kept"
    );
}

// ── Comment in first vs last branch ──────────────────────────────────

#[test]
fn block_comment_in_first_branch_only() {
    let src = "{ a /* c */ x | b }\n";
    check(src);
}

#[test]
fn block_comment_in_last_branch_only() {
    let src = "{ a | b /* c */ x }\n";
    check(src);
    let p = parse(src);
    let sequences: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE)
        .collect();
    assert_eq!(sequences.len(), 1);
    let branches: Vec<_> = sequences[0]
        .children()
        .filter(|n| n.kind() == SyntaxKind::BRANCH_CONTENT)
        .collect();
    assert_eq!(branches.len(), 2);
}

// ── Comment adjacent to the pipe on both sides ───────────────────────

#[test]
fn block_comment_adjacent_to_pipe_both_sides() {
    // Comment immediately before the `|` in branch 1, and immediately
    // after the `|` in branch 2.
    let src = "{ a /*c1*/| /*c2*/ b }\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let sequences: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE)
        .collect();
    assert_eq!(sequences.len(), 1);
    let branches: Vec<_> = sequences[0]
        .children()
        .filter(|n| n.kind() == SyntaxKind::BRANCH_CONTENT)
        .collect();
    assert_eq!(branches.len(), 2, "expected both branches intact");
}

// ── Conditional (not just sequence) branches ─────────────────────────

#[test]
fn block_comment_in_inline_conditional_branch() {
    check("{x: yes /* c */ indeed|no}\n");
}

// ── Comment between two interpolations in the same branch ────────────
//
// Unlike `content.rs`'s and `choice.rs`'s `L_BRACE` arms, `branch_content`'s
// `L_BRACE` arm was deliberately left as an unconditional `p.skip_ws()`
// (not given the `nth_raw(0) == L_BRACE` raw-position guard those two
// arms have): adding that guard here changed the CST shape for ordinary
// comment-free leading whitespace too (an existing snapshot test,
// `logic::cst::conditional_nested_inline_logic`, pinned the old
// `p.skip_ws()`-swallowed shape with no `TEXT` node), so `BRANCH_CONTENT`
// is not that ancestor's structural mirror here. `p.skip_ws()` still
// eats a comment sitting between two interpolations without stalling
// (it always advances -- it doesn't need the zero-progress guard the
// catch-all arm does), and it does so while `BRANCH_CONTENT` is still the
// open node, so the comment stays nested inside `IMPLICIT_SEQUENCE`
// either way. This pins that it keeps working, not that it goes through
// the catch-all's `skip_comment_tokens` retry.

#[test]
fn block_comment_between_interpolations_in_branch() {
    let src = "{ {a} /* c */ {b} | c }\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let comment_inside_sequence = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE)
        .expect("expected an IMPLICIT_SEQUENCE")
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        comment_inside_sequence,
        "BLOCK_COMMENT should stay nested inside IMPLICIT_SEQUENCE"
    );
}

// ── LINE_COMMENT control case ─────────────────────────────────────────
//
// `LINE_COMMENT` runs to end of line, so `branch_text` never makes zero
// progress on it inside an inline (single-line) alternative -- the
// alternative's own `}` never appears on that line, so this is a genuine
// parse error (an unterminated inline block), not a fragmentation bug.
// Pinned here as the "this fix must not change it" control, same as
// `content.rs`/`choice.rs`'s own `line_comment_*_unaffected_by_fix` tests.

#[test]
fn line_comment_in_inline_branch_is_unterminated_block_not_fragmentation() {
    let src = "{ a // c\n| b }\n";
    let p = parse(src);
    // An inline `{...}` cannot span the LINE_COMMENT's forced newline and
    // still close on the same construct -- no hang and a lossless
    // round-trip either way, but (unlike the sibling control test
    // `line_comment_in_choice_text_unaffected_by_fix`) the pre-comment
    // text does NOT survive: the `{` is left unterminated, so `a` is
    // never captured into a `TEXT` node at all. Pin the actual diagnosed
    // outcome, identical on `origin/main` before this fix and after it.
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let messages: Vec<&str> = p.errors().iter().map(|e| e.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "expected `}`",
            "expected newline at end of content line",
            "unexpected token",
            "expected newline at end of content line",
        ],
        "expected outcome unchanged from origin/main: {:#?}",
        p.errors()
    );
    assert_eq!(
        text_nodes(&p.syntax()),
        "b ",
        "the pre-comment `a` does not survive as TEXT -- the `{{` is left \
         unterminated by the LINE_COMMENT's forced newline, unlike the \
         fragmentation bug this file otherwise fixes"
    );
}

// ── Comment in a multiline branch body ───────────────────────────────
//
// Exercises the `multiline_branch_text` call site in
// `branchless_cond_body`, which is a genuine zero-progress retry (hit 3
// times running the full suite). The sibling test below,
// `block_comment_in_multiline_branch_body`, exercises the same-shaped
// retry in `multiline_branch_body`, but that site is unreachable: the
// loop's leading, unconditional `p.skip_ws()` already consumes comment
// trivia before the catch-all's `multiline_branch_text` call ever runs
// (confirmed by instrumentation: 0 hits), so that test pins pre-existing
// behavior rather than proving the retry fires there.

#[test]
fn block_comment_in_multiline_branchless_cond_body() {
    let src = "{x > 5:\n  Big /* c */ number.\n}\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::BRANCHLESS_COND_BODY)
        .expect("expected a BRANCHLESS_COND_BODY");
    let comment_inside_body = body
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        comment_inside_body,
        "BLOCK_COMMENT should stay nested inside BRANCHLESS_COND_BODY"
    );
    assert_eq!(
        text_nodes(&p.syntax()),
        "Big  number.",
        "comment span alone should be elided, both adjoining spaces kept"
    );
}

/// Pins pre-existing behavior (green on `origin/main` before this fix,
/// unmodified) rather than proving this fix's `multiline_branch_body`
/// retry -- that retry is unreachable, see the section comment above.
/// `multiline_branch_body`'s leading `p.skip_ws()` already elides the
/// comment as trivia every time, so this path never hit the
/// zero-progress bug this PR targets.
#[test]
fn block_comment_in_multiline_branch_body() {
    let src = "{\n- x > 5:\n  Big /* c */ number.\n- else:\n  Small.\n}\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::MULTILINE_BRANCH_BODY)
        .expect("expected a MULTILINE_BRANCH_BODY");
    let comment_inside_body = body
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        comment_inside_body,
        "BLOCK_COMMENT should stay nested inside MULTILINE_BRANCH_BODY"
    );
}

// ── Nested alternatives ────────────────────────────────────────────────

#[test]
fn block_comment_in_nested_alternative() {
    let src = "{ a | { x /* c */ y | z } }\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let sequences: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE)
        .collect();
    assert_eq!(
        sequences.len(),
        2,
        "expected outer and inner IMPLICIT_SEQUENCE"
    );
    for seq in &sequences {
        let branches = seq
            .children()
            .filter(|n| n.kind() == SyntaxKind::BRANCH_CONTENT)
            .count();
        assert_eq!(branches, 2, "expected both branches intact in {seq:?}");
    }
}

// ── No-hang regressions ────────────────────────────────────────────────
//
// The fix's guard: retry only when `skip_comment_tokens` actually
// advances. A stray structural token (PIPE/R_BRACE) with no comment ahead
// must still break rather than spin.

#[test]
fn stray_pipe_in_multiline_branch_body_does_not_hang() {
    // A second bare `|` inside a multiline branch body is not valid
    // syntax for that context; the parser must still terminate.
    let src = "{\n- x:\n  a | b\n}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

#[test]
fn stray_r_brace_in_multiline_branchless_cond_body_does_not_hang() {
    let src = "{x:\n  a } b\n";
    let p = parse(src);
    // Malformed/ambiguous input still round-trips losslessly -- only the
    // no-hang guarantee is under test here.
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
