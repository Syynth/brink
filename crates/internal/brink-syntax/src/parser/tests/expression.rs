use super::check;
use crate::parse;

#[test]
fn integer_literal() {
    check("~ x = 5\n");
}

#[test]
fn float_literal() {
    check("~ x = 3.14\n");
}

#[test]
fn boolean_literal() {
    check("~ x = true\n");
}

#[test]
fn string_literal() {
    check("~ x = \"hello\"\n");
}

#[test]
fn addition() {
    check("~ x = 1 + 2\n");
}

#[test]
fn complex_arithmetic() {
    check("~ x = 1 + 2 * 3\n");
}

#[test]
fn comparison() {
    check("~ x = a > 5\n");
}

#[test]
fn logical_and() {
    check("~ x = a && b\n");
}

#[test]
fn prefix_negate() {
    check("~ x = -1\n");
}

#[test]
fn prefix_not() {
    check("~ x = not true\n");
}

#[test]
fn postfix_increment() {
    check("~ x++\n");
}

#[test]
fn function_call() {
    check("~ x = foo(1, 2)\n");
}

#[test]
fn paren_expr() {
    check("~ x = (1 + 2) * 3\n");
}

#[test]
fn dotted_identifier() {
    check("~ x = knot.stitch\n");
}

#[test]
fn list_has() {
    check("~ x = items has sword\n");
}

#[test]
fn divert_target_expr() {
    check("~ x = -> knot\n");
}

#[test]
fn list_expression() {
    check("~ x = (a, b, c)\n");
}

#[test]
fn intersect_right_assoc() {
    check("~ x = 2 ^ 3 ^ 4\n");
}

#[test]
fn compound_assign() {
    check("~ x += 5\n");
}

#[test]
fn insta_complex_expr() {
    let p = parse("~ x = 1 + 2 * 3\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_function_call() {
    let p = parse("~ x = foo(1, 2)\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
