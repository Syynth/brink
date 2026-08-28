use crate::support::*;
use brink_ir::lir;

// ─── Sequences ──────────────────────────────────────────────────────

#[test]
fn stopping_sequence() {
    let p = lower_ink(
        "\
{stopping:
    - First time.
    - Every other time.
}
",
    );
    let r = root(&p);
    // Root body now has EnterContainer pointing at a sequence wrapper child.
    let has_enter = r
        .body
        .iter()
        .any(|s| matches!(&s.kind, lir::StmtKind::EnterContainer(_)));
    assert!(has_enter, "root should have EnterContainer for sequence");

    let seq_child = r
        .children
        .iter()
        .find(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(
        seq_child.is_some(),
        "root should have a Sequence child container"
    );
    let seq_child = seq_child.unwrap();

    let has_seq = seq_child.body.iter().any(
        |s| matches!(&s.kind, lir::StmtKind::Sequence(seq) if seq.kind == brink_ir::SequenceType::STOPPING),
    );
    assert!(
        has_seq,
        "sequence container should have a Stopping sequence"
    );
}

#[test]
fn cycle_sequence() {
    let p = lower_ink(
        "\
{cycle:
    - A.
    - B.
    - C.
}
",
    );
    let r = root(&p);
    let seq_child = r
        .children
        .iter()
        .find(|c| c.kind == lir::ContainerKind::Sequence)
        .expect("root should have a Sequence child container");

    let seq = seq_child.body.iter().find_map(|s| {
        if let lir::StmtKind::Sequence(s) = &s.kind {
            Some(s)
        } else {
            None
        }
    });
    assert!(seq.is_some());
    let seq = seq.unwrap();
    assert_eq!(seq.kind, brink_ir::SequenceType::CYCLE);
    assert_eq!(seq.branches.len(), 3);
}
