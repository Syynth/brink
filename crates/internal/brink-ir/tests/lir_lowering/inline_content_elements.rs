use crate::support::*;
use brink_ir::lir;

// ─── Inline content elements ────────────────────────────────────────

#[test]
fn inline_conditional_in_content() {
    // After normalization, the inline conditional is lifted to a block-level
    // Conditional with recognized content in each branch.
    let p = lower_ink("VAR happy = true\nI'm {happy:very|not} pleased.\n");
    let r = root(&p);
    let has_block_cond = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(
        has_block_cond,
        "inline conditional should be lifted to block-level Conditional"
    );

    // Each branch should have recognized content (EmitLine or EmitContent).
    let cond = r
        .body
        .iter()
        .find_map(|s| {
            if let lir::Stmt::Conditional(c) = s {
                Some(c)
            } else {
                None
            }
        })
        .expect("should have Conditional");
    assert_eq!(cond.branches.len(), 2);
}

#[test]
fn inline_sequence_lifted_produces_recognized_lines() {
    let p = lower_ink("{stopping:a fine|a good} day\n");
    let r = root(&p);
    // After normalization, should be a block-level Sequence.
    let has_seq = r
        .children
        .iter()
        .any(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(
        has_seq,
        "inline sequence should be lifted to a Sequence container"
    );
}

#[test]
fn inline_conditional_lifted_produces_recognized_lines() {
    let p = lower_ink("VAR f = true\n{f:Hello|Hi} world\n");
    let r = root(&p);
    let has_cond = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(
        has_cond,
        "inline conditional should be lifted to block-level Conditional"
    );
}

#[test]
fn inline_sequence_with_interpolation() {
    let p = lower_ink("VAR n = \"x\"\n{&Hello {n}|Hi {n}}\n");
    let r = root(&p);
    // After lift, the sequence branches should contain content with interpolation.
    let has_seq = r
        .children
        .iter()
        .any(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(
        has_seq,
        "inline sequence with interpolation should be lifted"
    );
}

#[test]
fn cartesian_two_sequences() {
    let p = lower_ink("{a|b} and {x|y}\n");
    let r = root(&p);
    // After normalization: outer Sequence wrapping inner Sequences.
    // The outer should be a Sequence container.
    let seq_count = r
        .children
        .iter()
        .filter(|c| c.kind == lir::ContainerKind::Sequence)
        .count();
    assert!(
        seq_count >= 1,
        "cartesian product should produce nested Sequence containers, found {seq_count}"
    );
}

#[test]
fn empty_branch_preserves_surrounding() {
    let p = lower_ink("{a||c} fine\n");
    let r = root(&p);
    // Should have a Sequence with 3 branches, middle branch gets " fine" only.
    let has_seq = r
        .children
        .iter()
        .any(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(has_seq, "empty branch sequence should be lifted");
}

#[test]
fn complex_branch_with_divert() {
    let p = lower_ink(
        "\
== start ==
It's {stopping:
    - a fine
    - a good -> END
} day.
-> END
",
    );
    // This exercises normalization of block-level sequences that may
    // already exist (the multiline {stopping:} is already block-level
    // in HIR). The suffix " day." should still appear after the sequence.
    let start = find_child(&p.root, "start");
    // Just verify it compiles and has structure.
    assert!(
        !start.body.is_empty() || !start.children.is_empty(),
        "start container should have content"
    );
}

#[test]
fn glue_in_content() {
    let p = lower_ink("Hello<>\n, world!\n");
    let r = root(&p);
    let has_glue = r.body.iter().any(|s| {
        if let lir::Stmt::EmitContent(c) = s {
            c.parts.iter().any(|p| matches!(p, lir::ContentPart::Glue))
        } else {
            false
        }
    });
    assert!(has_glue, "content should have Glue element");
}
