use crate::support::*;
use brink_ir::lir;

// ─── Glue stripping recognition tests ──────────────────────────────

#[test]
fn glue_leading_recognized_as_plain() {
    // `<>Hello world` — leading glue should be stripped, interior recognized as Plain.
    let program = lower_ink("<>Hello world\n");
    let body = &root(&program).body;

    // Should be: EmitContent(Glue), EmitLine(Plain("Hello world")), EndOfLine
    let mut found_glue = false;
    let mut found_line = false;
    for stmt in body {
        match &stmt.kind {
            lir::StmtKind::EmitContent(c)
                if c.parts.len() == 1 && matches!(c.parts[0], lir::ContentPart::Glue) =>
            {
                found_glue = true;
            }
            lir::StmtKind::EmitLine(e) => {
                assert!(
                    matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello world"),
                    "expected Plain(\"Hello world\")"
                );
                found_line = true;
            }
            _ => {}
        }
    }
    assert!(found_glue, "should emit a Glue statement");
    assert!(found_line, "should emit an EmitLine statement");
}

#[test]
fn glue_trailing_recognized_as_plain() {
    // `Hello world<>` — trailing glue should be stripped, interior recognized as Plain.
    let program = lower_ink("Hello world<>\n");
    let body = &root(&program).body;

    let mut found_line = false;
    let mut found_trailing_glue = false;
    let mut line_pos = None;
    let mut glue_pos = None;
    for (i, stmt) in body.iter().enumerate() {
        match &stmt.kind {
            lir::StmtKind::EmitLine(e) => {
                assert!(
                    matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello world"),
                    "expected Plain(\"Hello world\")"
                );
                found_line = true;
                line_pos = Some(i);
            }
            lir::StmtKind::EmitContent(c)
                if c.parts.len() == 1 && matches!(c.parts[0], lir::ContentPart::Glue) =>
            {
                found_trailing_glue = true;
                glue_pos = Some(i);
            }
            _ => {}
        }
    }
    assert!(found_line, "should emit an EmitLine statement");
    assert!(found_trailing_glue, "should emit a trailing Glue statement");
    assert!(
        line_pos.unwrap() < glue_pos.unwrap(),
        "EmitLine should come before trailing Glue"
    );
}

#[test]
fn glue_both_ends_recognized_as_template() {
    // `<>Hello {x}<>` — both glues stripped, interior recognized as Template.
    let program = lower_ink("VAR x = 1\n<>Hello {x}<>\n");
    let body = &root(&program).body;

    let mut glue_count = 0;
    let mut found_template = false;
    for stmt in body {
        match &stmt.kind {
            lir::StmtKind::EmitContent(c)
                if c.parts.len() == 1 && matches!(c.parts[0], lir::ContentPart::Glue) =>
            {
                glue_count += 1;
            }
            lir::StmtKind::EmitLine(e) => {
                assert!(
                    matches!(&e.line, lir::RecognizedLine::Template { .. }),
                    "expected Template"
                );
                found_template = true;
            }
            _ => {}
        }
    }
    assert!(
        found_template,
        "should emit an EmitLine(Template) statement"
    );
    assert_eq!(
        glue_count, 2,
        "should emit two Glue statements (leading + trailing)"
    );
}

#[test]
fn interior_text_glue_text_merged() {
    // `Hello<>world` — interior glue between two text parts merges them.
    let program = lower_ink("Hello<>world\n");
    let body = &root(&program).body;

    let found_line = body.iter().any(|s| {
        matches!(&s.kind, lir::StmtKind::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Helloworld"))
    });
    assert!(
        found_line,
        "interior Text-Glue-Text should merge into Plain(\"Helloworld\")"
    );
}

#[test]
fn no_glue_plain_still_works() {
    // Plain `Hello world` — no regression, still recognized as Plain.
    let program = lower_ink("Hello world\n");
    let body = &root(&program).body;

    let found_line = body.iter().any(|s| {
        matches!(&s.kind, lir::StmtKind::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello world"))
    });
    assert!(found_line, "plain text should still be recognized as Plain");
}
