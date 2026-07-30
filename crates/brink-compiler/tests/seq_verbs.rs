//! End-to-end compiler + runtime tests for the fn-value verb layer
//! (`docs/stdlib-spec.md` §4, issue #1679): the pure quartet `map`,
//! `filter`, `fold`, `filter_map`, and the effectful pair `each`,
//! `map_each` (slice 2).
//!
//! Covers, through `brink_compiler::compile_with_options` and then the VM:
//! - the ruled arity of each verb (`E031` otherwise);
//! - the E119 pure-callback contract gate, generalized off `sort_by` — a
//!   provably impure/unsilent inline `#fn(target)` callback is rejected, a
//!   pure one passes, and an opaque one is not *proven* and passes (the
//!   exceedance-only posture, whose runtime residual is the ops' isolation
//!   and fault machinery);
//! - the runtime dispatch faults (non-array receiver, non-function
//!   callback, non-bool `filter` predicate return, non-Option
//!   `filter_map` return);
//! - the dev-mode world-write guard naming the fn-value verb rather than
//!   `sort_by` (the shared `PureCallbackState` seam);
//! - `each`/`map_each`'s inverse contract: the SAME world-write that E119
//!   rejects for the pure quartet, and that `ComparatorWroteState` faults at
//!   runtime for an opaque pure callback, is legal for the effectful pair —
//!   neither is E119-gated, and the guard never fires for them;
//! - `each`/`map_each`'s other headline effect — printed output reaching
//!   the transcript instead of being captured and discarded — in both
//!   statement position and inside a `{…}` interpolation slot, pinning the
//!   ordering between a callback's own output and the surrounding literal
//!   text;
//! - strict-ink unreachability — the whole family is brink-dialect surface,
//!   so the oracle corpus can never reach these ops.

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

/// Slice 2 (issue #1679): `filter_map(a, f)` shares `map`/`filter`'s arity.
#[test]
fn filter_map_arity_mismatch_is_e031() {
    let message = arity_warning("~ temp a = #[1]\n~ temp b = filter_map(a)\n{b}\n-> END\n");
    assert!(
        message.contains("`filter_map` expects 2 argument(s)"),
        "{message}"
    );
}

/// Slice 2 (issue #1679): the effectful pair, `each(a, f)`.
#[test]
fn each_arity_mismatch_is_e031() {
    let message = arity_warning("~ temp a = #[1]\n~ each(a)\n-> END\n");
    assert!(
        message.contains("`each` expects 2 argument(s)"),
        "{message}"
    );
}

/// Slice 2 (issue #1679): the effectful pair, `map_each(a, f)`.
#[test]
fn map_each_arity_mismatch_is_e031() {
    let message = arity_warning("~ temp a = #[1]\n~ temp b = map_each(a)\n{b}\n-> END\n");
    assert!(
        message.contains("`map_each` expects 2 argument(s)"),
        "{message}"
    );
}

// ── E119: the pure-callback contract gate, all four pure verbs ───────
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

/// `filter_map`'s callback sits at argument index 1, exactly like
/// `map`/`filter` — it stayed on the pure roster in slice 2, unlike its
/// effectful siblings below.
#[test]
fn writing_filter_map_callback_is_e119() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp b = filter_map(a, #fn(spy))\n{b}\n-> END\n\n=== function spy(n: int) ===\n~ seen = seen + 1\n~ return some(n)\n";
    let diags = diagnostics_of(compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err());
    assert!(has_code(&diags, DiagnosticCode::E119), "{diags:?}");
}

/// The E119 message must name the verb and the quartet's own requirement,
/// not the comparator wording it is generalized from.
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

/// The rejection message names the real effectful exit now that it ships
/// (slice 2) — the pre-slice-2 wording called it "not shipped yet".
#[test]
fn e119_message_points_at_the_shipped_effectful_exit() {
    let source = "VAR seen = 0\n~ temp a = #[1]\n~ temp b = map(a, #fn(spy))\n{b}\n-> END\n\n=== function spy(n: int): int ===\n~ seen = seen + 1\n~ return n\n";
    let diags = diagnostics_of(compile_brink(source, Some(TypePolicy::Gradual)).unwrap_err());
    let message = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E119)
        .map(|d| d.message.clone())
        .expect("an E119");
    assert!(message.contains("each"), "{message}");
    assert!(message.contains("map_each"), "{message}");
    assert!(!message.contains("not shipped yet"), "{message}");
}

/// The effectful pair is deliberately absent from E119's roster (the
/// module doc's central claim) — the SAME world-write that
/// `writing_map_callback_is_e119` rejects for `map` must compile clean for
/// `each`, inline callback and all (no #1680 opacity dodge required, unlike
/// the pure quartet).
#[test]
fn each_and_map_each_are_not_e119_gated() {
    let source = "VAR seen = 0\n~ temp a = #[1, 2]\n~ each(a, #fn(spy))\n{seen}\n-> END\n\n=== function spy(n) ===\n~ seen = seen + n\n~ return n\n";
    compile_brink(source, Some(TypePolicy::Gradual))
        .expect("each's callback must not be E119-gated even though it writes a global");

    let source = "VAR seen = 0\n~ temp a = #[1, 2]\n~ temp b = map_each(a, #fn(spy))\n{seen}\n-> END\n\n=== function spy(n) ===\n~ seen = seen + n\n~ return n * 10\n";
    compile_brink(source, Some(TypePolicy::Gradual))
        .expect("map_each's callback must not be E119-gated either");
}

