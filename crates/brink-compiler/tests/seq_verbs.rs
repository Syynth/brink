//! End-to-end compiler + runtime tests for the fn-value verb layer's pure
//! trio (`docs/stdlib-spec.md` §4, issue #1679): `map`, `filter`, `fold`.
//!
//! Covers, through `brink_compiler::compile_with_options` and then the VM:
//! - the ruled arity of each verb (`E031` otherwise);
//! - the E119 pure-callback contract gate, generalized off `sort_by` — a
//!   provably impure/unsilent inline `#fn(target)` callback is rejected, a
//!   pure one passes, and an opaque one is not *proven* and passes (the
//!   exceedance-only posture, whose runtime residual is the ops' isolation
//!   and fault machinery);
//! - the runtime dispatch faults (non-array receiver, non-function
//!   callback, non-bool `filter` predicate return);
//! - the dev-mode world-write guard naming the fn-value verb rather than
//!   `sort_by` (the shared `PureCallbackState` seam);
//! - strict-ink unreachability — the trio is brink-dialect surface, so the
//!   oracle corpus can never reach these ops.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_ir::DiagnosticCode;
use brink_runtime::{DotNetRng, Line, Story};

fn compile_brink(
    source: &str,
    types: Option<TypePolicy>,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    compile_in(source, Dialect::Brink, types)
}

fn compile_in(
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

fn diagnostics_of(err: brink_compiler::CompileError) -> Vec<brink_compiler::ResolvedDiagnostic> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags,
        other => panic!("expected Diagnostics error, got {other:?}"),
    }
}

fn has_code(diags: &[brink_compiler::ResolvedDiagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == code)
}

/// Compile and run to completion, returning the concatenated output.
fn run(source: &str) -> String {
    let output = compile_brink(source, Some(TypePolicy::Gradual)).expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut out = String::new();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => panic!("these programs are choice-free"),
        }
    }
    out
}

/// Compile and run, expecting a turn-terminating runtime fault; returns its
/// rendered message.
fn run_expecting_fault(source: &str) -> String {
    let output = compile_brink(source, Some(TypePolicy::Gradual)).expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    loop {
        match story.continue_single() {
            Ok(Line::Text { .. }) => {}
            Ok(other) => panic!("expected a runtime fault, story finished: {other:?}"),
            Err(e) => return e.to_string(),
        }
    }
}

// ── arity (the ruled signatures) ─────────────────────────────────────
//
// Arity mismatches are warnings throughout this codebase (`resolve::
// check_arity`'s convention for ordinary calls too), so these assert on
// the successful compile's warning row rather than on a compile error.

fn arity_warning(source: &str) -> String {
    let out = compile_brink(source, Some(TypePolicy::Gradual))
        .expect("wrong arity is a warning, not a compile error");
    let Some(d) = out.warnings.iter().find(|d| d.code == DiagnosticCode::E031) else {
        panic!("expected E031, got {:?}", out.warnings)
    };
    d.message.clone()
}

#[test]
fn map_arity_mismatch_is_e031() {
    let message = arity_warning("~ temp a = #[1]\n~ temp b = map(a)\n{b}\n-> END\n");
    assert!(message.contains("`map` expects 2 argument(s)"), "{message}");
}

#[test]
fn filter_arity_mismatch_is_e031() {
    let message = arity_warning("~ temp a = #[1]\n~ temp b = filter(a)\n{b}\n-> END\n");
    assert!(
        message.contains("`filter` expects 2 argument(s)"),
        "{message}"
    );
}

/// `fold(a, init, f)` is the ONE three-argument verb in the trio — a
/// two-argument call must not be silently accepted as `map`-shaped.
#[test]
fn fold_arity_mismatch_is_e031() {
    let message = arity_warning(
        "~ temp a = #[1]\n~ temp b = fold(a, 0)\n{b}\n-> END\n\n=== function add(x, y) ===\n~ return x + y\n",
    );
    assert!(
        message.contains("`fold` expects 3 argument(s)"),
        "{message}"
    );
}

// ── E119: the pure-callback contract gate, all three verbs ───────────
//
// A precedent test in `ns_a4_ordering.rs` covers both exceedance kinds
// (write, emit) for the comparator pair; the trio covers every verb,
// because each one reads its callback from a different argument position
// (`fold`'s is the third).

#[test]
fn writing_map_callback_is_e119() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp b = map(a, #fn(spy))\n{b}\n-> END\n\n=== function spy(n: int): int ===\n~ seen = seen + 1\n~ return n\n";
    let diags = diagnostics_of(compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err());
    assert!(has_code(&diags, DiagnosticCode::E119), "{diags:?}");
}

#[test]
fn emitting_filter_callback_is_e119() {
    let source = "~ temp a = #[1]\n~ temp b = filter(a, #fn(loud))\n{b}\n-> END\n\n=== function loud(n: int): bool ===\nchecking!\n~ return true\n";
    let diags = diagnostics_of(compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err());
    assert!(has_code(&diags, DiagnosticCode::E119), "{diags:?}");
}

/// `fold`'s callback sits at argument index 2 — the gate must read the
/// position, not assume index 1 the way the comparator roster could.
#[test]
fn writing_fold_callback_is_e119() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp b = fold(a, 0, #fn(spy))\n{b}\n-> END\n\n=== function spy(acc: int, n: int): int ===\n~ seen = seen + 1\n~ return acc + n\n";
    let diags = diagnostics_of(compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err());
    assert!(has_code(&diags, DiagnosticCode::E119), "{diags:?}");
}

