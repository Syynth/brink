use super::*;

// ── Gather with inline choice ───────────────────────────────────────

#[test]
fn gather_with_inline_choice() {
    let gather = parse_first::<Gather>("- * hello\n");
    assert!(
        gather.mixed_content().is_none(),
        "should not have mixed content when choice is present"
    );
    let choice = gather
        .choice()
        .expect("gather should have an inline choice");
    let sc = choice
        .start_content()
        .expect("choice should have start content");
    let text: String = sc.texts().map(|t| t.to_string()).collect();
    assert_eq!(text, "hello");
}

#[test]
fn gather_with_inline_sticky_choice() {
    let gather = parse_first::<Gather>("- + sticky\n");
    let choice = gather
        .choice()
        .expect("gather should have an inline choice");
    assert!(choice.bullets().expect("should have bullets").is_sticky());
}

#[test]
fn labeled_gather_with_inline_choice() {
    let gather = parse_first::<Gather>("- (lbl) * hello\n");
    assert_eq!(
        gather.label().and_then(|l| l.name()),
        Some("lbl".to_string())
    );
    assert!(gather.choice().is_some(), "should have inline choice");
}

#[test]
fn gather_no_choice_plain_content() {
    let gather = parse_first::<Gather>("- just text\n");
    assert!(
        gather.choice().is_none(),
        "plain gather should not have a choice"
    );
    assert!(gather.mixed_content().is_some());
}
