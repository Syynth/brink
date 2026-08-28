use crate::support::*;
use brink_ir::lir;

// ─── Template recognition: slot-only and whitespace-only ─────────────
//
// Templates should only be created when there is non-whitespace source
// text that corresponds to output content. Slot-only lines and lines
// where the only text between slots is whitespace should fall through
// to EmitContent, which uses emit_value (correctly suppresses null).

#[test]
fn slot_only_content_not_recognized_as_template() {
    // `{name}` — single interpolation, no surrounding text.
    // Should be EmitContent, NOT EmitLine(Template).
    let p = lower_ink("VAR name = \"world\"\n{name}\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(&s.kind, lir::StmtKind::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        !has_template,
        "slot-only content `{{name}}` should NOT be recognized as Template; \
         should fall through to EmitContent",
    );
}

#[test]
fn whitespace_only_text_between_slots_not_recognized_as_template() {
    // `{x} {y}` — two interpolations with only whitespace between them.
    // No non-whitespace source text → should NOT be a template.
    let p = lower_ink("VAR x = 1\nVAR y = 2\n{x} {y}\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(&s.kind, lir::StmtKind::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        !has_template,
        "content with only whitespace between slots should NOT be a Template",
    );
}

#[test]
fn text_with_interpolation_recognized_as_template() {
    // `Hello {name}!` — has non-whitespace text around the slot.
    // Should be recognized as a Template.
    let p = lower_ink("VAR name = \"world\"\nHello {name}!\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(&s.kind, lir::StmtKind::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "content with non-whitespace text + interpolation should be Template",
    );
}
