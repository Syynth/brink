use crate::support::*;
use brink_ir::lir;

// ─── Stitches ───────────────────────────────────────────────────────

#[test]
fn stitch_creates_container() {
    let p = lower_ink(
        "\
== tavern ==
= order
What'll it be?
-> END
",
    );
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Stitch), 1);
    let stitch = find_by_path(&p, "tavern.order");
    assert_eq!(stitch.kind, lir::ContainerKind::Stitch);
    assert_eq!(collect_text(&stitch.body), vec!["What'll it be?"]);
}

#[test]
fn knot_with_stitches_and_no_own_body() {
    let p = lower_ink(
        "\
== tavern ==
= order
Ordering.
-> END
= pay
Paying.
-> END
",
    );
    let _knot = find_child(&p.root, "tavern");
    let stitch_order = find_by_path(&p, "tavern.order");
    let stitch_pay = find_by_path(&p, "tavern.pay");

    assert_eq!(collect_text(&stitch_order.body), vec!["Ordering."]);
    assert_eq!(collect_text(&stitch_pay.body), vec!["Paying."]);
}

#[test]
fn stitch_is_child_of_knot() {
    let p = lower_ink(
        "\
== tavern ==
= order
Hi.
-> END
",
    );
    let knot = find_child(&p.root, "tavern");
    let stitch = find_child(knot, "order");
    assert_eq!(stitch.kind, lir::ContainerKind::Stitch);
}
