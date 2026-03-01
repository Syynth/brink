mod choice;
mod content;
mod cst;
mod declaration;
mod divert;
mod expression;
mod gather;
mod inline;
mod knot;
mod logic;
mod story;
mod tag;

use crate::parse;

/// Parse and assert lossless round-trip.
fn check_lossless(src: &str) {
    let p = parse(src);
    let text = p.syntax().text().to_string();
    assert_eq!(src, text, "lossless round-trip failed");
}

/// Parse, assert lossless, and assert no errors.
fn check(src: &str) {
    let p = parse(src);
    assert_eq!(
        src,
        p.syntax().text().to_string(),
        "lossless round-trip failed"
    );
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── Basic parser tests ──────────────────────────────────────────────

#[test]
fn empty_source() {
    check_lossless("");
    let p = parse("");
    assert!(p.errors().is_empty());
}

#[test]
fn single_newline() {
    check_lossless("\n");
}

#[test]
fn hello_world_content() {
    let src = "Hello, world!\n";
    check_lossless(src);
    let p = parse(src);
    assert!(p.errors().is_empty());
}

#[test]
fn multi_line_content() {
    check("Line one.\nLine two.\n");
}

#[test]
fn content_with_glue() {
    check("Hello<>world\n");
}

#[test]
fn content_with_tags() {
    check("Hello #tag1 #tag2\n");
}

#[test]
fn tag_only_line() {
    check("#tag1 #tag2\n");
}

#[test]
fn content_with_comment() {
    check_lossless("Hello // comment\n");
}

#[test]
fn empty_lines() {
    check_lossless("\n\n\n");
}

#[test]
fn insta_hello_world() {
    let p = parse("Hello, world!\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_content_with_tags() {
    let p = parse("Hello #tag1 #tag2\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_tag_only_line() {
    let p = parse("#tag1 #tag2\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_glue_content() {
    let p = parse("a<>b\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_empty_lines() {
    let p = parse("\n\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

// ── Error recovery tests ──────────────────────────────────────────

#[test]
fn content_no_trailing_newline() {
    // Content line without trailing newline — EOF is a valid terminator
    let src = "Hello";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        p.errors().is_empty(),
        "EOF should be accepted without error"
    );
}

#[test]
fn error_unclosed_bracket_in_choice() {
    let src = "* Hello [world\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected an error for unclosed bracket"
    );
}

#[test]
fn error_missing_rparen_in_external() {
    let src = "EXTERNAL myFunc(a, b\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty(), "expected an error for missing `)`");
}

#[test]
fn error_unterminated_string() {
    let src = "~ x = \"hello\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected an error for unterminated string"
    );
}
