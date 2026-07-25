use crate::support::*;
use brink_ir::lir;

// ─── Assignments ────────────────────────────────────────────────────

#[test]
fn assignment_to_global() {
    let p = lower_ink("VAR x = 0\n~ x = 5\n");
    let r = root(&p);
    let has_assign = r.body.iter().any(|s| matches!(s, lir::Stmt::Assign { .. }));
    assert!(has_assign, "root should have an assignment statement");
}

#[test]
fn assignment_with_operator() {
    let p = lower_ink("VAR score = 0\n~ score += 10\n");
    let r = root(&p);
    let has_assign = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                op: brink_ir::AssignOp::Add,
                ..
            }
        )
    });
    assert!(has_assign, "should have += assignment");
}
