use crate::support::*;
use brink_ir::lir;

// ─── Choices ────────────────────────────────────────────────────────

#[test]
fn choice_set_creates_containers() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  After A.
* Choice B
  After B.
- Gathered.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    assert!(count_kind(scene, lir::ContainerKind::ChoiceTarget) >= 2);
    assert!(count_kind(scene, lir::ContainerKind::Gather) >= 1);
}

#[test]
fn choice_set_in_knot_body() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  After A.
* Choice B
  After B.
- Gathered.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let has_choice_set = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::ChoiceSet(_)));
    assert!(has_choice_set, "knot should contain a ChoiceSet statement");
}

#[test]
fn choice_targets_have_body_content() {
    let p = lower_ink(
        "\
== scene ==
* First
  Content after first.
* Second
  Content after second.
- Gather point.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let choice_targets = collect_kind(scene, lir::ContainerKind::ChoiceTarget);
    assert_eq!(choice_targets.len(), 2);

    let any_has_content = choice_targets
        .iter()
        .any(|c| !collect_text(&c.body).is_empty());
    assert!(any_has_content, "choice targets should have body content");
}

#[test]
fn gather_has_content() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  A body.
* Choice B
  B body.
- Gathered here.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let gathers = collect_kind(scene, lir::ContainerKind::Gather);
    assert!(!gathers.is_empty(), "should have at least one gather");

    let gather_texts: Vec<String> = gathers.iter().flat_map(|g| collect_text(&g.body)).collect();
    assert!(
        gather_texts.iter().any(|t| t.contains("Gathered here")),
        "gather should contain its inline content, got: {gather_texts:?}"
    );
}

#[test]
fn gather_includes_trailing_statements() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  A.
- Gather.
More content after gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let gathers = collect_kind(scene, lir::ContainerKind::Gather);
    assert!(!gathers.is_empty());

    let gather = &gathers[0];
    let texts = collect_text(&gather.body);
    assert!(
        texts.iter().any(|t| t.contains("More content")),
        "gather should include trailing statements from parent block, got: {texts:?}"
    );
    assert!(
        ends_with_divert(&gather.body),
        "gather should include trailing divert from parent block"
    );
}

#[test]
fn choice_set_has_gather_target() {
    let p = lower_ink(
        "\
== scene ==
* Alpha
  A.
* Beta
  B.
- Meet here.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let cs = knot.body.iter().find_map(|s| {
        if let lir::Stmt::ChoiceSet(cs) = s {
            Some(cs)
        } else {
            None
        }
    });
    assert!(cs.is_some(), "knot should have a ChoiceSet");
    let cs = cs.unwrap();
    assert!(
        cs.gather_target.is_some(),
        "ChoiceSet should have a gather target"
    );

    // The gather target should match a gather container's id
    let gather_id = cs.gather_target.unwrap();
    let gather_exists = find_any(&p.root, &|c| {
        c.id == gather_id && c.kind == lir::ContainerKind::Gather
    })
    .is_some();
    assert!(
        gather_exists,
        "gather_target should reference an existing gather container"
    );
}

#[test]
fn sticky_choice_flag() {
    let p = lower_ink(
        "\
== scene ==
+ Sticky choice
  Body.
- Done.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let choice = knot.body.iter().find_map(|s| {
        if let lir::Stmt::ChoiceSet(cs) = s {
            cs.choices.first()
        } else {
            None
        }
    });
    assert!(choice.is_some());
    assert!(choice.unwrap().is_sticky, "'+' choice should be sticky");
}

#[test]
fn once_only_choice_flag() {
    let p = lower_ink(
        "\
== scene ==
* Once-only choice
  Body.
- Done.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let choice = knot.body.iter().find_map(|s| {
        if let lir::Stmt::ChoiceSet(cs) = s {
            cs.choices.first()
        } else {
            None
        }
    });
    assert!(choice.is_some());
    assert!(
        !choice.unwrap().is_sticky,
        "'*' choice should NOT be sticky"
    );
}
