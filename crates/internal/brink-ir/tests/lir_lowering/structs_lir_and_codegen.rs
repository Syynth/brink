use crate::support::*;
use brink_ir::lir;

// ─── TM-4c (#666): structs LIR + codegen ────────────────────────────────
//
// Construction, field reads, and single-level field writes all lower for
// real now (TM-4b/#665 only landed grammar+HIR+analyzer, diagnostics-only).
// E072 (the old "reject everything" backstop) is retired; E073 (unresolved
// shape at construction) and E074 (chained/mixed field-write projection)
// are its narrower TM-4c replacements — still real, non-suppressible
// diagnostics, mirroring the E053-backstop discipline.

/// A `RecordNew`'s decoded parts: `(shape_id, fields, prelude)` — see
/// `lir::Expr::RecordNew`'s own doc for what `fields`/`prelude` mean.
type RecordNewParts<'a> = (
    u32,
    &'a [lir::Expr],
    &'a [(u16, brink_format::NameId, lir::Expr)],
);

fn find_record_new(expr: &lir::Expr) -> Option<RecordNewParts<'_>> {
    match expr {
        lir::Expr::RecordNew {
            shape_id,
            fields,
            prelude,
        } => Some((*shape_id, fields, prelude)),
        _ => None,
    }
}

/// Resolve a `RecordNew` field to the actual initializer expression it was
/// built from: the well-formed path's `fields` entries are `GetTemp` reads
/// of a `prelude` slot (#676 source-order staging), so this chases that
/// indirection back to the staged expression; the fault path's `fields`
/// entries are already the raw initializer, returned as-is.
fn resolve_field<'a>(
    field: &'a lir::Expr,
    prelude: &'a [(u16, brink_format::NameId, lir::Expr)],
) -> &'a lir::Expr {
    match field {
        lir::Expr::GetTemp(slot, _) => prelude
            .iter()
            .find(|(s, _, _)| s == slot)
            .map_or(field, |(_, _, e)| e),
        _ => field,
    }
}

fn declare_temp_value(stmt: &lir::Stmt) -> Option<&lir::Expr> {
    match stmt {
        lir::Stmt::DeclareTemp { value: Some(v), .. } => Some(v),
        _ => None,
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "exact literal from source (1.0/2.0), not a computed value"
)]
fn struct_literal_lowers_to_record_new_in_shape_order() {
    let src = "STRUCT Point = #{x: float, y: float}\n~ temp p = Point#{y: 2.0, x: 1.0}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct constructs lower for real under TM-4c");
    assert_eq!(program.struct_shapes.len(), 1);
    assert_eq!(program.struct_shapes[0].fields.len(), 2);

    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp p := Point#{...} should be a DeclareTemp");
    let (shape_id, fields, prelude) =
        find_record_new(value).expect("Point#{...} should lower to RecordNew");
    assert_eq!(shape_id, program.struct_shapes[0].id);
    // `fields` is reordered into shape decl order (x, y) despite being
    // written y, x — each entry is a `GetTemp` read of a `prelude` slot,
    // resolved back to its staged value here.
    assert!(matches!(
        resolve_field(&fields[0], prelude),
        lir::Expr::Float(f) if *f == 1.0
    ));
    assert!(matches!(
        resolve_field(&fields[1], prelude),
        lir::Expr::Float(f) if *f == 2.0
    ));
    // `prelude` itself is staged in **source** order (#676): y (2.0) first,
    // then x (1.0) — the author's left-to-right order, not shape order.
    assert_eq!(prelude.len(), 2, "one staged slot per supplied initializer");
    assert!(matches!(prelude[0].2, lir::Expr::Float(f) if f == 2.0));
    assert!(matches!(prelude[1].2, lir::Expr::Float(f) if f == 1.0));
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "exact literal from source (1.0/2.0/3.0), not a computed value"
)]
fn duplicate_field_still_stages_every_initializer_no_silent_drop() {
    // #675's LIR-level defense-in-depth: `structs::check_duplicates` (E084)
    // is what normally stops this from compiling at all, but this test
    // proves lowering itself never silently drops an initializer's side
    // effect even under suppression — both `x` initializers get staged
    // into `prelude` (hence both would still be evaluated at runtime),
    // even though only the *last* one's value (2.0, last-wins) ends up
    // placed in the record. `E084` itself is an analyzer diagnostic this
    // LIR-only harness never surfaces — covered instead by
    // `brink-analyzer`'s own unit tests and the `e0xx_diagnostics` pipeline
    // suite.
    let src = "STRUCT Point = #{x: float, y: float}\n\
        ~ temp p = Point#{x: 1.0, x: 2.0, y: 3.0}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct constructs still lower for real under TM-4c");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp p := Point#{...} should be a DeclareTemp");
    let (_shape_id, fields, prelude) =
        find_record_new(value).expect("Point#{...} should lower to RecordNew");
    assert_eq!(
        prelude.len(),
        3,
        "every supplied initializer is staged, including the shadowed duplicate"
    );
    // The winning (last-wins) `x` initializer's value (2.0) is what
    // actually lands in the record at offset 0.
    assert!(matches!(
        resolve_field(&fields[0], prelude),
        lir::Expr::Float(f) if *f == 2.0
    ));
}

