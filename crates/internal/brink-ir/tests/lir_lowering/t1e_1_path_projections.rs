use crate::support::*;
use brink_ir::lir;

// ─── T1e-1 path projections (docs/t1e-spec.md §2/§8, issue #831) ──────

#[test]
fn ref_marked_bare_var_call_arg_lowers_exactly_like_the_unmarked_form() {
    // `ref gold` (zero path segments — no dotted field, no `[…]` index) is
    // not a real T1e *projection*; it binds exactly like today's unmarked
    // `gold` always has (`lower_ref_path_call_arg`), never hitting the
    // T1e-1 E052-fence (`E099`).
    let src = "VAR gold = 100\n~ heal(ref gold)\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E099).is_none(),
        "a bare single-name `ref` must never hit the T1e-1 fence: {diags:?}"
    );
    let program = program.expect("well-formed program lowers cleanly");
    let gold_id = find_global(&program, "gold").id;
    let heal = find_child(&program.root, "heal");
    let call = program
        .root
        .body
        .iter()
        .find_map(|s| match s {
            lir::Stmt::ExprStmt(e @ lir::Expr::Call { .. }) => Some(e),
            _ => None,
        })
        .expect("heal(ref gold) should lower to an ExprStmt(Call)");
    match call {
        lir::Expr::Call { target, args } => {
            assert_eq!(*target, heal.id);
            assert_eq!(args.len(), 1);
            match &args[0] {
                lir::CallArg::RefGlobal(id) => assert_eq!(*id, gold_id),
                lir::CallArg::RefTemp(..) => panic!("expected RefGlobal(gold), got RefTemp"),
                lir::CallArg::Value(_) => panic!("expected RefGlobal(gold), got Value"),
                lir::CallArg::RefProjection { .. } => {
                    panic!("expected RefGlobal(gold), got RefProjection")
                }
            }
        }
        _ => unreachable!(),
    }
}
