use super::*;

// ── ChoiceBullets ────────────────────────────────────────────────────

#[test]
fn bullets_once_not_sticky() {
    let bullets = parse_first::<ChoiceBullets>("* Choice\n");
    assert_eq!(bullets.depth(), 1);
    assert!(!bullets.is_sticky());
}

#[test]
fn bullets_sticky() {
    let bullets = parse_first::<ChoiceBullets>("+ Sticky\n");
    assert_eq!(bullets.depth(), 1);
    assert!(bullets.is_sticky());
}

#[test]
fn bullets_nested_depth() {
    let bullets = parse_first::<ChoiceBullets>("** Nested\n");
    assert_eq!(bullets.depth(), 2);
    assert!(!bullets.is_sticky());
}

#[test]
fn bullets_triple_depth() {
    let bullets = parse_first::<ChoiceBullets>("*** Deep\n");
    assert_eq!(bullets.depth(), 3);
}

#[test]
fn bullets_is_mixed_star_star() {
    let bullets = parse_first::<ChoiceBullets>("** Both stars\n");
    assert!(!bullets.is_mixed());
}

#[test]
fn bullets_is_mixed_plus_plus() {
    let bullets = parse_first::<ChoiceBullets>("++ Both plus\n");
    assert!(!bullets.is_mixed());
}

#[test]
fn bullets_is_mixed_star_plus() {
    let bullets = parse_first::<ChoiceBullets>("*+ Mixed\n");
    assert!(bullets.is_mixed());
}

#[test]
fn bullets_is_mixed_plus_star() {
    let bullets = parse_first::<ChoiceBullets>("+* Mixed\n");
    assert!(bullets.is_mixed());
}

// ── Label ────────────────────────────────────────────────────────────

#[test]
fn choice_label() {
    let choice = parse_first::<Choice>("* (myLabel) Choice text\n");
    let label = choice.label().unwrap();
    assert_eq!(label.name().as_deref(), Some("myLabel"));
}

#[test]
fn choice_no_label() {
    let choice = parse_first::<Choice>("* Just text\n");
    assert!(choice.label().is_none());
}

// ── Choice accessors ─────────────────────────────────────────────────

#[test]
fn choice_start_content() {
    let choice = parse_first::<Choice>("* Start text\n");
    let sc = choice.start_content().unwrap();
    assert!(sc.texts().next().is_some());
}

#[test]
fn choice_bracket_content() {
    let choice = parse_first::<Choice>("* Start[bracket]inner\n");
    let bc = choice.bracket_content().unwrap();
    assert!(bc.texts().next().is_some());
}

#[test]
fn choice_inner_content() {
    let choice = parse_first::<Choice>("* Start[bracket]inner\n");
    let ic = choice.inner_content().unwrap();
    assert!(ic.texts().next().is_some());
}

#[test]
fn choice_with_divert() {
    let choice = parse_first::<Choice>("* Choice -> target\n");
    assert!(choice.divert().is_some());
}

#[test]
fn choice_without_divert() {
    let choice = parse_first::<Choice>("* Just text\n");
    assert!(choice.divert().is_none());
}

#[test]
fn choice_with_tags() {
    let choice = parse_first::<Choice>("* Choice #tagged\n");
    let tags = choice.tags().unwrap();
    assert_eq!(tags.tags().count(), 1);
}

#[test]
fn choice_with_condition() {
    let choice = parse_first::<Choice>("* {flag} Conditional choice\n");
    assert_eq!(choice.conditions().count(), 1);
}

#[test]
fn choice_with_multiple_conditions() {
    let choice = parse_first::<Choice>("* {a} {b} Double cond\n");
    assert_eq!(choice.conditions().count(), 2);
}

// ── ChoiceStartContent accessors ─────────────────────────────────────

#[test]
fn choice_start_content_texts() {
    let sc = parse_first::<ChoiceStartContent>("* Hello world\n");
    let texts: Vec<_> = sc.texts().collect();
    assert!(!texts.is_empty());
}

#[test]
fn choice_start_content_inline_logic() {
    let sc = parse_first::<ChoiceStartContent>("* Hello {x}\n");
    assert!(sc.inline_logics().next().is_some());
}

#[test]
fn choice_start_content_glue() {
    let sc = parse_first::<ChoiceStartContent>("* Hello<>world\n");
    assert!(sc.glue_nodes().next().is_some());
}

#[test]
fn choice_start_content_escape() {
    let sc = parse_first::<ChoiceStartContent>("* Hello\\#world\n");
    assert!(sc.escapes().next().is_some());
}

// ── ChoiceBracketContent accessors ───────────────────────────────────

#[test]
fn choice_bracket_content_texts() {
    let bc = parse_first::<ChoiceBracketContent>("* Start[bracket text]inner\n");
    assert!(bc.texts().next().is_some());
}

#[test]
fn choice_bracket_content_inline_logic() {
    let bc = parse_first::<ChoiceBracketContent>("* Start[{x}]inner\n");
    assert!(bc.inline_logics().next().is_some());
}

// ── ChoiceInnerContent accessors ─────────────────────────────────────

#[test]
fn choice_inner_content_texts() {
    let ic = parse_first::<ChoiceInnerContent>("* Start[bracket]inner text\n");
    assert!(ic.texts().next().is_some());
}

#[test]
fn choice_inner_content_inline_logic() {
    let ic = parse_first::<ChoiceInnerContent>("* Start[bracket]{x}\n");
    assert!(ic.inline_logics().next().is_some());
}
