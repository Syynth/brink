use crate::support::*;

// ─── String interpolation in const context ──────────────────────────

#[test]
fn string_interpolation_in_const_emits_e030() {
    let source = "VAR name = \"world\"\nCONST greeting = \"hello {name}\"\n{greeting}\n";
    let (_program, warnings) = lower_ink_with_warnings(source);
    assert!(
        warnings
            .iter()
            .any(|w| w.code == brink_ir::DiagnosticCode::E030),
        "expected E030 warning for string interpolation in const, got: {warnings:?}"
    );
}
