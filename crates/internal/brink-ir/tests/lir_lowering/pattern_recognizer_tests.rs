use crate::support::*;
use brink_ir::lir;

// ─── Pattern recognizer tests ───────────────────────────────────────

#[test]
fn plain_text_recognized() {
    let program = lower_ink("Hello, world!\n");
    let body = &root(&program).body;
    assert!(
        matches!(&body[0], lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello, world!")),
        "plain text should be recognized as EmitLine(Plain(...))"
    );
}

#[test]
fn plain_text_source_hash() {
    let program = lower_ink("Hello\n");
    let body = &root(&program).body;
    if let lir::Stmt::EmitLine(emission) = &body[0] {
        assert_eq!(
            emission.metadata.source_hash,
            brink_format::content_hash("Hello"),
            "source_hash should match content_hash of the text"
        );
    } else {
        panic!(
            "expected EmitLine, got {:?}",
            std::mem::discriminant(&body[0])
        );
    }
}

#[test]
fn plain_text_with_tag_recognized() {
    let program = lower_ink("Hello #tag\n");
    let body = &root(&program).body;
    if let lir::Stmt::EmitLine(emission) = &body[0] {
        assert!(
            matches!(&emission.line, lir::RecognizedLine::Plain(s) if s == "Hello "),
            "text before tag should be plain"
        );
        assert_eq!(emission.tags.len(), 1, "should have one tag");
    } else {
        panic!("expected EmitLine for plain text with tag");
    }
}

fn find_template(body: &[lir::Stmt]) -> Option<(Vec<brink_format::LinePart>, usize)> {
    body.iter().find_map(|s| {
        if let lir::Stmt::EmitLine(e) = s
            && let lir::RecognizedLine::Template { parts, slot_exprs } = &e.line
        {
            return Some((parts.clone(), slot_exprs.len()));
        }
        None
    })
}

#[test]
fn interpolation_recognized_as_template() {
    let program = lower_ink("VAR name = \"world\"\nHello, {name}!\n");
    let body = &root(&program).body;
    let (parts, slot_count) = find_template(body).expect("should be recognized as Template");
    assert_eq!(slot_count, 1, "one slot expression");
    assert_eq!(parts.len(), 3, "literal + slot + literal");
    assert!(matches!(&parts[0], brink_format::LinePart::Literal(s) if s == "Hello, "));
    assert!(matches!(&parts[1], brink_format::LinePart::Slot(0)));
    assert!(matches!(&parts[2], brink_format::LinePart::Literal(s) if s == "!"));
}

#[test]
fn multiple_interpolations_recognized() {
    let program = lower_ink("VAR x = 1\nVAR y = 2\n{x} and {y}\n");
    let body = &root(&program).body;
    let (parts, slot_count) =
        find_template(body).expect("multiple interpolations should be recognized as Template");
    assert_eq!(slot_count, 2, "two slot expressions");
    assert!(matches!(&parts[0], brink_format::LinePart::Slot(0)));
    assert!(matches!(&parts[1], brink_format::LinePart::Literal(s) if s == " and "));
    assert!(matches!(&parts[2], brink_format::LinePart::Slot(1)));
}

#[test]
fn interpolation_only_not_recognized_as_template() {
    // Single interpolation with no surrounding text should NOT be a Template —
    // it falls through to EmitContent, which uses emit_value (correctly
    // suppresses null/void results).
    let program = lower_ink("VAR x = 1\n{x}\n");
    let body = &root(&program).body;
    let has_template = find_template(body).is_some();
    assert!(
        !has_template,
        "slot-only content {{x}} should NOT be recognized as Template"
    );
    // Should be EmitContent instead.
    let has_emit_content = body.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)));
    assert!(
        has_emit_content,
        "slot-only content should fall through to EmitContent"
    );
}

#[test]
fn glue_not_recognized() {
    let program = lower_ink("Hello<>\n");
    let body = &root(&program).body;
    let has_emit_content = body.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)));
    assert!(
        has_emit_content,
        "content with glue should fall back to EmitContent"
    );
}

#[test]
fn glue_with_interpolation_not_recognized() {
    let program = lower_ink("VAR x = 1\nHello<>{x}\n");
    let body = &root(&program).body;
    let has_emit_content = body.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)));
    assert!(
        has_emit_content,
        "content with glue and interpolation should fall back to EmitContent"
    );
}

#[test]
fn multiple_plain_lines() {
    let program = lower_ink("Line one\nLine two\n");
    let body = &root(&program).body;
    let emit_lines: Vec<_> = body
        .iter()
        .filter(|s| matches!(s, lir::Stmt::EmitLine(_)))
        .collect();
    assert_eq!(
        emit_lines.len(),
        2,
        "two plain text lines should both be recognized"
    );
}

#[test]
fn collect_text_includes_recognized() {
    let program = lower_ink("Hello, world!\n");
    let texts = collect_text(&root(&program).body);
    assert_eq!(texts, vec!["Hello, world!"]);
}
