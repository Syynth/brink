use crate::support::*;
use brink_ir::lir;

// ─── T1e-2 path projections (docs/t1e-spec.md §2/§3, tracking #828) ───
//
// T1e-2 replaces the T1e-1 E052-fence (`E099`) with real `MakeProjection`
// lowering for a real path segment — `CallArg::RefProjection`.

#[test]
fn ref_dotted_field_projection_call_arg_lowers_to_ref_projection() {
    // `ref npc.hp` has a real path segment (a dotted field) — a genuine
    // T1e projection. `brink-analyzer`'s durable-root/position checks pass
    // (`npc` is a durable global VAR, direct call-argument position), and
    // T1e-2 now lowers it for real: a single field segment, a literal
    // string expression carrying the field name.
    let src = "VAR npc = 100\n~ heal(ref npc.hp)\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E099).is_none(),
        "a real path-segment projection must no longer hit the T1e-1 fence: {diags:?}"
    );
    let program = program.expect("well-formed program lowers cleanly");
    let npc_id = find_global(&program, "npc").id;
    let call = program
        .root
        .body
        .iter()
        .find_map(|s| match s {
            lir::Stmt::ExprStmt(e @ lir::Expr::Call { .. }) => Some(e),
            _ => None,
        })
        .expect("heal(ref npc.hp) should lower to an ExprStmt(Call)");
    match call {
        lir::Expr::Call { args, .. } => {
            assert_eq!(args.len(), 1);
            match &args[0] {
                lir::CallArg::RefProjection { root, segments } => {
                    assert_eq!(*root, npc_id);
                    assert_eq!(segments.len(), 1, "expected one dotted-field segment");
                    match &segments[0] {
                        lir::Expr::String(s) => {
                            assert_eq!(s.parts.len(), 1);
                            match &s.parts[0] {
                                lir::StringPart::Literal(text) => assert_eq!(text, "hp"),
                                lir::StringPart::Interpolation(_) => {
                                    panic!("expected a literal field-name part, got Interpolation")
                                }
                            }
                        }
                        _ => panic!("expected a literal field-name string"),
                    }
                }
                _ => panic!("expected RefProjection(npc, [hp])"),
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn ref_index_projection_call_arg_lowers_to_ref_projection() {
    let src = "VAR inventory = 100\n~ temp idx = 0\n~ heal(ref inventory[idx])\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E099).is_none(),
        "an index-segment projection must no longer hit the T1e-1 fence: {diags:?}"
    );
    let program = program.expect("well-formed program lowers cleanly");
    let inventory_id = find_global(&program, "inventory").id;
    let call = program
        .root
        .body
        .iter()
        .find_map(|s| match s {
            lir::Stmt::ExprStmt(e @ lir::Expr::Call { .. }) => Some(e),
            _ => None,
        })
        .expect("heal(ref inventory[idx]) should lower to an ExprStmt(Call)");
    match call {
        lir::Expr::Call { args, .. } => {
            assert_eq!(args.len(), 1);
            match &args[0] {
                lir::CallArg::RefProjection { root, segments } => {
                    assert_eq!(*root, inventory_id);
                    assert_eq!(segments.len(), 1, "expected one index segment");
                    // The index expression is `idx` (a temp read) — snapshot
                    // at creation, evaluated once here as an ordinary
                    // `GetTemp`.
                    assert!(
                        matches!(&segments[0], lir::Expr::GetTemp(..)),
                        "expected the index segment to lower `idx` via GetTemp"
                    );
                }
                _ => panic!("expected RefProjection(inventory, [idx])"),
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn block_scoped_temp_visible_for_the_rest_of_its_own_block_no_false_positive() {
    // Nested scopes (an `if` inside the block) must still see the outer
    // block's temp — E082 is only for reads *after* the declaring block has
    // fully closed, never for a live nested scope.
    let src = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n    if gold > 0 {\n        temp y = name\n    }\n}\nDone.\n-> END\n";
    let (_program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E082).is_none(),
        "a nested scope must still see the outer block's live temp: {diags:?}"
    );
}
