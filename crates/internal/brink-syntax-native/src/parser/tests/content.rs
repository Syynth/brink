//! Content lines & interpolation — glue, labels, prose whitespace.
//! Family for #1194.

use super::*;

// ── Significant inter-token whitespace in content position ───────────────

#[test]
fn space_after_glue_marker_survives_inside_the_text_node() {
    // `<> But surely.` — the space after the `<>` glue marker is the leading
    // char of the following prose run and must be preserved in the `TEXT`
    // node, not discarded (the exhibit-fogg glue lines' divergence).
    let src = "flow f() {\n  <> But surely.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&line, SyntaxKind::GLUE_NODE));
    assert_eq!(
        text_run_concat(&line),
        " But surely.",
        "space after `<>` must be folded into the following TEXT node"
    );
}

#[test]
fn interior_prose_whitespace_between_words_is_unchanged() {
    // Guard the baseline the fix must not regress: a plain content line's
    // interior word spacing was already preserved by `text_run_until`, and
    // still is.
    let src = "flow f() {\n  You have three gold coins.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert_eq!(text_run_concat(&line), "You have three gold coins.");
}

// ── G-1: labeled content lines ────────────────────────────────────────

#[test]
fn labeled_content_line_produces_a_label_node() {
    let src = "flow f() {\n  (start) You arrive at the garden.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let content_line = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .expect("CONTENT_LINE");
    assert!(has_node_kind(&content_line, SyntaxKind::LABEL));
    assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
}

#[test]
fn labeled_content_line_as_backward_loop_divert_target() {
    // Ink's `- (start)` mid-flow re-entry pattern (README G-1 finding):
    // a label on a plain content line, later diverted back to from
    // further down the same flow.
    let src = concat!(
        "flow loop() {\n",
        "  (start) You spin around.\n",
        "  {?\n",
        "    * [Again] -> start\n",
        "    * [Stop] -> END\n",
        "  }\n",
        "}\n",
    );
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
    // Both diverts (one to the label, one to END) parse as real nodes —
    // exercises N-1 and G-1 together, the realistic combined idiom.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::DIVERT_STMT), 2);
}

#[test]
fn unlabeled_prose_starting_with_paren_is_unaffected() {
    // A multi-word parenthetical does not match the `L_PAREN IDENT
    // R_PAREN` lookahead shape, so it stays plain prose, not a spurious
    // LABEL + error.
    let src = "flow f() {\n  (a very long aside) continues here.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn label_inside_conditional_body_is_still_a_content_line_label() {
    // G-1 says "ANY content line" — including one nested inside the
    // annotated-brace family's colon/braced bodies, since those recurse
    // through `body_line`/`content_line` too.
    let src = "flow f() {\n  {if hp > 0: (alive) You live.}\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}
