//! B5 exit-criterion tests: the `TypeName { … }` construction initializer
//! and the `construct` protocol registry (issue #1464; #1103 RULED
//! 2026-07-23 — `docs/decision-log.md` "Collection/construction
//! initializer", `docs/stdlib-spec.md` §9.6).
//!
//! Lives as an integration test for the same reason
//! `b06_native_declarations.rs` does: admission checking needs
//! `brink-analyzer`, a dev-dependency that depends back on `brink-ir` (see
//! that file's module doc for the two-crate-instances explanation).
//!
//! What these prove, in the ruling's own terms:
//!
//! - Construction is **dispatch, not grammar**: one CST shape
//!   (`CONSTRUCT_LITERAL`) reaches four different HIR shapes depending on
//!   what the `construct` registry says about the type name, and an
//!   unregistered name falls through to the declared-struct reading rather
//!   than erroring.
//! - The **std-only fence** holds: nothing outside
//!   `ConstructTarget::ALL` registers.
//! - Cascade ruling (A): duplicate map keys are a **compile error**
//!   (`E138`), reached through the analyzer on a real native file.
//! - Cascade ruling (B): only the **total** `Weighted { … }` literal
//!   exists — it desugars onto the existing `weighted(…)` intrinsic, which
//!   already faults on an invalid table.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::construct::{ConstructForm, ConstructTarget};
use brink_ir::hir::lower_native;
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, StringPart, SymbolManifest};

fn lower_fixture(src: &str) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    lower_native::lower(FileId(0), &parse.tree())
}

/// The initializer expression of the fixture's single `var` declaration —
/// the shortest real `.brink` path from source to `lower_native::expr`.
fn var_initializer(src: &str) -> (Expr, Vec<Diagnostic>) {
    let (hir, _manifest, diags) = lower_fixture(src);
    let value = hir
        .variables
        .first()
        .expect("fixture declares one var")
        .value
        .clone();
    (value, diags)
}

