//! The native `@[allow(Exxx, …)]` source-level suppression channel
//! (issue #1161).
//!
//! `@[allow]` is the third tenant of the native `@[…]` annotation channel
//! wired by issue #1563 (`hir::lower_native::annotation`), alongside
//! `effects` and the file-level `was`. It records a
//! `(declaration span, codes)` scope on `HirFile::allow_scopes`, which
//! `brink_ir::suppressions::apply_suppressions` — the same filter the
//! `//brink-disable` comment channel already flows through — consumes at
//! every diagnostic consumer.
//!
//! These tests pin the *lowering* half: what becomes a scope, what span the
//! scope covers, and the three ways a malformed `@[allow]` is rejected. The
//! filter half lives in `brink-ir`'s `suppressions` unit tests, and the
//! end-to-end compile behaviour (including the source-`allow`-beats-project-
//! `deny` ruling) in `brink-compiler`'s `e0xx_diagnostics.rs`.
//!
//! Integration test rather than an in-`src` `mod tests` for the same reason
//! `b06_native_annotations.rs` is one: it links the already-built
//! `brink-ir` rlib, so `brink-analyzer` (a dev-dependency that depends back
//! on `brink-ir`) stays type-compatible.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_ir::hir::lower_native;
use brink_ir::suppressions::AllowScope;
use brink_ir::{DiagnosticCode, FileId, HirFile};

fn lower(src: &str) -> (HirFile, Vec<brink_ir::Diagnostic>) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, _manifest, diags) = lower_native::lower(FileId(0), &parse.tree());
    (hir, diags)
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

/// The scopes of a fixture that must lower without any diagnostic.
fn clean_scopes(src: &str) -> Vec<AllowScope> {
    let (hir, diags) = lower(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    hir.allow_scopes
}

/// The source text a scope covers — the sharpest way to assert the span,
/// since a scope is only ever meaningful relative to the file it indexes.
fn covered<'a>(src: &'a str, scope: &AllowScope) -> &'a str {
    let start: usize = scope.range.start().into();
    let end: usize = scope.range.end().into();
    &src[start..end]
}

// ── What becomes a scope, and how wide ───────────────────────────────

/// The headline case: one warning code, one `fn`, and the scope covers the
/// whole declaration — head *and* body, since a diagnostic about the body is
/// exactly what an author writing this wants silenced.
#[test]
fn allow_above_a_fn_scopes_the_whole_declaration() {
    let src = "@[allow(E014)]\nfn heal(amount) {\n  return amount;\n}\n";
    let scopes = clean_scopes(src);
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(scopes[0].codes, [DiagnosticCode::E014]);
    assert_eq!(
        covered(src, &scopes[0]),
        "fn heal(amount) {\n  return amount;\n}"
    );
}

/// Several codes in one annotation, kept in the order written (the record is
/// a `Vec`, and the filter is a membership test — order is only ever a
/// determinism property, never a semantic one).
#[test]
fn allow_records_every_code_in_source_order() {
    let scopes = clean_scopes("@[allow(E151, E014, E035)]\nflow duel() {\n  Steel.\n}\n");
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(
        scopes[0].codes,
        [
            DiagnosticCode::E151,
            DiagnosticCode::E014,
            DiagnosticCode::E035
        ]
    );
}

/// Unlike `@[effects(…)]`, which only attaches to a `flow`/`fn` head some
/// container actually lowers, a suppression scope attaches to *any*
/// declaration — the scope is a span fact, not a container property.
#[test]
fn allow_attaches_to_a_var_declaration() {
    let src = "@[allow(E014)]\nvar gold = 0\n\nflow main() {\n  Coins.\n}\n";
    let scopes = clean_scopes(src);
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(covered(src, &scopes[0]), "var gold = 0");
}

/// A nested `flow` (the `Stitch` level) is reached by the whole-tree walk,
/// and its scope is the inner declaration only — not the enclosing flow.
#[test]
fn allow_inside_a_body_scopes_only_the_inner_declaration() {
    let src = "flow main() {\n  Outer.\n  @[allow(E151)]\n  flow inner() {\n    Inner.\n  }\n  -> inner\n}\n";
    let scopes = clean_scopes(src);
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(covered(src, &scopes[0]), "flow inner() {\n    Inner.\n  }");
}

