//! Expressions & precedence. Family for #1193.
//!
//! Parity target studied: `brink-syntax/src/parser/tests/expression/{mod,cst}.rs`
//! (71 tests). This grammar is deliberately a *skeleton* (module doc on
//! `parser/expr.rs`): no array/fn-value sigil literals, no ranges, no
//! indexing, no field access, no postfix `++`/`--` — those don't exist as
//! `SyntaxKind`s here yet (checked against `syntax_kind.rs`). So this file
//! mirrors the parity target's *structure and depth* for the forms that DO
//! exist: literals, paths, prefix/infix expressions, parenthesization,
//! `CALL_EXPR`/`ARG_LIST`, `LAMBDA_EXPR`/`LAMBDA_PARAMS` (shape only here;
//! their lowering is tested in `brink-ir/tests/native_lambdas.rs`),
//! and — since B5 (issue #1464) — the one construction-initializer grammar
//! `TypeName { … }` (`CONSTRUCT_LITERAL`/`CONSTRUCT_ENTRY`), which is how
//! maps and struct construction are spelled on the native surface (there is
//! no `#{…}`/`Name#{…}` sigil here; that is the brink dialect's spelling).
//! `parser/expr.rs`): no fn-value sigil literals, no ranges, no indexing, no
//! field access, no postfix `++`/`--` — those don't exist as `SyntaxKind`s
//! here yet (checked against `syntax_kind.rs`). So this file mirrors the
//! parity target's *structure and depth* for the forms that DO exist:
//! literals, paths, prefix/infix expressions, parenthesization,
//! `CALL_EXPR`/`ARG_LIST`, `LAMBDA_EXPR`/`LAMBDA_PARAMS` (tokenized only —
//! lowering is B0.8, per the node's own doc comment in `syntax_kind.rs`),
//! the one construction-initializer grammar `TypeName { … }` (B5, issue
//! #1464 — `CONSTRUCT_LITERAL`/`CONSTRUCT_ENTRY`, how maps and struct
//! construction are spelled on the native surface; there is no
//! `#{…}`/`Name#{…}` sigil here, that is the brink dialect's spelling), and
//! — since NG-D (issue #1490) — the array/sequence literal `[1, 2, 3]`
//! (`ARRAY_LITERAL`), the everyday collection literal's own lightest
//! spelling (no `#[…]` sigil on the native surface either).
//!
//! Entry point: every case below goes through `var name = <expr>` (or, for
//! the accessor tests, `const`), since that's the shortest reachable path
//! to `expr::expression` from `source_file` (`decl.rs::var_decl`).

use super::*;

// ── A. Literals ─────────────────────────────────────────────────────

