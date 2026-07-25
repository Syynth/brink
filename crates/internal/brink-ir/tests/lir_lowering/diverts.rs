use crate::support::*;
use brink_ir::lir;

// ─── Diverts ────────────────────────────────────────────────────────

#[test]
fn divert_to_done() {
    let p = lower_ink("-> DONE\n");
    let r = root(&p);
    let has_done = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Done)));
    assert!(has_done, "should have a DONE divert");
}

#[test]
fn divert_to_end() {
    let p = lower_ink("-> END\n");
    let r = root(&p);
    let has_end = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::End)));
    assert!(has_end, "should have an END divert");
}

#[test]
fn divert_between_knots() {
    let p = lower_ink(
        "\
== start ==
-> middle

== middle ==
-> finish

== finish ==
The end.
-> END
",
    );
    let start = find_child(&p.root, "start");
    let middle = find_child(&p.root, "middle");

    let start_diverts_to_middle = start.body.iter().any(|s| {
        matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(id) if id == middle.id))
    });
    assert!(start_diverts_to_middle);
}

#[test]
fn divert_to_stitch() {
    let p = lower_ink(
        "\
== tavern ==
-> tavern.order

= order
One ale, please.
-> END
",
    );
    let knot = find_child(&p.root, "tavern");
    let stitch = find_child(knot, "order");

    let diverts_to_stitch = knot.body.iter().any(|s| {
        matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(id) if id == stitch.id))
    });
    assert!(diverts_to_stitch, "knot should divert to its stitch");
}
