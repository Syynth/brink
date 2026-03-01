mod cst;

use super::{check, check_lossless};
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
    // Block comments are stop characters in text_content, so the parser
    // produces an error here. Just verify lossless round-trip.
    check_lossless("Hello /*comment*/ world\n");
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
