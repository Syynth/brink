use crate::support::*;
use brink_ir::lir;

// ─── Const folding for binary expressions ───────────────────────────

#[test]
fn const_fold_int_addition() {
    let program = lower_ink("VAR x = 2 + 3\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(5));
}

#[test]
fn const_fold_int_subtraction() {
    let program = lower_ink("VAR x = 10 - 4\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(6));
}

#[test]
fn const_fold_int_multiplication() {
    let program = lower_ink("VAR x = 3 * 7\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(21));
}

#[test]
fn const_fold_int_division() {
    let program = lower_ink("VAR x = 20 / 4\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(5));
}

#[test]
fn const_fold_int_modulo() {
    let program = lower_ink("VAR x = 7 % 3\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(1));
}

#[test]
fn const_fold_comparison_eq() {
    let program = lower_ink("VAR x = 5 == 5\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(true));
}

#[test]
fn const_fold_comparison_lt() {
    let program = lower_ink("VAR x = 3 < 5\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(true));
}

#[test]
fn const_fold_logical_and() {
    let program = lower_ink("VAR x = true && false\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(false));
}

#[test]
fn const_fold_logical_or() {
    let program = lower_ink("VAR x = false || true\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(true));
}

#[test]
fn const_fold_string_concatenation() {
    let program = lower_ink("VAR x = \"hello\" + \" world\"\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::String("hello world".into()));
}

#[test]
fn const_fold_nested_arithmetic() {
    // (2 + 3) * 4 — depends on parser precedence, but the key test is
    // that nested infix expressions are recursively folded.
    let program = lower_ink("VAR x = 2 + 3 * 4\n{x}\n");
    let g = find_global(&program, "x");
    // 3 * 4 = 12, 2 + 12 = 14 (standard precedence)
    assert_eq!(g.default, lir::ConstValue::Int(14));
}

#[test]
fn const_fold_const_reference_in_binary() {
    let program = lower_ink("CONST a = 10\nVAR x = a + 5\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(15));
}

#[test]
fn const_fold_division_by_zero_yields_null() {
    let program = lower_ink("VAR x = 10 / 0\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Null);
}
