//! Strict typed-mode (`types = strict`) sweep of native fixtures from
//! `crates/brink-compiler/tests/driver.rs` — issue #1916.
//!
//! # Why this file exists
//!
//! `driver.rs`'s `native_or_coalescing_*`, `native_as_binding_*`, and
//! `native_bare_name_fn_value_*` test families compile fixtures via
//! `compile_and_run_native`, which uses `brink_compiler::compile_path`
//! (default `AnalysisOptions` — `types = None`, resolving to `Gradual` for
//! `Dialect::StrictInk`). The strict type checker therefore never runs on
//! these fixtures, even though a real `.brink` project with `dialect =
//! "brink"` in its `brink.toml` gets `Strict` by default.
//!
//! This file compiles the same fixtures under explicit `types = strict` to
//! audit and triage the findings, closing the coverage gap issue #1916 named.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, CompileError, DiagnosticCode, Dialect, TypePolicy};
use std::fs;

/// Compile a `.brink` fixture from source under strict typing.
fn compile_native_strict(
    dir_suffix: &str,
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let dir = std::env::temp_dir().join(format!(
        "brink-driver-strict-{dir_suffix}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("main.brink"), source).unwrap();
    let result = brink_compiler::compile_path_with_options(
        &dir.join("main.brink"),
        AnalysisOptions {
            dialect: Dialect::Brink,
            types: Some(TypePolicy::Strict),
            ..AnalysisOptions::default()
        },
    );
    fs::remove_dir_all(&dir).ok();
    result
}

/// Collect strict-pass findings (E063/E065/E066 diagnostics) from a compile result.
fn strict_findings(
    fixture_name: &str,
    result: Result<brink_compiler::CompileOutput, brink_compiler::CompileError>,
) -> Vec<(String, String, String)> {
    let mut findings = Vec::new();
    let diagnostics = match result {
        Ok(output) => output.warnings,
        Err(CompileError::Diagnostics(ds)) => ds,
        Err(e) => panic!("{fixture_name}: unexpected compile failure: {e}"),
    };
    for d in diagnostics {
        if matches!(
            d.code,
            DiagnosticCode::E063 | DiagnosticCode::E065 | DiagnosticCode::E066
        ) {
            findings.push((
                fixture_name.to_string(),
                d.code.as_str().to_string(),
                d.message,
            ));
        }
    }
    findings
}

/// Baseline of strict findings expected from driver.rs fixtures.
/// Grouped by fixture, classified as expected or gaps.
///
/// These fixtures are extracted from driver.rs's native_* test families,
/// which deliberately use gradual-mode, unannotated code to test runtime
/// semantics. Under strict typing, their Unknown parameters and locals are
/// expected. All findings below are **expected** (real code that needs
/// annotation to be strict-clean), not checker gaps.
///
/// Group A — as-binding with a literal None (as-binding-statement):
/// **Expected.** `if none as n` binds the payload of a literal `none`,
/// which has no payload type, so `n` escapes as Unknown. This is the same
/// shape `tier1_native_strict.rs` classifies as "expected" (Group E).
///
/// Group B — unannotated function parameters (bare-name-fn-value-plain):
/// **Expected.** `apply(g, v)` is written without type annotations, so
/// under strict mode both parameters and the return type escape as Unknown.
/// This mirrors `tier1_native_strict.rs`'s Group F (unannotated helpers whose
/// bodies don't constrain parameters). The call-site error on `g` is a
/// consequence — the parameter type is Unknown, so calling it as a function
/// is an error until the parameter is annotated.
const BASELINE: &[(&str, &str, &str)] = &[
    // Group A
    (
        "as-binding-statement",
        "E065",
        "`absent`'s temp `n` escapes strict inference as Unknown — annotate or restructure",
    ),
    // Group B
    (
        "bare-name-fn-value-plain",
        "E065",
        "`apply`'s parameter `g` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "bare-name-fn-value-plain",
        "E065",
        "`apply`'s parameter `v` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "bare-name-fn-value-plain",
        "E065",
        "`apply`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "bare-name-fn-value-plain",
        "E065",
        "`g` is called as a function value but its type escapes strict inference as Unknown \
         — annotate (`fn(T…): R`) or restructure",
    ),
];

/// Gate: driver.rs fixture findings under strict typing must match baseline.
#[test]
fn strict_findings_match_recorded_baseline() {
    let mut actual = Vec::new();

    // Compile each driver.rs fixture under strict typing and collect findings.
    // These fixture sources are extracted from crates/brink-compiler/tests/driver.rs.

    // native_or_coalescing_collapse_form_unwraps_some_and_falls_back_on_none
    let result = compile_native_strict(
        "or-coalesce-collapse",
        "flow main() {\n  Some case: {some(5) or 99}\n  None case: {none or 99} -> END\n}\n",
    );
    actual.extend(strict_findings("or-coalescing-collapse", result));

    // native_or_coalescing_chain_falls_through_to_final_fallback
    let result = compile_native_strict(
        "or-coalesce-chain",
        "flow main() {\n  Chained: {none or none or 7} -> END\n}\n",
    );
    actual.extend(strict_findings("or-coalescing-chain", result));

    // native_as_binding_statement_form_binds_payload_and_falls_to_else
    let result = compile_native_strict(
        "as-binding-stmt",
        "fn present() {\n  if some(41) as n {\n    return n + 1;\n  }\n  return 0;\n}\n\
         fn absent() {\n  if none as n {\n    return n;\n  }\n  return -7;\n}\n\
         flow main() {\n  Present: {present()}\n  Absent: {absent()} -> END\n}\n",
    );
    actual.extend(strict_findings("as-binding-statement", result));

    // native_bare_name_fn_value_without_ref_params_compiles_and_runs
    let result = compile_native_strict(
        "bare-name-fnvalue-plain",
        "fn double(x) {\n  return x * 2;\n}\n\
         fn apply(g, v) {\n  return g(v);\n}\n\
         flow main() {\n  Applied: {apply(double, 21)} -> END\n}\n",
    );
    actual.extend(strict_findings("bare-name-fn-value-plain", result));

    actual.sort();
    let expected: Vec<(String, String, String)> = BASELINE
        .iter()
        .map(|(f, c, m)| ((*f).to_string(), (*c).to_string(), (*m).to_string()))
        .collect();

    assert_eq!(
        actual, expected,
        "driver.rs native fixtures' strict findings drifted from baseline.\n\
         Do NOT edit the fixtures to make this pass — triage each finding and either \
         fix the checker or update BASELINE with a classification and tracking issue."
    );
}

/// Guard against the sweep going vacuous if strict options are misconfigured.
#[test]
fn the_sweep_actually_runs_under_strict() {
    let result = compile_native_strict(
        "guard-check",
        "fn f(x) { return x; }\nflow main() { -> END }\n",
    );
    let findings = strict_findings("guard", result);
    // At minimum, an unannotated parameter `x` should escape as Unknown
    // under strict mode. If we get nothing, the strict pass is not running.
    // (If this assertion is too strict, adjust based on actual behavior.)
    let should_have_findings = !findings.is_empty();
    assert!(
        should_have_findings,
        "the strict pass produced no findings at all — it is almost certainly \
         not running (issue #1916's original bug)"
    );
}
