//! Issue #2856 point 3: a declared knot/`VAR` shadowing a classic uppercase
//! ink built-in (`is_builtin_function`/`recognize_builtin`) must actually be
//! CALLED at a call site, not silently replaced by the real builtin.
//!
//! `brink-analyzer::resolve::resolve_function`/`resolve_variable` fixed
//! their half of this (author declarations now win resolution, matching the
//! `E035` "name shadows a built-in function" warning's own documented
//! intent — shadowing is legal, not silent). This file pins the deeper,
//! separate codegen-layer half of the same bug: `brink_ir::lir::lower::expr
//! ::lower_call` checked `recognize_builtin` (the LIR-lowering copy of
//! `is_builtin_function`, hand-synced across the crate boundary) BEFORE
//! consulting the analyzer's resolution map at all — so even a *correctly
//! resolved* knot named `FLOOR` was silently discarded in favor of the real
//! `FLOOR()` builtin at the call site. Confirmed via `brink-cli compile` +
//! `play` before this fix: `Result: {FLOOR(5)}` against a knot
//! `=== function FLOOR(x) === ~ return x - 1000` printed `Result: 5` (the
//! real builtin's answer) instead of `Result: -995` (the author's).
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

/// `docs/diagnostics/E035.md` shape 4 (issue #2862): a `CONST` named after a
/// builtin behaves the same as the `VAR` case above at a call site — the
/// real `MAX()` builtin answers `MAX(1, 2)`, not the constant. Mechanically
/// this is not the same guard: `resolve_function`'s call-site lookups never
/// try `SymbolKind::Constant` at all (only `SymbolKind::Variable`/`List` are
/// tried and then gated by `reserved_call_site`), so a `CONST` never reaches
/// a callable-lookup arm to begin with — it falls through to
/// `is_builtin_function`/`recognize_builtin` by the same path an
/// undeclared name would. The *outcome* at the call site is identical to
/// `VAR`, though: the constant is bypassed, and the real builtin runs.
#[test]
fn const_named_builtin_is_not_callable_at_call_site() {
    let out = run("\
CONST MAX = 10

value: {MAX(1, 2)}
-> DONE
");
    assert_eq!(out.trim(), "value: 2");
}

/// `docs/diagnostics/E035.md` shape 5 (issue #2862): a `LIST` item named
/// after a T1b stdlib verb (`push`) shadows it at a **bare-reference** read
/// (`{push}`, an ordinary list-item value read, `arg_count.is_none()`) —
/// but with no `E035` warning, because `manifest.rs`'s shadow-warn gate only
/// covers `SymbolKind::{Knot, Variable, Constant, External}`, not
/// `List`/`ListItem`. This is not an inconsistency: a bare list-item read
/// and a stdlib-verb **call** (`push(arr, 3)`, `arg_count.is_some()`) are
/// disjoint syntactic positions that never actually collide — see
/// `list_item_named_push_does_not_break_the_real_push_call` below, which
/// proves the real `push()` verb still works normally against an unrelated
/// array in the very same program. Nothing is silently overridden either
/// way, so there is nothing for E035 to warn about.
#[test]
fn list_item_shadows_stdlib_verb_name_at_bare_reference_without_e035_warning() {
    let source = "\
LIST Ops = alpha, push, gamma

Item: {push}
-> DONE
";
    let out = compile_brink(source).expect("compile");
    assert!(
        !out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "a list item is not in E035's warn set, expected no warning, got {:?}",
        out.warnings
    );
    let text = run_brink(source);
    assert_eq!(text.trim(), "Item: push");
}

/// Companion to the test above: the list item named `push` does not shadow
/// the real `push()` stdlib verb at its own call site — `arg_count.is_some()`
/// keeps routing to the stdlib call regardless of the list item's existence.
#[test]
fn list_item_named_push_does_not_break_the_real_push_call() {
    let out = run_brink(
        "LIST Ops = alpha, push, gamma\n~ temp arr = #[1, 2]\n~ push(arr, 3)\nLen: {len(arr)}\n-> DONE\n",
    );
    assert_eq!(out.trim(), "Len: 3");
}

fn compile_brink(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
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

/// Brink-dialect counterpart of `run` above, for the T1b-stdlib-verb cases
/// (`push`/`len`) that `--dialect brink` — not `StrictInk` — is the natural
/// home for.
fn run_brink(source: &str) -> String {
    let output = compile_brink(source).expect("compile");
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
