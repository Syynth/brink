use crate::support::*;
use brink_ir::lir;

// ─── Complex integration scenarios ──────────────────────────────────

#[test]
fn full_story_structure() {
    let p = lower_ink(
        "\
VAR visited_inn = false

-> town_square

== town_square ==
You stand in the town square.
* [Go to the inn] -> inn
* [Go to the market] -> market

== inn ==
~ visited_inn = true
The inn is warm and cozy.
* Order a drink
  You order an ale.
* Sit by the fire
  The fire crackles.
- The innkeeper nods.
-> town_square

== market ==
{visited_inn: The innkeeper waves from across the square.}
Stalls line the street.
-> END
",
    );

    // Structural assertions
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Root), 1);
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 3);
    assert!(count_kind(&p.root, lir::ContainerKind::ChoiceTarget) >= 4);
    assert!(count_kind(&p.root, lir::ContainerKind::Gather) >= 1);

    // Globals
    assert_eq!(p.globals.len(), 1);
    let visited = find_global(&p, "visited_inn");
    assert!(matches!(visited.default, lir::ConstValue::Bool(false)));
    assert!(visited.mutable);

    // Root diverts to town_square
    let r = root(&p);
    let town = find_child(&p.root, "town_square");
    let root_diverts = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(id) if id == town.id))
    });
    assert!(root_diverts, "root should divert to town_square");

    // Inn has assignment to visited_inn
    let inn = find_child(&p.root, "inn");
    let has_assign = inn
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Assign { .. }));
    assert!(has_assign, "inn should assign visited_inn = true");

    // Market has a block-level conditional (inline conditional was lifted by normalization)
    let market = find_child(&p.root, "market");
    let has_cond = market
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(
        has_cond,
        "market should have block-level conditional for visited_inn"
    );
}

#[test]
fn multiple_choice_sets_cascade_gathers() {
    let p = lower_ink(
        "\
== scene ==
* A
  A body.
* B
  B body.
- First gather.
* C
  C body.
* D
  D body.
- Second gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let gather_count = count_kind(scene, lir::ContainerKind::Gather);
    assert!(
        gather_count >= 2,
        "should have at least 2 gathers, got {gather_count}"
    );

    // One gather should contain -> END
    let gathers = collect_kind(scene, lir::ContainerKind::Gather);
    let any_gather_has_end = gathers.iter().any(|g| {
        g.body.iter().any(
            |s| matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::End)),
        )
    });
    assert!(
        any_gather_has_end,
        "one gather should contain the -> END divert"
    );
}

#[test]
fn list_variable_default_references_items() {
    let p = lower_ink("LIST mood = (happy), sad, (excited)\n");
    assert_eq!(p.lists.len(), 1);
    assert_eq!(p.list_items.len(), 3);

    let ordinals: Vec<i32> = p.list_items.iter().map(|i| i.ordinal).collect();
    assert_eq!(ordinals, vec![1, 2, 3]);
}

#[test]
fn divert_with_arguments() {
    let p = lower_ink("-> greet(42)\n\n== greet(name) ==\nHello.\n-> END\n");
    let r = root(&p);
    let divert = r.body.iter().find_map(|s| {
        if let lir::Stmt::Divert(d) = s
            && matches!(d.target, lir::DivertTarget::Address(_))
        {
            return Some(d);
        }
        None
    });
    assert!(divert.is_some(), "should have a divert with args");
    assert!(
        !divert.unwrap().args.is_empty(),
        "divert should have arguments"
    );
}

#[test]
fn expr_statement() {
    let p = lower_ink(
        "\
EXTERNAL do_something()
~ do_something()
",
    );
    let r = root(&p);
    let has_expr_stmt = r.body.iter().any(|s| matches!(s, lir::Stmt::ExprStmt(_)));
    assert!(
        has_expr_stmt,
        "should have an ExprStmt for the function call"
    );
}

#[test]
fn choice_body_content_in_conditional_branch() {
    let program = lower_ink(
        "\
== scene(x) ==
{true:
    + A choice
        Body content.
        -> END
}
->->
",
    );
    let scene = find_by_path(&program, "scene");
    let choice_target = scene
        .children
        .iter()
        .flat_map(|c| std::iter::once(c).chain(c.children.iter()))
        .find(|c| c.kind == lir::ContainerKind::ChoiceTarget)
        .expect("should have a choice target");
    // Choice target should have body content (not just the choice output)
    let has_end_divert = choice_target.body.iter().any(|s| {
        if let lir::Stmt::Divert(d) = s {
            matches!(d.target, lir::DivertTarget::End)
        } else {
            false
        }
    });
    assert!(
        has_end_divert,
        "choice target should contain -> END from the choice body"
    );
}
