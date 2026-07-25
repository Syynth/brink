use crate::support::*;

// ─── AUTHOR_WARNING handling ────────────────────────────────────────

#[test]
fn author_warning_does_not_panic() {
    // TODO: author warning — should be silently skipped without hitting
    // the debug_assert in lower_body_children.
    let program = lower_ink("TODO: fix this later\nHello\n");
    let body = &root(&program).body;
    // The TODO line is skipped, but "Hello" content should still be present.
    let texts = collect_text(body);
    assert!(
        texts.iter().any(|t| t.contains("Hello")),
        "content after AUTHOR_WARNING should be preserved"
    );
}
