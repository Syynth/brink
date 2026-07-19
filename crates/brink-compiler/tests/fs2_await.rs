//! End-to-end FS-2 `await` compiler slice tests
//! (docs/flow-suspension-spec.md §3/§5, issue #928).
//!
//! Exercises the full pipeline through `brink_compiler::compile_with_options`,
//! proving the concrete consumer path a CLI/library caller reaches:
//! - `await` grammar → HIR (parse succeeds);
//! - strict-ink gate (`E051`) — `await` is a brink extension;
//! - the LIR lowering fence (`E052`) — every `await` construct is fenced until
//!   the FS-3 runtime lands;
//! - the effect-free purity gate (`E105`) — a condition that transitively
//!   writes a global (or performs an effectful call) is rejected, while a
//!   read-only condition passes the gate (it still hits the fence).

#![allow(clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_ir::DiagnosticCode;

fn compile_mem_with_dialect(
    source: &str,
    dialect: Dialect,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect,
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

// ── strict-ink rejects `await` (E051) ────────────────────────────────

#[test]
fn strict_ink_rejects_await_logic_line() {
    let source = "VAR gold = 0\n=== start ===\n~ await gold > 100\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E051
            && d.message.contains("brink extension")
            && d.message.contains("await")),
        "{diags:?}"
    );
}

#[test]
fn strict_ink_rejects_while_await() {
    let source =
        "VAR alarm = false\n=== start ===\n~ {\nwhile await alarm {\nalarm = false\n}\n}\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E051), "{diags:?}");
}

// ── brink fences every `await` at lowering (E052) ────────────────────

#[test]
fn brink_fences_await_logic_line() {
    let source = "VAR gold = 0\n=== start ===\n~ await gold > 100\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E052 && d.message.contains("await")),
        "{diags:?}"
    );
}

#[test]
fn brink_fences_await_inside_block() {
    let source = "VAR gold = 0\n=== start ===\n~ {\nawait gold > 100\n}\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E052), "{diags:?}");
}

#[test]
fn brink_fences_while_await() {
    let source =
        "VAR alarm = false\n=== start ===\n~ {\nwhile await alarm {\nalarm = false\n}\n}\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E052), "{diags:?}");
}

// ── the purity gate (E105) ───────────────────────────────────────────

/// A read-only condition (`gold > 100`) passes the purity gate — it hits the
/// lowering fence (E052) but never the purity error (E105).
#[test]
fn brink_pure_condition_passes_purity_gate() {
    let source = "VAR gold = 0\n=== start ===\n~ await gold > 100\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        has_code(&diags, DiagnosticCode::E052),
        "expected fence: {diags:?}"
    );
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a read-only condition must not trip the purity gate: {diags:?}"
    );
}

/// A bare fn-value reference used as a dynamic condition
/// (`await ready`, no call syntax) is read-only by construction (spec §3) —
/// no E105.
#[test]
fn brink_bare_reference_condition_passes_purity_gate() {
    let source = "VAR ready = false\n=== start ===\n~ await ready\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a bare reference condition must not trip the purity gate: {diags:?}"
    );
}

/// A condition that transitively **writes** a global (calling a function that
/// assigns a VAR) is not effect-free → E105.
#[test]
fn brink_effectful_condition_writing_global_is_rejected() {
    let source = concat!(
        "VAR alarm = false\n",
        "=== function raise_alarm() ===\n",
        "~ alarm = true\n",
        "~ return true\n",
        "=== start ===\n",
        "~ await raise_alarm()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "an effectful condition must trip the purity gate: {diags:?}"
    );
}

/// Transitive write, two hops out: the condition calls `outer()`, which calls
/// `inner()`, which writes a global. The effect row is transitively closed, so
/// `outer`'s row carries the write even though `outer` never touches the global
/// directly — E105 must still fire (PR #935 review: transitive-write coverage).
#[test]
fn brink_effectful_condition_writing_global_two_hops_is_rejected() {
    let source = concat!(
        "VAR sirens = false\n",
        "=== function inner() ===\n",
        "~ sirens = true\n",
        "~ return true\n",
        "=== function outer() ===\n",
        "~ return inner()\n",
        "=== start ===\n",
        "~ await outer()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a two-hop transitive write must trip the purity gate: {diags:?}"
    );
}

/// An effectful call nested inside a **struct-construction** condition
/// (`await Flag#{on: raise_alarm()}`) must trip E105 — the field initializer is
/// evaluated, so its write is observable on re-evaluation. Regression for the
/// PR #935 review item: `Expr::StructLiteral` was a non-recursing leaf in the
/// purity walk, so this write slipped past the gate.
#[test]
fn brink_effectful_call_in_struct_literal_condition_is_rejected() {
    let source = concat!(
        "STRUCT Flag = #{on: bool}\n",
        "VAR sirens = false\n",
        "=== function raise_alarm() ===\n",
        "~ sirens = true\n",
        "~ return true\n",
        "=== start ===\n",
        "~ await Flag#{on: raise_alarm()}\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "an effectful call nested in a struct-construction condition must trip \
         the purity gate: {diags:?}"
    );
}

/// A condition calling a **pure** function (one that only reads a global)
/// stays read-only → no E105 (still fenced by E052).
#[test]
fn brink_condition_calling_pure_function_passes_purity_gate() {
    let source = concat!(
        "VAR alarm = false\n",
        "=== function alarm_raised() ===\n",
        "~ return alarm\n",
        "=== start ===\n",
        "~ await alarm_raised()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a pure-function condition must not trip the purity gate: {diags:?}"
    );
}

/// NS-A6 (issue #1112, docs/stdlib-spec.md §7 — the ruled free
/// consequence): a wake condition calling a draw-bearing function is
/// excluded by the existing purity machinery, because the draw is an
/// ordinary write (to the RNG cell) in the callee's row. A re-evaluated
/// draw would be re-roll-unstable — E105 is the correct rejection.
#[test]
fn brink_draw_bearing_condition_is_rejected_by_the_purity_gate() {
    let source = concat!(
        "=== function lucky() ===\n",
        "~ return chance(0.5)\n",
        "=== start ===\n",
        "~ await lucky()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a draw-bearing condition must trip the purity gate: {diags:?}"
    );
}
