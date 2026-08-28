use crate::support::*;
use brink_ir::lir;

// ─── Temp scoping across choice/gather boundaries ─────────────────────

/// Return true if any expression in the container tree is `GetGlobal`.
fn has_get_global(container: &lir::Container) -> bool {
    fn in_expr(e: &lir::Expr) -> bool {
        match e {
            lir::Expr::GetGlobal(_) => true,
            lir::Expr::Prefix(_, inner) | lir::Expr::Postfix(inner, _) => in_expr(inner),
            lir::Expr::Infix(a, _, b) => in_expr(a) || in_expr(b),
            _ => false,
        }
    }
    fn in_content(c: &lir::Content) -> bool {
        c.parts.iter().any(|p| match p {
            lir::ContentPart::Interpolation(e) => in_expr(e),
            lir::ContentPart::InlineConditional(cond) => cond
                .branches
                .iter()
                .any(|b| b.condition.as_ref().is_some_and(in_expr) || in_stmts(&b.body)),
            _ => false,
        })
    }
    fn in_stmts(stmts: &[lir::Stmt]) -> bool {
        stmts.iter().any(|s| match &s.kind {
            lir::StmtKind::ExprStmt(e) => in_expr(e),
            lir::StmtKind::Assign { value, .. } => in_expr(value),
            lir::StmtKind::DeclareTemp { value, .. } => value.as_ref().is_some_and(in_expr),
            lir::StmtKind::Conditional(c) => c
                .branches
                .iter()
                .any(|b| b.condition.as_ref().is_some_and(in_expr) || in_stmts(&b.body)),
            lir::StmtKind::ChoiceSet(cs) => cs
                .choices
                .iter()
                .any(|ch| ch.condition.as_ref().is_some_and(in_expr)),
            lir::StmtKind::EmitContent(c) => in_content(c),
            lir::StmtKind::EmitLine(em) | lir::StmtKind::EvalLine(em) => {
                if let lir::RecognizedLine::Template { slot_exprs, .. } = &em.line {
                    slot_exprs.iter().any(in_expr)
                } else {
                    false
                }
            }
            lir::StmtKind::ChoiceOutput { content, emission } => {
                in_content(content)
                    || emission.as_ref().is_some_and(|em| {
                        if let lir::RecognizedLine::Template { slot_exprs, .. } = &em.line {
                            slot_exprs.iter().any(in_expr)
                        } else {
                            false
                        }
                    })
            }
            _ => false,
        })
    }
    in_stmts(&container.body) || container.children.iter().any(has_get_global)
}

#[test]
fn temp_visible_in_choice_body_after_gather() {
    // A temp declared in a gather continuation must be visible in the
    // next choice set's bodies. A program with no VAR declarations
    // should produce no globals and no GetGlobal expressions.
    // Multiple levels of choice+gather+labeled-block to match TheIntercept.
    let p = lower_ink(
        "\
-> test_knot
=== test_knot ===
 * [A]
   A.
 * [B]
   B.
- First gather.
 * [C]
   C.
 * [D]
   D.
- Second gather.
- (labeled)
  ~ temp saved = true
 * [Yes]
   -> DONE
 * [No]
   {saved:Saved was true.}
   -> DONE
",
    );

    assert!(
        p.globals.is_empty(),
        "program has no VAR — should have no globals"
    );
    assert!(
        !has_get_global(&p.root),
        "program has no VAR — should have no GetGlobal expressions",
    );
}
