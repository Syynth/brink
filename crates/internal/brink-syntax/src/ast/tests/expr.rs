use super::*;

// ── Expr enum variant casting ────────────────────────────────────────

#[test]
fn expr_variant_integer_lit() {
    let expr = parse_first::<Expr>("VAR x = 42\n");
    assert!(matches!(expr, Expr::IntegerLit(_)));
}

#[test]
fn expr_variant_float_lit() {
    let expr = parse_first::<Expr>("VAR x = 3.14\n");
    assert!(matches!(expr, Expr::FloatLit(_)));
}

#[test]
fn expr_variant_string_lit() {
    let expr = parse_first::<Expr>("VAR x = \"hi\"\n");
    assert!(matches!(expr, Expr::StringLit(_)));
}

#[test]
fn expr_variant_boolean_lit() {
    let expr = parse_first::<Expr>("VAR x = true\n");
    assert!(matches!(expr, Expr::BooleanLit(_)));
}

#[test]
fn expr_variant_path() {
    let expr = parse_first::<Expr>("=== k ===\n~ temp x = y\n");
    // The first Expr at assignment level should be Path for 'x' (target)
    // but we want a path on the RHS — use InfixExpr or assignment target
    assert!(matches!(expr, Expr::Path(_)));
}

#[test]
fn expr_variant_infix() {
    let expr = parse_first::<Expr>("VAR x = 1 + 2\n");
    assert!(matches!(expr, Expr::Infix(_)));
}

#[test]
fn expr_variant_prefix() {
    let expr = parse_first::<Expr>("VAR x = -1\n");
    assert!(matches!(expr, Expr::Prefix(_)));
}

#[test]
fn expr_variant_paren() {
    let expr = parse_first::<Expr>("VAR x = (1)\n");
    assert!(matches!(expr, Expr::Paren(_)));
}

#[test]
fn expr_variant_function_call() {
    let tree = parse_tree("=== k ===\n~ temp x = foo()\n");
    let fc: FunctionCall = first(tree.syntax());
    let expr = Expr::cast(fc.syntax().clone()).unwrap();
    assert!(matches!(expr, Expr::FunctionCall(_)));
}

#[test]
fn expr_variant_divert_target() {
    let tree = parse_tree("=== k ===\n~ temp x = -> target\n");
    let dte: DivertTargetExpr = first(tree.syntax());
    let expr = Expr::cast(dte.syntax().clone()).unwrap();
    assert!(matches!(expr, Expr::DivertTarget(_)));
}

#[test]
fn expr_variant_list_expr() {
    let tree = parse_tree("VAR x = (a, b)\n");
    let le: ListExpr = first(tree.syntax());
    let expr = Expr::cast(le.syntax().clone()).unwrap();
    assert!(matches!(expr, Expr::ListExpr(_)));
}

// ── InfixExpr lhs/rhs ───────────────────────────────────────────────

#[test]
fn infix_expr_lhs_rhs_integer() {
    let infix = parse_first::<InfixExpr>("VAR x = 1 + 2\n");
    let lhs = infix.lhs().unwrap();
    let rhs = infix.rhs().unwrap();
    assert!(matches!(lhs, Expr::IntegerLit(_)));
    assert!(matches!(rhs, Expr::IntegerLit(_)));
}

#[test]
fn infix_expr_lhs_rhs_mixed() {
    let infix = parse_first::<InfixExpr>("VAR x = 1 + 2.5\n");
    let lhs = infix.lhs().unwrap();
    let rhs = infix.rhs().unwrap();
    assert!(matches!(lhs, Expr::IntegerLit(_)));
    assert!(matches!(rhs, Expr::FloatLit(_)));
}

#[test]
fn infix_expr_op_token() {
    let infix = parse_first::<InfixExpr>("VAR x = 1 + 2\n");
    let op = infix.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::PLUS);
}

#[test]
fn infix_expr_nested() {
    // 1 + 2 * 3 => INFIX(1, +, INFIX(2, *, 3))
    let infix = parse_first::<InfixExpr>("VAR x = 1 + 2 * 3\n");
    let lhs = infix.lhs().unwrap();
    assert!(matches!(lhs, Expr::IntegerLit(_)));
    let rhs = infix.rhs().unwrap();
    assert!(matches!(rhs, Expr::Infix(_)));
}

// ── Assignment target ────────────────────────────────────────────────

