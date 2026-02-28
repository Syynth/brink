use super::check;
use crate::parse;

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
fn insta_content_with_escape() {
    let p = parse("Hello \\# not a tag\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_content_multiple_elements() {
    let p = parse("Hello <>world {name} -> knot #tag\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
