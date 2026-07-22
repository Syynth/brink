//! End-to-end NS-A4 ordering-verb compiler tests
//! (`docs/stdlib-spec.md` §4b, issue #1110).
//!
//! Exercises the full pipeline through `brink_compiler::compile_with_options`:
//! - the F0 statement/expression split — imperative `sort`/`sort_by` are
//!   statement-only (`E056` in expression position, `E055` on an rvalue
//!   receiver, `E058` on arity), `sorted`/`sorted_by` are expressions;
//! - the E119 comparator-contract gate — a provably impure/unsilent inline
//!   `#fn(target)` comparator is rejected; a pure one passes; an opaque
//!   comparator (a variable) is not *proven* and passes (the exceedance-only
//!   posture — the VM's isolation + `ComparatorEscaped` fault are the
//!   runtime residual).

#![allow(clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_ir::DiagnosticCode;

fn compile_brink(
    source: &str,
    types: Option<TypePolicy>,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}

fn diagnostics_of(err: brink_compiler::CompileError) -> Vec<brink_compiler::ResolvedDiagnostic> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags,
        other => panic!("expected Diagnostics error, got {other:?}"),
    }
}

fn has_code(diags: &[brink_compiler::ResolvedDiagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == code)
}

// ── the F0 statement/expression split ────────────────────────────────

#[test]
fn sort_in_expression_position_is_e056() {
    let source = "~ temp a = #[2, 1]\n~ temp b = sort(a)\n-> END\n";
    let err = compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err();
    assert!(has_code(&diagnostics_of(err), DiagnosticCode::E056));
}

#[test]
fn sort_by_in_expression_position_is_e056() {
    let source = "~ temp a = #[2, 1]\n~ temp b = sort_by(a, #fn(cmp))\n-> END\n\n=== function cmp(x, y) ===\n~ return 0\n";
    let err = compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err();
    assert!(has_code(&diagnostics_of(err), DiagnosticCode::E056));
}

#[test]
fn sort_on_an_rvalue_receiver_is_e055() {
    let source = "~ sort(#[2, 1])\n-> END\n";
    let err = compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err();
    assert!(has_code(&diagnostics_of(err), DiagnosticCode::E055));
}

#[test]
fn sort_arity_mismatch_is_e058() {
    let source = "~ temp a = #[2, 1]\n~ sort(a, 1)\n-> END\n";
    let err = compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err();
    assert!(has_code(&diagnostics_of(err), DiagnosticCode::E058));
}

#[test]
fn sorted_expression_and_sort_statement_compile_clean() {
    let source = "~ temp a = #[2, 1]\n~ sort(a)\n~ temp b = sorted(a)\nsorted: {b}.\n-> END\n";
    compile_brink(source, Some(TypePolicy::Gradual)).expect("clean sort/sorted must compile");
}

// ── E119: the comparator contract (stdlib-spec §4b, exceedance-only) ──

#[test]
fn writing_comparator_is_e119() {
    let source = "VAR seen = 0\n~ temp a = #[2, 1]\n~ sort_by(a, #fn(spy))\n-> END\n\n=== function spy(x: int, y: int): int ===\n~ seen = seen + 1\n~ return x - y\n";
    let err = compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E119), "{diags:?}");
}

#[test]
fn emitting_comparator_is_e119() {
    let source = "~ temp a = #[2, 1]\n~ temp b = sorted_by(a, #fn(loud))\n{b}\n-> END\n\n=== function loud(x: int, y: int): int ===\ncomparing!\n~ return x - y\n";
    let err = compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E119), "{diags:?}");
}

#[test]
fn pure_comparator_passes_the_gate() {
    let source = "~ temp a = #[2, 1]\n~ sort_by(a, #fn(cmp))\n{a}\n-> END\n\n=== function cmp(x: int, y: int): int ===\n~ return y - x\n";
    compile_brink(source, None).expect("a pure comparator must pass E119 (strict default)");
}

#[test]
fn opaque_comparator_is_not_proven_and_passes() {
    // The comparator arrives through a variable, not an inline `#fn` —
    // E119 is exceedance-only and must not fire on what it cannot prove
    // (the runtime residual covers it). The *row* still degrades via the
    // pending-value-call machinery, which is the ⊕cmp posture, but that is
    // not a diagnostic.
    let source = "VAR seen = 0\n~ temp a = #[2, 1]\n~ temp f = #fn(spy)\n~ sort_by(a, f)\n-> END\n\n=== function spy(x: int, y: int): int ===\n~ seen = seen + 1\n~ return x - y\n";
    compile_brink(source, Some(TypePolicy::Gradual))
        .expect("an opaque comparator is not provably in violation");
}

// ── strict-ink: the verbs are brink-dialect surface ──────────────────

#[test]
fn strict_ink_never_reaches_the_sort_machinery() {
    // Under strict-ink the sigil literal is already rejected and the verb
    // names stay ordinary unresolved calls — the A4 surface is
    // vanilla-unreachable (the oracle-safety property). Any diagnostics
    // are fine; compiling *successfully into sort opcodes* would not be.
    let source = "~ temp a = 1\n~ sort(a)\n-> END\n";
    let result = compile_brink_strict_ink(source);
    assert!(result.is_err(), "strict-ink must reject the brink verb");
}

fn compile_brink_strict_ink(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect: Dialect::StrictInk,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}