#[test]
fn integer_literal() {
    let p = assert_lossless("var x = 5\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTEGER_LIT));
}

#[test]
fn float_literal() {
    let p = assert_lossless("var x = 3.14\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::FLOAT_LIT));
}

#[test]
fn boolean_literal_true() {
    let p = assert_lossless("var x = true\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::BOOLEAN_LIT));
}

#[test]
fn boolean_literal_false() {
    let p = assert_lossless("var x = false\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::BOOLEAN_LIT));
}

#[test]
fn string_literal() {
    let p = assert_lossless("var x = \"hello\"\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRING_LIT));
}

#[test]
fn string_literal_empty() {
    let p = assert_lossless("var x = \"\"\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRING_LIT));
}

/// `STRING_LIT` reuses `content::interpolation` for `{expr}` runs inside a
/// quoted string (the same `INTERPOLATION` node prose content uses, per
/// `expr::string_lit`'s doc comment) — a shape test at the boundary, not a
/// duplication of `content.rs`'s own interpolation-family coverage.
#[test]
fn string_literal_with_interpolation_nests_an_expression() {
    let p = assert_lossless("var x = \"hi {name}\"\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STRING_LIT));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTERPOLATION));
    let string_lit = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::STRING_LIT)
        .expect("STRING_LIT");
    assert!(has_node_kind(&string_lit, SyntaxKind::PATH_EXPR));
}

// ── B. Paths ─────────────────────────────────────────────────────────

#[test]
fn path_single_segment() {
    let p = assert_lossless("var x = y\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PATH_EXPR));
    // `var`'s own LHS name is a bare `IDENT` token (decl.rs::var_decl), not
    // a `PATH_SEGMENT` — only the RHS initializer `y` goes through the
    // expression grammar's `path()`.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::PATH_SEGMENT), 1);
}

#[test]
fn path_two_segments_dot() {
    let p = assert_lossless("var x = a.b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let path_expr = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PATH_EXPR)
        .expect("PATH_EXPR");
    let path: ast::Path = find_child(&path_expr).expect("PATH");
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segs, vec!["a".to_string(), "b".to_string()]);
    assert!(!path.crosses_module_wall());
}

#[test]
fn path_three_segments_dot() {
    let p = assert_lossless("var x = a.b.c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let path_expr = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PATH_EXPR)
        .expect("PATH_EXPR");
    let path: ast::Path = find_child(&path_expr).expect("PATH");
    assert_eq!(path.segments().count(), 3);
}

#[test]
fn path_double_colon_crosses_module_wall() {
    let p = assert_lossless("var x = a::b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let path_expr = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PATH_EXPR)
        .expect("PATH_EXPR");
    let path: ast::Path = find_child(&path_expr).expect("PATH");
    assert!(path.crosses_module_wall());
}

#[test]
fn path_mixed_dot_and_double_colon() {
    let p = assert_lossless("var x = a::b.c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let path_expr = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PATH_EXPR)
        .expect("PATH_EXPR");
    let path: ast::Path = find_child(&path_expr).expect("PATH");
    let segs: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(
        segs,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert!(path.crosses_module_wall());
}

// ── C. Prefix expressions ───────────────────────────────────────────

#[test]
fn prefix_negate_integer() {
    let p = assert_lossless("var x = -1\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR));
}

#[test]
fn prefix_bang_path() {
    let p = assert_lossless("var x = !flag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR));
}

#[test]
fn prefix_negate_paren() {
    let p = assert_lossless("var x = -(a + b)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let prefix = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PREFIX_EXPR)
        .expect("PREFIX_EXPR");
    assert!(has_node_kind(&prefix, SyntaxKind::PAREN_EXPR));
}

/// `--x` — two adjacent `MINUS` tokens. There is no compound `--` token in
/// this lexer's punctuation set (unlike `brink-syntax`'s postfix `--`,
/// which doesn't exist here at all — `syntax_kind.rs` has no
/// `POSTFIX_EXPR`), so this is prefix-negate applied twice: `-(-x)`.
#[test]
fn prefix_double_negate_is_nested_prefix() {
    let p = assert_lossless("var x = --y\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR), 2);
    let outer: ast::PrefixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::PrefixExpr::cast)
        .expect("outer PREFIX_EXPR");
    let operand = outer.operand().expect("operand");
    assert_eq!(
        operand.kind(),
        SyntaxKind::PREFIX_EXPR,
        "outer's operand is the inner prefix"
    );
}

#[test]
fn prefix_bang_bang_is_nested_prefix() {
    let p = assert_lossless("var x = !!flag\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::PREFIX_EXPR), 2);
}

// ── D. Infix — one test per operator ────────────────────────────────

fn infix_op_test(src: &str, op: SyntaxKind) {
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
    let infix: ast::InfixExpr = p
        .syntax()
        .descendants()
        .find_map(ast::InfixExpr::cast)
        .expect("INFIX_EXPR should be present");
    assert_eq!(
        infix.op_token().map(|t| t.kind()),
        Some(op),
        "{src:?}: unexpected operator token"
    );
    assert!(infix.lhs().is_some(), "{src:?}: missing lhs");
    assert!(infix.rhs().is_some(), "{src:?}: missing rhs");
}

#[test]
fn infix_plus() {
    infix_op_test("var x = a + b\n", SyntaxKind::PLUS);
}

#[test]
fn infix_minus() {
    infix_op_test("var x = a - b\n", SyntaxKind::MINUS);
}

#[test]
fn infix_star() {
    infix_op_test("var x = a * b\n", SyntaxKind::STAR);
}

#[test]
fn infix_slash() {
    infix_op_test("var x = a / b\n", SyntaxKind::SLASH);
}

#[test]
fn infix_percent() {
    infix_op_test("var x = a % b\n", SyntaxKind::PERCENT);
}

#[test]
fn infix_lt() {
    infix_op_test("var x = a < b\n", SyntaxKind::LT);
}

#[test]
fn infix_gt() {
    infix_op_test("var x = a > b\n", SyntaxKind::GT);
}

#[test]
fn infix_lte() {
    infix_op_test("var x = a <= b\n", SyntaxKind::LT_EQ);
}

#[test]
fn infix_gte() {
    infix_op_test("var x = a >= b\n", SyntaxKind::GT_EQ);
}

#[test]
fn infix_eq_eq() {
    infix_op_test("var x = a == b\n", SyntaxKind::EQ_EQ);
}

#[test]
fn infix_bang_eq() {
    infix_op_test("var x = a != b\n", SyntaxKind::BANG_EQ);
}

#[test]
fn infix_amp_amp() {
    infix_op_test("var x = a && b\n", SyntaxKind::AMP_AMP);
}

/// `||` is two adjacent `PIPE` tokens, not one compound lexer token (module
/// doc on `expr::expression_bp`) — `op_token()` returns the first, and
/// `is_double_pipe()` disambiguates it from a bare single `|` (which can't
/// actually appear as an infix operator here, but the accessor still needs
/// covering).
#[test]
fn infix_pipe_pipe() {
    let p = assert_lossless("var x = a || b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let infix: ast::InfixExpr = p
        .syntax()
        .descendants()
        .find_map(ast::InfixExpr::cast)
        .expect("INFIX_EXPR");
    assert_eq!(infix.op_token().map(|t| t.kind()), Some(SyntaxKind::PIPE));
    assert!(infix.is_double_pipe());
}

/// Regression precedent for the `brink-syntax` sibling: `||` must skip
/// whitespace before bumping the two `PIPE` tokens, else it would swallow a
/// trivia token instead of the second `|` and double-wrap a parenthesized
/// RHS. `expr.rs`'s `||` branch does call `p.skip_ws()` up front — this
/// pins that behavior against regression.
#[test]
fn infix_pipe_pipe_paren_rhs_does_not_double_wrap() {
    let p = assert_lossless("var x = 0 || (0)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let infix: ast::InfixExpr = p
        .syntax()
        .descendants()
        .find_map(ast::InfixExpr::cast)
        .expect("INFIX_EXPR");
    let rhs = infix.rhs().expect("rhs");
    assert_eq!(rhs.kind(), SyntaxKind::PAREN_EXPR);
    // Exactly one PAREN_EXPR — a double-wrap bug would nest a second one.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR), 1);
}

// ── E. Precedence and associativity (Coalesce < Or < And < Eq < Cmp < Add < Mul) ──

/// `a or b == c` → `or` outer, `==` inner RHS (B1, `docs/stdlib-spec.md`
/// §1.6a, issue #1460: `Prec::Coalesce` sits looser than every other
/// operator, so an equality comparison on the fallback side stays nested
/// under `or`, never the other way around — `a or (b == c)`).
#[test]
fn prec_coalesce_over_eq() {
    let p = assert_lossless("var x = a or b == c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::KW_OR));
    assert_eq!(
        outer.lhs().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR),
        "lhs should be the bare `a`"
    );
    let rhs = outer.rhs().expect("rhs");
    assert_eq!(
        rhs.kind(),
        SyntaxKind::INFIX_EXPR,
        "`b == c` should nest under `or` as its RHS — see the section doc above"
    );
    let inner = ast::InfixExpr::cast(rhs).expect("inner INFIX_EXPR");
    assert_eq!(inner.op_token().map(|t| t.kind()), Some(SyntaxKind::EQ_EQ));
}

/// `a || b or c` → `or` outer, `||` inner LHS (`or` is looser than `||`
/// too, not just the operators between them — the whole `a || b` disjunction
/// becomes the coalescing left-hand side).
#[test]
fn prec_coalesce_over_double_pipe() {
    let p = assert_lossless("var x = a || b or c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::KW_OR));
    let lhs = outer.lhs().expect("lhs");
    assert_eq!(
        lhs.kind(),
        SyntaxKind::INFIX_EXPR,
        "`a || b` should nest under `or` as its LHS — see the section doc above"
    );
    let inner = ast::InfixExpr::cast(lhs).expect("inner INFIX_EXPR");
    assert!(inner.is_double_pipe());
    assert_eq!(
        outer.rhs().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR),
        "rhs should be the bare `c`"
    );
}

/// `a or b or c` → left-nested (`(a or b) or c`), same left-associativity
/// fix section F documents for every other symmetric-precedence operator —
/// `or` shares it, and it is also the ruled coalescing associativity
/// (`infer::ty::coalesce`'s doc: left-associative chaining).
#[test]
fn prec_coalesce_chain_is_left_associative() {
    let p = assert_lossless("var x = a or b or c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::KW_OR));
    let lhs = outer.lhs().expect("lhs");
    assert_eq!(
        lhs.kind(),
        SyntaxKind::INFIX_EXPR,
        "`a or b or c` should parse left-associative as `(a or b) or c` \
         (INFIX_EXPR on the LHS)"
    );
    assert_eq!(
        outer.rhs().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR),
        "rhs should be the bare `c` under left-associative parsing"
    );
}

/// `1 + 2 * 3` → `+` outer, `*` inner right (mul binds tighter than add).
#[test]
fn prec_mul_over_add() {
    let p = assert_lossless("var x = 1 + 2 * 3\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::PLUS));
    let rhs = outer.rhs().expect("rhs");
    assert_eq!(rhs.kind(), SyntaxKind::INFIX_EXPR);
    let inner = ast::InfixExpr::cast(rhs).expect("inner INFIX_EXPR");
    assert_eq!(inner.op_token().map(|t| t.kind()), Some(SyntaxKind::STAR));
}

/// `1 * 2 + 3` → `+` outer, `*` inner left.
#[test]
fn prec_mul_then_add() {
    let p = assert_lossless("var x = 1 * 2 + 3\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::PLUS));
    let lhs = outer.lhs().expect("lhs");
    assert_eq!(lhs.kind(), SyntaxKind::INFIX_EXPR);
}

/// `a && b || c` → `||` outer, `&&` inner left (And binds tighter than Or).
#[test]
fn prec_and_over_or() {
    let p = assert_lossless("var x = a && b || c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert!(outer.is_double_pipe());
    let lhs = outer.lhs().expect("lhs");
    assert_eq!(lhs.kind(), SyntaxKind::INFIX_EXPR);
    let inner = ast::InfixExpr::cast(lhs).expect("inner INFIX_EXPR");
    assert_eq!(
        inner.op_token().map(|t| t.kind()),
        Some(SyntaxKind::AMP_AMP)
    );
}

/// `a == b && c` → `&&` outer, `==` inner left (Eq binds tighter than And).
#[test]
fn prec_eq_over_and() {
    let p = assert_lossless("var x = a == b && c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(
        outer.op_token().map(|t| t.kind()),
        Some(SyntaxKind::AMP_AMP)
    );
    let lhs = outer.lhs().expect("lhs");
    let inner = ast::InfixExpr::cast(lhs).expect("inner INFIX_EXPR");
    assert_eq!(inner.op_token().map(|t| t.kind()), Some(SyntaxKind::EQ_EQ));
}

/// `a < b == c` → `==` outer, `<` inner left (Cmp binds tighter than Eq).
#[test]
fn prec_cmp_over_eq() {
    let p = assert_lossless("var x = a < b == c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::EQ_EQ));
    let lhs = outer.lhs().expect("lhs");
    let inner = ast::InfixExpr::cast(lhs).expect("inner INFIX_EXPR");
    assert_eq!(inner.op_token().map(|t| t.kind()), Some(SyntaxKind::LT));
}

/// `a + b < c` → `<` outer, `+` inner left (Add binds tighter than Cmp).
#[test]
fn prec_add_over_cmp() {
    let p = assert_lossless("var x = a + b < c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::LT));
    let lhs = outer.lhs().expect("lhs");
    let inner = ast::InfixExpr::cast(lhs).expect("inner INFIX_EXPR");
    assert_eq!(inner.op_token().map(|t| t.kind()), Some(SyntaxKind::PLUS));
}

/// `1 + 2 * 3 > 4` → three-level nesting: `>` outer, `+` middle, `*` inner.
#[test]
fn mixed_precedence_three_levels() {
    let p = assert_lossless("var x = 1 + 2 * 3 > 4\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let gt: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(gt.op_token().map(|t| t.kind()), Some(SyntaxKind::GT));
    let plus = ast::InfixExpr::cast(gt.lhs().expect("lhs")).expect("+ node");
    assert_eq!(plus.op_token().map(|t| t.kind()), Some(SyntaxKind::PLUS));
    let star = ast::InfixExpr::cast(plus.rhs().expect("rhs")).expect("* node");
    assert_eq!(star.op_token().map(|t| t.kind()), Some(SyntaxKind::STAR));
}

/// `-a + b` → `INFIX_EXPR { PREFIX_EXPR { PATH_EXPR }, PATH_EXPR }`: prefix
/// binds tighter than every infix level (`Prec::Prefix` is the highest).
#[test]
fn prefix_binds_tighter_than_infix() {
    let p = assert_lossless("var x = -a + b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let plus: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(plus.op_token().map(|t| t.kind()), Some(SyntaxKind::PLUS));
    let lhs = plus.lhs().expect("lhs");
    assert_eq!(lhs.kind(), SyntaxKind::PREFIX_EXPR);
}

// ── F. Symmetric-precedence operators are left-associative (#1251) ────
//
// `expr::expression_bp`'s recursive call for an infix RHS used to be
// `expression_bp(p, prec)` — reusing the JUST-CONSUMED operator's OWN
// precedence as the child's `min_bp`. Combined with the loop's strict
// `<` break check, a second operator at the SAME precedence didn't stop
// that recursive call — it got pulled into the child instead of being
// left for the parent's own loop. Net effect: every symmetric-precedence
// operator chain in this grammar (`-`, `/`, `%`, `<`, `>`, `<=`, `>=`,
// `==`, `!=`, `&&`, `||`, `or`) parsed RIGHT-associative, not left-associative.
// For `+`/`*` this was unobservable (they're mathematically associative),
// but for `-` and `/` it silently changed the computed VALUE: `10 - 3 - 2`
// grouped as `10 - (3 - 2)` (= 9 if evaluated naively), not
// `(10 - 3) - 2` (= 5).
//
// Fixed by recursing with `min_bp = prec.next()` (`prec + 1`, saturating
// at `Prec::Prefix`) — only strictly-higher-precedence operators get
// pulled into the RHS; a same-precedence operator now falls through to
// the parent's own loop, producing left-associative nesting.

#[test]
fn minus_chain_is_left_associative() {
    let p = assert_lossless("var x = a - b - c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(outer.op_token().map(|t| t.kind()), Some(SyntaxKind::MINUS));
    // Left-assoc shape puts the nested INFIX_EXPR on the LHS: `(a - b) - c`.
    let lhs = outer.lhs().expect("lhs");
    assert_eq!(
        lhs.kind(),
        SyntaxKind::INFIX_EXPR,
        "`a - b - c` should parse left-associative as `(a - b) - c` \
         (INFIX_EXPR on the LHS) — see the section doc above"
    );
    assert_eq!(
        outer.rhs().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR),
        "rhs should be the bare `c` under left-associative parsing"
    );
}

#[test]
fn slash_chain_is_left_associative() {
    let p = assert_lossless("var x = a / b / c\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let outer: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    let lhs = outer.lhs().expect("lhs");
    assert_eq!(
        lhs.kind(),
        SyntaxKind::INFIX_EXPR,
        "same left-associativity fix as `-`, see `minus_chain_is_left_associative`"
    );
}

// ── G. Parenthesized expressions ────────────────────────────────────

#[test]
fn paren_simple() {
    let p = assert_lossless("var x = (1 + 2)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let paren: ast::ParenExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::ParenExpr::cast)
        .expect("PAREN_EXPR");
    let inner = paren.inner().expect("inner");
    assert_eq!(inner.kind(), SyntaxKind::INFIX_EXPR);
}

#[test]
fn paren_nested() {
    let p = assert_lossless("var x = ((a))\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR), 2);
}

#[test]
fn paren_overrides_precedence() {
    let p = assert_lossless("var x = (1 + 2) * 3\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let star: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(star.op_token().map(|t| t.kind()), Some(SyntaxKind::STAR));
    let lhs = star.lhs().expect("lhs");
    assert_eq!(lhs.kind(), SyntaxKind::PAREN_EXPR);
}

// ── H. Function calls (CALL_EXPR / ARG_LIST) ────────────────────────

#[test]
fn call_zero_args() {
    let p = assert_lossless("var x = foo()\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call: ast::CallExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::CallExpr::cast)
        .expect("CALL_EXPR");
    let callee = call.callee().expect("callee");
    assert_eq!(
        callee
            .segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["foo".to_string()]
    );
    let args = call.arg_list().expect("arg list");
    assert!(args.is_open());
    assert_eq!(args.syntax().children().count(), 0);
}

#[test]
fn call_one_arg() {
    let p = assert_lossless("var x = foo(1)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call: ast::CallExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::CallExpr::cast)
        .expect("CALL_EXPR");
    let args = call.arg_list().expect("arg list");
    assert_eq!(args.syntax().children().count(), 1);
}

#[test]
fn call_many_args() {
    let p = assert_lossless("var x = foo(1, 2, 3)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call: ast::CallExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::CallExpr::cast)
        .expect("CALL_EXPR");
    let args = call.arg_list().expect("arg list");
    assert_eq!(args.syntax().children().count(), 3);
}

#[test]
fn call_trailing_comma() {
    let p = assert_lossless("var x = foo(1, 2,)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call: ast::CallExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::CallExpr::cast)
        .expect("CALL_EXPR");
    let args = call.arg_list().expect("arg list");
    assert_eq!(args.syntax().children().count(), 2);
}

#[test]
fn call_arg_is_an_expression() {
    let p = assert_lossless("var x = foo(1 + 2)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call: ast::CallExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::CallExpr::cast)
        .expect("CALL_EXPR");
    let args = call.arg_list().expect("arg list");
    let first = args.syntax().children().next().expect("first arg");
    assert_eq!(first.kind(), SyntaxKind::INFIX_EXPR);
}

#[test]
fn call_nested() {
    let p = assert_lossless("var x = foo(bar(y))\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CALL_EXPR), 2);
}

#[test]
fn call_dotted_callee() {
    let p = assert_lossless("var x = a.b.c(1)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let call: ast::CallExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::CallExpr::cast)
        .expect("CALL_EXPR");
    let callee = call.callee().expect("callee");
    assert_eq!(callee.segments().count(), 3);
}

#[test]
fn call_as_infix_operand() {
    let p = assert_lossless("var x = foo(1) + bar(2)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let plus: ast::InfixExpr = find_child(&p.syntax())
        .and_then(|vd: ast::VarDecl| vd.value())
        .and_then(ast::InfixExpr::cast)
        .expect("outer INFIX_EXPR");
    assert_eq!(plus.lhs().map(|n| n.kind()), Some(SyntaxKind::CALL_EXPR));
    assert_eq!(plus.rhs().map(|n| n.kind()), Some(SyntaxKind::CALL_EXPR));
}

/// `a.b` (no trailing `(`) is `PATH_EXPR`, never `CALL_EXPR` — the two
/// forms share `path_or_call`'s checkpoint and only diverge on lookahead.
#[test]
fn dotted_without_call_is_path_expr_not_call_expr() {
    let p = assert_lossless("var x = a.b\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PATH_EXPR));
    let path_expr = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PATH_EXPR)
        .expect("PATH_EXPR");
    assert!(!has_node_kind(&path_expr, SyntaxKind::CALL_EXPR));
}

// ── I. Lambda expressions ───────────────────────────────────────────
//
// These are parse-only *shape* tests: the semantic coverage for lambdas
// lives with the lowering that consumes these nodes
// (`brink-ir/tests/native_lambdas.rs`, issue #1685).

/// The declared names of a `LAMBDA_PARAMS` node's parameters, in source
/// order. Each one is a `PARAM` node (the same shape `fn`/`flow` headers
/// use) since NG-A gave lambda parameters optional `: type` annotations —
/// before that they were bare `IDENT` tokens directly under
/// `LAMBDA_PARAMS`.
fn lambda_param_names(params: &SyntaxNode) -> Vec<String> {
    params
        .children()
        .filter_map(ast::Param::cast)
        .filter_map(|p| p.name_token())
        .map(|t| t.text().to_string())
        .collect()
}

#[test]
fn lambda_pipe_tokenizes_and_parses() {
    let src = "var f = |x, y| x + y\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn lambda_zero_params() {
    let p = assert_lossless("var f = || 1\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
        .expect("LAMBDA_EXPR");
    let params = lambda
        .children()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_PARAMS)
        .expect("LAMBDA_PARAMS");
    assert_eq!(count_node_kind(&params, SyntaxKind::PATH), 0);
}

#[test]
fn lambda_one_param() {
    let p = assert_lossless("var f = |x| x\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
        .expect("LAMBDA_EXPR");
    let params = lambda
        .children()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_PARAMS)
        .expect("LAMBDA_PARAMS");
    let idents = lambda_param_names(&params);
    assert_eq!(idents, vec!["x".to_string()]);
}

#[test]
fn lambda_multiple_params() {
    let p = assert_lossless("var f = |x, y, z| x\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
        .expect("LAMBDA_EXPR");
    let params = lambda
        .children()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_PARAMS)
        .expect("LAMBDA_PARAMS");
    let idents = lambda_param_names(&params);
    assert_eq!(
        idents,
        vec!["x".to_string(), "y".to_string(), "z".to_string()]
    );
}

#[test]
fn lambda_params_trailing_comma() {
    let p = assert_lossless("var f = |x, y,| x\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
        .expect("LAMBDA_EXPR");
    let params = lambda
        .children()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_PARAMS)
        .expect("LAMBDA_PARAMS");
    let idents = lambda_param_names(&params);
    assert_eq!(idents, vec!["x".to_string(), "y".to_string()]);
}

#[test]
fn lambda_body_is_a_full_expression() {
    let p = assert_lossless("var f = |x| x + 1 * 2\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
        .expect("LAMBDA_EXPR");
    assert!(has_node_kind(&lambda, SyntaxKind::INFIX_EXPR));
}

#[test]
fn lambda_nested_in_call_argument() {
    let p = assert_lossless("var x = apply(|n| n + 1, 5)\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LAMBDA_EXPR));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CALL_EXPR));
}

// ── I-bis. Lambda type annotations (NG-A, issue #1487) ───────────────
//
// GRAMMAR ONLY here — the annotations' *lowering* (into `Param.annotation`
// and `LambdaExpr.return_type`) is covered by
// `brink-ir/tests/native_lambdas.rs` (issue #1685); these are parse-shape
// tests, exactly like every other lambda test above.

fn lambda_of(p: &Parse) -> ast::LambdaExpr {
    p.syntax()
        .descendants()
        .find_map(ast::LambdaExpr::cast)
        .expect("LAMBDA_EXPR")
}

fn lambda_params_of(lambda: &ast::LambdaExpr) -> SyntaxNode {
    lambda
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::LAMBDA_PARAMS)
        .expect("LAMBDA_PARAMS")
}

#[test]
fn lambda_param_takes_a_type_annotation() {
    let p = assert_lossless("var f = |g: Guest| g\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = lambda_of(&p);
    let params = lambda_params_of(&lambda);
    assert_eq!(lambda_param_names(&params), vec!["g".to_string()]);
    let param = params.children().find_map(ast::Param::cast).expect("PARAM");
    let te = param
        .type_annotation()
        .expect("annotation")
        .type_expr()
        .expect("type expr");
    let Some(ast::TypeExprKind::Name(n)) = te.kind() else {
        unreachable!("expected a nominal type, tree: {:#?}", te.syntax())
    };
    assert_eq!(n.name(), Some("Guest".to_string()));
}

#[test]
fn lambda_takes_a_colon_return_annotation_before_a_braced_body() {
    // The ratified surface (2026-07-23): `|g: Guest|: bool { … }`. `bool`
    // is the *return type*, and the brace opens the body — it must NOT be
    // read as a `bool { … }` construction literal.
    let p = assert_lossless("var f = |g: Guest|: bool { g }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = lambda_of(&p);
    let annotation = lambda
        .syntax()
        .children()
        .find_map(ast::TypeAnnotation::cast)
        .expect("the lambda's own `: bool` return annotation");
    let te = annotation.type_expr().expect("type expr");
    let Some(ast::TypeExprKind::Name(n)) = te.kind() else {
        unreachable!("expected a nominal type, tree: {:#?}", te.syntax())
    };
    assert_eq!(n.name(), Some("bool".to_string()));
    assert!(
        !has_node_kind(lambda.syntax(), SyntaxKind::CONSTRUCT_LITERAL),
        "the body brace must not read as a construction literal, tree: {:#?}",
        lambda.syntax()
    );
    assert!(has_node_kind(lambda.syntax(), SyntaxKind::STMT_BLOCK));
}

#[test]
fn zero_arg_lambda_takes_a_return_annotation() {
    let p = assert_lossless("var f = ||: int { 1 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = lambda_of(&p);
    assert!(lambda_param_names(&lambda_params_of(&lambda)).is_empty());
    assert!(
        lambda
            .syntax()
            .children()
            .any(|n| n.kind() == SyntaxKind::TYPE_ANNOTATION)
    );
}

#[test]
fn unannotated_lambda_has_no_return_annotation() {
    let p = assert_lossless("var f = |x| x\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = lambda_of(&p);
    assert!(
        !lambda
            .syntax()
            .children()
            .any(|n| n.kind() == SyntaxKind::TYPE_ANNOTATION)
    );
}

#[test]
fn a_lambda_key_in_a_construction_entry_keeps_the_entry_colon() {
    // The one adjacency worth pinning: a `:` that follows a lambda *body*
    // belongs to the enclosing construction entry, not to the lambda —
    // only a `:` immediately after the closing `|` is a return annotation.
    let p = assert_lossless("var m = Map { \"k\": |x| x }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lambda = lambda_of(&p);
    assert!(
        !lambda
            .syntax()
            .children()
            .any(|n| n.kind() == SyntaxKind::TYPE_ANNOTATION)
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_ENTRY));
}

// ── J. Typed AST accessors (arithmetic/call accessor coverage — the ──
// ── B0.6 review's "zero coverage" finding this issue was seeded on)   ──

#[test]
fn integer_lit_value_accessor() {
    let p = parse("var x = 42\n");
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    let lit = ast::IntegerLit::cast(value).expect("INTEGER_LIT");
    assert_eq!(lit.value(), Some(42));
}

#[test]
fn float_lit_value_accessor() {
    let p = parse("var x = 3.5\n");
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    let lit = ast::FloatLit::cast(value).expect("FLOAT_LIT");
    assert!((lit.value().expect("float value") - 3.5).abs() < f64::EPSILON);
}

#[test]
fn boolean_lit_value_accessor_true_and_false() {
    for (src, expected) in [("var x = true\n", true), ("var x = false\n", false)] {
        let p = parse(src);
        let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
        let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
        let value = var_decl.value().expect("initializer node");
        let lit = ast::BooleanLit::cast(value).expect("BOOLEAN_LIT");
        assert_eq!(lit.value(), Some(expected), "{src:?}");
    }
}

#[test]
fn prefix_expr_op_token_and_operand_accessors() {
    let p = parse("var x = -a\n");
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    let prefix = ast::PrefixExpr::cast(value).expect("PREFIX_EXPR");
    assert_eq!(prefix.op_token().map(|t| t.kind()), Some(SyntaxKind::MINUS));
    assert_eq!(
        prefix.operand().map(|n| n.kind()),
        Some(SyntaxKind::PATH_EXPR)
    );
}

#[test]
fn call_expr_callee_and_arg_list_accessors_multi_arg() {
    // Mirrors `ast::tests::call_expr_callee_resolves_the_path_not_a_path_expr`
    // (which only covers a 2-arg call's `is_open()`) — this exercises the
    // arg-list SHAPE (arg count, arg kinds) that finding didn't touch.
    let p = parse("const x = compute(a, 1 + 2, foo())\n");
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let const_decl: ast::ConstDecl = find_child(file.syntax()).expect("const decl");
    let value = const_decl.value().expect("initializer node");
    let call = ast::CallExpr::cast(value).expect("CALL_EXPR");
    let callee = call.callee().expect("callee");
    assert_eq!(
        callee
            .segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["compute".to_string()]
    );
    let args = call.arg_list().expect("arg list");
    let kinds: Vec<_> = args.syntax().children().map(|n| n.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::PATH_EXPR,
            SyntaxKind::INFIX_EXPR,
            SyntaxKind::CALL_EXPR,
        ]
    );
}

#[test]
fn paren_expr_inner_accessor() {
    let p = parse("var x = (a)\n");
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    let paren = ast::ParenExpr::cast(value).expect("PAREN_EXPR");
    assert_eq!(paren.inner().map(|n| n.kind()), Some(SyntaxKind::PATH_EXPR));
}

#[test]
fn path_expr_path_accessor() {
    let p = parse("var x = knot.stitch\n");
    let file = ast::SourceFile::cast(p.syntax()).expect("SOURCE_FILE");
    let var_decl: ast::VarDecl = find_child(file.syntax()).expect("var decl");
    let value = var_decl.value().expect("initializer node");
    let path_expr = ast::PathExpr::cast(value).expect("PATH_EXPR");
    let path = path_expr.path().expect("path");
    assert_eq!(
        path.segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["knot".to_string(), "stitch".to_string()]
    );
}

// ── K. Structural invariants ────────────────────────────────────────

fn assert_infix_has_two_node_children(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::INFIX_EXPR {
            let child_count = node.children().count();
            assert_eq!(
                child_count, 2,
                "INFIX_EXPR should have exactly 2 node children, found {child_count} in `{src}`"
            );
        }
    }
}

fn assert_prefix_has_one_node_child(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::PREFIX_EXPR {
            let child_count = node.children().count();
            assert_eq!(
                child_count, 1,
                "PREFIX_EXPR should have exactly 1 node child, found {child_count} in `{src}`"
            );
        }
    }
}

#[test]
fn invariant_infix_simple() {
    assert_infix_has_two_node_children("var x = a + b\n");
}

#[test]
fn invariant_infix_chained_precedence() {
    assert_infix_has_two_node_children("var x = 1 + 2 * 3\n");
}

#[test]
fn invariant_infix_comparison() {
    assert_infix_has_two_node_children("var x = a > 5\n");
}

#[test]
fn invariant_infix_double_pipe() {
    assert_infix_has_two_node_children("var x = a || b\n");
}

#[test]
fn invariant_prefix_negate() {
    assert_prefix_has_one_node_child("var x = -1\n");
}

#[test]
fn invariant_prefix_bang() {
    assert_prefix_has_one_node_child("var x = !flag\n");
}

#[test]
fn invariant_call_expr_first_child_is_path() {
    for src in [
        "var x = foo()\n",
        "var x = foo(1, 2)\n",
        "var x = foo(bar(y))\n",
    ] {
        let p = parse(src);
        assert!(p.errors().is_empty(), "{src:?} errors: {:?}", p.errors());
        for node in p.syntax().descendants() {
            if node.kind() == SyntaxKind::CALL_EXPR {
                let first_child = node
                    .children()
                    .next()
                    .expect("CALL_EXPR should have at least one child");
                assert_eq!(
                    first_child.kind(),
                    SyntaxKind::PATH,
                    "CALL_EXPR first child should be PATH in `{src}`"
                );
            }
        }
    }
}

// ── L. Positive/negative assertions ─────────────────────────────────

#[test]
fn integer_literal_not_float_literal() {
    let p = parse("var x = 5\n");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::INTEGER_LIT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::FLOAT_LIT));
}

#[test]
fn float_literal_not_integer_literal() {
    let p = parse("var x = 5.0\n");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::FLOAT_LIT));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::INTEGER_LIT));
}

#[test]
fn call_not_paren() {
    let p = parse("var x = foo(y)\n");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CALL_EXPR));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR));
}

#[test]
fn paren_not_call() {
    let p = parse("var x = (1 + 2)\n");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PAREN_EXPR));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CALL_EXPR));
}

#[test]
fn call_not_lambda() {
    let p = parse("var x = foo(1)\n");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CALL_EXPR));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::LAMBDA_EXPR));
}

// ── M. Error recovery ───────────────────────────────────────────────

#[test]
fn error_unterminated_string() {
    let src = "var x = \"hello\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unterminated string"
    );
}

#[test]
fn error_missing_rparen_call() {
    let src = "var x = foo(1, 2\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `)` in call"
    );
}

#[test]
fn error_missing_rparen_paren_expr() {
    let src = "var x = (1 + 2\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `)` in paren expression"
    );
}

#[test]
fn error_missing_operand_after_infix() {
    let src = "var x = 1 +\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for a dangling infix operator"
    );
}

#[test]
fn error_missing_operand_at_eof_no_trailing_newline() {
    // No trailing NEWLINE token at all — pure EOF-adjacent malformed input.
    let src = "var x = 1 +";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_empty_parens_has_no_expression() {
    // `()` — L_PAREN immediately followed by R_PAREN. `atom()` can't start
    // an expression on R_PAREN, so this records an error but still closes
    // the PAREN_EXPR node losslessly (no operand list here — unlike
    // `brink-syntax`, this grammar has no `LIST_EXPR` fallback).
    let src = "var x = ()\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
    let paren = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PAREN_EXPR)
        .expect("PAREN_EXPR still opens");
    assert_eq!(paren.children().count(), 0);
}

#[test]
fn error_malformed_arg_list_leading_comma() {
    // `foo(,)` — a stray leading COMMA can't start an expression;
    // `arg_list`'s zero-progress guard must recover via `error_recover`
    // (consume the COMMA as an ERROR-wrapped token) rather than looping
    // forever or panicking.
    let src = "var x = foo(,)\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
}

#[test]
fn error_malformed_arg_list_double_comma() {
    let src = "var x = foo(1,,2)\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_unclosed_call_at_eof() {
    let src = "var x = foo(";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_unclosed_lambda_pipe_still_recovers_a_body() {
    // `|x, y expr` — missing the closing `|`. `lambda_params` breaks its
    // loop on the un-comma'd `expr` token, `expect(PIPE)` records an error
    // without consuming, and the lambda body still parses from wherever
    // the cursor landed. Round-trip must still hold.
    let src = "var f = |x, y expr\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LAMBDA_EXPR));
}

#[test]
fn error_unexpected_token_cannot_start_expression() {
    // A bare `+` is not a prefix operator here (only `-`/`!` are, per
    // `expr::is_prefix_op`) — it can't start an expression at all.
    let src = "var x = +\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn error_unexpected_token_percent_cannot_start_expression() {
    let src = "var x = %5\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

// ── N. Adversarial / fuzz-style inputs ──────────────────────────────

#[test]
fn adversarial_deeply_nested_parens_does_not_panic() {
    // 300 > MAX_DEPTH (256, `parser::MAX_DEPTH`). `enter_depth` must bail
    // with an error at the limit rather than blowing the Rust call stack —
    // this is exactly the "guard against unbounded growth" rule
    // (CLAUDE.md) applied to the expression grammar's own recursion.
    let src = format!("var x = {}1{}\n", "(".repeat(300), ")".repeat(300));
    let p = parse(&src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected a max-nesting-depth error, not silent success or a panic"
    );
}

#[test]
fn adversarial_deeply_nested_calls_does_not_panic() {
    let mut src = "var x = ".to_string();
    for _ in 0..300 {
        src.push_str("foo(");
    }
    src.push('1');
    src.push_str(&")".repeat(300));
    src.push('\n');
    let p = parse(&src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    // Either it hits MAX_DEPTH (error) or it parses clean — either way
    // it must not panic or hang, which reaching this assertion proves.
    let _ = p.errors();
}

#[test]
fn adversarial_long_infix_chain_does_not_panic() {
    // 500 `+`-chained terms. Left-associative parsing (see section F)
    // builds this iteratively in the loop rather than recursing once per
    // operator, but this still must not blow the stack or hang.
    let mut src = "var x = 1".to_string();
    for _ in 0..500 {
        src.push_str(" + 1");
    }
    src.push('\n');
    let p = parse(&src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

#[test]
fn adversarial_truncated_source_mid_operator() {
    let src = "var x = a =";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

#[test]
fn adversarial_unicode_in_string_literal() {
    let src = "var x = \"héllo wörld 🎉\"\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn adversarial_mixed_garbage_tokens_in_call_args() {
    let src = "var x = foo(1, @, )#, 2)\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    // Just must not panic; garbage tokens are expected to produce errors.
    assert!(!p.errors().is_empty());
}

// ── N2. Construction initializers, `TypeName { … }` (B5, #1464) ──────
// The grammar is one shape for all three ruled entry forms; *meaning* is
// the `construct` protocol's job one layer up (`brink_ir::hir::construct`),
// so these tests only ever assert CST shape, never per-type semantics.

/// The empty form — legal grammar, and the shortest thing that proves the
/// `IDENT`-then-`{` commit fires at all.
#[test]
fn construct_literal_empty() {
    let p = assert_lossless("var m = Map { }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_ENTRY));
}

#[test]
fn construct_literal_pair_form_produces_one_entry_per_pair() {
    let p = assert_lossless("var m = Map { \"a\": 1, \"b\": 2 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_ENTRY), 2);
}

#[test]
fn construct_literal_element_form_produces_one_entry_per_element() {
    let p = assert_lossless("var f = Flags { Red, Blue, Green }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_ENTRY), 3);
}

/// The field form is *the same node shape* as the pair form — pair and
/// field differ only in what the target type makes of the left side, which
/// is dispatch, not grammar (`SyntaxKind::CONSTRUCT_ENTRY`'s doc).
#[test]
fn construct_literal_field_form_is_the_same_node_shape_as_the_pair_form() {
    let field = assert_lossless("var p = Point { x: 1, y: 2 }\n");
    let pair = assert_lossless("var m = Map { x: 1, y: 2 }\n");
    assert!(field.errors().is_empty(), "errors: {:?}", field.errors());
    assert_eq!(
        count_node_kind(&field.syntax(), SyntaxKind::CONSTRUCT_ENTRY),
        count_node_kind(&pair.syntax(), SyntaxKind::CONSTRUCT_ENTRY),
    );
}

#[test]
fn construct_literal_accepts_a_trailing_comma() {
    let p = assert_lossless("var m = Map { \"a\": 1, }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_ENTRY), 1);
}

#[test]
fn construct_literal_entries_may_span_lines() {
    let p = assert_lossless("var m = Map {\n  \"a\": 1,\n  \"b\": 2,\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_ENTRY), 2);
}

#[test]
fn construct_literal_nests() {
    let p = assert_lossless("var m = Map { \"p\": Point { x: 1 } }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL),
        2
    );
}

/// A `::`-qualified type name is one `PATH`, so the whole spelling is still
/// a single construction literal (registry lookup is on the last segment).
#[test]
fn construct_literal_accepts_a_qualified_type_path() {
    let p = assert_lossless("var m = std::map::Map { \"a\": 1 }\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL));
}

/// The brace must sit on the type name's own line — a `NEWLINE` is never
/// trivia here, so this is a plain path followed by an unrelated block, not
/// a construction literal (the same rule a call's `(` already follows).
#[test]
fn a_brace_on_the_next_line_is_not_a_construct_literal() {
    let p = assert_lossless("var m = Map\n\nflow main() {\n}\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL));
}

/// The `no-struct-literal` restriction (Rust's precedent): in an `if`/
/// `while`/`for` head the brace opens the body, so a bare path there must
/// stay a `PATH_EXPR` and the block must stay a `STMT_BLOCK`.
#[test]
fn a_control_flow_head_does_not_swallow_its_body_brace() {
    for src in [
        "var x = { if ready { 1; } };\n",
        "var x = { while ready { 1; } };\n",
        "var x = { for k in bag { 1; } };\n",
    ] {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src}: errors: {:?}", p.errors());
        assert!(
            !has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL),
            "{src}: head brace must open the body, not a construction literal"
        );
        assert!(has_node_kind(&p.syntax(), SyntaxKind::STMT_BLOCK));
    }
}

/// …and the restriction lifts inside parentheses, so the literal form is
/// still reachable in a head when the author asks for it.
#[test]
fn parentheses_restore_the_construct_literal_inside_a_control_flow_head() {
    let p = assert_lossless("var x = { if (Point { x: 1 }) == p { 1; } };\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STMT_BLOCK));
}

/// The same restriction on the *content-ground* `{if …}`/`{match …}` heads
/// (`parser::family`), whose arm bodies also open with `{`.
#[test]
fn a_content_ground_conditional_head_does_not_swallow_its_arm_brace() {
    for src in [
        "flow main() {\n  {if ready {\n    Yes\n  }}\n}\n",
        "flow main() {\n  {match mood {\n    calm => Calm\n  }}\n}\n",
    ] {
        let p = assert_lossless(src);
        assert!(p.errors().is_empty(), "{src}: errors: {:?}", p.errors());
        assert!(
            !has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL),
            "{src}: head brace must open the arm, not a construction literal"
        );
    }
}

/// A construction literal nested *inside* a control-flow body is
/// unrestricted — the restriction is scoped to the head, and `stmt_block`
/// clears it again.
#[test]
fn a_control_flow_body_may_contain_a_construct_literal() {
    let p = assert_lossless("var x = { if ready { let m = Map { \"a\": 1 }; } };\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL));
}

/// A construction literal is an ordinary atom, so it composes with the rest
/// of the expression grammar (call argument position here).
#[test]
fn construct_literal_in_call_argument_position() {
    let p = assert_lossless("var x = size(Map { \"a\": 1 })\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ARG_LIST));
}

#[test]
fn unterminated_construct_literal_never_panics() {
    let src = "var m = Map { \"a\": 1\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn garbage_inside_a_construct_literal_never_panics() {
    let src = "var m = Map { @@@ }\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

/// Typed-AST accessors: form detection reads the `COLON` token, and
/// `key`/`value` line up with it.
#[test]
fn construct_entry_accessors_distinguish_the_two_forms() {
    let p = assert_lossless("var m = Map { \"a\": 1 }\n");
    let lit = p
        .syntax()
        .descendants()
        .find_map(ast::ConstructLiteral::cast)
        .expect("one CONSTRUCT_LITERAL");
    assert_eq!(
        lit.type_path()
            .expect("type path")
            .segments()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>(),
        vec!["Map".to_string()]
    );
    let entry = lit.entries().next().expect("one entry");
    assert!(entry.is_pair());
    assert_eq!(entry.key().expect("key").kind(), SyntaxKind::STRING_LIT);
    assert_eq!(
        entry.value().expect("value").kind(),
        SyntaxKind::INTEGER_LIT
    );

    let p = assert_lossless("var f = Flags { Red }\n");
    let lit = p
        .syntax()
        .descendants()
        .find_map(ast::ConstructLiteral::cast)
        .expect("one CONSTRUCT_LITERAL");
    let entry = lit.entries().next().expect("one entry");
    assert!(!entry.is_pair());
    assert!(entry.key().is_none());
    assert_eq!(entry.value().expect("value").kind(), SyntaxKind::PATH_EXPR);
}

// ── N3. Array/sequence literals, `[…]` (NG-D, issue #1490, RULED ─────
// ── 2026-07-27: "`[1, 2, 3]`. Bracket literal on the native surface") ─
// A plain atom, not a construction-registry entry — the B5-symmetric
// `Array { … }` spelling was weighed and rejected in the same ruling.
// Elements are bare expression children directly under `ARRAY_LITERAL`
// (mirrors `ARG_LIST`'s shape); there is no per-element wrapper node the
// way `CONSTRUCT_ENTRY` wraps a construction literal's entries, since an
// array element is never a key/value pair.

/// The empty form — legal grammar, and the shortest thing that proves the
/// `L_BRACKET` atom commits at all.
#[test]
fn array_literal_empty() {
    let p = assert_lossless("var a = []\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ARRAY_LITERAL));
}

#[test]
fn array_literal_produces_one_child_per_element() {
    let p = assert_lossless("var a = [1, 2, 3]\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lit = p
        .syntax()
        .descendants()
        .find_map(ast::ArrayLiteral::cast)
        .expect("one ARRAY_LITERAL");
    assert_eq!(lit.elements().count(), 3);
}

#[test]
fn array_literal_accepts_a_trailing_comma() {
    let p = assert_lossless("var a = [1, 2, ]\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lit = p
        .syntax()
        .descendants()
        .find_map(ast::ArrayLiteral::cast)
        .expect("one ARRAY_LITERAL");
    assert_eq!(lit.elements().count(), 2);
}

#[test]
fn array_literal_elements_may_span_lines() {
    let p = assert_lossless("var a = [\n  1,\n  2,\n]\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lit = p
        .syntax()
        .descendants()
        .find_map(ast::ArrayLiteral::cast)
        .expect("one ARRAY_LITERAL");
    assert_eq!(lit.elements().count(), 2);
}

#[test]
fn array_literal_nests() {
    let p = assert_lossless("var a = [[1, 2], [3, 4]]\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ARRAY_LITERAL), 3);
}

/// An array literal is an ordinary atom, so it composes with the rest of
/// the expression grammar (call argument position here) — the same proof
/// `construct_literal_in_call_argument_position` gives the construction
/// initializer.
#[test]
fn array_literal_in_call_argument_position() {
    let p = assert_lossless("var x = size([1, 2, 3])\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ARRAY_LITERAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ARG_LIST));
}

/// A construction literal composes inside an array element without the
/// no-construct-literal restriction ever engaging — `[` never triggers it
/// (unlike a bare path followed by `{` in a control-flow head, see
/// `a_control_flow_head_does_not_swallow_its_body_brace` above).
#[test]
fn array_literal_elements_may_be_construction_literals() {
    let p = assert_lossless("var a = [Point { x: 1 }, Point { x: 2 }]\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL),
        2
    );
}

/// The array literal's own bracket lifts `no_construct_literal` for its
/// elements even when the array itself sits inside a genuinely restricted
/// head (`head_expression`, `control_flow.rs`'s `if`/`while`/`for-in`) —
/// unlike a bare path followed by `{`, the brackets already disambiguate
/// where the head's expression ends, so a construction literal composes
/// freely as an element without needing the parenthesized-restoration
/// escape hatch `parentheses_restore_the_construct_literal_inside_a_control_flow_head`
/// exercises above.
#[test]
fn array_literal_in_a_for_in_head_still_allows_construction_literal_elements() {
    let p = assert_lossless("var x = { for q in [Point { x: 1 }] { 1; } };\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ARRAY_LITERAL));
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::CONSTRUCT_LITERAL),
        1
    );
}

#[test]
fn unterminated_array_literal_never_panics() {
    let src = "var a = [1, 2\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

#[test]
fn garbage_inside_an_array_literal_never_panics() {
    let src = "var a = [ @@@ ]\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(!p.errors().is_empty());
}

// ── O. Proptest round-trip generator (local to this family file — see  ──
// ── the PR discussion: #1199's own scope is `fuzz_repro.rs`, a         ──
// ── `.brink`-corpus round-trip test, and a new `fuzz/` crate, not      ──
// ── `tests/proptest_native.rs`, so this generator stays local here     ──
// ── rather than claiming ownership of the shared harness. Its keyword  ──
// ── filter reuses the crate's real classifier so it can never drift    ──
// ── from `classify_keyword`'s actual keyword set again.)               ──

mod proptest_roundtrip {
    use super::*;
    use proptest::prelude::*;

    fn arb_ident() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9]{0,5}".prop_filter("not a keyword", |s| {
            crate::lexer::classify_keyword(s) == SyntaxKind::IDENT
        })
    }

    fn arb_integer() -> impl Strategy<Value = String> {
        (0..10_000i64).prop_map(|n| n.to_string())
    }

    fn arb_infix_op() -> impl Strategy<Value = &'static str> {
        prop_oneof![
            Just("+"),
            Just("-"),
            Just("*"),
            Just("/"),
            Just("%"),
            Just("<"),
            Just(">"),
            Just("<="),
            Just(">="),
            Just("=="),
            Just("!="),
            Just("&&"),
            Just("||"),
            Just("or"),
        ]
    }

    /// A self-contained, depth-bounded expression generator covering every
    /// operator this grammar's `infix_binding_power`/`is_prefix_op` know
    /// about, plus calls and parens. Every string this produces is
    /// well-formed by construction, so the round-trip property below
    /// additionally asserts zero parse errors (not just losslessness).
    fn arb_expr() -> impl Strategy<Value = String> {
        let leaf = prop_oneof![
            arb_integer(),
            Just("true".to_string()),
            Just("false".to_string()),
            arb_ident(),
        ];
        leaf.prop_recursive(3, 20, 3, |inner| {
            prop_oneof![
                inner.clone().prop_map(|e| format!("-{e}")),
                inner.clone().prop_map(|e| format!("!{e}")),
                inner.clone().prop_map(|e| format!("({e})")),
                (inner.clone(), arb_infix_op(), inner.clone())
                    .prop_map(|(l, op, r)| format!("{l} {op} {r}")),
                (arb_ident(), prop::collection::vec(inner, 0..=2))
                    .prop_map(|(name, args)| format!("{name}({})", args.join(", "))),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn expr_round_trips_losslessly_and_parses_clean(body in arb_expr()) {
            let src = format!("var x = {body}\n");
            let p = parse(&src);
            prop_assert_eq!(&src, &p.syntax().text().to_string());
            prop_assert!(
                p.errors().is_empty(),
                "well-formed generated expr `{src}` produced errors: {:?}",
                p.errors()
            );
        }

        #[test]
        fn expr_as_call_argument_round_trips_losslessly(body in arb_expr()) {
            let src = format!("var x = wrap({body})\n");
            let p = parse(&src);
            prop_assert_eq!(&src, &p.syntax().text().to_string());
            prop_assert!(
                p.errors().is_empty(),
                "well-formed generated call-arg `{src}` produced errors: {:?}",
                p.errors()
            );
        }
    }
}
