//! Arity + unknown-name checking for a divert-with-args site
//! (`-> knot(args)`), issue #2156.
//!
//! PR #2150 (issue #2136) wired native's `-> knot(args)` call-args syntax
//! into `DivertTarget::args` for the first time — before that, the shape
//! hard-failed `E129` on native and never reached any argument-checking
//! pass at all. This file proves the two questions #2156 asked:
//!
//! 1. **Arity** (`E176`, newly added): a divert-with-args call whose
//!    argument count disagrees with its resolved target's declared
//!    parameter count is flagged, on **both** dialects. Before this issue's
//!    fix, `brink_ir::symbols::project::Projector::walk_divert_target`
//!    hardcoded `arg_count: None` for every `RefKind::Divert` reference
//!    regardless of `DivertTarget::args.len()` — so
//!    `brink_analyzer::resolve::resolve_divert`'s arity check (gated on
//!    `arg_count.is_some()`, mirroring `resolve_function`'s `check_arity`)
//!    could never fire for a divert, on either dialect. Every arity test
//!    below fails if that hardcoded `None` is restored (verified by hand
//!    before this file was added).
//! 2. **Unknown target name** (`E024`, pre-existing): this was *already*
//!    reachable and already fires correctly for a divert-with-args site on
//!    both dialects, because `resolve_divert` pushes an `UnresolvedRef` and
//!    raises `E024` on failed resolution unconditionally, independent of
//!    `arg_count`/dialect. No new diagnostic code is needed for this half —
//!    `E177` (pre-assigned for this issue) is deliberately **not** used;
//!    the tests below are the evidence for why.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::{DiagnosticCode, FileId, SymbolManifest};

/// Lower one native (`.brink`) source through `hir::lower_native` and run
/// the analyzer's per-file `resolve` query, returning every diagnostic
/// `resolve` itself produces (arity/unknown-name — not lowering
/// diagnostics, which the caller asserts empty separately).
fn resolve_diags_native(src: &str) -> Vec<brink_ir::Diagnostic> {
    let parse = brink_syntax_native::parse(src);
    let (hir, manifest, lower_diags) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
    assert!(
        lower_diags.is_empty(),
        "fixture must lower cleanly: {lower_diags:?}"
    );
    assert!(hir.native, "lower_native must stamp HirFile::native");
    resolve_diags(&manifest)
}

/// The ink counterpart of [`resolve_diags_native`].
fn resolve_diags_ink(src: &str) -> Vec<brink_ir::Diagnostic> {
    let parsed = brink_syntax::parse(src);
    let (hir, manifest, lower_diags) = brink_ir::lower(FileId(0), &parsed.tree());
    assert!(
        lower_diags.is_empty(),
        "fixture must lower cleanly: {lower_diags:?}"
    );
    assert!(
        !hir.native,
        "the ink frontend must not stamp HirFile::native"
    );
    resolve_diags(&manifest)
}

fn resolve_diags(manifest: &SymbolManifest) -> Vec<brink_ir::Diagnostic> {
    let (index, _merge_diags) = brink_analyzer::symbol_index(&[(FileId(0), manifest)]);
    let (_resolutions, resolve_diags) = brink_analyzer::resolve(
        FileId(0),
        manifest,
        &index,
        &brink_analyzer::ImportScope::default(),
    );
    resolve_diags
}

// ─── Arity: native ──────────────────────────────────────────────────────

#[test]
fn native_divert_with_too_many_args_emits_e176() {
    let diags = resolve_diags_native("flow b(x, y) {\n  Bye.\n}\nflow a() {\n  -> b(1, 2, 3)\n}\n");
    let e176 = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E176)
        .unwrap_or_else(|| {
            panic!("expected E176 for a 3-arg divert against a 2-param target: {diags:?}")
        });
    assert!(e176.message.contains("expects 2"), "{}", e176.message);
    assert!(e176.message.contains("got 3"), "{}", e176.message);
}

#[test]
fn native_divert_with_too_few_args_emits_e176() {
    let diags = resolve_diags_native("flow b(x, y) {\n  Bye.\n}\nflow a() {\n  -> b(1)\n}\n");
    let e176 = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E176)
        .unwrap_or_else(|| {
            panic!("expected E176 for a 1-arg divert against a 2-param target: {diags:?}")
        });
    assert!(e176.message.contains("expects 2"), "{}", e176.message);
    assert!(e176.message.contains("got 1"), "{}", e176.message);
}

#[test]
fn native_divert_with_matching_args_emits_no_e176() {
    let diags = resolve_diags_native("flow b(x, y) {\n  Bye.\n}\nflow a() {\n  -> b(1, 2)\n}\n");
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E176),
        "matching arity must not warn: {diags:?}"
    );
}

