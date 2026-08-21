mod cst;

use super::check;
use crate::{SyntaxKind, parse};

#[test]
fn simple_choice() {
    check("* Choice text\n");
}

#[test]
fn sticky_choice() {
    check("+ Choice text\n");
}

#[test]
fn nested_choice() {
    check("* * Nested choice\n");
}

#[test]
fn choice_with_bracket() {
    check("* [hidden] shown\n");
}

#[test]
fn choice_with_label() {
    check("* (myLabel) Choice text\n");
}

#[test]
fn choice_with_condition() {
    check("* {x > 5} Choice text\n");
}

#[test]
fn choice_with_divert() {
    check("* Choice -> knot\n");
}

#[test]
fn choice_with_tags() {
    check("* Choice #tag1\n");
}

#[test]
fn choice_three_regions() {
    check("* Start[middle]end\n");
}

#[test]
fn double_plus_choice() {
    let p = parse("++[text] inner\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let dbg = format!("{:#?}", p.syntax());
    assert!(dbg.contains("CHOICE@"), "expected CHOICE node, got:\n{dbg}");
}

#[test]
fn triple_plus_choice() {
    let p = parse("+++[text] deep\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let dbg = format!("{:#?}", p.syntax());
    assert!(dbg.contains("CHOICE@"), "expected CHOICE node, got:\n{dbg}");
}

#[test]
fn double_plus_choice_in_knot() {
    let p = parse("== k ==\n+[a] Hello\n++[b] World\n+++[c] Deep\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let dbg = format!("{:#?}", p.syntax());
    let choice_count = dbg.matches("CHOICE@").count();
    assert_eq!(
        choice_count, 3,
        "expected 3 CHOICE nodes, got {choice_count}:\n{dbg}"
    );
}

#[test]
fn insta_choice_with_bracket() {
    let p = parse("* Hello[hidden]world\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_choice_with_condition() {
    let p = parse("* {visited} Been here.\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

// ── #2960: mid-line comments in choice text must not fragment CHOICE ────
//
// Same root cause as #2366/#2958 (`content::mixed_content`'s catch-all and
// `L_BRACE` arms), one function away: `choice_content_elements` broke on
// zero forward progress instead of retrying past a mid-line comment via
// `Parser::skip_comment_tokens`, and `choice_content_element`'s `L_BRACE`
// arm dispatched to `inline_logic` unconditionally instead of checking
// whether the raw position actually sat on `{` (vs. trivia in front of
// it). Fixed by mirroring both of `content.rs`'s guards exactly.
//
// These tests check the CST/`TEXT`-node level, the same layer #2958's own
// precedent tests (`content/mod.rs`) use — see
// `crates/brink-compiler/tests/issue_2960_choice_midline_comment.rs` for
// the real-pipeline (compile + runtime transcript) proof, where this same
// double space collapses to one at the output-buffer layer (pre-existing,
// unrelated ink whitespace-collapse behavior, not part of this fix).

/// A mid-line block comment in choice START content (before any `[`) used
/// to fragment the choice: `choice_content_elements` broke on zero
/// progress, `p.skip_ws()` at the trailing-divert check then ate the
/// comment AND its trailing space, `expected newline after choice` fired,
/// and the text after the comment spilled into a separate `CONTENT_LINE`.
/// Must now produce exactly one `CHOICE`, no errors, and both spaces
/// around the elided comment must survive as `TEXT`.
#[test]
fn block_comment_in_choice_start_content_stays_one_choice() {
    let src = "* Hello /* c */ world\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let choices: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CHOICE)
        .collect();
    assert_eq!(choices.len(), 1, "expected exactly one CHOICE: {choices:?}");
    let comment_inside_choice = choices[0]
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        comment_inside_choice,
        "BLOCK_COMMENT should be nested inside the CHOICE"
    );
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        text_nodes, "Hello  world",
        "comment span alone should be elided, both adjoining spaces kept \
         (double space), matching #2958's content.rs precedent"
    );
}

/// Same bug, choice INNER content (the region after the `]` bracket) — the
/// third of the issue's three choice regions, alongside bracket content
/// (already working via bump-not-break, pinned below) and start content
/// (previous test).
#[test]
fn block_comment_in_choice_inner_content_stays_one_choice() {
    let src = "* [opt] Hello /* c */ world\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let choices: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CHOICE)
        .collect();
    assert_eq!(choices.len(), 1, "expected exactly one CHOICE: {choices:?}");
    let inner_content = choices[0]
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("expected a CHOICE_INNER_CONTENT node");
    let text_nodes: String = inner_content
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        text_nodes, " Hello  world",
        "comment span alone should be elided, both adjoining spaces kept, \
         inside CHOICE_INNER_CONTENT"
    );
}

/// A block comment sitting directly between two inline-logic interpolations
/// in choice text hits the `L_BRACE` arm's own zero-progress recovery, not
/// the catch-all's. Whitespace on both sides of the comment must still
/// survive. Leading text ("Hi ") is deliberate: a choice line starting
/// directly with `{` parses that `{...}` as a leading `choice_condition`
/// (see `choice()`'s `while p.current() == L_BRACE` loop), a different,
/// unrelated grammar rule — not the bug under test here.
#[test]
fn block_comment_between_interpolations_in_choice_text() {
    let src = "* Hi {a} /* c */ {b}\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let choices = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CHOICE)
        .count();
    assert_eq!(choices, 1, "expected exactly one CHOICE");
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        text_nodes, "Hi   ",
        "both spaces around the elided comment must survive between \
         interpolations"
    );
}

/// `LINE_COMMENT` (`//`) mid-line has different semantics from
/// `BLOCK_COMMENT`: it runs to end of line, so the next non-trivia token
/// after it is always `NEWLINE` — `choice_text` breaks via its ordinary
/// `NEWLINE`-adjacent stop, never reaching the zero-progress retry path at
/// all. This case was never broken by #2960 and this fix must not change
/// it (mirrors `content.rs`'s `line_comment_mid_line_unaffected_by_fix`).
#[test]
fn line_comment_in_choice_text_unaffected_by_fix() {
    let src = "* Hello // note\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let choices = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CHOICE)
        .count();
    assert_eq!(choices, 1, "expected exactly one CHOICE");
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(text_nodes, "Hello ", "text before the line comment only");
}

/// `choice_bracket_content` already survives a mid-line comment (its
/// stuck-token recovery is `p.bump()`, not `break`) — pinned here so a
/// future change can't silently regress the one choice region that was
/// never broken by #2960.
#[test]
fn block_comment_in_choice_bracket_content_already_works() {
    let src = "* Hello[hidden /* c */ bracket]world\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let choices = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CHOICE)
        .count();
    assert_eq!(choices, 1, "expected exactly one CHOICE");
}

/// #2960's fix shape is a guarded retry: only retry past zero progress
/// when `skip_comment_tokens` actually advanced the position. A stray `]`
/// (the existing safety comment's own example) inside choice inner content
/// is a genuine non-trivia stop token with no comment to elide —
/// `skip_comment_tokens` is a no-op there, so `choice_content_elements`
/// must still `break` rather than spin. This is the regression test for
/// that hang: if the retry guard were missing, this call would never
/// return. (`]` can't appear in start/inner content as ordinary text at
/// all — it's `choice_content_elements`'s own stop condition — so this
/// exercises the guard the same way `stray_r_brace_in_content_does_not_hang`
/// does for `content.rs`, just with the token this loop's own doc comment
/// names.)
#[test]
fn stray_r_bracket_in_choice_inner_content_does_not_hang() {
    let src = "* [opt] Hello ] world\n";
    let p = parse(src);
    // Malformed input still round-trips losslessly — only the *no-hang*
    // guarantee is under test here, not error-freeness.
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// Same guard, `}` flavor: a stray `R_BRACE` in choice text (outside any
/// `{ ... }` sequence) is also a non-trivia zero-progress stop token for
/// `choice_text`.
#[test]
fn stray_r_brace_in_choice_text_does_not_hang() {
    let src = "* Hello } world\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