#[test]
fn field_access_on_construction_literal_lowers_to_record_get() {
    let src = "STRUCT Point = #{x: float}\n~ temp v = Point#{x: 1.0}.x\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("field access lowers for real under TM-4c");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp v := Point#{...}.x should be a DeclareTemp");
    assert!(matches!(value, lir::Expr::RecordGet { base, .. } if find_record_new(base).is_some()));
}

#[test]
fn resolution_fallback_field_access_lowers_to_record_get_dyn() {
    // The ambiguous `p.x` shape (ordinary dotted `Path`, not the
    // `FieldAccessExpr` grammar) — `brink-analyzer`'s resolution fallback
    // resolves it to the variable `p` via a multi-segment path (TM-4b);
    // LIR lowering must produce the equivalent `RecordGet` chain, not
    // silently load `p` and drop `.x`. `p` holds an int here (gradual mode
    // never checks this statically), so `static_offset` must be `None` — a
    // by-name op, which faults cleanly at runtime instead of trusting an
    // unproven offset.
    let src = "VAR p = 0\n~ temp y = p.x\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("ambiguous-path field access lowers cleanly");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp y := p.x should be a DeclareTemp");
    match value {
        lir::Expr::RecordGet {
            base,
            static_offset,
            ..
        } => {
            assert!(matches!(**base, lir::Expr::GetGlobal(_)));
            assert_eq!(
                *static_offset, None,
                "gradual mode never emits static offsets"
            );
        }
        _ => panic!("expected RecordGet"),
    }
}

#[test]
fn ordinary_dotted_path_still_lowers_as_a_static_visit_count() {
    // A genuine static path (`knot.stitch`) must never be reinterpreted as
    // field access — only the TM-4b fallback case (resolved to
    // Variable/Constant/Param/Temp via a multi-segment path) is. A bare
    // knot/stitch reference used as a *value* (not a divert target) means
    // its visit count in ink semantics (`SymbolKind::Knot | Stitch | Label`
    // → `Expr::VisitCount`) — the key property under test is that it's
    // never a `RecordGet`.
    let src =
        "=== knot ===\n= stitch\nHello.\n-> DONE\n=== main ===\n~ temp x = knot.stitch\n-> DONE\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("ordinary dotted path must lower cleanly");
    let main_knot = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("the `main` knot should exist as a root child container");
    let value = main_knot
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp x := knot.stitch should be a DeclareTemp");
    assert!(
        matches!(value, lir::Expr::VisitCount(_)),
        "a static dotted path must never become a RecordGet"
    );
}

#[test]
fn unresolved_struct_shape_at_construction_emits_e073() {
    let src = "~ temp p = Bogus#{x: 1}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e073 = find_diag(&diags, brink_ir::DiagnosticCode::E073)
        .expect("expected E073 for an unresolved struct shape reaching LIR");
    assert_eq!(e073.code.severity(), brink_ir::Severity::Error);
    // `lower_to_program` stays total (like E053/E057/E058) — it still
    // returns a program; `brink-db`'s `lir_query` is what turns an
    // Error-severity LIR diagnostic into `program: None` for compilation
    // purposes, not `lower_to_program` itself.
    program.expect("lower_to_program is total — it still returns Some");
}

#[test]
fn chained_field_write_emits_e074() {
    let src = "STRUCT Inner = #{v: float}\nSTRUCT Outer = #{inner: Inner}\n\
        VAR o = Outer#{inner: Inner#{v: 1.0}}\n~ o.inner.v = 2.0\nHello.\n";
    let (_program, diags) = lower_ink_with_warnings(src);
    let e074 = find_diag(&diags, brink_ir::DiagnosticCode::E074)
        .expect("expected E074 for a chained field write (o.inner.v = ...)");
    assert_eq!(e074.code.severity(), brink_ir::Severity::Error);
}

#[test]
fn mixed_index_then_field_write_emits_e074() {
    // #674: `arr[i].field = v` (an `Index`-based root followed by a
    // `.field` write) now parses as a real `FIELD_ACCESS_EXPR` assignment
    // target (the grammar gap tracked in the NOTE this test replaces —
    // formerly a generic E015 parse error, PR #665/#668's pre-existing
    // gap). LIR still fences it off as a chained/mixed field write: this
    // pins that `E074` actually fires end-to-end through the parser, not
    // just by code-review inspection of `try_lower_field_assignment`'s
    // `hir::Expr::FieldAccess` branch (blocks.rs).
    let src = "VAR arr = #[1, 2, 3]\n~ arr[0].x = 2.0\nHello.\n";
    let (_program, diags) = lower_ink_with_warnings(src);
    let e074 = find_diag(&diags, brink_ir::DiagnosticCode::E074).expect(
        "expected E074 for a mixed index-then-field write (arr[0].x = ...), now reachable \
         through the parser",
    );
    assert_eq!(e074.code.severity(), brink_ir::Severity::Error);
}

