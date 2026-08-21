mod cst;

use super::check;
use crate::{SyntaxKind, parse};

// ── Smoke tests (check = lossless + no errors) ─────────────────────

#[test]
fn plain_text() {
    check("Hello, world!\n");
}

#[test]
fn multi_word_text() {
    check("The quick brown fox\n");
}

#[test]
fn text_with_punctuation() {
    check("Hello! How are you?\n");
}

#[test]
fn content_then_divert() {
    check("Hello -> knot\n");
}

#[test]
fn content_with_escape() {
    check("Hello \\# not a tag\n");
}

#[test]
fn content_with_glue() {
    check("Hello<>world\n");
}

#[test]
fn content_with_inline_logic() {
    check("Hello {name}\n");
}

#[test]
fn content_divert_tags() {
    check("Hello -> knot #tag1\n");
}

#[test]
fn bare_divert_line() {
    check("-> knot\n");
}

#[test]
fn content_multiple_elements() {
    check("Hello <>world {name} -> knot #tag\n");
}

#[test]
fn multiple_glue_operators() {
    check("a<>b<>c\n");
}

#[test]
fn escape_backslash() {
    check("Hello \\\\ world\n");
}

#[test]
fn escape_open_brace() {
    check("Hello \\{ world\n");
}

#[test]
fn text_at_eof_no_newline() {
    check("Hello");
}

#[test]
fn content_with_line_comment() {
    check("Hello // comment\n");
}

#[test]
fn consecutive_content_lines() {
    check("Line one.\nLine two.\n");
}

#[test]
fn glue_between_text() {
    check("first<>second\n");
}

#[test]
fn multiple_escapes() {
    check("\\# and \\{ and \\|\n");
}

#[test]
fn content_with_block_comment() {
    // A mid-line block comment must not fragment the containing content
    // line (#2366): `mixed_content`'s zero-progress recovery now elides
    // just the comment tokens (not surrounding whitespace) and folds the
    // TEXT runs on either side back together in one CONTENT_LINE, so this
    // is lossless AND error-free.
    check("Hello /*comment*/ world\n");
}

#[test]
fn glue_at_start() {
    check("<>continued\n");
}

#[test]
fn glue_at_end_before_newline() {
    check("text<>\n");
}

// ── Snapshot tests ──────────────────────────────────────────────────

#[test]
fn insta_content_with_escape() {
    let p = parse("Hello \\# not a tag\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_content_multiple_elements() {
    let p = parse("Hello <>world {name} -> knot #tag\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_plain_text() {
    let p = parse("Hello, world!\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_glue_between_text() {
    let p = parse("a<>b\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_multiple_escapes() {
    let p = parse("\\# \\{ \\\\\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_content_divert_tags() {
    let p = parse("Hello -> knot #tag\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

// ── Positive/negative node assertions ───────────────────────────────

#[test]
fn plain_text_has_mixed_content() {
    let p = parse("Hello\n");
    let root = p.syntax();
    let has_mixed = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MIXED_CONTENT);
    let has_text = root.descendants().any(|n| n.kind() == SyntaxKind::TEXT);
    let has_glue = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GLUE_NODE);
    let has_escape = root.descendants().any(|n| n.kind() == SyntaxKind::ESCAPE);
    let has_divert = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::DIVERT_NODE);
    assert!(has_mixed, "plain text should have MIXED_CONTENT");
    assert!(has_text, "plain text should have TEXT");
    assert!(!has_glue, "plain text should not have GLUE_NODE");
    assert!(!has_escape, "plain text should not have ESCAPE");
    assert!(!has_divert, "plain text should not have DIVERT_NODE");
}

#[test]
fn glue_produces_glue_node() {
    let p = parse("a<>b\n");
    let root = p.syntax();
    let has_glue = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GLUE_NODE);
    let has_escape = root.descendants().any(|n| n.kind() == SyntaxKind::ESCAPE);
    let has_divert = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::DIVERT_NODE);
    assert!(has_glue, "glue line should have GLUE_NODE");
    assert!(!has_escape, "glue line should not have ESCAPE");
    assert!(!has_divert, "glue line should not have DIVERT_NODE");
}

#[test]
fn escape_produces_escape() {
    let p = parse("\\# tag\n");
    let root = p.syntax();
    let has_escape = root.descendants().any(|n| n.kind() == SyntaxKind::ESCAPE);
    let has_glue = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GLUE_NODE);
    let has_divert = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::DIVERT_NODE);
    assert!(has_escape, "escape line should have ESCAPE");
    assert!(!has_glue, "escape line should not have GLUE_NODE");
    assert!(!has_divert, "escape line should not have DIVERT_NODE");
}