#[test]
fn native_tunnel_call_with_wrong_arity_emits_e176() {
    // The same `lower_divert_target` helper backs `-> b(args) ->` tunnel
    // calls, not just a plain divert statement (mirrors
    // `tunnel_call_target_args_are_wired_through_not_dropped` in
    // `brink-ir`'s own lowering tests) — pin that the arity check applies
    // uniformly rather than only to the bare-divert call site.
    let diags = resolve_diags_native("flow b(x, y) {\n  Bye.\n}\nflow a() {\n  -> b(1) ->\n}\n");
    let e176 = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E176)
        .unwrap_or_else(|| {
            panic!("expected E176 for a 1-arg tunnel call against a 2-param target: {diags:?}")
        });
    assert!(e176.message.contains("expects 2"), "{}", e176.message);
    assert!(e176.message.contains("got 1"), "{}", e176.message);
}

#[test]
fn native_return_redirect_with_wrong_arity_emits_e176() {
    // Review finding on this PR (issue #2173): `return -> b(1, 2)` (native's
    // tunnel-return respell, charter §11) lowers to `Stmt::Return { kind:
    // TunnelRedirect, value: Some(Expr::DivertTarget(_)), onwards_args }` —
    // a third divert-with-args shape, distinct from the plain-divert and
    // tunnel-call ones pinned above, and one the generic `Expr::DivertTarget`
    // walk can never see the arg count for (the args live in
    // `Return::onwards_args`, not on the `DivertTarget` expression itself).
    // Pinned by `brink_ir::hir::lower_native::tests::
    // return_redirect_target_call_args_are_wired_through_not_dropped`.
    let diags =
        resolve_diags_native("flow b(x, y) {\n  Bye.\n}\nflow a() {\n  return -> b(1)\n}\n");
    let e176 = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E176)
        .unwrap_or_else(|| {
            panic!("expected E176 for a 1-arg return-redirect against a 2-param target: {diags:?}")
        });
    assert!(e176.message.contains("expects 2"), "{}", e176.message);
    assert!(e176.message.contains("got 1"), "{}", e176.message);
}

#[test]
fn native_return_redirect_with_matching_arity_emits_no_e176() {
    let diags =
        resolve_diags_native("flow b(x, y) {\n  Bye.\n}\nflow a() {\n  return -> b(1, 2)\n}\n");
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E176),
        "matching arity must not warn: {diags:?}"
    );
}

// ─── Arity: ink ─────────────────────────────────────────────────────────

#[test]
fn ink_divert_with_too_many_args_emits_e176() {
    let diags = resolve_diags_ink("=== b(x, y) ===\nBye.\n-> END\n\n=== a ===\n-> b(1, 2, 3)\n");
    let e176 = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E176)
        .unwrap_or_else(|| {
            panic!("expected E176 for a 3-arg divert against a 2-param target: {diags:?}")
        });
    assert!(e176.message.contains("expects 2"), "{}", e176.message);
    assert!(e176.message.contains("got 3"), "{}", e176.message);
}

#[test]
fn ink_divert_with_matching_args_emits_no_e176() {
    let diags = resolve_diags_ink("=== b(x, y) ===\nBye.\n-> END\n\n=== a ===\n-> b(1, 2)\n");
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E176),
        "matching arity must not warn: {diags:?}"
    );
}

// ─── Unknown target name: already-covered E024, both dialects ──────────

#[test]
fn native_divert_with_args_to_unknown_target_emits_e024_not_e177() {
    let diags = resolve_diags_native("flow a() {\n  -> nonexistent_knot(1, 2)\n}\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E024),
        "an unresolvable divert-with-args target must still raise E024: {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E176),
        "an unresolved target has no declared param count to check arity against: {diags:?}"
    );
}

#[test]
fn ink_divert_with_args_to_unknown_target_emits_e024_not_e177() {
    let diags = resolve_diags_ink("=== a ===\n-> nonexistent_knot(1, 2)\n");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E024),
        "an unresolvable divert-with-args target must still raise E024: {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E176),
        "an unresolved target has no declared param count to check arity against: {diags:?}"
    );
}

// The "arity check must not misfire on an indirect (Variable) target" case
// (a divert through a stored divert-target value, e.g. `-> some_var`) is
// covered at the `brink-analyzer::resolve` unit level instead of here —
// see `resolve::tests::divert_through_variable_is_not_arity_checked` — to
// avoid depending on exactly which native/ink expression syntax stores a
// divert-target value in a variable, which isn't otherwise pinned by this
// file's fixtures.
