use crate::support::*;
use brink_ir::lir;

// ─── Call through variable ──────────────────────────────────────────

#[test]
fn call_through_global_variable() {
    let prog = lower_ink(
        "\
VAR s = -> knot
~ s()

== function knot ==
~ return 1
",
    );

    // The root body should contain an ExprStmt with CallVariable
    let has_call_var = root(&prog).body.iter().any(|stmt| {
        matches!(
            &stmt.kind,
            lir::StmtKind::ExprStmt(lir::Expr::CallVariable { .. })
        )
    });
    assert!(
        has_call_var,
        "call through global variable should produce CallVariable"
    );
}

#[test]
fn call_through_temp_variable() {
    let prog = lower_ink(
        "\
== function run ==
~ temp s = -> helper
~ return s()

== function helper ==
~ return 42
",
    );

    let run = find_by_path(&prog, "run");
    let has_call_var_temp = run.body.iter().any(|stmt| {
        matches!(
            &stmt.kind,
            lir::StmtKind::Return {
                value: Some(lir::Expr::CallVariableTemp { .. }),
                ..
            }
        )
    });
    assert!(
        has_call_var_temp,
        "call through temp variable should produce CallVariableTemp"
    );
}