#[test]
fn single_level_field_write_lowers_via_take_make_mut_write_back() {
    // This test's concern is the RMW field-write desugaring, not the
    // declaration default: `p` keeps a scalar placeholder default (no TM-2
    // annotation here, so `types = gradual`'s advisory-only E063 is the
    // worst this scalar/struct mismatch could trigger) and the real `Point`
    // value is constructed via assignment, same as
    // `tests/tier1-brink/struct-construct-read-write/story.ink`'s
    // established pattern. (#673 *required* that shape; since #1530 a
    // construction literal is a legal default too, and
    // `collection_struct_literal_declaration_defaults.rs` covers it — the
    // placeholder is kept here so the assignment path stays exercised.)
    let src = "STRUCT Point = #{x: float, y: float}\nVAR p = 0\n\
        ~ p = Point#{x: 1.0, y: 2.0}\n~ p.x = 9.0\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("single-level field write lowers for real");
    // The RMW desugaring produces a TakeGlobal(p) somewhere, feeding a
    // RecordSet, whose result is written back into the p global.
    let has_take_then_record_set = program.root.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::RecordSet { base, .. },
                ..
            } if matches!(**base, lir::Expr::TakeTemp(..) | lir::Expr::TakeGlobal(_))
        )
    });
    assert!(
        has_take_then_record_set,
        "expected a TakeGlobal/TakeTemp feeding a RecordSet"
    );
    let writes_back_to_p = program.root.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                target: lir::AssignTarget::Global(_),
                ..
            }
        )
    });
    assert!(
        writes_back_to_p,
        "expected a write-back Assign into a global"
    );
}

#[test]
fn gradual_construction_field_mismatch_uses_fault_sentinel_shape_id() {
    // Missing declared field `y` — under `types = gradual` (the default;
    // strict would already be E069, a compile error) this must still lower
    // to *something* the VM can execute deterministically: the
    // construction-fault sentinel `RecordNew` (see
    // `expr::lower_struct_literal`'s doc) rather than a stack-desyncing
    // partial construction.
    let src = "STRUCT Point = #{x: float, y: float}\n~ temp p = Point#{x: 1.0}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("gradual mismatch still lowers, faulting at runtime instead");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp p := Point#{...} should be a DeclareTemp");
    let (shape_id, fields, prelude) =
        find_record_new(value).expect("mismatched construction should still be a RecordNew");
    assert_eq!(shape_id, u32::MAX, "sentinel shape id signals the fault");
    assert_eq!(
        fields.len(),
        1,
        "the one supplied initializer is still evaluated"
    );
    assert!(
        prelude.is_empty(),
        "the fault path's fields are already source order — no staging needed"
    );
}

#[test]
fn strict_mode_known_shape_field_read_uses_static_offset() {
    // `VAR p: Point = Point#{...}` was the pattern #673 silently dropped to
    // `Null`, then refused with E075; since #1530 it folds into a real
    // `ConstValue::Record`. This test keeps the `VAR p: Point = 0` +
    // assignment shape
    // `tm4c_structs_codegen.rs`'s `strict_and_gradual_produce_equivalent_
    // output_for_well_formed_program` already establishes (a scalar
    // placeholder default under a struct annotation doesn't trip E063 —
    // that fixture already proves it compiles clean under strict): shape
    // resolution for the static-offset decision is driven purely by the
    // TM-2 annotation (`structs::build_global_shape_map` reads
    // `var.annotation`, never the default expression), so the placeholder
    // default doesn't affect what this test actually checks.
    let src = "STRUCT Point = #{x: float, y: float}\nVAR p: Point = 0\n\
        ~ p = Point#{x: 1.0, y: 2.0}\n~ temp v = p.y\nHello.\n";
    let (program, diags) = lower_ink_with_type_mode(src, lir::TypeMode::Strict);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct-typed VAR under strict lowers cleanly");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp v := p.y should be a DeclareTemp");
    match value {
        lir::Expr::RecordGet { static_offset, .. } => {
            assert_eq!(*static_offset, Some(1), "y is field offset 1 in Point");
        }
        _ => panic!("expected RecordGet"),
    }
}

#[test]
fn gradual_mode_never_emits_static_offset_even_with_annotation() {
    // Same source as the strict test above, but under `types = gradual`
    // (the default) — the annotation is "optional seasoning" there, never
    // enforced, so trusting it for a static offset would be unsound (see
    // `expr::static_offset_for`'s doc). Must fall back to the by-name op.
    // Same fixture shape as
    // `strict_mode_known_shape_field_read_uses_static_offset` above — `p`
    // keeps a scalar placeholder default and the annotation stays for this
    // test's actual point (the annotation being *ignored* under gradual).
    let src = "STRUCT Point = #{x: float, y: float}\nVAR p: Point = 0\n\
        ~ p = Point#{x: 1.0, y: 2.0}\n~ temp v = p.y\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct-typed VAR under gradual lowers cleanly");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp v := p.y should be a DeclareTemp");
    match value {
        lir::Expr::RecordGet { static_offset, .. } => {
            assert_eq!(*static_offset, None);
        }
        _ => panic!("expected RecordGet"),
    }
}
