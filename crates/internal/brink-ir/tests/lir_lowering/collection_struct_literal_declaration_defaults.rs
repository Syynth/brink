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

// ─── #1530: a well-formed construction literal *is* a legal default ──
//
// #673 refused every struct literal in this position with `E075` because
// `ConstValue` had no record-carrying variant. That made a struct-typed
// durable global unspellable, and with it the whole T1e projection-receiver
// path (`docs/t1e-spec.md` §2: a projection's root must be a durable cell).
// `E075` now covers only the shape-mismatch cases below.

#[test]
fn struct_literal_default_folds_to_const_record_not_null() {
    let source =
        "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\nVAR p = Point#{x: 1.0, y: 2.0}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    // Lowering is still total (matches E055/E056/E073/E074's existing
    // "diagnostic doesn't stop `Some(program)`" convention) — the caller
    // above `lower_to_program` (brink-db's `lir_query`) is what turns an
    // Error-severity diagnostic into a blocked compile.
    let program = program.expect("lowering stays total; severity partitioning happens upstream");
    let g = find_global(&program, "p");
    match &g.default {
        lir::ConstValue::Record { shape_id, fields } => {
            assert_eq!(
                *shape_id,
                shape_id_of(&program, "Point"),
                "the folded record names the shape the shape table assigned"
            );
            assert_eq!(
                fields,
                &[lir::ConstValue::Float(1.0), lir::ConstValue::Float(2.0)]
            );
        }
        other => panic!("expected ConstValue::Record, got {other:?}"),
    }
    assert!(
        diagnostics.is_empty(),
        "a well-formed construction literal default is clean, got {diagnostics:?}"
    );
}

#[test]
fn struct_literal_default_folds_fields_into_shape_declaration_order() {
    // Source order (`y` first) must not survive into the record — the flat
    // field vector is shape-ordered, which is what `RecordGet`'s static
    // offsets and `Value::Record`'s equality both key on.
    let source =
        "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\nVAR p = Point#{y: 2.0, x: 1.0}\n";
    let p = lower_ink(source);
    let g = find_global(&p, "p");
    match &g.default {
        lir::ConstValue::Record { fields, .. } => {
            assert_eq!(
                fields,
                &[lir::ConstValue::Float(1.0), lir::ConstValue::Float(2.0)],
                "fields must be reordered into shape declaration order"
            );
        }
        other => panic!("expected ConstValue::Record, got {other:?}"),
    }
}

#[test]
fn struct_literal_default_folds_nested_literals_recursively() {
    // The declared-default folder is fully recursive, same as the array and
    // map arms: a nested construction literal, an array field and a `CONST`
    // reference all fold for real one level in.
    let source = "\
STRUCT Inner = #{\n    v: int,\n}
STRUCT Outer = #{\n    inner: Inner,\n    tags: Array<int>,\n}

CONST BASE = 7
VAR o = Outer#{inner: Inner#{v: BASE}, tags: #[1, 2]}
";
    let p = lower_ink(source);
    let g = find_global(&p, "o");
    match &g.default {
        lir::ConstValue::Record { fields, .. } => {
            assert_eq!(
                fields,
                &[
                    lir::ConstValue::Record {
                        shape_id: shape_id_of(&p, "Inner"),
                        fields: vec![lir::ConstValue::Int(7)],
                    },
                    lir::ConstValue::Array(vec![lir::ConstValue::Int(1), lir::ConstValue::Int(2)]),
                ]
            );
        }
        other => panic!("expected nested ConstValue::Record, got {other:?}"),
    }
}

#[test]
fn struct_literal_default_missing_a_declared_field_is_a_real_compile_error() {
    // Mid-story this is the gradual construction fault (`RecordNew` against
    // an invalid shape id); a declaration default is baked into `StoryData`
    // with no runtime construction step to fault at, so the compile-time
    // equivalent is required — never a half-built record.
    let source = "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\nVAR p = Point#{x: 1.0}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "p");
    assert!(
        matches!(g.default, lir::ConstValue::Null),
        "a mismatched literal must not bake a partial record, got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E075),
        "expected non-suppressible E075 for a missing field, got {diagnostics:?}"
    );
}

#[test]
fn struct_literal_default_with_an_undeclared_field_is_a_real_compile_error() {
    let source = "STRUCT Point = #{\n    x: float,\n}\n\nVAR p = Point#{x: 1.0, z: 3.0}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "p");
    assert!(
        matches!(g.default, lir::ConstValue::Null),
        "a mismatched literal must not bake a partial record, got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E075),
        "expected non-suppressible E075 for an undeclared field, got {diagnostics:?}"
    );
}

#[test]
fn struct_literal_default_naming_an_unresolved_shape_is_a_real_compile_error() {
    // Same code the expression-position path's
    // `reject_unresolved_struct_shape` uses — an undeclared shape has no
    // field order to fold against at all.
    let (program, diagnostics) = lower_ink_with_warnings("VAR p = Nope#{x: 1.0}\n");
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "p");
    assert!(matches!(g.default, lir::ConstValue::Null));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E073),
        "expected E073 for an unresolved shape in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn struct_literal_default_with_a_non_constant_field_is_a_real_compile_error() {
    // The E077 story reaches struct fields exactly as it reaches array
    // elements and map values — before #1530 the whole literal was
    // unconditionally E075, so a bad field never got its own diagnostic.
    let source = "STRUCT Point = #{\n    x: int,\n}\n\nVAR p = Point#{x: f()}\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "p");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Record { fields, .. } if fields.as_slice() == [lir::ConstValue::Null]
        ),
        "the never-constant field still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a non-constant struct field, got {diagnostics:?}"
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
