mod cst;

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

// ── T1b superset: multi-line `~ { … }` blocks (docs/t1b-surface-spec.md §2) ──

#[test]
fn block_empty() {
    check("~ {\n}\n");
}

#[test]
fn block_temp_and_assignment() {
    check("~ {\ntemp total = 0\ntotal = total + 1\n}\n");
}

#[test]
fn block_if_else() {
    check("~ {\nif total > 3 {\nscore = total\n} else {\nscore = 0\n}\n}\n");
}

#[test]
fn block_if_elseif_else() {
    check("~ {\nif a {\nx = 1\n} else if b {\nx = 2\n} else {\nx = 3\n}\n}\n");
}

#[test]
fn block_while() {
    check("~ {\nwhile total > 3 {\ntotal = total - 1\n}\n}\n");
}

#[test]
fn block_for_in() {
    check("~ {\nfor item in list {\ntotal = total + item\n}\n}\n");
}

#[test]
fn block_break_continue() {
    check("~ {\nwhile true {\nbreak\ncontinue\n}\n}\n");
}

#[test]
fn block_return_bare() {
    check("~ {\nreturn\n}\n");
}

#[test]
fn block_return_with_value() {
    check("~ {\nreturn 5\n}\n");
}

#[test]
fn block_expr_stmt() {
    check("~ {\nfoo()\n}\n");
}

#[test]
fn block_indexed_assignment() {
    check("~ {\ngrid[y][x] = v\n}\n");
}

#[test]
fn block_array_literal_for_loop() {
    check("~ {\nfor item in #[1, 2, 3] {\ntotal = total + item\n}\n}\n");
}

/// `if`/`while`/`for`/`break`/`continue`/`in` are contextual keywords — they
/// must stay ordinary identifiers everywhere outside a `~ { … }` block.
#[test]
fn block_keywords_are_contextual_not_reserved() {
    check("~ if = 5\n");
    check("~ for = 1\n");
    check("~ while = 2\n");
    check("~ break = 3\n");
    check("~ continue = 4\n");
    check("~ in = 6\n");
}

#[test]
fn insta_block() {
    let p = parse("~ {\ntemp x = 0\nif x > 0 {\nx = x - 1\n}\n}\n");
    insta::assert_snapshot!(format!("{:#?}", p.syntax()));
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
