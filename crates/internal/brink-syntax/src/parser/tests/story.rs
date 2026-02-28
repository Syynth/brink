use super::check;
use crate::parse;

#[test]
fn todo_warning() {
    check("TODO: fix this later\n");
}

#[test]
fn todo_warning_no_colon() {
    check("TODO fix this later\n");
}

#[test]
fn stray_closing_brace() {
    let p = parse("}\n");
    // Lossless round-trip
    assert_eq!("}\n", p.syntax().text().to_string());
    // No errors — stray brace is a recovery node, not a parse error
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn multiline_block_at_top_level() {
    check("{\n- x > 5:\n  Big.\n- else:\n  Small.\n}\n");
}

#[test]
fn knot_then_content() {
    check("== myKnot ==\nHello from knot.\n");
}

#[test]
fn declaration_before_knot() {
    check("VAR x = 5\n== myKnot ==\nHello.\n");
}

#[test]
fn insta_todo_warning() {
    let p = parse("TODO: fix this later\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_stray_closing_brace() {
    let p = parse("}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
