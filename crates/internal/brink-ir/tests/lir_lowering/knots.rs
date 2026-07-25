use crate::support::*;
use brink_ir::lir;

// ─── Knots ──────────────────────────────────────────────────────────

#[test]
fn knot_creates_container() {
    let p = lower_ink("== greet ==\nHello!\n-> END\n");
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 1);
    let knot = find_child(&p.root, "greet");
    assert_eq!(knot.kind, lir::ContainerKind::Knot);
}

#[test]
fn knot_body_has_content() {
    let p = lower_ink("== greet ==\nWelcome.\n-> END\n");
    let knot = find_child(&p.root, "greet");
    let texts = collect_text(&knot.body);
    assert_eq!(texts, vec!["Welcome."]);
}

#[test]
fn knot_divert_to_end() {
    let p = lower_ink("== greet ==\nHi.\n-> END\n");
    let knot = find_child(&p.root, "greet");
    assert!(ends_with_divert(&knot.body));
    if let Some(lir::Stmt::Divert(d)) = knot.body.last() {
        assert!(matches!(d.target, lir::DivertTarget::End));
    }
}

#[test]
fn multiple_knots() {
    let p = lower_ink(
        "\
== alpha ==
First.
-> END

== beta ==
Second.
-> END
",
    );
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 2);
    let a = find_child(&p.root, "alpha");
    let b = find_child(&p.root, "beta");
    assert_eq!(collect_text(&a.body), vec!["First."]);
    assert_eq!(collect_text(&b.body), vec!["Second."]);
}

#[test]
fn root_divert_to_knot_resolves() {
    let p = lower_ink("-> greet\n== greet ==\nHi.\n-> END\n");
    let r = root(&p);
    let knot = find_child(&p.root, "greet");

    let has_divert_to_knot = r.body.iter().any(|stmt| {
        if let lir::Stmt::Divert(d) = stmt {
            matches!(d.target, lir::DivertTarget::Address(id) if id == knot.id)
        } else {
            false
        }
    });
    assert!(has_divert_to_knot, "root should divert to knot 'greet'");
}
