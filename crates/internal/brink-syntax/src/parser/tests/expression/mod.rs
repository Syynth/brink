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

// ── TM-4b structs (docs/typed-mode-spec.md §6) ────────────────────────

#[test]
fn struct_literal_empty() {
    check("~ x = Point#{}\n");
}

#[test]
fn struct_literal_basic() {
    check("~ p = Point#{x: 1.0, y: 2.0}\n");
}

#[test]
fn struct_literal_trailing_comma() {
    check("~ p = Point#{x: 1.0, y: 2.0,}\n");
}

#[test]
fn struct_literal_single_field() {
    check("~ w = Wrapper#{value: 1}\n");
}

/// Nesting: an array of struct literals (issue #665's own example).
#[test]
fn struct_literal_nested_in_array() {
    check("~ pts = #[Point#{x: 1.0, y: 2.0}, Point#{x: 3.0, y: 4.0}]\n");
}

/// A struct literal as a map value.
#[test]
fn struct_literal_nested_in_map() {
    check("~ m = #{\"a\": Point#{x: 1.0, y: 2.0}}\n");
}

#[test]
fn field_access_after_struct_literal() {
    check("~ x = Point#{x: 1.0, y: 2.0}.x\n");
}

#[test]
fn field_access_chained_after_struct_literal() {
    check("~ x = Point#{x: 1.0, y: 2.0}.x.y\n");
}

#[test]
fn field_access_after_index_expr() {
    check("~ x = pts[0].x\n");
}

#[test]
fn field_access_after_paren_expr() {
    check("~ x = (p).x\n");
}

/// A bare `ident.ident` chain is NOT the new `FIELD_ACCESS_EXPR` grammar —
/// it stays one `PATH` node (the existing dotted-identifier parse), same
/// CST shape as `knot.stitch`. The resolution-fallback disambiguation
/// (static path vs. field access) is a `brink-analyzer` concern.
#[test]
fn bare_dotted_identifier_is_still_a_path_not_field_access() {
    let p = parse("~ x = p.x\n");
    let text = format!("{:#?}", p.syntax());
    assert!(text.contains("PATH"), "{text}");
    assert!(!text.contains("FIELD_ACCESS_EXPR"), "{text}");
}

#[test]
fn insta_struct_literal() {
    let p = parse("~ p = Point#{x: 1.0, y: 2.0}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

#[test]
fn insta_field_access_after_struct_literal() {
    let p = parse("~ x = Point#{x: 1.0, y: 2.0}.x\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
}

// ── T1c function values (docs/t1c-spec.md §2) ─────────────────────────

#[test]
fn fn_literal_zero_args() {
    check("~ f = #fn(heal)\n");
}

#[test]
fn fn_literal_with_args() {
    check("~ f = #fn(heal, player_hp, 5)\n");
}

#[test]
fn fn_literal_trailing_comma() {
    check("~ f = #fn(heal, player_hp,)\n");
}

#[test]
fn fn_literal_dotted_target() {
    check("~ f = #fn(knot.helper, x)\n");
}

#[test]
fn fn_literal_nested_in_call_argument() {
    check("~ x = apply(#fn(heal, hp), 5)\n");
}

#[test]
fn fn_literal_arg_can_be_a_collection_literal() {
    check("~ f = #fn(heal, #[1, 2])\n");
}

/// `fn` stays a contextual keyword: an ordinary identifier named `fn` (or a
/// call to a function named `fn`) parses exactly as before.
#[test]
fn bare_fn_identifier_is_still_an_ordinary_ident() {
    check("~ x = fn\n");
    check("~ x = fn(1)\n");
}

/// Prose position is untouched: `#` still opens a tag there, so `#fn(...)`
/// mid-prose is tag text, not a function-value literal (the T1b sigil rule,
/// docs/t1b-surface-spec.md §3, carried over per t1c-spec §2 PROPOSED).
#[test]
fn prose_position_hash_fn_is_a_tag_not_a_fn_literal() {
    let p = parse("Hello #fn(heal)\n");
    let text = format!("{:#?}", p.syntax());
    assert!(text.contains("TAG"), "{text}");
    assert!(!text.contains("FN_LITERAL"), "{text}");
}

#[test]
fn insta_fn_literal() {
    let p = parse("~ f = #fn(heal, player_hp, 5)\n");
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
