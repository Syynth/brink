//! Issue #2872: empirical audit of whether a value-call mistake (calling a
//! non-function value, e.g. `~ temp count = 10` then `{count(1, 2)}`) is
//! ever caught statically as compile error **E063**, across every
//! `Dialect` (`StrictInk`, `Brink`) x `TypePolicy` (`None`, `Gradual`,
//! `Strict`) combination.
//!
//! **The claim under audit** (`docs/diagnostics/E035.md`, pre-#2872):
//! "the brink dialect with gradual typing... catches the same shape
//! statically as compile error E063". Reproduced live here — it is
//! **false**. `E063`'s value-call producer is `brink_analyzer::strict::
//! check_value_calls`, reachable only from `strict::check`, which
//! `strict_diagnostics` (`crates/internal/brink-analyzer/src/lib.rs`) runs
//! only when `opts.type_policy() == TypePolicy::Strict`. `TypePolicy::
//! Gradual` never runs that pass, under either dialect — confirmed by
//! `named_knot_e063_fires_only_under_effective_strict_policy` below.
//!
//! **Two independent findings, not one:**
//!
//! 1. **Type-policy gating**: `E063`'s value-call producer fires only when
//!    the *effective* type policy is `Strict`. `resolve_type_policy`
//!    (`strict.rs`) defaults an unset `types` to `Strict` under `Dialect::
//!    Brink` and to `Gradual` under `Dialect::StrictInk` — so `dialect =
//!    brink` with `types` left **unset** silently runs under `Strict`, not
//!    `Gradual`. This is almost certainly how the original PR #2865 review
//!    observation ("brink dialect... E063") happened: dialect and type
//!    policy were conflated because leaving `types` unset under `Brink`
//!    *is* strict typing by default.
//! 2. **Scope gap, orthogonal to (1)**: `strict::check_value_calls` walks
//!    only `hir.knots` (named knots/stitches/functions) to find inference
//!    bodies to check — never the file's own default/entry flow (content
//!    before the first `===` heading, or reached without an explicit
//!    named-knot divert). A value-call mistake sitting in that default flow
//!    — exactly where `docs/diagnostics/E035.md`'s own `~ temp MAX = 10` /
//!    `{MAX(1, 2)}` example puts it — **never** produces E063, under *any*
//!    dialect/policy combination, `Strict` included. See
//!    `root_flow_e063_never_fires_regardless_of_policy` below.
//!
//! Both tests below compile through the same public `brink_compiler::
//! compile_with_options` entry point real callers use (`brink-cli compile`,
//! `brink_environment::compile`), and the second additionally links and
//! runs the compiled `.inkb` through `brink_runtime::Story` to confirm the
//! real user-visible outcome (a `RuntimeError::NotCallable` fault, not a
//! silent no-op) for every cell that compiles clean.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};

/// One audited cell's expected outcome.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Compiles clean; the mistake is left to fault at runtime.
    CompilesCleanFaultsAtRuntime,
    /// Fails to compile with `E063` (the value-call `NotCallable` check).
    E063,
    /// Fails to compile with `E064` (`types = strict` requires
    /// `dialect = brink` — `strict::check` never even runs).
    E064,
}

const COMBOS: [(&str, Dialect, Option<TypePolicy>); 6] = [
    ("StrictInk", Dialect::StrictInk, None),
    ("StrictInk", Dialect::StrictInk, Some(TypePolicy::Gradual)),
    ("StrictInk", Dialect::StrictInk, Some(TypePolicy::Strict)),
    ("Brink", Dialect::Brink, None),
    ("Brink", Dialect::Brink, Some(TypePolicy::Gradual)),
    ("Brink", Dialect::Brink, Some(TypePolicy::Strict)),
];

