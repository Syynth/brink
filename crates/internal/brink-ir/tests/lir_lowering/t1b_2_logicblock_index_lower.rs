use crate::support::*;

// ─── T1b-2 (#570): LogicBlock/Index lower for real, no dialect gate needed ──
//
// `lower_ink_with_warnings` deliberately mirrors a caller that (like a
// suppressed dialect gate) never checks `brink_analyzer::analyze`'s
// diagnostics before lowering to LIR. Through T1b-1, a `LogicBlock`/`Index`/…
// HIR node reaching `lower_to_program` this way would panic via
// `debug_assert!` in debug builds and silently drop data (`None` /
// `lir::ExprKind::Null`) in release builds — caught by the (now-retired) E053
// backstop, which refused to produce a program at all (#572 review). T1b-2
// replaces that rejection with real lowering, so these HIR node kinds are no
// longer "residual" — this test now proves the opposite of its T1b-1
// version: the program lowers successfully and correctly.

#[test]
fn logic_block_lowers_without_a_dialect_gate_in_the_loop() {
    let (program, _diags) =
        lower_ink_with_warnings("Hello\n~ {\ntemp x = 0\nx = x + 1\n}\nWorld\n");
    let program = program.expect("LogicBlock should lower to a real program in T1b-2");
    assert!(!program.root.body.is_empty());
}

#[test]
fn index_expression_lowers_without_a_dialect_gate_in_the_loop() {
    let (program, _diags) = lower_ink_with_warnings("VAR a = 5\n~ x = a[0]\n");
    let program = program.expect("Index expression should lower to a real program in T1b-2");
    assert!(!program.root.body.is_empty());
}
