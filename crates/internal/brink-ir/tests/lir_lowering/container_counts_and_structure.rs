use crate::support::*;
use brink_ir::lir;

// ─── Container counts and structure ─────────────────────────────────

#[test]
fn empty_program_has_only_root() {
    let p = lower_ink("");
    assert_eq!(count_all(&p.root), 1);
    assert_eq!(p.root.kind, lir::ContainerKind::Root);
}

#[test]
fn name_table_contains_definitions() {
    let p = lower_ink("VAR score = 0\nLIST colors = red, green\n");
    assert!(
        p.name_table.iter().any(|n| n == "score"),
        "name table should contain 'score'"
    );
    assert!(
        p.name_table.iter().any(|n| n == "colors"),
        "name table should contain 'colors'"
    );
}

#[test]
fn container_count_knots_stitches() {
    let p = lower_ink(
        "\
Start.
-> knot_a

== knot_a ==
= stitch_1
One.
-> END
= stitch_2
Two.
-> END

== knot_b ==
Three.
-> END
",
    );
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Root), 1);
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 2);
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Stitch), 2);
}