fn compile_with(
    source: &str,
    dialect: Dialect,
    types: Option<TypePolicy>,
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

/// Compile `source` under every (dialect, types) combo in [`COMBOS`] and
/// assert each cell matches `expected`, in the same order.
fn assert_table(source: &str, expected: [Outcome; 6]) {
    for ((label, dialect, types), want) in COMBOS.into_iter().zip(expected) {
        let result = compile_with(source, dialect, types);
        let got = match &result {
            Ok(_) => Outcome::CompilesCleanFaultsAtRuntime,
            Err(err) => {
                let diags = match err {
                    brink_compiler::CompileError::Diagnostics(diags) => diags.clone(),
                    other => panic!(
                        "dialect={label} types={types:?}: expected Diagnostics error, got {other:?}"
                    ),
                };
                let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
                if codes == [brink_compiler::DiagnosticCode::E063] {
                    Outcome::E063
                } else if codes == [brink_compiler::DiagnosticCode::E064] {
                    Outcome::E064
                } else {
                    panic!("dialect={label} types={types:?}: unexpected diagnostics {codes:?}");
                }
            }
        };
        assert_eq!(
            got, want,
            "dialect={label} types={types:?}: expected {want:?}, got {got:?} ({result:?})"
        );
    }
}

/// Finding (1): inside a **named** knot, `E063` fires if and only if the
/// effective type policy is `Strict` — explicit `Strict`, or `Brink`'s own
/// default when `types` is left unset. It never fires under `Gradual`,
/// under either dialect, and `StrictInk` can never even reach `Strict` (a
/// project-level `E064` config error blocks it first).
#[test]
fn named_knot_e063_fires_only_under_effective_strict_policy() {
    let source = "\
-> main
=== main ===
~ temp count = 10
value: {count(1, 2)}
-> DONE
";
    assert_table(
        source,
        [
            Outcome::CompilesCleanFaultsAtRuntime, // StrictInk, None -> Gradual
            Outcome::CompilesCleanFaultsAtRuntime, // StrictInk, Gradual
            Outcome::E064,                         // StrictInk, Strict (config error)
            Outcome::E063,                         // Brink, None -> Strict (default!)
            Outcome::CompilesCleanFaultsAtRuntime, // Brink, Gradual
            Outcome::E063,                         // Brink, Strict
        ],
    );
}

/// Finding (2): the exact same mistake, in the file's default/entry flow
/// rather than a named knot (`docs/diagnostics/E035.md`'s own `~ temp MAX
/// = 10` example is shaped this way) — `E063` never fires here, in any
/// cell, `Brink`+`Strict` included, because `check_value_calls` only walks
/// `hir.knots` and this content belongs to none of them. Every clean-
/// compile cell is run end-to-end to confirm the real fault a user hits:
/// `RuntimeError::NotCallable`, with no compile-time diagnostic at all.
#[test]
fn root_flow_e063_never_fires_regardless_of_policy() {
    let source = "\
~ temp count = 10

value: {count(1, 2)}
-> DONE
";
    assert_table(
        source,
        [
            Outcome::CompilesCleanFaultsAtRuntime, // StrictInk, None -> Gradual
            Outcome::CompilesCleanFaultsAtRuntime, // StrictInk, Gradual
            Outcome::E064,                         // StrictInk, Strict (config error)
            Outcome::CompilesCleanFaultsAtRuntime, // Brink, None -> Strict, still unchecked
            Outcome::CompilesCleanFaultsAtRuntime, // Brink, Gradual
            Outcome::CompilesCleanFaultsAtRuntime, // Brink, Strict, still unchecked
        ],
    );

    // Confirm the "compiles clean" half of the claim is not a silent
    // no-op: every clean-compile cell above actually faults at runtime.
    for (label, dialect, types) in [
        ("StrictInk", Dialect::StrictInk, None),
        ("StrictInk", Dialect::StrictInk, Some(TypePolicy::Gradual)),
        ("Brink", Dialect::Brink, None),
        ("Brink", Dialect::Brink, Some(TypePolicy::Gradual)),
        ("Brink", Dialect::Brink, Some(TypePolicy::Strict)),
    ] {
        let compiled = compile_with(source, dialect, types).unwrap_or_else(|e| {
            panic!("dialect={label} types={types:?}: expected clean compile, got {e:?}")
        });
        let (program, line_tables) = brink_runtime::link(&compiled.data).expect("link");
        let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
            std::sync::Arc::new(program),
            line_tables,
        );
        let mut fault = None;
        for _ in 0..8 {
            match story.continue_single() {
                Ok(brink_runtime::Step::Line(_) | brink_runtime::Step::Done) => {}
                Ok(
                    brink_runtime::Step::Choices(_)
                    | brink_runtime::Step::Suspended
                    | brink_runtime::Step::End,
                ) => break,
                Err(err) => {
                    fault = Some(err);
                    break;
                }
            }
        }
        assert!(
            matches!(fault, Some(brink_runtime::RuntimeError::NotCallable(_))),
            "dialect={label} types={types:?}: expected a NotCallable runtime fault, got {fault:?}"
        );
    }
}
