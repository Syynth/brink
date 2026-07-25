use crate::support::*;

// ─── Counting flags ─────────────────────────────────────────────────

#[test]
fn knots_have_empty_counting_flags_by_default() {
    let p = lower_ink("== greet ==\nHi.\n-> END\n");
    let knot = find_child(&p.root, "greet");
    assert!(
        knot.counting_flags.is_empty(),
        "knots should have empty counting flags by default (VISITS added only when referenced)"
    );
}

#[test]
fn visit_count_reference_sets_flag() {
    let p = lower_ink(
        "\
== scene ==
-> END

== check ==
{scene > 0: Already visited.}
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    assert!(
        scene
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "referenced container should have VISITS flag"
    );
}

#[test]
fn variable_divert_target_gets_visit_flags() {
    let program = lower_ink(
        "\
VAR x = -> here
-> there
== there ==
-> x
== here ==
Here.
-> DONE
",
    );
    let here = find_by_path(&program, "here");
    assert!(
        here.counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "container targeted by variable divert must have VISITS flag"
    );
    assert!(
        here.counting_flags
            .contains(brink_format::CountingFlags::TURNS),
        "container targeted by variable divert must have TURNS flag"
    );
}

#[test]
fn variable_tunnel_target_gets_visit_flags() {
    let program = lower_ink(
        "\
VAR x = -> tunnel
-> x ->
== tunnel ==
->->
",
    );
    let tunnel = find_by_path(&program, "tunnel");
    assert!(
        tunnel
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "container targeted by variable tunnel must have VISITS flag"
    );
}

#[test]
fn divert_target_expr_gets_visit_flags() {
    let program = lower_ink(
        "\
~ temp x = -> target
-> x
== target ==
Done.
-> DONE
",
    );
    let target = find_by_path(&program, "target");
    assert!(
        target
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "container whose address is taken in an expr must have VISITS flag"
    );
}

#[test]
fn labeled_gather_with_visits_gets_count_start_only() {
    let program = lower_ink(
        "\
== scene ==
- (loop)
{loop} times.
{loop < 3: -> loop}
-> DONE
",
    );
    let scene = find_by_path(&program, "scene");
    // Find the gather container with the label
    let gather = scene
        .children
        .iter()
        .find(|c| c.labeled)
        .expect("should have a labeled gather child");
    assert!(
        gather
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "labeled gather referenced by visit count should have VISITS"
    );
    assert!(
        gather
            .counting_flags
            .contains(brink_format::CountingFlags::COUNT_START_ONLY),
        "labeled gather with VISITS should have COUNT_START_ONLY for self-goto loops"
    );
}