#[test]
fn assignment_target_is_path() {
    let assignment = parse_first::<Assignment>("=== k ===\n~ x = 1\n");
    let target = assignment.target().unwrap();
    assert!(matches!(target, Expr::Path(_)));
}

#[test]
fn assignment_op_eq() {
    let assignment = parse_first::<Assignment>("=== k ===\n~ x = 1\n");
    let op = assignment.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::EQ);
}

#[test]
fn assignment_op_plus_eq() {
    let assignment = parse_first::<Assignment>("=== k ===\n~ x += 1\n");
    let op = assignment.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::PLUS_EQ);
}

#[test]
fn assignment_op_minus_eq() {
    let assignment = parse_first::<Assignment>("=== k ===\n~ x -= 1\n");
    let op = assignment.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::MINUS_EQ);
}

// ── ReturnStmt ───────────────────────────────────────────────────────

#[test]
fn return_stmt_with_value() {
    let ret = parse_first::<ReturnStmt>("== function f() ==\n~ return 42\n");
    assert!(ret.has_value());
    let val = ret.value().unwrap();
    assert!(matches!(val, Expr::IntegerLit(_)));
}

#[test]
fn return_stmt_bare() {
    let ret = parse_first::<ReturnStmt>("== function f() ==\n~ return\n");
    assert!(!ret.has_value());
    assert!(ret.value().is_none());
}

// ── ParenExpr ────────────────────────────────────────────────────────

#[test]
fn paren_expr_inner_infix() {
    let paren = parse_first::<ParenExpr>("VAR x = (1 + 2)\n");
    let inner = paren.inner().unwrap();
    assert!(matches!(inner, Expr::Infix(_)));
}

#[test]
fn paren_expr_inner_literal() {
    let paren = parse_first::<ParenExpr>("VAR x = (42)\n");
    let inner = paren.inner().unwrap();
    assert!(matches!(inner, Expr::IntegerLit(_)));
}

// ── PrefixExpr ───────────────────────────────────────────────────────

#[test]
fn prefix_expr_negation() {
    let prefix = parse_first::<PrefixExpr>("VAR x = -5\n");
    let op = prefix.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::MINUS);
}

#[test]
fn prefix_expr_not() {
    let prefix = parse_first::<PrefixExpr>("VAR x = not true\n");
    let op = prefix.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::KW_NOT);
}

#[test]
fn prefix_expr_bang() {
    let prefix = parse_first::<PrefixExpr>("VAR x = !true\n");
    let op = prefix.op_token().unwrap();
    assert_eq!(op.kind(), crate::SyntaxKind::BANG);
}

// ── FunctionCall ─────────────────────────────────────────────────────

#[test]
fn function_call_name_and_args() {
    let fc = parse_first::<FunctionCall>("=== k ===\n~ temp x = foo(1, 2)\n");
    assert_eq!(fc.name().as_deref(), Some("foo"));
    assert_eq!(fc.arg_list().unwrap().arg_count(), 2);
}

#[test]
fn function_call_no_args() {
    let fc = parse_first::<FunctionCall>("=== k ===\n~ temp x = bar()\n");
    assert_eq!(fc.name().as_deref(), Some("bar"));
    // Empty arg list: the parser may or may not emit an ARG_LIST node.
    // If present, count should be 0; if absent, that's also valid.
    if let Some(args) = fc.arg_list() {
        assert_eq!(args.arg_count(), 0);
    }
}

// ── InnerExpression ──────────────────────────────────────────────────

#[test]
fn inner_expression_path() {
    let tree = parse_tree("=== k ===\nHello {x}\n");
    let ie: InnerExpression = first(tree.syntax());
    let expr = ie.expr().unwrap();
    assert!(matches!(expr, Expr::Path(_)));
}

#[test]
fn inner_expression_infix() {
    let tree = parse_tree("=== k ===\nHello {x + 1}\n");
    let ie: InnerExpression = first(tree.syntax());
    let expr = ie.expr().unwrap();
    assert!(matches!(expr, Expr::Infix(_)));
}

// ── ChoiceCondition expr ─────────────────────────────────────────────

#[test]
fn choice_condition_expr_path() {
    let cond = parse_first::<ChoiceCondition>("* {visited} Choice\n");
    let expr = cond.expr().unwrap();
    assert!(matches!(expr, Expr::Path(_)));
}

#[test]
fn choice_condition_expr_infix() {
    let cond = parse_first::<ChoiceCondition>("* {x > 0} Choice\n");
    let expr = cond.expr().unwrap();
    assert!(matches!(expr, Expr::Infix(_)));
}
