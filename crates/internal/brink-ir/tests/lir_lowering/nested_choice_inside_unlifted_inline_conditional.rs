use crate::support::*;

// ─── #585: nested choice inside an un-lifted inline conditional (sibling
// of #578, live-reproduced) ─────────────────────────────────────────────
//
// `stmts::lower_stmt`'s `ChoiceSet`/`LabeledBlock`/`Conditional`/`Sequence`
// arm is reached the exact same way #578's `LogicBlock` arm was: a
// multiline conditional embedded in a *choice's own* display/bracket/inner
// text (`content::lower_inline_block`'s doc comment) keeps its
// `InlineConditional` shape all the way to LIR lowering instead of being
// lifted to a top-level `Stmt::Conditional` — but unlike `LogicBlock`, a
// nested choice inside one of that inline conditional's branches can't be
// "properly routed" in place: a `ChoiceSet` needs an addressable child
// container for the runtime to divert into on selection, and
// `lower_inline_block` (unlike `lower_block_with_children`) has no way to
// hand a child container back to its caller. Before this fix that reached
// `lower_stmt`'s `debug_assert!(false, …)` arm — a panic in debug builds,
// a silent drop in release. It now emits a real, non-suppressible E059
// compile error and drops just the malformed nested statement (not the
// whole program).

fn find_e059(diags: &[brink_ir::Diagnostic]) -> Option<&brink_ir::Diagnostic> {
    diags
        .iter()
        .find(|d| d.code == brink_ir::DiagnosticCode::E059)
}

#[test]
fn nested_choice_in_choice_text_inline_conditional_emits_e059_not_panic() {
    // The reproducing input: a top-level choice ("Pick") whose own display
    // text is a multiline `{x > 0: ... }` conditional, one branch of which
    // contains a *nested* `*` choice — never lifted out of choice text by
    // HIR normalization (`content::lower_inline_block`'s doc comment).
    let src =
        "VAR x = 1\n* Pick {x > 0:\n- true: * nested\n    -> END\n- else: text\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e059 = find_e059(&diags).expect("expected E059 for the nested un-lifted choice");
    assert_eq!(e059.code.severity(), brink_ir::Severity::Error);
    assert!(
        e059.message.contains("nested choice"),
        "message should name the offending construct: {}",
        e059.message
    );
    // `lower_to_program` stays total (like E057/E058) — it still returns a
    // program, just without the malformed nested statement.
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(!program.root.body.is_empty());
}

#[test]
fn nested_sequence_in_choice_text_inline_conditional_emits_e059_not_panic() {
    // A multiline sequence nested the same way as the `ChoiceSet` case
    // above — proves the `Sequence` sub-arm (which, unlike `ChoiceSet`,
    // carries its own `AstPtr`) is exercised too, not just asserted by
    // analogy.
    let src = "VAR x = 1\n* Pick {x > 0:\n- true:\n{stopping:\n- one\n- two\n}\n- else: text\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e059 = find_e059(&diags).expect("expected E059 for the nested un-lifted sequence");
    assert_eq!(e059.code.severity(), brink_ir::Severity::Error);
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(!program.root.body.is_empty());
}
