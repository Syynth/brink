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

/// Single parameterized compile entry point — `dialect`/`types` are the only
/// axis the tests below vary; carrying separate `compile_ink`/`compile_brink`
/// copies duplicated ~30 lines for no reason (PR #2865 review).
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

/// Compile (asserting no `E035`/`E183` diagnostic) and run to completion,
/// returning the runtime fault the program dies with, if any. Shared by
/// `local_named_builtin_faults_at_runtime_not_callable` and
/// `param_named_builtin_faults_at_runtime_not_callable` — extracted per PR
/// #2865's review, which rejected the same ~28 lines duplicated verbatim
/// between the temp and param variants of this exact test shape.
fn run_expecting_fault(
    source: &str,
    dialect: Dialect,
    types: Option<TypePolicy>,
) -> Option<brink_runtime::RuntimeError> {
    let compiled = compile_with(source, dialect, types)
        .expect("compile should succeed with no E035/E183 diagnostic");
    assert!(
        !compiled
            .warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "expected no E035 warning, got {:?}",
        compiled.warnings
    );
    let (program, line_tables) = brink_runtime::link(&compiled.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut fault = None;
    loop {
        match story.continue_single() {
            Ok(Step::Line(_)) => {}
            Ok(Step::Choices(_)) => panic!("this program is choice-free"),
            Ok(Step::Done | Step::End | Step::Suspended) => break,
            Err(err) => {
                fault = Some(err);
                break;
            }
        }
    }
    fault
}

/// Compile and run to completion, returning the concatenated output.
fn run_with(source: &str, dialect: Dialect, types: Option<TypePolicy>) -> String {
    let output = compile_with(source, dialect, types).expect("compile");
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
    let out = run_with(
        "\
VAR RANDOM = 42

The value is {RANDOM}.
-> DONE
",
        Dialect::StrictInk,
        Some(TypePolicy::Gradual),
    );
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
    let out = run_with(
        "\
Result: {FLOOR(5)}
-> DONE

=== function FLOOR(x) ===
~ return x - 1000
",
        Dialect::StrictInk,
        Some(TypePolicy::Gradual),
    );
    assert_eq!(out.trim(), "Result: -995");
}

/// Regression guard: an *unshadowed* built-in name still dispatches to the
/// real VM-native builtin exactly as before — the fix only changes
/// precedence when a real author declaration exists.
#[test]
fn unshadowed_builtin_still_works() {
    let out = run_with(
        "The sum: {FLOOR(3.7)}\n-> DONE\n",
        Dialect::StrictInk,
        Some(TypePolicy::Gradual),
    );
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
    let out = run_with(
        "\
VAR MAX = 10

value: {MAX(1, 2)}
-> DONE
",
        Dialect::StrictInk,
        Some(TypePolicy::Gradual),
    );
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
    let out = run_with(
        "\
CONST MAX = 10

value: {MAX(1, 2)}
-> DONE
",
        Dialect::StrictInk,
        Some(TypePolicy::Gradual),
    );
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
    let out = compile_with(source, Dialect::Brink, None).expect("compile");
    assert!(
        !out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "a list item is not in E035's warn set, expected no warning, got {:?}",
        out.warnings
    );
    let text = run_with(source, Dialect::Brink, None);
    assert_eq!(text.trim(), "Item: push");
}

/// Companion to the test above: the list item named `push` does not shadow
/// the real `push()` stdlib verb at its own call site — `arg_count.is_some()`
/// keeps routing to the stdlib call regardless of the list item's existence.
#[test]
fn list_item_named_push_does_not_break_the_real_push_call() {
    let out = run_with(
        "LIST Ops = alpha, push, gamma\n~ temp arr = #[1, 2]\n~ push(arr, 3)\nLen: {len(arr)}\n-> DONE\n",
        Dialect::Brink,
        None,
    );
    assert_eq!(out.trim(), "Len: 3");
}

/// Issue #2877 review (PR #2878): `none` is the one E035-reserved name
/// that is *itself* read bare — the Option absence literal
/// (`resolve_variable`'s NS-A1 comment) — not reached only at a call
/// site the way `push`/`len`/etc. are. So unlike the disjoint-position
/// argument the two tests above rely on, a `LIST` item literally named
/// `none` occupies the exact same bare-read syntactic position as the
/// reserved literal, and genuinely shadows it there:
/// `resolve_variable`'s `lookup_variable` finds the list item first and
/// the reference never reaches the bare-`none`-literal fallback in
/// `resolve.rs`. `LIST_VALUE(none)` disambiguates the two readings —
/// only the list item has an ordinal for `LIST_VALUE` to report, so a
/// clean compile printing that ordinal proves the list item won, not
/// the (textually identical, `Hi: none`) Option-literal rendering. No
/// `E035` fires all the same, but for the blunter reason `docs/stdlib-
/// spec.md` item 3 now states: `List`/`ListItem` are outside the
/// warned-kind set outright, not because this collision had nothing to
/// report.
#[test]
fn list_item_named_none_shadows_option_literal_at_bare_reference_without_e035_warning() {
    let source = "LIST Answers = yes, none = 7, maybe\n\nVal: {LIST_VALUE(none)}\n-> DONE\n";
    let out = compile_with(source, Dialect::Brink, None).expect("compile");
    assert!(
        !out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "a list item is not in E035's warn set, expected no warning, got {:?}",
        out.warnings
    );
    let text = run_with(source, Dialect::Brink, None);
    assert_eq!(text.trim(), "Val: 7");
}

/// Issue #2877 review (PR #2878): `docs/stdlib-spec.md` item 6/F6 pins that
/// the `E113` protocol-name reservation (`display`/`compare`/`next`) and
/// the `E035` shadow warning are NOT the same rule scoped by dialect —
/// `E113` is brink-only (the protocol registry doesn't exist under
/// strict-ink), while `E035` never reserved these three names in *either*
/// dialect to begin with (they are absent from `is_builtin_function`,
/// `is_t1b_stdlib_name`, and the special-cased `none`). A knot named
/// `display` therefore gets `E113` under brink and nothing at all under
/// strict-ink, but never `E035` in either dialect.
#[test]
fn protocol_name_never_gets_e035_in_either_dialect() {
    let source = "== display ==\nHello.\n-> DONE\n";

    // Brink: E113 fires (hard error); E035 must not appear alongside it.
    let brink_err = compile_with(source, Dialect::Brink, None)
        .expect_err("display should be E113-reserved under brink");
    let brink_compiler::CompileError::Diagnostics(diags) = brink_err else {
        panic!("expected a Diagnostics compile error, got a different variant");
    };
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E113),
        "expected E113, got {diags:?}"
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E035),
        "E035 never reserved `display` — expected no E035 alongside E113, got {diags:?}"
    );

    // Strict-ink: no protocol registry at all — clean compile, no E113,
    // and (the point of this test) still no E035 either.
    let out = compile_with(source, Dialect::StrictInk, None)
        .expect("strict-ink has no protocol registry, display is an ordinary knot name");
    assert!(
        out.warnings.is_empty(),
        "expected zero warnings under strict-ink, got {:?}",
        out.warnings
    );
}