/// Two annotations, two independent scopes — including a nested one whose
/// span sits strictly inside the outer one.
#[test]
fn nested_allows_produce_independent_scopes() {
    let src = "@[allow(E014)]\nflow main() {\n  @[allow(E151)]\n  flow inner() {\n    Inner.\n  }\n  -> inner\n}\n";
    let scopes = clean_scopes(src);
    assert_eq!(scopes.len(), 2, "{scopes:?}");
    assert_eq!(scopes[0].codes, [DiagnosticCode::E014]);
    assert_eq!(scopes[1].codes, [DiagnosticCode::E151]);
    assert!(
        scopes[0].range.contains_range(scopes[1].range),
        "the outer scope must contain the inner one: {scopes:?}"
    );
}

/// The annotation line itself is *outside* the scope it creates, so an
/// `@[allow(E153)]`-style self-silencing trick can never work — and neither
/// can an author accidentally hide a diagnostic reported on the directive.
#[test]
fn the_annotation_line_is_outside_its_own_scope() {
    let src = "@[allow(E014)]\nfn heal() {\n  return;\n}\n";
    let scopes = clean_scopes(src);
    let start: usize = scopes[0].range.start().into();
    assert!(
        start >= src.find("fn heal").unwrap(),
        "scope must begin at the declaration, not the annotation: {scopes:?}"
    );
}