#[test]
fn pure_callbacks_pass_the_gate() {
    let source = "~ temp a = #[1, 2]\n{map(a, #fn(double))}\n{filter(a, #fn(is_even))}\n{fold(a, 0, #fn(add))}\n{filter_map(a, #fn(keep_all))}\n-> END\n\n=== function double(n: int): int ===\n~ return n * 2\n\n=== function is_even(n: int): bool ===\n~ return n % 2 == 0\n\n=== function add(acc: int, n: int): int ===\n~ return acc + n\n\n=== function keep_all(n: int) ===\n~ return some(n)\n";
    compile_brink(source, None).expect("pure callbacks must pass E119 under the strict default");
}

/// The #1679 gap made concrete: routed through a variable, the very same
/// impure callback is no longer *provable*, so the gate cannot fire. This
/// is the exceedance-only posture, not an oversight — and it is exactly why
/// "pure-required" is not enforceable through a fn value today. #1680 step
/// 3 put the substrate in place (`Ty::Fn` carries a creation-target row),
/// but this gate is handed no inferred types; see effects-spec §6.1c.
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

/// `filter_map` drops `none` and unwraps `some(v)`, in iteration order —
/// the Option-mapper companion of `map` (§4).
#[test]
fn filter_map_runs_end_to_end() {
    let source = "~ temp a = #[1, 2, 3, 4, 5]\n{filter_map(a, #fn(keep_even_doubled))}\n-> END\n\n=== function keep_even_doubled(n: int) ===\n~ {\n    if n % 2 == 0 {\n        return some(n * 10)\n    }\n    return none\n}\n";
    assert_eq!(run(source), "[20, 40]\n");
}

/// `each`'s whole point: the callback's world-write reaches the story's
/// visible state, and `each` itself contributes no printed value (it
/// produces no result — the "do something per element" spelling).
#[test]
fn each_runs_a_callback_with_legal_world_writes() {
    let source = "VAR seen = 0\n~ temp a = #[1, 2, 3]\n~ each(a, #fn(bump))\n{seen}\n-> END\n\n=== function bump(n) ===\n~ seen = seen + n\n";
    assert_eq!(run(source), "6\n");
}

/// `map_each` is `map`'s effectful twin: it both returns the transformed
/// array AND lets its callback write a global — `map`'s own E119 test
/// (`writing_map_callback_is_e119`) rejects the identical write; this is
/// the whole reason the effectful spelling exists.
#[test]
fn map_each_runs_a_callback_with_legal_world_writes() {
    let source = "VAR seen = 0\n~ temp a = #[1, 2, 3]\n~ temp b = map_each(a, #fn(tally))\n{b} seen={seen}\n-> END\n\n=== function tally(n: int): int ===\n~ seen = seen + n\n~ return n * 10\n";
    assert_eq!(run(source), "[10, 20, 30] seen=6\n");
}

/// The headline effectful contract — "output reaches the transcript" — is
/// asserted in prose across the changeset, `docs/stdlib-spec.md` §4,
/// `docs/stdlib-inventory.md`, `docs/book/src/toolchain/dialect/
/// iteration.md`, `RuntimeError`'s docs and `call_callback`'s doc, but
/// nothing before this test actually made a callback *emit text*: every
/// existing `each`/`map_each` test only assigns a global. This is the
/// `capture_output: false` branch at `vm.rs`'s `call_callback` exercised
/// end to end, in statement position — `each` contributes no printed value
/// of its own, so every character in the output is the callback's.
#[test]
fn each_callback_output_reaches_the_transcript_in_statement_position() {
    let source = "~ temp a = #[1, 2, 3]\n~ each(a, #fn(shout))\n-> END\n\n=== function shout(n) ===\nLoud {n}!\n~ return n\n";
    assert_eq!(run(source), "Loud 1!Loud 2!Loud 3!\n");
}

/// The same contract, but for `map_each` inside a `{…}` interpolation
/// slot — slot evaluation runs before the surrounding literal text is
/// emitted, so the callback's own output must land BEFORE the interpolated
/// array value and before the literal text that follows the slot in
/// source order. Pinning this ordering, not merely "doesn't fault".
#[test]
fn map_each_callback_output_reaches_the_transcript_inside_an_interpolation() {
    let source = "~ temp a = #[1, 2]\nresult: {map_each(a, #fn(shout))}\n-> END\n\n=== function shout(n) ===\nLoud {n}!\n~ return n * 10\n";
    assert_eq!(run(source), "Loud 1!Loud 2!result: [10, 20]\n");
}

// ── runtime dispatch faults (the gradual-mode residual) ──────────────

