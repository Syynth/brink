//! End-to-end TM-3 (#619) strict-policy tests (docs/typed-mode-spec.md
//! §1/§9-step-3). Mirrors `t1b_dialect_gate.rs`'s shape: exercises the full
//! pipeline through the public `brink_compiler::compile_with_options` entry
//! point — the concrete consumer path a CLI/library caller uses — proving
//! `AnalysisOptions::types` flows from the caller through `brink-driver` →
//! `brink-db`'s `lir_query` diagnostic gate → `CompileError::Diagnostics`,
//! and that a compile error under strict actually blocks `StoryData`
//! emission (not merely a reported warning).

#![allow(clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_ir::DiagnosticCode;

fn compile_mem(
    source: &str,
    dialect: Dialect,
    types: TypePolicy,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect,
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

#[test]
fn default_types_is_gradual() {
    // No `types` set — `AnalysisOptions::default()` — under `dialect =
    // brink`, an unannotated, never-narrowed param compiles clean (gradual
    // is the byte-identical-forever floor).
    let result = compile_mem(
        "=== noop(x) ===\nHello.\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_plus_strict_ink_dialect_is_a_targeted_config_error() {
    let err = compile_mem(
        "=== noop(x) ===\nHello.\n-> DONE\n",
        Dialect::StrictInk,
        TypePolicy::Strict,
    )
    .expect_err("strict + strict-ink must fail to compile");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E064),
        "{diags:?}"
    );
}

#[test]
fn strict_plus_brink_dialect_blocks_compilation_on_unknown_escape() {
    let err = compile_mem(
        "=== noop(x) ===\nHello.\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("an unannotated, unused param must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E065),
        "{diags:?}"
    );
}

#[test]
fn strict_clean_project_compiles_to_story_data() {
    let result = compile_mem(
        "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n\
         === main ===\nHello.\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_heterogeneous_collection_blocks_compilation() {
    // spec §5: `#[1, "a"]` is an error.
    let err = compile_mem(
        "=== main ===\n~ temp x = #[1, \"a\"]\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("heterogeneous collection literal must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E066),
        "{diags:?}"
    );
}

// ── Review fix 1: E063 is error-eligible under strict (#640-round ruling) ──

#[test]
fn strict_annotation_inference_mismatch_blocks_compilation_with_e063() {
    // A plain (non-function) knot, deliberately clean of every *other*
    // strict diagnostic: `x`'s annotation exempts it from Unknown-escape
    // (E065), and no return-type check applies (not a function knot) — the
    // only way this fixture can fail to compile is if E063 itself partitions
    // as an error. `x`'s annotation says `int`, but the body's own use
    // (`x + "!"`, string concatenation) forces a concrete `string` body type
    // — a genuine annotation-vs-inference disagreement, not an
    // Unknown/Conflicted body type (`mismatches()` skips unresolved body
    // types via `Ty::is_unresolved`, so the fixture must produce a
    // *concrete* disagreement to exercise E063 at all).
    //
    // Before this fix, `DiagnosticCode::E063.severity()` was hardcoded to
    // `Warning` and both `brink-db` partition sites split on that raw
    // severity — with no other error-severity diagnostic in this fixture,
    // `errors` would come out empty and this compile would have returned
    // `Ok` (with a warning), making `.expect_err` below panic. That is the
    // review's blocking finding.
    let err = compile_mem(
        "=== f(x: int) ===\n~ temp y = x + \"!\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "an annotation disagreeing with a concrete inferred body type must fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic \
         so E063 alone must be what fails compilation: {diags:?}"
    );
}

#[test]
fn gradual_annotation_inference_mismatch_still_compiles() {
    // Same fixture, `types` left at its default (`Gradual`) — E063 stays
    // advisory-only and never blocks compilation (the #618/PR#640 ruling
    // this issue explicitly does not touch).
    let result = compile_mem(
        "=== f(x: int) ===\n~ temp y = x + \"!\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Review fix 2: void-assignment is an error under strict (spec §3) ──────

#[test]
fn strict_void_assignment_blocks_compilation_with_e067() {
    let err = compile_mem(
        "=== function noop(): void ===\n~ return\n\
         === main ===\n~ temp x = noop()\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("assigning a void call's result must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E067),
        "{diags:?}"
    );
}

#[test]
fn strict_void_statement_position_call_compiles_clean() {
    // `~ f()` (no assignment) is never flagged — only the assignment/temp-
    // decl RHS-root shape is.
    let result = compile_mem(
        "=== function noop(): void ===\n~ return\n\
         === main ===\n~ noop()\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}
