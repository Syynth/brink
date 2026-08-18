//! Issue #2856 point 3: a declared knot/`VAR` shadowing a classic uppercase
//! ink built-in (`is_builtin_function`/`recognize_builtin`) must actually be
//! CALLED at a call site, not silently replaced by the real builtin.
//!
//! `brink-analyzer::resolve::resolve_function`/`resolve_variable` fixed
//! their half of this (author declarations now win resolution, matching the
//! `E035` "name shadows a built-in function" warning's own documented
//! intent — shadowing is legal, not silent). This file pins the deeper,
//! separate codegen-layer half of the same bug: `brink_ir::lir::lower::expr
//! ::lower_call` checked `recognize_builtin` BEFORE consulting the
//! analyzer's resolution map at all — so even a *correctly resolved* knot
//! named `FLOOR` was silently discarded in favor of the real `FLOOR()`
//! builtin at the call site. (Issue #2863 made `brink_ir::lir::
//! is_builtin_function`/`recognize_builtin` the single canonical
//! definition — `brink-analyzer` now delegates to it instead of
//! hand-keeping its own copy — but that unification is orthogonal to this
//! test: it closes the *content*-drift risk, not the resolution-*order*
//! bug this file pins, which is why this regression stays.) Confirmed via
//! `brink-cli compile` + `play` before the original fix: `Result:
//! {FLOOR(5)}` against a knot `=== function FLOOR(x) === ~ return x -
//! 1000` printed `Result: 5` (the real builtin's answer) instead of
//! `Result: -995` (the author's).
//!
//! Mirrors `is_t1b_stdlib_name`'s already-correct ordering in this same
//! function (`lower_t1b_stdlib_call` is only consulted once
//! `ctx.resolve_path` has already failed) and the `seq_verbs.rs` /
//! `run`-style compile-then-execute pattern used throughout this test
//! directory.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, Step, Story};

fn compile_ink(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect: Dialect::StrictInk,
        types: Some(TypePolicy::Gradual),
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

/// Compile and run to completion, returning the concatenated output.
fn run(source: &str) -> String {
    let output = compile_ink(source).expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut out = String::new();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(line) => out.push_str(&line.text),
            Step::Choices(_) => panic!("these programs are choice-free"),
            Step::Done | Step::End | Step::Suspended => break,
        }
    }
    out
}

/// A `VAR` named after a classic uppercase built-in (`RANDOM`) shadows it at
/// every bare-variable-read reference — the `resolve_variable` half of the
/// fix.
#[test]
fn var_shadows_builtin_at_read_site() {
    let out = run("\
VAR RANDOM = 42

The value is {RANDOM}.
-> DONE
");
    assert_eq!(out.trim(), "The value is 42.");
}

/// A knot declared `=== function FLOOR(x) ===` shadows the real `FLOOR()`
/// built-in at its own call site — the `resolve_function` + `lower_call`
/// codegen half of the fix. Without the `lower_call` fix, `recognize_builtin`
/// still wins even after the analyzer resolves `FLOOR` to the knot: the
/// call site would silently ignore the resolution and run the real builtin
/// instead, printing `5` (`FLOOR(5.0)`) rather than the author's `-995`.
#[test]
fn knot_shadows_builtin_at_call_site() {
    let out = run("\
Result: {FLOOR(5)}
-> DONE

=== function FLOOR(x) ===
~ return x - 1000
");
    assert_eq!(out.trim(), "Result: -995");
}

/// Regression guard: an *unshadowed* built-in name still dispatches to the
/// real VM-native builtin exactly as before — the fix only changes
/// precedence when a real author declaration exists.
#[test]
fn unshadowed_builtin_still_works() {
    let out = run("The sum: {FLOOR(3.7)}\n-> DONE\n");
    assert_eq!(out.trim(), "The sum: 3");
}

/// PR review regression guard: a `VAR` is not itself callable — unlike a
/// knot (tunnel-as-function, `knot_shadows_builtin_at_call_site` above) or
/// a variable holding a stored divert target — so `VAR MAX = 10` must not
/// let `resolve_function`'s `SymbolKind::Variable` lookup claim
/// `{MAX(1, 2)}`'s *call site* on the strength of this issue's own
/// shadowing fix. Before the review fix, this compiled clean with no
/// diagnostic and then died at runtime with
/// `RuntimeError::NotCallable("int")` (`lower_call`'s `CallVariable`
/// emitted against an int) — the exact "clean compile, runtime fault"
/// shape issue #2830's list-item gate exists to prevent, and which this
/// same PR's own `E183` refusal exists to turn into a compile diagnostic
/// for other non-callable kinds. The call site must keep reaching the real
/// `MAX()` built-in instead, exactly as it did before this whole issue's
/// fix — `var_shadows_builtin_at_read_site` above still pins that the
/// *bare read* `{RANDOM}` shadow keeps working.
#[test]
fn var_named_builtin_is_not_callable_at_call_site() {
    let out = run("\
VAR MAX = 10

value: {MAX(1, 2)}
-> DONE
");
    assert_eq!(out.trim(), "value: 2");
}