#[test]
fn map_over_a_non_array_faults() {
    let source = "~ temp a = 1\n~ temp b = map(a, #fn(double))\n{b}\n-> END\n\n=== function double(n) ===\n~ return n * 2\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`map`"), "{message}");
    assert!(message.contains("an array"), "{message}");
}

/// `map`'s own `CallbackNotAFunction` case — `filter` and `fold` (below)
/// each already cover it; `map` had only the non-array-receiver fault.
#[test]
fn map_with_a_non_function_callback_faults() {
    let source = "~ temp a = #[1]\n~ temp b = map(a, 7)\n{b}\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`map` callback"), "{message}");
    assert!(message.contains("`fn(T): U`"), "{message}");
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
    // `call_pure_callback`/`guard_comparator_write` are generalized over
    // the verb (issue #1679), but must not generalize the *noun*: a `map`
    // author must not be told they wrote a bad comparator.
    assert!(!message.contains("comparator"), "{message}");
}

// ── slice 2 dispatch faults: filter_map, each, map_each ──────────────

#[test]
fn filter_map_over_a_non_array_faults() {
    let source = "~ temp a = 1\n~ temp b = filter_map(a, #fn(keep))\n{b}\n-> END\n\n=== function keep(n) ===\n~ return some(n)\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`filter_map`"), "{message}");
    assert!(message.contains("an array"), "{message}");
}

#[test]
fn filter_map_with_a_non_function_callback_faults() {
    let source = "~ temp a = #[1]\n~ temp b = filter_map(a, 7)\n{b}\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`filter_map` callback"), "{message}");
    assert!(message.contains("`fn(T): Option[U]`"), "{message}");
}

/// A non-Option return is a fault, never a silent pass-through — coercing
/// would either drop nothing or unwrap garbage.
#[test]
fn filter_map_callback_returning_a_non_option_faults() {
    let source = "~ temp a = #[1]\n~ temp b = filter_map(a, #fn(nonsense))\n{b}\n-> END\n\n=== function nonsense(n) ===\n~ return 7\n";
    let message = run_expecting_fault(source);
    assert!(
        message.contains("`filter_map` callback must return an Option"),
        "{message}"
    );
}

#[test]
fn each_over_a_non_array_faults() {
    let source =
        "~ temp a = 1\n~ each(a, #fn(bump))\n-> END\n\n=== function bump(n) ===\n~ return n\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`each`"), "{message}");
    assert!(message.contains("an array"), "{message}");
}

#[test]
fn each_with_a_non_function_callback_faults() {
    let source = "~ temp a = #[1]\n~ each(a, 7)\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`each` callback"), "{message}");
    assert!(message.contains("`fn(T)`"), "{message}");
}

#[test]
fn map_each_over_a_non_array_faults() {
    let source = "~ temp a = 1\n~ temp b = map_each(a, #fn(bump))\n{b}\n-> END\n\n=== function bump(n) ===\n~ return n\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`map_each`"), "{message}");
    assert!(message.contains("an array"), "{message}");
}

#[test]
fn map_each_with_a_non_function_callback_faults() {
    let source = "~ temp a = #[1]\n~ temp b = map_each(a, 7)\n{b}\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`map_each` callback"), "{message}");
    assert!(message.contains("`fn(T): U`"), "{message}");
}

/// `each`/`map_each`'s escaping-behavior faults (choice/DONE/END/external
/// call) are architectural, not a purity rule — being effectful doesn't
/// lift the "no handler exists mid-opcode" limitation. A callback that
/// diverges to `-> END` mid-`each` still faults, exactly like `map`'s
/// callback would.
#[test]
fn each_callback_reaching_end_still_escapes() {
    let source =
        "~ temp a = #[1]\n~ each(a, #fn(leaves))\n-> END\n\n=== function leaves(n) ===\n-> END\n";
    let message = run_expecting_fault(source);
    assert!(message.contains("`each`"), "{message}");
    assert!(message.contains("DONE"), "{message}");
}

// ── strict-ink: the trio is brink-dialect surface ────────────────────

#[test]
fn strict_ink_never_reaches_the_fn_value_verbs() {
    // Under strict-ink the sigil array literal is already rejected too, so
    // this alone would fail even with the `map(...)` line deleted — that's
    // not the claim under test. `lower_t1b_stdlib_call` is dialect-agnostic
    // (reachability is fenced by the dialect gate, not by lowering), so the
    // load-bearing assertion is that the gate's *own* diagnostic fires on
    // the `map` call specifically: `is_t1b_stdlib_call_name` recognizes it
    // and `GateVisitor::flag`s it under `StrictInk` (E051, `` `map` stdlib
    // function is a brink extension ``) — proving the trio is unreachable
    // by the dialect gate itself, not merely that compilation failed for
    // some unrelated reason.
    let source = "~ temp a = #[1]\n~ temp b = map(a, #fn(double))\n{b}\n-> END\n\n=== function double(n) ===\n~ return n * 2\n";
    let diags = diagnostics_of(
        compile_in(source, Dialect::StrictInk, None)
            .expect_err("strict-ink must reject the brink verb surface"),
    );
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("`map` stdlib function")),
        "expected a dialect-gate diagnostic naming the `map` call, got {diags:?}"
    );
}
