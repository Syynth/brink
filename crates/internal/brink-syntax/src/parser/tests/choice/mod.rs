mod cst;

use super::check;
use crate::parse;

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
