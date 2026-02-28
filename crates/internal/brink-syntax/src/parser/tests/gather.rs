use super::check;
use crate::parse;

#[test]
fn simple_gather() {
    check("- Gathered.\n");
}

#[test]
fn nested_gather() {
    check("- - Deep gather.\n");
}

#[test]
fn gather_with_label() {
    check("- (myLabel) Gathered.\n");
}

#[test]
fn gather_with_divert() {
    check("- -> knot\n");
}

#[test]
fn gather_with_tags() {
    check("- Gathered #tag1\n");
}

#[test]
fn bare_gather() {
    check("-\n");
}

#[test]
fn insta_gather_with_label() {
    let p = parse("- (end) The end.\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_gather_with_divert() {
    let p = parse("- -> done\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