/// `@[allow]` coexists with `@[effects]` on the same declaration — the two
/// tenants read the same attached run and neither reports the other.
#[test]
fn allow_and_effects_coexist_on_one_declaration() {
    let src = "@[allow(E014)]\n@[effects(pure)]\nfn double(n) {\n  return n * 2;\n}\n";
    let (hir, diags) = lower(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    assert_eq!(hir.allow_scopes.len(), 1, "{:?}", hir.allow_scopes);
    assert!(hir.knots[0].effects_assertion.as_ref().unwrap().pure);
}

// ── The three rejections ─────────────────────────────────────────────

/// A typo'd code is `E153` and records nothing. This is the whole point of
/// the reserved-`@`-namespace rule: a suppression that silently does nothing
/// is the failure mode the directive exists to avoid.
#[test]
fn an_unknown_code_is_a_hard_error_and_records_no_scope() {
    let (hir, diags) = lower("@[allow(E1511)]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E153], "{diags:?}");
    assert!(hir.allow_scopes.is_empty(), "{:?}", hir.allow_scopes);
}

/// A non-code identifier lands on the same `E153` — `@[allow(dead_code)]` is
/// a Rust habit, not a brink one.
#[test]
fn a_non_code_identifier_is_also_an_unknown_code() {
    let (_, diags) = lower("@[allow(dead_code)]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E153], "{diags:?}");
}

/// Errors are NOT suppressible: an `Error`-default code is `E154`. Without
/// this an author could silence a real compile failure and ship a broken
/// artifact — the same reason `[lints]` refuses to re-level error-tier codes
/// (issue #1160).
#[test]
fn an_error_severity_code_is_not_suppressible() {
    let (hir, diags) = lower("@[allow(E103)]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E154], "{diags:?}");
    assert!(hir.allow_scopes.is_empty(), "{:?}", hir.allow_scopes);
}

/// Issue #1617 moved `E095`'s *default* severity to `Hint` (previously
/// `Warning`). Suppressibility is gated on "not `Error`", not "exactly
/// `Warning`" (see `parse_allow`'s doc comment) — an `@[allow(E095)]` an
/// author already had in source before #1617 landed must keep working
/// exactly as before, not start tripping `E154` the moment the code's
/// default tier moved underneath it.
#[test]
fn a_hint_default_code_is_still_suppressible() {
    let scopes = clean_scopes("@[allow(E095)]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(scopes[0].codes, [DiagnosticCode::E095]);
}

/// The B0.3 admission-validator family (`docs/hir-admission-contract.md`
/// §4.2) is exempt by the issue's own wording. It needs no special case:
/// every one of `E121`–`E128` is `Error`-severity, so the blanket rule above
/// already rejects it — *and* admission diagnostics never route through
/// `apply_suppressions` at all (`ProjectDb::admission_diagnostics` is its own
/// channel), so the exemption holds twice over.
#[test]
fn admission_validator_codes_are_not_suppressible() {
    for code in [
        "E121", "E122", "E123", "E124", "E125", "E126", "E127", "E128",
    ] {
        let src = format!("@[allow({code})]\nflow main() {{\n  Steel.\n}}\n");
        let (hir, diags) = lower(&src);
        assert_eq!(
            codes(&diags),
            vec![DiagnosticCode::E154],
            "{code}: {diags:?}"
        );
        assert!(
            hir.allow_scopes.is_empty(),
            "{code}: {:?}",
            hir.allow_scopes
        );
    }
}

/// One bad code poisons the whole directive — a partially-applied
/// suppression would silence some codes while the author believes all of
/// them are handled.
#[test]
fn a_single_bad_code_discards_the_whole_directive() {
    let (hir, diags) = lower("@[allow(E014, E103)]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E154], "{diags:?}");
    assert!(hir.allow_scopes.is_empty(), "{:?}", hir.allow_scopes);
}

/// A bare `@[allow]` with no argument list at all.
#[test]
fn a_bare_allow_is_malformed() {
    let (hir, diags) = lower("@[allow]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E155], "{diags:?}");
    assert!(hir.allow_scopes.is_empty(), "{:?}", hir.allow_scopes);
}

/// An empty argument list — parses, silences nothing.
#[test]
fn an_empty_allow_is_malformed() {
    let (_, diags) = lower("@[allow()]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E155], "{diags:?}");
}

/// A quoted code is malformed, not an unknown code: the argument grammar is
/// bare identifiers (`@[was("…")]` is the one string-taking tenant).
#[test]
fn a_quoted_code_is_malformed() {
    let (_, diags) = lower("@[allow(\"E014\")]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E155], "{diags:?}");
}

/// A nested clause is malformed too — `allow` takes a flat list, unlike
/// `effects`'s `reads(…)`/`writes(…)`/`calls(…)`.
#[test]
fn a_nested_clause_is_malformed() {
    let (_, diags) = lower("@[allow(reads(E014))]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E155], "{diags:?}");
}

// ── Placement ────────────────────────────────────────────────────────

/// A trailing `@[allow]` with no declaration after it is `E112` (the
/// channel's own misplacement code, reported once by the erasure
/// chokepoint), and records no scope — not a second report from the
/// suppression pass.
#[test]
fn a_trailing_allow_is_misplaced_and_reported_once() {
    let (hir, diags) = lower("flow main() {\n  Steel.\n}\n\n@[allow(E014)]\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E112], "{diags:?}");
    assert!(hir.allow_scopes.is_empty(), "{:?}", hir.allow_scopes);
}

/// `allow` did not widen the channel's name set for anything else: an
/// unruled name is still `E111`.
#[test]
fn an_unknown_annotation_name_is_still_e111() {
    let (_, diags) = lower("@[deny(E014)]\nflow main() {\n  Steel.\n}\n");
    assert_eq!(codes(&diags), vec![DiagnosticCode::E111], "{diags:?}");
}

// ── Statement-position attachment ──────────────────────────────────────
//
// `is_consumed_position`'s `ALLOW` arm is `attached_declaration(line).
// is_some()` — the next sibling of *any* kind, not only a declaration head.
// So `@[allow(…)]` above a plain content line inside a body is accepted
// (not `E112`) and scopes exactly that one statement, matching issue
// #1161's own wording ("on a declaration/statement") and
// directive-annotations-spec.md §5d as widened alongside this test.

/// `@[allow(E014)]` directly above a content line — not a declaration head
/// at all — is a well-formed, statement-scoped suppression: no `E112`, one
/// scope recorded.
#[test]
fn allow_attaches_to_a_plain_statement_not_only_a_declaration() {
    let src = "flow main() {\n  @[allow(E014)]\n  Steel.\n  Copper.\n}\n";
    let scopes = clean_scopes(src);
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(scopes[0].codes, [DiagnosticCode::E014]);
}

/// The statement-position scope covers only the annotated statement, not
/// its unannotated siblings — same "attaches to the next sibling only"
/// contract a declaration-position scope has, just proven on a content line
/// instead of a `flow`/`fn`/`var` head.
#[test]
fn allow_on_a_statement_scopes_only_that_statement() {
    let src = "flow main() {\n  @[allow(E014)]\n  Steel.\n  Copper.\n}\n";
    let scopes = clean_scopes(src);
    assert_eq!(scopes.len(), 1, "{scopes:?}");

    let text = covered(src, &scopes[0]);
    assert!(
        text.contains("Steel."),
        "scope should cover Steel.: {text:?}"
    );
    assert!(
        !text.contains("Copper."),
        "scope should not reach the sibling statement: {text:?}"
    );
}