#[test]
fn bare_divert_no_mixed_content() {
    let p = parse("-> knot\n");
    let root = p.syntax();
    let has_divert = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::DIVERT_NODE);
    // CONTENT_LINE should not have MIXED_CONTENT as a direct child
    let content_line = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("expected CONTENT_LINE");
    let has_mixed_child = content_line
        .children()
        .any(|c| c.kind() == SyntaxKind::MIXED_CONTENT);
    assert!(has_divert, "bare divert should have DIVERT_NODE");
    assert!(
        !has_mixed_child,
        "bare divert CONTENT_LINE should not have MIXED_CONTENT child"
    );
}

#[test]
fn content_with_divert_has_both() {
    let p = parse("Hello -> knot\n");
    let root = p.syntax();
    let has_mixed = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MIXED_CONTENT);
    let has_divert = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::DIVERT_NODE);
    let has_glue = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GLUE_NODE);
    assert!(has_mixed, "content + divert should have MIXED_CONTENT");
    assert!(has_divert, "content + divert should have DIVERT_NODE");
    assert!(!has_glue, "content + divert should not have GLUE_NODE");
}

#[test]
fn tags_line_not_content_line() {
    let p = parse("#tag\n");
    let root = p.syntax();
    let has_tag_line = root.descendants().any(|n| n.kind() == SyntaxKind::TAG_LINE);
    let has_content_line = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONTENT_LINE);
    assert!(has_tag_line, "hash-only line should produce TAG_LINE");
    assert!(
        !has_content_line,
        "hash-only line should not produce CONTENT_LINE"
    );
}

#[test]
fn empty_line_not_content_line() {
    let p = parse("\n");
    let root = p.syntax();
    let has_empty = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EMPTY_LINE);
    let has_content_line = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONTENT_LINE);
    assert!(has_empty, "bare newline should produce EMPTY_LINE");
    assert!(
        !has_content_line,
        "bare newline should not produce CONTENT_LINE"
    );
}

#[test]
fn logic_line_not_content_line() {
    let p = parse("~ x = 5\n");
    let root = p.syntax();
    let has_logic = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LOGIC_LINE);
    let has_content_line = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONTENT_LINE);
    assert!(has_logic, "tilde line should produce LOGIC_LINE");
    assert!(
        !has_content_line,
        "tilde line should not produce CONTENT_LINE"
    );
}

// ── #2366: mid-line comments must not fragment CONTENT_LINE ─────────

/// A single line of `CONTENT_LINE` around a mid-line block comment used to
/// fragment into two `CONTENT_LINE`s with the `BLOCK_COMMENT` hoisted to
/// `SOURCE_FILE` level (#2366) because `mixed_content`'s catch-all arm broke
/// on zero progress instead of retrying past the comment like its `L_BRACE`
/// arm sibling already did. This must now produce exactly one `CONTENT_LINE`.
#[test]
fn block_comment_mid_line_stays_one_content_line() {
    let p = parse("Hello /* note */ world\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let content_lines: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .collect();
    assert_eq!(
        content_lines.len(),
        1,
        "expected exactly one CONTENT_LINE, got {content_lines:?}"
    );
    // The comment must be nested inside that one CONTENT_LINE, not hoisted
    // to SOURCE_FILE level as a sibling.
    let comment_inside_content_line = content_lines[0]
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        comment_inside_content_line,
        "BLOCK_COMMENT should be nested inside the CONTENT_LINE"
    );
}

/// Byte-for-byte semantic check: only the comment span is elided, the
/// whitespace on both sides of it survives untouched. Verified against real
/// inklecate output — `tests/tests_github/astrochili__narrator/test/units/
/// comments.ink` compiles (per its checked-in `comments.ink.json`) `Before
/// comment ... /* A comment */ ... and after.` to `Before comment ...  ...
/// and after.` (both surrounding spaces survive, producing a double space
/// where the comment used to be — nothing about comment removal collapses
/// or trims adjacent whitespace).
#[test]
fn block_comment_elision_preserves_surrounding_whitespace() {
    let p = parse("Hello /* note */ world\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        text_nodes, "Hello  world",
        "comment span alone should be elided, both adjoining spaces kept \
         (double space), matching inklecate's own semantics"
    );
}

