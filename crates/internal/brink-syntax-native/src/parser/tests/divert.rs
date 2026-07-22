//! Diverts, tunnels, and splice. Family for #1196.

use super::*;

#[test]
fn divert_and_tunnel_and_return() {
    let src = "flow a() {\n  -> b\n}\nflow b() {\n  -> c ->\n  return\n}\nflow c() {\n  return -> a\n  -> END\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn splice_inside_choice_point() {
    let src = "flow hub() {\n  {?\n    <- options()\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── N-1: inline diverts in content position ─────────────────────────

#[test]
fn divert_after_choice_bracket_text_is_a_divert_node_not_text() {
    // The exact shape from the exhibit/manual-stitch-v1 fixtures:
    // `* [text] -> target`. Before N-1's fix this parsed with zero errors
    // but folded `-> know_about_wager` into a literal TEXT run.
    let src = "flow f() {\n  {?\n    * [The wager.] -> know_about_wager\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice_inner = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_INNER_CONTENT)
        .expect("CHOICE_INNER_CONTENT");
    assert!(
        has_node_kind(&choice_inner, SyntaxKind::DIVERT_STMT),
        "expected a DIVERT_STMT inside CHOICE_INNER_CONTENT, tree: {choice_inner:#?}"
    );
    // The divert's target must be a real PATH, not swallowed text.
    let divert = choice_inner
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    let path = target.path().expect("path");
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["know_about_wager".to_string()]);
}

#[test]
fn divert_after_dotted_path_target_in_choice_text_parses() {
    // manual-stitch-v1's other shape: a dotted stitch-addressing target.
    let src = "flow f() {\n  {?\n    * [go] -> f.g\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    let path = target.path().expect("path");
    assert!(!path.crosses_module_wall()); // `.` not `::`
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["f".to_string(), "g".to_string()]);
}

#[test]
fn divert_inside_multiline_choice_body_after_prose_is_a_divert_node() {
    // The sticky-choice shape: a `->` following prose on the SAME content
    // line inside a braced CHOICE_BODY (as opposed to a divert on its own
    // line, which was already recognized before this fix).
    let src = "flow f() {\n  {?\n    + [Eat] {\n      You eat another donut. -> f\n    }\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let body = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE_BODY)
        .expect("CHOICE_BODY");
    let content_line = body
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    // The divert is a child of the same CONTENT_LINE as the preceding
    // prose, not a sibling body item.
    assert!(
        has_node_kind(&content_line, SyntaxKind::DIVERT_STMT),
        "expected DIVERT_STMT nested inside the CONTENT_LINE, tree: {content_line:#?}"
    );
    assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
}

#[test]
fn tunnel_call_in_content_position_parses() {
    // `->->` in content position: still a TUNNEL_CALL, not a divert
    // followed by stray text.
    let src = "flow f() {\n  {?\n    * [go] visit -> place ->\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::TUNNEL_CALL));
}

#[test]
fn divert_to_end_in_content_position_parses() {
    let src = "flow f() {\n  {?\n    * [go] The end. -> END\n  }\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let divert = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT");
    let target = find_child::<crate::ast::DivertTarget>(&divert).expect("DIVERT_TARGET");
    assert!(target.is_end());
}
