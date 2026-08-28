use crate::support::*;
use brink_ir::lir;

// ─── READ_COUNT builtin ─────────────────────────────────────────────

#[test]
fn builtin_read_count_with_divert_target() {
    // READ_COUNT(-> knot) should lower to CallBuiltin { builtin: ReadCount, args: [DivertTarget] }
    let p = lower_ink(
        "\
VAR t = 0
== knot ==
~ t = READ_COUNT(-> knot)
-> END
",
    );
    let knot = find_child(&p.root, "knot");
    let has_read_count = knot.body.iter().any(|s| {
        matches!(
            &s.kind,
            lir::StmtKind::Assign {
                value: lir::Expr::CallBuiltin {
                    builtin: lir::BuiltinFn::ReadCount,
                    ..
                },
                ..
            }
        )
    });
    assert!(
        has_read_count,
        "READ_COUNT(-> knot) should be recognized as a ReadCount builtin, not null"
    );
}

#[test]
fn builtin_read_count_with_variable() {
    // READ_COUNT(x) where x is a variable holding a divert target
    // should lower to CallBuiltin { builtin: ReadCount, args: [GetGlobal] }
    let p = lower_ink(
        "\
VAR x = -> knot
VAR t = 0
== knot ==
~ t = READ_COUNT(x)
-> END
",
    );
    let knot = find_child(&p.root, "knot");
    let has_read_count = knot.body.iter().any(|s| {
        matches!(
            &s.kind,
            lir::StmtKind::Assign {
                value: lir::Expr::CallBuiltin {
                    builtin: lir::BuiltinFn::ReadCount,
                    ..
                },
                ..
            }
        )
    });
    assert!(
        has_read_count,
        "READ_COUNT(x) should be recognized as a ReadCount builtin, not null"
    );
}
