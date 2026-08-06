use crate::support::*;
use brink_ir::lir;

// ─── Expressions ────────────────────────────────────────────────────

#[test]
fn interpolation_in_content() {
    let p = lower_ink("VAR name = \"world\"\nHello {name}!\n");
    let r = root(&p);
    // Interpolations are now recognized as templates (phase 3).
    let has_template = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "content with interpolation should be recognized as Template"
    );
}

#[test]
fn infix_expression_in_assignment() {
    let p = lower_ink("VAR x = 0\n~ x = 2 + 3\n");
    let r = root(&p);
    let has_infix = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::Infix(_, brink_ir::InfixOp::Add, _),
                ..
            }
        )
    });
    assert!(has_infix, "assignment should have infix Add expression");
}

#[test]
fn prefix_negate() {
    let p = lower_ink("VAR x = 0\n~ x = -x\n");
    let r = root(&p);
    let has_prefix = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::Prefix(brink_ir::PrefixOp::Negate, _),
                ..
            }
        )
    });
    assert!(has_prefix, "assignment should have prefix negate");
}

#[test]
fn boolean_not() {
    let p = lower_ink("VAR flag = true\n~ flag = not flag\n");
    let r = root(&p);
    let has_not = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::Prefix(brink_ir::PrefixOp::Not, _),
                ..
            }
        )
    });
    assert!(has_not, "assignment should have prefix not");
}
