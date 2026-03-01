use super::*;

// ── Choice accessor invariants ───────────────────────────────────────
//
// These tests verify that all accessors on `Choice` return the expected
// Some/None pattern for a given input.

#[test]
fn choice_full_accessors() {
    // * (label) {cond} Start[bracket]inner -> target #tag
    let choice = parse_first::<Choice>("* (label) {cond} Start[bracket]inner -> target #tag\n");
    assert!(choice.bullets().is_some(), "bullets");
    assert!(choice.label().is_some(), "label");
    assert_eq!(choice.conditions().count(), 1, "conditions count");
    assert!(choice.start_content().is_some(), "start_content");
    assert!(choice.bracket_content().is_some(), "bracket_content");
    assert!(choice.inner_content().is_some(), "inner_content");
    assert!(choice.divert().is_some(), "divert");
    assert!(choice.tags().is_some(), "tags");
}

#[test]
fn choice_minimal_accessors() {
    // * Just text
    let choice = parse_first::<Choice>("* Just text\n");
    assert!(choice.bullets().is_some(), "bullets");
    assert!(choice.label().is_none(), "no label");
    assert_eq!(choice.conditions().count(), 0, "no conditions");
    assert!(choice.start_content().is_some(), "start_content");
    assert!(choice.bracket_content().is_none(), "no bracket_content");
    assert!(choice.inner_content().is_none(), "no inner_content");
    assert!(choice.divert().is_none(), "no divert");
    assert!(choice.tags().is_none(), "no tags");
}

// ── ContentLine accessor invariants ──────────────────────────────────

#[test]
fn content_line_full_accessors() {
    // Text -> target #tag
    let cl = parse_first::<ContentLine>("Text -> target #tag\n");
    assert!(cl.mixed_content().is_some(), "mixed_content");
    assert!(cl.divert().is_some(), "divert");
    assert!(cl.tags().is_some(), "tags");
}

#[test]
fn content_line_text_only() {
    let cl = parse_first::<ContentLine>("Just text\n");
    assert!(cl.mixed_content().is_some(), "mixed_content");
    assert!(cl.divert().is_none(), "no divert");
    assert!(cl.tags().is_none(), "no tags");
}

#[test]
fn content_line_divert_only() {
    let cl = parse_first::<ContentLine>("-> target\n");
    assert!(cl.mixed_content().is_none(), "no mixed_content");
    assert!(cl.divert().is_some(), "divert");
    assert!(cl.tags().is_none(), "no tags");
}

#[test]
fn content_line_tags_only() {
    // A line with just tags parses as a TagLine, not ContentLine.
    // A content line with text + tags:
    let cl = parse_first::<ContentLine>("Hello #tag\n");
    assert!(cl.mixed_content().is_some(), "mixed_content");
    assert!(cl.tags().is_some(), "tags");
}

// ── Gather accessor invariants ───────────────────────────────────────

#[test]
fn gather_full_accessors() {
    let gather = parse_first::<Gather>("- (lbl) Text -> target #tag\n");
    assert!(gather.dashes().is_some(), "dashes");
    assert!(gather.label().is_some(), "label");
    assert!(gather.mixed_content().is_some(), "mixed_content");
    assert!(gather.divert().is_some(), "divert");
    assert!(gather.tags().is_some(), "tags");
}

#[test]
fn gather_minimal_accessors() {
    let gather = parse_first::<Gather>("- Text\n");
    assert!(gather.dashes().is_some(), "dashes");
    assert!(gather.label().is_none(), "no label");
    assert!(gather.mixed_content().is_some(), "mixed_content");
    assert!(gather.divert().is_none(), "no divert");
    assert!(gather.tags().is_none(), "no tags");
}

#[test]
fn gather_divert_only() {
    let gather = parse_first::<Gather>("- -> target\n");
    assert!(gather.dashes().is_some(), "dashes");
    assert!(gather.divert().is_some(), "divert");
}

// ── KnotBody accessor invariants ─────────────────────────────────────

#[test]
fn knot_body_mixed_content() {
    let tree = parse_tree(
        "=== k ===\n\
         Line one\n\
         ~ temp x = 1\n\
         * Choice A\n\
         * Choice B\n\
         - Gather\n\
         = stitch_one\n\
         Stitch content\n",
    );
    let body: KnotBody = first(tree.syntax());
    assert!(body.content_lines().count() >= 1, "has content_lines");
    assert!(body.logic_lines().count() >= 1, "has logic_lines");
    assert_eq!(body.choices().count(), 2, "choices count");
    assert_eq!(body.gathers().count(), 1, "gathers count");
    assert_eq!(body.stitches().count(), 1, "stitches count");
}

#[test]
fn knot_body_empty() {
    let tree = parse_tree("=== k ===\n");
    let body: KnotBody = first(tree.syntax());
    assert_eq!(body.content_lines().count(), 0, "no content_lines");
    assert_eq!(body.logic_lines().count(), 0, "no logic_lines");
    assert_eq!(body.choices().count(), 0, "no choices");
    assert_eq!(body.gathers().count(), 0, "no gathers");
    assert_eq!(body.stitches().count(), 0, "no stitches");
}

// ── StitchBody accessor invariants ───────────────────────────────────

#[test]
fn stitch_body_mixed_content() {
    let tree = parse_tree(
        "=== k ===\n\
         = s\n\
         Line one\n\
         ~ temp x = 1\n\
         * Choice A\n\
         - Gather\n",
    );
    let body: StitchBody = first(tree.syntax());
    assert!(body.content_lines().count() >= 1, "has content_lines");
    assert!(body.logic_lines().count() >= 1, "has logic_lines");
    assert_eq!(body.choices().count(), 1, "choices count");
    assert_eq!(body.gathers().count(), 1, "gathers count");
}
