use crate::support::*;
use brink_ir::lir;

// ─── Nested choices ─────────────────────────────────────────────────

#[test]
fn nested_choices_create_nested_containers() {
    let p = lower_ink(
        "\
== scene ==
* Outer A
  ** Inner A1
     Deep.
  ** Inner A2
     Also deep.
  - Inner gather.
* Outer B
  B body.
- Outer gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let choice_targets = count_kind(scene, lir::ContainerKind::ChoiceTarget);
    assert!(
        choice_targets >= 4,
        "should have at least 4 choice targets (2 outer + 2 inner), got {choice_targets}"
    );
}

#[test]
fn nested_choice_bodies_have_content() {
    let p = lower_ink(
        "\
== scene ==
* Outer
  ** Inner choice
     Inner body text.
  - Inner gather.
- Outer gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let has_inner = collect_kind(scene, lir::ContainerKind::ChoiceTarget)
        .iter()
        .any(|c| {
            collect_text(&c.body)
                .iter()
                .any(|t| t.contains("Inner body"))
        });
    assert!(
        has_inner,
        "nested choice target should have inner body content"
    );
}
