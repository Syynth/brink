use crate::support::*;
use brink_ir::lir;

// ─── #673: collection/struct literal declaration defaults ───────────
//
// `eval_const_expr` (decls) previously had no arm for `ArrayLiteral`/
// `MapLiteral`/`StructLiteral` and fell through to `ConstValue::Null` with
// no diagnostic. These fixtures deliberately do NOT use the `VAR p = 0` +
// reassignment workaround idiom (`tests/tier1-brink/nested-index-
// assignment/story.ink`'s precedent) — the literal is the declaration's
// actual default.

#[test]
fn var_array_literal_default_folds_to_const_array_not_null() {
    let p = lower_ink("VAR arr = #[1, 2, 3]\n");
    let g = find_global(&p, "arr");
    match &g.default {
        lir::ConstValue::Array(items) => {
            assert_eq!(
                items,
                &[
                    lir::ConstValue::Int(1),
                    lir::ConstValue::Int(2),
                    lir::ConstValue::Int(3),
                ]
            );
        }
        other => panic!("expected ConstValue::Array, got {other:?} (silent Null regression)"),
    }
}

#[test]
fn var_map_literal_default_folds_to_const_map_not_null() {
    let p = lower_ink("VAR m = #{\"a\": 1, \"b\": 2}\n");
    let g = find_global(&p, "m");
    match &g.default {
        lir::ConstValue::Map(entries) => {
            assert_eq!(
                entries,
                &[
                    (
                        lir::ConstMapKey::Str("a".to_string()),
                        lir::ConstValue::Int(1)
                    ),
                    (
                        lir::ConstMapKey::Str("b".to_string()),
                        lir::ConstValue::Int(2)
                    ),
                ]
            );
        }
        other => panic!("expected ConstValue::Map, got {other:?} (silent Null regression)"),
    }
}

#[test]
fn const_array_literal_default_folds_to_const_array() {
    // The issue names both VAR and CONST declaration defaults.
    let p = lower_ink("CONST arr = #[9, 8]\n");
    let g = find_global(&p, "arr");
    assert!(!g.mutable);
    match &g.default {
        lir::ConstValue::Array(items) => {
            assert_eq!(items, &[lir::ConstValue::Int(9), lir::ConstValue::Int(8)]);
        }
        other => panic!("expected ConstValue::Array, got {other:?}"),
    }
}

#[test]
fn nested_array_literal_default_folds_recursively() {
    let p = lower_ink("VAR grid = #[#[1, 2], #[3, 4]]\n");
    let g = find_global(&p, "grid");
    match &g.default {
        lir::ConstValue::Array(items) => {
            assert_eq!(
                items,
                &[
                    lir::ConstValue::Array(vec![lir::ConstValue::Int(1), lir::ConstValue::Int(2)]),
                    lir::ConstValue::Array(vec![lir::ConstValue::Int(3), lir::ConstValue::Int(4)]),
                ]
            );
        }
        other => panic!("expected nested ConstValue::Array, got {other:?}"),
    }
}

#[test]
fn struct_literal_default_is_a_real_compile_error_not_silent_null() {
    let source =
        "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\nVAR p = Point#{x: 1.0, y: 2.0}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    // Lowering is still total (matches E055/E056/E073/E074's existing
    // "diagnostic doesn't stop `Some(program)`" convention) — the caller
    // above `lower_to_program` (brink-db's `lir_query`) is what turns an
    // Error-severity diagnostic into a blocked compile.
    let program = program.expect("lowering stays total; severity partitioning happens upstream");
    let g = find_global(&program, "p");
    assert!(
        matches!(g.default, lir::ConstValue::Null),
        "struct defaults have no ConstValue representation yet — expected Null, got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E075),
        "expected non-suppressible E075 for a struct literal VAR default, got {diagnostics:?}"
    );
}

#[test]
fn map_literal_default_with_non_scalar_key_is_a_real_compile_error() {
    // Float is not in the ratified map-key domain (int/string/bool) —
    // mid-story `MapNew` faults on this at runtime; a declaration default
    // has no runtime construction step to fault at, so this must be a
    // compile-time diagnostic, not a silently-dropped entry.
    let source = "VAR m = #{3.5: 1}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "m");
    assert!(
        matches!(&g.default, lir::ConstValue::Map(entries) if entries.is_empty()),
        "expected the invalid-key entry to be dropped from the map, got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E076),
        "expected non-suppressible E076 for a non-scalar map key in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn array_literal_default_with_non_constant_element_is_a_real_compile_error() {
    // #679 review: a function call can never constant-fold — before E077
    // the element recursed into `eval_const_expr`'s catch-all and silently
    // became `Null`, #673's silent-Null bug one level down inside the
    // literal. Keyed off the source expr *kind* (a call), not the folded
    // result.
    let source = "VAR arr = #[f(), 2]\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice() == [lir::ConstValue::Null, lir::ConstValue::Int(2)]
        ),
        "the never-constant element still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a non-constant array element in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn map_literal_default_with_non_constant_value_is_a_real_compile_error() {
    // #679 review: same E077 story as the array-element test, for a map
    // *value*. (A never-constant map *key* is already E076 — it folds to
    // Null, outside the scalar key domain.)
    let source = "VAR m = #{\"a\": f()}\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "m");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Map(entries)
                if entries.as_slice()
                    == [(
                        lir::ConstMapKey::Str("a".to_string()),
                        lir::ConstValue::Null
                    )]
        ),
        "the never-constant value still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a non-constant map value in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn nested_array_literal_with_non_constant_element_propagates_e077() {
    // #679 review, nested case: the outer element is itself a literal (a
    // constant-foldable *kind*), so the outer check passes and the E077
    // must come from the recursion into the inner literal's own
    // per-element check — the hole must not reopen one level down.
    let source = "VAR grid = #[#[f()], #[2]]\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "grid");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice()
                    == [
                        lir::ConstValue::Array(vec![lir::ConstValue::Null]),
                        lir::ConstValue::Array(vec![lir::ConstValue::Int(2)]),
                    ]
        ),
        "the nested never-constant element still folds to Null (now diagnosed), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected E077 to propagate out of the nested literal, got {diagnostics:?}"
    );
}

#[test]
fn constant_infix_array_element_does_not_false_positive_e077() {
    // The E077 check recurses through Prefix/Infix — `1 + 2` and `-3` are
    // constant-foldable kinds and must not be flagged.
    let source = "VAR arr = #[1 + 2, -3]\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice() == [lir::ConstValue::Int(3), lir::ConstValue::Int(-3)]
        ),
        "constant infix/prefix elements fold for real, got {:?}",
        g.default
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "constant-foldable elements must not trip E077, got {diagnostics:?}"
    );
}

#[test]
fn const_reference_array_element_folds_and_does_not_false_positive_e077() {
    // A `CONST` reference is a `Path` — a constant-foldable kind. It must
    // resolve through `const_values` to the real value and must not trip
    // E077 (the check is keyed off the source expr kind, and `Path`
    // constness depends on resolution — deliberately not flagged).
    let source = "CONST SOME_CONST = 7\nVAR arr = #[SOME_CONST, 2]\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice() == [lir::ConstValue::Int(7), lir::ConstValue::Int(2)]
        ),
        "CONST-reference element folds to the constant's value, got {:?}",
        g.default
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "a CONST-reference element must not trip E077, got {diagnostics:?}"
    );
}
