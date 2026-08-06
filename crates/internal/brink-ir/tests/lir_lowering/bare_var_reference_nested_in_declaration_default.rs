use crate::support::*;
use brink_ir::lir;

// ── #743: bare VAR reference nested one level inside a declaration-default
// collection/fn literal — the residue #679's scope notes flagged and #692/
// E083 deliberately left alone. `is_const_foldable_kind` now resolves a
// nested `Path` the same way `is_const_foldable_decl_default` resolves the
// top-level one: `SymbolKind::Variable` is never a compile-time constant, so
// it reports the standard E077 instead of silently folding to `Null`.

#[test]
fn var_reference_array_element_is_now_a_real_compile_error() {
    let source = "VAR a = 1\nVAR arr = #[a]\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items) if items.as_slice() == [lir::ConstValue::Null]
        ),
        "the never-constant VAR-reference element still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a VAR-reference array element in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn var_reference_map_value_is_now_a_real_compile_error() {
    let source = "VAR a = 1\nVAR m = #{\"k\": a}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "m");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Map(entries)
                if entries.as_slice()
                    == [(lir::ConstMapKey::Str("k".to_string()), lir::ConstValue::Null)]
        ),
        "the never-constant VAR-reference value still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a VAR-reference map value in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn var_reference_bound_fn_val_arg_is_now_a_real_compile_error() {
    // `eval_const_fn_literal`'s `val` branch previously called
    // `eval_const_expr` with no `is_const_foldable_kind` gate at all — a
    // bare VAR reference bound by value silently folded to `Null` inside
    // the closure's `env`, zero diagnostic.
    let source = "=== function heal(hp) ===\n~ return hp + 1\n\nVAR g = 5\nVAR f = #fn(heal, g)\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "f");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Closure { env, .. }
                if matches!(
                    env.as_slice(),
                    [lir::ConstClosureEntry::Val { value: lir::ConstValue::Null, .. }]
                )
        ),
        "the never-constant VAR-reference bound val arg still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a VAR-reference #fn bound val arg in a VAR default, got {diagnostics:?}"
    );
}
