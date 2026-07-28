//! NG-D exit-criterion tests: the `[1, 2, 3]` array/sequence literal
//! (issue #1490, RULED 2026-07-27 "`[1, 2, 3]`. Bracket literal on the
//! native surface").
//!
//! Lives as an integration test for the same reason
//! `b5_native_construction.rs` does — see that file's module doc for the
//! two-crate-instances explanation (`brink-analyzer` is a dev-dependency
//! that depends back on `brink-ir`).
//!
//! What these prove, in the ruling's own terms:
//!
//! - `[…]` lowers **directly** to `Expr::ArrayLiteral` — no dispatch layer,
//!   unlike the B5-symmetric `Array { … }` spelling the same ruling
//!   rejected (`brink_ir::hir::construct`'s registry is never consulted).
//! - It is the exact same HIR shape the brink dialect's `#[…]` sigil
//!   literal already produces (`hir::lower::expr::sigils`), so every
//!   dialect-agnostic analyzer pass that already generalizes over
//!   `Ty::Array`/`Expr::ArrayLiteral` picks this up with zero changes.
//! - The empty form `[]` and nesting both construct real values, not just
//!   parse.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::{Diagnostic, Expr, FileId, HirFile, SymbolManifest};

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

#[test]
fn array_literal_lowers_to_expr_array_literal() {
    let Expr::ArrayLiteral(a) = clean_var_initializer("var a = [1, 2, 3]\n") else {
        panic!("[…] must lower to Expr::ArrayLiteral");
    };
    assert_eq!(a.elements, vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]);
}

#[test]
fn empty_array_literal_lowers_to_an_empty_array_literal() {
    let Expr::ArrayLiteral(a) = clean_var_initializer("var a = []\n") else {
        panic!("[] must lower to Expr::ArrayLiteral");
    };
    assert!(a.elements.is_empty());
}

#[test]
fn array_literal_elements_are_lowered_expressions_not_just_integers() {
    // `var_initializer` only inspects the *first* `var` — a second `var`
    // referencing the first (`x`) proves elements are lowered through the
    // real expression grammar (a path, an infix op), not special-cased as
    // bare literals, without disturbing that single-var contract.
    let (hir, _manifest, diags) = lower_fixture("var a = [x, x + 1]\nvar x = 1\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let Expr::ArrayLiteral(a) = &hir
        .variables
        .first()
        .expect("fixture declares a var named `a` first")
        .value
    else {
        panic!("[…] must lower to Expr::ArrayLiteral");
    };
    assert_eq!(a.elements.len(), 2);
    assert!(matches!(a.elements[0], Expr::Path(_)));
    assert!(matches!(a.elements[1], Expr::Infix(_)));
}

#[test]
fn array_literals_nest() {
    let Expr::ArrayLiteral(outer) = clean_var_initializer("var a = [[1, 2], [3]]\n") else {
        panic!("outer must be Expr::ArrayLiteral");
    };
    assert_eq!(outer.elements.len(), 2);
    let Expr::ArrayLiteral(inner) = &outer.elements[0] else {
        panic!("nested element must also be Expr::ArrayLiteral");
    };
    assert_eq!(inner.elements, vec![Expr::Int(1), Expr::Int(2)]);
}

/// Not a construction-registry dispatch: unlike `TypeName { … }`, an array
/// literal never touches `brink_ir::hir::construct::ConstructTarget` — no
/// diagnostic fires for any element shape, since there is no registered
/// target to mismatch against.
#[test]
fn array_literal_never_dispatches_through_the_construct_registry() {
    let (_expr, diags) = var_initializer("var a = [\"a\", true, 1.5]\n");
    assert!(
        diags.is_empty(),
        "an array literal must never consult the construct registry: {diags:?}"
    );
}
