use crate::support::*;
use brink_ir::lir;

// ─── Choice inline divert folding ───────────────────────────────────

/// Choice with inline divert: choice target body starts with `ChoiceOutput`,
/// then `Divert`, then `EndOfLine` (the divert comes from the HIR body preamble).
#[test]
fn choice_inline_divert_in_target_body() {
    let p = lower_ink(
        "\
== scene ==
* Go somewhere -> other
- Gathered.
-> END
== other ==
Arrived.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let c0 = find_child(scene, "c-0");
    assert_eq!(c0.kind, lir::ContainerKind::ChoiceTarget);

    // Body should be: EmitLine("Go somewhere") or ChoiceOutput, Divert(other), EndOfLine, Divert(gather)
    assert!(
        matches!(
            &c0.body[0].kind,
            lir::StmtKind::EmitLine(_) | lir::StmtKind::ChoiceOutput { .. }
        ),
        "first stmt should be EmitLine or ChoiceOutput with content, got {:?}",
        std::mem::discriminant(&c0.body[0].kind)
    );
    assert!(
        matches!(&c0.body[1].kind, lir::StmtKind::Divert(d) if matches!(d.target, lir::DivertTarget::Address(_))),
        "second stmt should be Divert to 'other'"
    );
    assert!(
        matches!(&c0.body[2].kind, lir::StmtKind::EndOfLine),
        "third stmt should be EndOfLine"
    );
}

/// Choice without inline divert: choice target body starts with `ChoiceOutput`,
/// then `EndOfLine` (no divert in preamble).
#[test]
fn choice_no_divert_endofline_in_target_body() {
    let p = lower_ink(
        "\
== scene ==
* Stay here
  Some body text.
- Gathered.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let c0 = find_child(scene, "c-0");

    // Body: EmitLine("Stay here") or ChoiceOutput, EndOfLine, EmitContent("Some body text."), EndOfLine, Divert(gather)
    assert!(
        matches!(
            &c0.body[0].kind,
            lir::StmtKind::EmitLine(_) | lir::StmtKind::ChoiceOutput { .. }
        ),
        "first stmt should be EmitLine or ChoiceOutput"
    );
    assert!(
        matches!(&c0.body[1].kind, lir::StmtKind::EndOfLine),
        "second stmt should be EndOfLine"
    );
    assert!(
        matches!(
            &c0.body[2].kind,
            lir::StmtKind::EmitContent(_) | lir::StmtKind::EmitLine(_)
        ),
        "third stmt should be EmitContent or EmitLine"
    );
}

/// Fallback choice (no content) with only a divert: no `ChoiceOutput`, body starts
/// with `Divert` then `EndOfLine`.
#[test]
fn fallback_choice_divert_only_in_target_body() {
    let p = lower_ink(
        "\
== scene ==
* [Visible choice] text
* -> other
- Gathered.
-> END
== other ==
Arrived.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    // c-1 is the fallback choice
    let c1 = find_child(scene, "c-1");

    // Fallback has no start/inner content → no ChoiceOutput.
    // Body: Divert(other), EndOfLine, Divert(gather)
    assert!(
        matches!(&c1.body[0].kind, lir::StmtKind::Divert(d) if matches!(d.target, lir::DivertTarget::Address(_))),
        "first stmt should be Divert to 'other', got {:?}",
        std::mem::discriminant(&c1.body[0].kind)
    );
    assert!(
        matches!(&c1.body[1].kind, lir::StmtKind::EndOfLine),
        "second stmt should be EndOfLine"
    );
}

/// `ChoiceOutput` is purely content — no divert, no newline. `Divert` and `EndOfLine`
/// are separate body stmts.
#[test]
fn choice_output_is_content_only() {
    let p = lower_ink(
        "\
== scene ==
* Hello world -> END
- Gathered.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let c0 = find_child(scene, "c-0");

    // Output should be EmitLine (recognized) or ChoiceOutput (fallback)
    match &c0.body[0].kind {
        lir::StmtKind::EmitLine(emission) => {
            assert!(
                matches!(&emission.line, lir::RecognizedLine::Plain(s) if s == "Hello world"),
                "EmitLine should contain 'Hello world'"
            );
        }
        lir::StmtKind::ChoiceOutput { content, .. } => {
            assert!(
                content
                    .parts
                    .iter()
                    .all(|p| matches!(p, lir::ContentPart::Text(_) | lir::ContentPart::Spring)),
                "ChoiceOutput should only contain text parts (Text or Spring)"
            );
        }
        other => panic!(
            "expected EmitLine or ChoiceOutput as first body stmt, got {:?}",
            std::mem::discriminant(other)
        ),
    }

    // The divert to END follows as a separate stmt
    assert!(
        matches!(&c0.body[1].kind, lir::StmtKind::Divert(d) if matches!(d.target, lir::DivertTarget::End)),
        "second stmt should be Divert to END"
    );
    assert!(
        matches!(&c0.body[2].kind, lir::StmtKind::EndOfLine),
        "third stmt should be EndOfLine"
    );
}

#[test]
fn interpolated_choice_text_is_recognized_as_template() {
    let p = lower_ink(
        "\
== scene ==
VAR name = \"Alice\"
* Hello {name}[ world.] goodbye.
- -> END
",
    );
    let scene = find_child(&p.root, "scene");

    // Find the ChoiceSet statement
    let choice_set = scene
        .body
        .iter()
        .find_map(|s| match &s.kind {
            lir::StmtKind::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .unwrap();

    let choice = &choice_set.choices[0];

    // Display (start + bracket) should be recognized as a Template
    assert!(
        matches!(
            choice.display_emission.as_ref().map(|e| &e.line),
            Some(lir::RecognizedLine::Template { .. })
        ),
        "display_emission should be Some(Template)"
    );

    // Output (start + inner) should be recognized as a Template
    assert!(
        matches!(
            choice.output_emission.as_ref().map(|e| &e.line),
            Some(lir::RecognizedLine::Template { .. })
        ),
        "output_emission should be Some(Template)"
    );
}
