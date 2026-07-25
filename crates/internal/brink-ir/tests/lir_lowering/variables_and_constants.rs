use crate::support::*;
use brink_ir::lir;

// ─── Variables and constants ────────────────────────────────────────

#[test]
fn var_declaration_creates_mutable_global() {
    let p = lower_ink("VAR x = 5\n");
    let g = find_global(&p, "x");
    assert!(g.mutable);
    assert!(matches!(g.default, lir::ConstValue::Int(5)));
}

#[test]
fn const_declaration_creates_immutable_global() {
    let p = lower_ink("CONST y = 10\n");
    let g = find_global(&p, "y");
    assert!(!g.mutable);
    assert!(matches!(g.default, lir::ConstValue::Int(10)));
}

#[test]
fn var_float_default() {
    let p = lower_ink("VAR f = 2.5\n");
    let g = find_global(&p, "f");
    if let lir::ConstValue::Float(v) = g.default {
        assert!((v - 2.5).abs() < 0.01);
    } else {
        panic!("expected Float default, got something else");
    }
}

#[test]
fn var_string_default() {
    let p = lower_ink("VAR name = \"hello\"\n");
    let g = find_global(&p, "name");
    assert!(matches!(&g.default, lir::ConstValue::String(s) if s == "hello"));
}

#[test]
fn var_bool_default() {
    let p = lower_ink("VAR flag = true\n");
    let g = find_global(&p, "flag");
    assert!(matches!(g.default, lir::ConstValue::Bool(true)));
}

#[test]
fn var_negative_default() {
    let p = lower_ink("VAR n = -42\n");
    let g = find_global(&p, "n");
    assert!(matches!(g.default, lir::ConstValue::Int(-42)));
}