/// `docs/diagnostics/E035.md`'s locals shape (PR #2865 review, issue
/// #2862): a `temp` local named after a classic uppercase built-in
/// (`MAX`) is not covered by E035 at all — `insert_local`
/// (`brink-analyzer::manifest`) never runs the module-level shadow-warn
/// check. And unlike `VAR`/`List`, `resolve_function`'s locals arm
/// (`lookup_local_in_scope`) is not gated by `reserved_call_site`: a local
/// DOES claim its own reserved-name call site. So this compiles clean
/// under strict-ink with no E035 and no E183, then faults at runtime with
/// `RuntimeError::NotCallable("int")` — the mirror image of the `VAR`/
/// `CONST` cases above, which fall through to the real builtin instead of
/// faulting.
#[test]
fn local_named_builtin_faults_at_runtime_not_callable() {
    let source = "\
~ temp MAX = 10

value: {MAX(1, 2)}
-> DONE
";
    let fault = run_expecting_fault(source, Dialect::StrictInk, Some(TypePolicy::Gradual));
    assert!(
        matches!(fault, Some(brink_runtime::RuntimeError::NotCallable(_))),
        "expected a NotCallable runtime fault, got {fault:?}"
    );
}

/// Issue #2867: the param-shaped sibling of
/// `local_named_builtin_faults_at_runtime_not_callable` above.
/// `lookup_local_in_scope` (`resolve.rs`) can only ever return
/// `SymbolKind::Param` or `SymbolKind::Temp` (the complete enumeration —
/// see `push_local`'s call sites in `brink_ir::symbols::project`); the
/// `temp` test above pins one, this pins the other, exactly as issue
/// #2867 asks ("pin the param variant alongside the temp variant — #2865
/// pinned only the strict-ink `temp` shape, deliberately, since it was
/// documenting rather than fixing"). `=== function f(MAX) ===` shadows the
/// classic uppercase built-in `MAX` as its own parameter name; calling
/// `MAX(1, 2)` inside `f`'s body compiles clean under strict-ink (no
/// `E035`, no `E183`) and then faults at runtime with
/// `RuntimeError::NotCallable("int")` — identical shape to the `temp`
/// case, confirmed live against the real compiler + VM. This test
/// documents current (unfixed) behavior; it does not assert this is
/// correct. See issue #2867 for the maintainer ruling this is pending on.
#[test]
fn param_named_builtin_faults_at_runtime_not_callable() {
    let source = "\
value: {f(0)}
-> DONE

=== function f(MAX) ===
~ return MAX(1, 2)
";
    let fault = run_expecting_fault(source, Dialect::StrictInk, Some(TypePolicy::Gradual));
    assert!(
        matches!(fault, Some(brink_runtime::RuntimeError::NotCallable(_))),
        "expected a NotCallable runtime fault, got {fault:?}"
    );
}