/// The E119 message must name the verb and the trio's own requirement, not
/// the comparator wording it is generalized from.
#[test]
fn trio_e119_message_names_the_callback_not_a_comparator() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp b = map(a, #fn(spy))\n{b}\n-> END\n\n=== function spy(n: int): int ===\n~ seen = seen + 1\n~ return n\n";
    let diags = diagnostics_of(compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err());
    let message = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E119)
        .map(|d| d.message.clone())
        .expect("an E119");
    assert!(message.contains("`map`'s callback"), "{message}");
    assert!(!message.contains("comparator"), "{message}");
}

#[test]
fn pure_callbacks_pass_the_gate() {
    let source = "~ temp a = #[1, 2]\n{map(a, #fn(double))}\n{filter(a, #fn(is_even))}\n{fold(a, 0, #fn(add))}\n-> END\n\n=== function double(n: int): int ===\n~ return n * 2\n\n=== function is_even(n: int): bool ===\n~ return n % 2 == 0\n\n=== function add(acc: int, n: int): int ===\n~ return acc + n\n";
    compile_brink(source, None).expect("pure callbacks must pass E119 under the strict default");
}

/// The #1680 gap made concrete: routed through a variable, the very same
/// impure callback is no longer *provable*, so the gate cannot fire. This
/// is the exceedance-only posture, not an oversight — and it is exactly why
/// "pure-required" is not enforceable through a fn value today.
#[test]
fn opaque_callback_is_not_proven_and_passes() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp f = #fn(spy)\n~ temp b = map(a, f)\n{b}\n-> END\n\n=== function spy(n: int): int ===\n~ seen = seen + 1\n~ return n\n";
    compile_brink(source, Some(TypePolicy::Gradual))
        .expect("an opaque callback is not provably in violation");
}

// ── the ops execute (reachability from real author-facing source) ────

#[test]
fn the_trio_runs_end_to_end() {
    let source = "~ temp a = #[1, 2, 3]\nmap={map(a, #fn(double))} filter={filter(a, #fn(is_even))} fold={fold(a, 0, #fn(add))}\n-> END\n\n=== function double(n: int): int ===\n~ return n * 2\n\n=== function is_even(n: int): bool ===\n~ return n % 2 == 0\n\n=== function add(acc: int, n: int): int ===\n~ return acc + n\n";
    assert_eq!(run(source), "map=[2, 4, 6] filter=[2] fold=6\n");
}

// ── runtime dispatch faults (the gradual-mode residual) ──────────────

#[test]
fn map_over_a_non_array_faults() {
    let source = "~ temp a = 1\n~ temp b = map(a, #fn(double))\n{b}\n-> END\n\n=== function double(n) ===\n~ return n * 2\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`map`"), "{message}");
    assert!(message.contains("an array"), "{message}");
}

#[test]
fn filter_with_a_non_function_callback_faults() {
    let source = "~ temp a = #[1]\n~ temp b = filter(a, 7)\n{b}\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`filter` callback"), "{message}");
    assert!(message.contains("`fn(T): bool`"), "{message}");
}

#[test]
fn fold_with_a_non_function_callback_faults() {
    let source = "~ temp a = #[1]\n~ temp b = fold(a, 0, 7)\n{b}\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`fold` callback"), "{message}");
    assert!(message.contains("`fn(U, T): U`"), "{message}");
}

/// A non-bool predicate return is a fault, never a truthiness coercion —
/// coercing would silently change which elements survive.
#[test]
fn filter_predicate_returning_a_non_bool_faults() {
    let source = "~ temp a = #[1]\n~ temp b = filter(a, #fn(nonsense))\n{b}\n-> END\n\n=== function nonsense(n) ===\n~ return 7\n";
    let message = run_expecting_fault(source);
    assert!(
        message.contains("`filter` callback must return a bool"),
        "{message}"
    );
}

/// The dev-mode world-write guard is shared with `sort_by` but must report
/// the verb whose callback actually ran. Reached here through an opaque
/// callback, since a provable one would be stopped by E119 at compile time.
#[test]
fn dev_mode_world_write_inside_a_map_callback_names_map() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp f = #fn(spy)\n~ temp b = map(a, f)\n{b}\n-> END\n\n=== function spy(n) ===\n~ seen = seen + 1\n~ return n\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`map`"), "{message}");
    assert!(!message.contains("sort_by"), "{message}");
}

// ── strict-ink: the trio is brink-dialect surface ────────────────────

#[test]
fn strict_ink_never_reaches_the_fn_value_verbs() {
    // Under strict-ink the sigil array literal is already rejected and the
    // verb names stay ordinary unresolved calls — the trio is
    // vanilla-unreachable, which is what keeps the oracle corpus safe from
    // these ops. Any diagnostics are fine; compiling *successfully into
    // SeqVerb opcodes* would not be.
    let source = "~ temp a = #[1]\n~ temp b = map(a, #fn(double))\n{b}\n-> END\n\n=== function double(n) ===\n~ return n * 2\n";
    assert!(
        compile_in(source, Dialect::StrictInk, None).is_err(),
        "strict-ink must reject the brink verb surface"
    );
}