fn clean_var_initializer(src: &str) -> Expr {
    let (expr, diags) = var_initializer(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    expr
}

fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

// ── Dispatch: one grammar, four HIR shapes ──────────────────────────

#[test]
fn map_constructs_a_map_literal() {
    let Expr::MapLiteral(m) = clean_var_initializer("var m = Map { \"a\": 1, \"b\": 2 }\n") else {
        panic!("Map {{ … }} must lower to Expr::MapLiteral");
    };
    assert_eq!(m.entries.len(), 2);
    let (key, value) = &m.entries[0];
    assert!(matches!(key, Expr::String(s) if s.parts == vec![StringPart::Literal("a".into())]));
    assert_eq!(*value, Expr::Int(1));
}

#[test]
fn flags_constructs_a_list_literal() {
    let Expr::ListLiteral(items) = clean_var_initializer("var f = Flags { calm, wary }\n") else {
        panic!("Flags {{ … }} must lower to Expr::ListLiteral");
    };
    let names: Vec<String> = items
        .iter()
        .map(|p| {
            p.segments
                .iter()
                .map(|s| s.text.clone())
                .collect::<Vec<_>>()
                .join(".")
        })
        .collect();
    assert_eq!(names, vec!["calm".to_string(), "wary".to_string()]);
}

/// Cascade ruling (B): the **total** literal desugars onto the existing
/// `weighted(w, v, …)` flattened-pair intrinsic — construction is a
/// protocol over the value model that already exists, not a new node.
#[test]
fn weighted_constructs_the_total_weighted_call() {
    let Expr::Call(path, args) =
        clean_var_initializer("var w = Weighted { 3: \"gold\", 1: \"iron\" }\n")
    else {
        panic!("Weighted {{ … }} must lower to the `weighted(…)` intrinsic call");
    };
    assert_eq!(
        path.segments.iter().map(|s| &s.text).collect::<Vec<_>>(),
        vec!["weighted"]
    );
    assert_eq!(args.len(), 4, "flattened weight/value row: {args:?}");
    assert_eq!(args[0], Expr::Int(3));
    assert_eq!(args[2], Expr::Int(1));
}

/// The std-only fence's other half: an *unregistered* name is not an error,
/// it is the declared-struct reading the compiler already had. User types
/// do not register this round — they simply keep working.
#[test]
fn an_unregistered_type_name_constructs_a_struct_literal() {
    let Expr::StructLiteral(sl) = clean_var_initializer("var p = Point { x: 1, y: 2 }\n") else {
        panic!("an unregistered name must lower to Expr::StructLiteral");
    };
    assert_eq!(sl.shape.text, "Point");
    assert_eq!(
        sl.fields
            .iter()
            .map(|(n, _)| n.text.clone())
            .collect::<Vec<_>>(),
        vec!["x".to_string(), "y".to_string()]
    );
}

#[test]
fn a_qualified_registered_name_dispatches_to_the_same_entry() {
    assert!(matches!(
        clean_var_initializer("var m = std::map::Map { \"a\": 1 }\n"),
        Expr::MapLiteral(_)
    ));
}

// ── The empty and nested forms ──────────────────────────────────────

#[test]
fn empty_literals_construct_empty_values_in_every_form() {
    assert!(matches!(
        clean_var_initializer("var m = Map { }\n"),
        Expr::MapLiteral(m) if m.entries.is_empty()
    ));
    assert!(matches!(
        clean_var_initializer("var f = Flags { }\n"),
        Expr::ListLiteral(items) if items.is_empty()
    ));
    assert!(matches!(
        clean_var_initializer("var p = Point { }\n"),
        Expr::StructLiteral(sl) if sl.fields.is_empty()
    ));
}

#[test]
fn construction_literals_nest() {
    let Expr::MapLiteral(m) = clean_var_initializer("var m = Map { \"p\": Point { x: 1 } }\n")
    else {
        panic!("outer must be a map literal");
    };
    assert!(matches!(m.entries[0].1, Expr::StructLiteral(_)));
}

// ── Form mismatch is `E139`, at dispatch, not in the parser ─────────

#[test]
fn element_entries_for_a_key_value_target_are_e139() {
    let (expr, diags) = var_initializer("var m = Map { a, b }\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E139], "{diags:?}");
    assert_eq!(expr, Expr::Null);
}

#[test]
fn key_value_entries_for_an_element_target_are_e139() {
    let (_expr, diags) = var_initializer("var f = Flags { calm: 1 }\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E139], "{diags:?}");
}

#[test]
fn a_flags_element_that_is_not_a_name_is_e139() {
    let (_expr, diags) = var_initializer("var f = Flags { 1 }\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E139], "{diags:?}");
}

#[test]
fn a_struct_field_key_that_is_not_a_bare_name_is_e139() {
    for src in ["var p = Point { 1: 2 }\n", "var p = Point { a.b: 2 }\n"] {
        let (_expr, diags) = var_initializer(src);
        assert_eq!(
            codes(&diags),
            vec![DiagnosticCode::E139],
            "{src}: {diags:?}"
        );
    }
}

// ── The registry fence itself ───────────────────────────────────────

/// Every registry entry must actually dispatch — no entry may exist that
/// the lowering ignores, and no lowering arm may exist for a name that is
/// not in the registry (the unregistered case is the struct fall-through,
/// asserted above).
#[test]
fn every_registry_entry_dispatches_to_a_distinct_hir_shape() {
    for target in ConstructTarget::ALL {
        let body = match target.form() {
            ConstructForm::Pair => "1: 2",
            ConstructForm::Element => "one",
        };
        let src = format!("var v = {} {{ {body} }}\n", target.type_name());
        let (expr, diags) = var_initializer(&src);
        assert!(diags.is_empty(), "{src}: unexpected diagnostics: {diags:?}");
        let shape = match expr {
            Expr::MapLiteral(_) => "map",
            Expr::ListLiteral(_) => "list",
            Expr::Call(_, _) => "call",
            other => panic!("{src}: unexpected HIR shape {other:?}"),
        };
        let expected = match target {
            ConstructTarget::Map => "map",
            ConstructTarget::Flags => "list",
            ConstructTarget::Weighted => "call",
        };
        assert_eq!(shape, expected, "{src}");
    }
}

// ── Cascade ruling (A): duplicate map keys are a compile error ──────

/// Reached the way a user reaches it: a real `.brink` file through
/// `brink_analyzer::per_file_diagnostics` with `is_native = true`. The
/// `dialect` axis is left at its default (`StrictInk`) on purpose — that
/// is what a native project carries unless it opts in, and the error must
/// still fire (see `per_file_diagnostics`' own comment on this wiring).
fn native_diagnostics(src: &str) -> Vec<Diagnostic> {
    let (hir, manifest, lower_diags) = lower_fixture(src);
    assert!(lower_diags.is_empty(), "lowering: {lower_diags:?}");
    let files = vec![(FileId(0), &hir, &manifest)];
    let analysis = brink_analyzer::analyze(&files);
    brink_analyzer::per_file_diagnostics(
        FileId(0),
        &hir,
        &analysis.resolutions,
        &analysis.index,
        brink_analyzer::Dialect::default(),
        true,
        None,
    )
}

#[test]
fn a_duplicate_map_key_is_e138_on_a_native_file() {
    let diags = native_diagnostics("var m = Map { \"a\": 1, \"a\": 2 }\n");
    let dup: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E138)
        .collect();
    assert_eq!(dup.len(), 1, "expected exactly one E138: {diags:?}");
}

#[test]
fn duplicate_keys_are_caught_across_the_whole_in_domain_key_set() {
    for src in [
        "var m = Map { 1: \"a\", 1: \"b\" }\n",
        "var m = Map { true: 1, true: 2 }\n",
        "var m = Map { \"k\": 1, \"k\": 2 }\n",
    ] {
        let diags = native_diagnostics(src);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E138),
            "{src}: expected E138, got {diags:?}"
        );
    }
}

#[test]
fn distinct_keys_of_the_same_kind_are_not_duplicates() {
    let diags = native_diagnostics("var m = Map { 1: \"a\", 2: \"b\", true: 3, \"1\": 4 }\n");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E138),
        "no key repeats here: {diags:?}"
    );
}

/// "Unknown never disagrees": a key the compiler cannot compare statically
/// (a variable, an interpolated string) is left to the runtime rather than
/// guessed at — the same posture the key-domain check takes.
#[test]
fn dynamic_keys_are_not_reported_as_duplicates() {
    let diags = native_diagnostics("var m = Map { k: 1, k: 2 }\n");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E138),
        "a non-literal key is not statically comparable: {diags:?}"
    );
}
