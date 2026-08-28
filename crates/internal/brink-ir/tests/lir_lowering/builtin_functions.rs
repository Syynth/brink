use crate::support::*;
use brink_ir::lir;

// ─── Builtin functions ──────────────────────────────────────────────

#[test]
fn builtin_random_recognized() {
    let p = lower_ink("VAR x = 0\n~ x = RANDOM(1, 10)\n");
    let r = root(&p);
    let has_builtin = r.body.iter().any(|s| {
        matches!(
            &s.kind,
            lir::StmtKind::Assign {
                value,
                ..
            } if matches!(
                &value.kind,
                lir::ExprKind::CallBuiltin {
                    builtin: lir::BuiltinFn::Random,
                    ..
                }
            )
        )
    });
    assert!(has_builtin, "RANDOM should be recognized as builtin");
}

#[test]
fn builtin_turns_since() {
    let p = lower_ink(
        "\
VAR t = 0
== scene ==
~ t = TURNS_SINCE(-> scene)
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let has_turns = knot.body.iter().any(|s| {
        matches!(
            &s.kind,
            lir::StmtKind::Assign {
                value,
                ..
            } if matches!(
                &value.kind,
                lir::ExprKind::CallBuiltin {
                    builtin: lir::BuiltinFn::TurnsSince,
                    ..
                }
            )
        )
    });
    assert!(has_turns, "TURNS_SINCE should be recognized as builtin");
}