/// The exact corpus case from `astrochili__narrator`'s `comments.ink`, which
/// documents (in its own comment) that a mid-line block comment was "known
/// limitation" for that third-party tool. inklecate itself handles it fine
/// (see `comments.ink.json`), and so must brink.
#[test]
fn block_comment_mid_line_matches_astrochili_corpus_case() {
    let src = "Before comment ... /* A comment */ ... and after.\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let content_lines = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .count();
    assert_eq!(content_lines, 1, "expected exactly one CONTENT_LINE");
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        text_nodes, "Before comment ...  ... and after.",
        "must match inklecate's compiled output byte-for-byte (double \
         space — both spaces surrounding the elided comment survive)"
    );
}

/// A block comment sitting directly between two inline-logic interpolations
/// hits the sibling `L_BRACE` arm's own zero-progress recovery, not the
/// catch-all arm's. Whitespace on both sides of the comment must still
/// survive (same root cause, same fix, both arms now share
/// `skip_comment_tokens`).
#[test]
fn block_comment_between_interpolations_preserves_whitespace() {
    let p = parse("Hello {a} /* c */ {b}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let content_lines = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .count();
    assert_eq!(content_lines, 1, "expected exactly one CONTENT_LINE");
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        text_nodes, "Hello   ",
        "both spaces around the elided comment must survive"
    );
}

/// `LINE_COMMENT` (`//`) mid-line has different semantics from `BLOCK_COMMENT`:
/// it runs to end of line, so the next non-trivia token after it is always
/// `NEWLINE` — `mixed_content` breaks via its ordinary `NEWLINE => break` arm,
/// never reaching the catch-all arm's zero-progress path at all. This case
/// was never broken by #2366 and this fix must not change it.
#[test]
fn line_comment_mid_line_unaffected_by_fix() {
    let src = "Hello // note\n";
    let p = parse(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    let content_lines = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .count();
    assert_eq!(content_lines, 1, "expected exactly one CONTENT_LINE");
    let text_nodes: String = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TEXT)
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(text_nodes, "Hello ", "text before the line comment only");
}

/// A comment at the very start of a line (before any content) is untouched
/// line-leading trivia handling, unrelated to `mixed_content`'s in-line
/// recovery arms — out of scope for #2366, and this fix must not disturb
/// it. It stays hoisted at `SOURCE_FILE` level exactly as before.
#[test]
fn block_comment_at_line_start_unchanged() {
    let src = "/* c */ Hello\n";
    check(src);
    let p = parse(src);
    let content_lines: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .collect();
    assert_eq!(content_lines.len(), 1, "expected exactly one CONTENT_LINE");
    let comment_inside_content_line = content_lines[0]
        .descendants_with_tokens()
        .any(|c| c.kind() == SyntaxKind::BLOCK_COMMENT);
    assert!(
        !comment_inside_content_line,
        "a line-leading comment stays hoisted above CONTENT_LINE, unchanged by this fix"
    );
}

/// #2366's fix shape is a guarded retry: only retry past zero progress when
/// `skip_comment_tokens` actually advanced the position. A stray `}` in
/// plain content is a genuine non-trivia stop token with no comment to
/// elide — `skip_comment_tokens` is a no-op there, so the catch-all arm
/// must still `break` rather than spin. This is the regression test for
/// that hang: if the retry guard were missing, this call would never
/// return.
#[test]
fn stray_r_brace_in_content_does_not_hang() {
    let src = "Hello } world\n";
    let p = parse(src);
    // Malformed input still round-trips losslessly and still diagnoses —
    // only the *no-hang* guarantee is under test here, not error-freeness.
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "stray `{{}}` should still be diagnosed"
    );
}

/// Same guard, `|` flavor: a stray `PIPE` in plain content (outside any
/// `{ ... }` sequence) is also a non-trivia zero-progress stop token.
#[test]
fn stray_pipe_in_content_does_not_hang() {
    let src = "Hello | world\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "stray `|` should still be diagnosed"
    );
}

#[test]
fn choice_not_content_line() {
    let p = parse("* Hello\n");
    let root = p.syntax();
    let has_choice = root.descendants().any(|n| n.kind() == SyntaxKind::CHOICE);
    let has_content_line = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONTENT_LINE);
    assert!(has_choice, "star line should produce CHOICE");
    assert!(
        !has_content_line,
        "star line should not produce CONTENT_LINE"
    );
}
