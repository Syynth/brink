use crate::support::*;
use brink_ir::lir;

// ─── Temp variables ─────────────────────────────────────────────────

#[test]
fn temp_decl_in_knot() {
    let p = lower_ink(
        "\
== func ==
~ temp x = 42
-> END
",
    );
    let knot = find_child(&p.root, "func");
    let has_temp = knot.body.iter().any(|s| {
        matches!(
            &s.kind,
            lir::StmtKind::DeclareTemp {
                slot: 0,
                value: Some(lir::Expr::Int(42)),
                ..
            }
        )
    });
    assert!(has_temp, "knot should have temp declaration at slot 0");
}

#[test]
fn params_occupy_first_temp_slots() {
    let p = lower_ink(
        "\
== func(a, b) ==
~ temp c = 0
-> END
",
    );
    let knot = find_child(&p.root, "func");
    assert_eq!(knot.params.len(), 2);
    assert_eq!(knot.params[0].slot, 0);
    assert_eq!(knot.params[1].slot, 1);
    assert_eq!(knot.temp_slot_count, 3);

    let has_temp_at_2 = knot
        .body
        .iter()
        .any(|s| matches!(&s.kind, lir::StmtKind::DeclareTemp { slot: 2, .. }));
    assert!(has_temp_at_2, "temp 'c' should be at slot 2 (after params)");
}
