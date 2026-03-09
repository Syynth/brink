mod cst;

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
fn double_dash_gather() {
    check("--\n");
}

#[test]
fn triple_dash_gather() {
    check("---\n");
}

#[test]
fn gather_with_label_and_divert() {
    check("- (end) -> done\n");
}

#[test]
fn gather_with_label_and_tags() {
    check("- (lbl) Text #tag\n");
}

#[test]
fn gather_all_parts() {
    check("- (lbl) Text -> next #tag\n");
}

#[test]
fn gather_with_glue() {
    check("- Hello<>world\n");
}

#[test]
fn gather_with_tunnel_call() {
    check("- -> target ->\n");
}

#[test]
fn gather_with_tunnel_onwards() {
    check("- ->->\n");
}

#[test]
fn gather_with_thread() {
    check("- <- background\n");
}

#[test]
fn gather_with_inline_logic() {
    check("- Hello {x}\n");
}

#[test]
fn gather_with_escape() {
    check("- Hello \\# tag\n");
}

// ── Gather-choice same line ─────────────────────────────────────────

#[test]
fn gather_with_inline_choice() {
    check("- * hello\n");
}

#[test]
fn gather_with_inline_sticky_choice() {
    check("- + sticky\n");
}

#[test]
fn labeled_gather_with_inline_choice() {
    check("- (lbl) * hello\n");
}

#[test]
fn gather_with_inline_choice_bracket_divert() {
    check("- * [bracket]inner -> target\n");
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
