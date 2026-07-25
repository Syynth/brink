use crate::support::*;

// ─── Knot parameters ───────────────────────────────────────────────

#[test]
fn knot_with_params() {
    let p = lower_ink(
        "\
== greet(name) ==
Hello.
-> END
",
    );
    let knot = find_child(&p.root, "greet");
    assert_eq!(knot.params.len(), 1);
    assert_eq!(knot.params[0].slot, 0);
    assert!(!knot.params[0].is_ref);
}

#[test]
fn knot_with_ref_param() {
    let p = lower_ink(
        "\
== modify(ref x) ==
~ x = 10
-> END
",
    );
    let knot = find_child(&p.root, "modify");
    assert_eq!(knot.params.len(), 1);
    assert!(knot.params[0].is_ref);
}
