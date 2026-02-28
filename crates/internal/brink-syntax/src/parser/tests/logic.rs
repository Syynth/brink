use super::check;
use crate::parse;

#[test]
fn return_bare() {
    check("~ return\n");
}

#[test]
fn return_with_expr() {
    check("~ return 5\n");
}

#[test]
fn temp_declaration() {
    check("~ temp x = 5\n");
}

#[test]
fn assignment() {
    check("~ x = 10\n");
}

#[test]
fn compound_assign_plus() {
    check("~ x += 1\n");
}

#[test]
fn compound_assign_minus() {
    check("~ x -= 1\n");
}

#[test]
fn bare_expression() {
    check("~ foo()\n");
}

#[test]
fn bare_increment() {
    check("~ x++\n");
}

#[test]
fn insta_temp_decl() {
    let p = parse("~ temp x = 5\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_return_with_expr() {
    let p = parse("~ return x + 1\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}
