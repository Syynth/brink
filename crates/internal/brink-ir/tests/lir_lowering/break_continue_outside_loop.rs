use crate::support::*;
use brink_ir::lir;

// ─── #577: break/continue outside a loop is a targeted compile error ──────
//
// Previously `~ { break }`/`~ { continue }` with no enclosing `while`/`for`
// lowered unconditionally to `lir::Stmt::LogicBreak`/`LogicContinue`, and
// codegen's `container.rs` silently degraded the resulting unguarded jump
// to `Opcode::Nop` (`self.loop_stack.is_empty()`) instead of ever surfacing
// an error. `blocks::lower_block_stmt` now rejects it at LIR-lowering time
// (E057, Error severity) and skips emitting the statement — a real,
// non-suppressible compile error (`brink-db`'s `lir_query` gates `program:
// None` on any Error-severity LIR diagnostic, bypassing `// brink-disable-
// all`, which only covers analysis-phase diagnostics), not a cosmetic note.

fn find_e057(diags: &[brink_ir::Diagnostic]) -> Option<&brink_ir::Diagnostic> {
    diags
        .iter()
        .find(|d| d.code == brink_ir::DiagnosticCode::E057)
}

#[test]
fn break_outside_any_loop_emits_e057_error_and_is_not_lowered() {
    let (program, diags) = lower_ink_with_warnings("Hello\n~ {\nbreak\n}\n");
    let e057 = find_e057(&diags).expect("expected E057 for break outside a loop");
    assert_eq!(e057.code.severity(), brink_ir::Severity::Error);
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(
        !program
            .root
            .body
            .iter()
            .any(|s| matches!(s, lir::Stmt::LogicBreak)),
        "the unguarded break must not be lowered to a LogicBreak statement"
    );
}

#[test]
fn continue_outside_any_loop_emits_e057_error_and_is_not_lowered() {
    let (program, diags) = lower_ink_with_warnings("Hello\n~ {\ncontinue\n}\n");
    let e057 = find_e057(&diags).expect("expected E057 for continue outside a loop");
    assert_eq!(e057.code.severity(), brink_ir::Severity::Error);
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(
        !program
            .root
            .body
            .iter()
            .any(|s| matches!(s, lir::Stmt::LogicContinue)),
        "the unguarded continue must not be lowered to a LogicContinue statement"
    );
}

#[test]
fn break_after_a_while_loop_at_the_same_depth_still_errors() {
    // `loop_depth` must be decremented back to 0 on exiting the while body —
    // a break textually after the loop (sibling, not nested) is still an
    // error, proving the counter doesn't leak across sibling statements.
    let (_program, diags) =
        lower_ink_with_warnings("Hello\n~ {\ntemp x = 0\nwhile x < 3 {\nx = x + 1\n}\nbreak\n}\n");
    assert!(
        find_e057(&diags).is_some(),
        "expected E057 for a break textually after (not inside) the loop"
    );
}

#[test]
fn break_inside_if_inside_while_is_allowed() {
    // `if`/`else` nesting inside a loop body must not reset loop_depth —
    // break/continue reached through conditional nesting is still valid.
    let (program, diags) = lower_ink_with_warnings(
        "Hello\n~ {\ntemp x = 0\nwhile x < 3 {\nif x == 1 {\nbreak\n}\nx = x + 1\n}\n}\n",
    );
    assert!(
        find_e057(&diags).is_none(),
        "break nested in if-inside-while must not error: {diags:?}"
    );
    let program = program.expect("should lower to a real program");
    assert!(!program.root.body.is_empty());
}

#[test]
fn continue_inside_for_loop_is_allowed() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1, 2, 3]\n~ {\nfor v in a {\nif v == 2 {\ncontinue\n}\n}\n}\n",
    );
    assert!(find_e057(&diags).is_none(), "unexpected E057: {diags:?}");
    let program = program.expect("should lower to a real program");
    assert!(!program.root.body.is_empty());
}
