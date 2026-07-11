mod cst;

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

// ── T1b superset: sigil literals + indexing (docs/t1b-surface-spec.md §3-4) ──

#[test]
fn array_literal_empty() {
    check("~ x = #[]\n");
}

#[test]
fn array_literal_basic() {
    check("~ x = #[1, 2, 3]\n");
}

#[test]
fn array_literal_trailing_comma() {
    check("~ x = #[1, 2, 3,]\n");
}

#[test]
fn map_literal_empty() {
    check("~ x = #{}\n");
}

#[test]
fn map_literal_basic() {
    check("~ x = #{\"a\": 1, \"b\": 2}\n");
}

#[test]
fn map_literal_trailing_comma() {
    check("~ x = #{\"a\": 1,}\n");
}

#[test]
fn sigil_literal_nesting() {
    check("~ x = #[#{\"a\": 1}, #{\"a\": 2}]\n");
}

#[test]
fn index_basic() {
    check("~ x = a[0]\n");
}

#[test]
fn index_string_key() {
    check("~ x = m[\"k\"]\n");
}

#[test]
fn index_chained() {
    check("~ x = grid[y][x]\n");
}

#[test]
fn index_on_array_literal() {
    check("~ x = #[1, 2, 3][0]\n");
}

#[test]
fn indexed_assignment_basic() {
    check("~ a[0] = 5\n");
}

#[test]
fn indexed_assignment_chained() {
    check("~ grid[y][x] = v\n");
}

#[test]
fn insta_array_literal() {
    let p = parse("~ x = #[1, 2, 3]\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_map_literal() {
    let p = parse("~ x = #{\"a\": 1}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_index_chained() {
    let p = parse("~ grid[y][x] = v\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
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
