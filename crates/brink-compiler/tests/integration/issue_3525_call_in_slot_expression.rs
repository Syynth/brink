//! Issue #3525: a printing function called INSIDE a slot expression —
//! `a{f() == "x"}`, `a{n + f()}` — had its output emitted before the
//! line's earlier text, because only a slot that IS a call was composed
//! (its output captured into the slot with its value; the 2026-08-01
//! "Content-as-value" composition). A compound slot was evaluated bare
//! before `emit_line`, so the call's text reached the transcript ahead of
//! the `LineRef`. Any slot containing a call now composes. Reference
//! outputs below are inkjs 2.4.0 via `tools/inkjs-oracle` (the sanctioned
//! stand-in, `docs/program-generator-spec.md` §6); the corpus case with a
//! C# golden is owed (dotnet, maintainer-local).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file and play it to the end, returning every
/// delivered line's text verbatim.
fn play(source: &str) -> Vec<String> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => lines.push(l.text.clone()),
            Step::Choices(_) => panic!("unexpected choices in {source}"),
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

/// `f` prints `a!` and returns `"alpha"`.
const STR_F: &str = "\n=== function f() ===\na!\n~ return \"alpha\"\n";

/// The shrunk differential counterexample. Reference: `aa!true`.
#[test]
fn comparison_with_a_call_keeps_the_prefix_first() {
    let src = format!("a{{(f() == \"alpha\")}}\n-> END\n{STR_F}");
    assert_eq!(play(&src), vec!["aa!true\n"]);
}

/// Arithmetic with a call. Reference: `aa!3c`.
#[test]
fn arithmetic_with_a_call_keeps_the_prefix_first() {
    let src = "VAR n = 1\na{n + f()}c\n-> END\n\n=== function f() ===\na!\n~ return 2\n";
    assert_eq!(play(src), vec!["aa!3c\n"]);
}

/// Two calls under `and`, with text before the slot (issue #3519's
/// sibling shape). Reference: `xaatruey`.
#[test]
fn two_calls_under_and_keep_the_prefix_first() {
    let src =
        "x{f(true) and f(true)}y\n-> END\n\n=== function f(p) ===\n{ true:\n    a\n}\n~ return p\n";
    assert_eq!(play(src), vec!["xaatruey\n"]);
}

/// A bare call was already composed; unchanged. Reference: `aa!alpha`.
#[test]
fn bare_call_control() {
    let src = format!("a{{f()}}\n-> END\n{STR_F}");
    assert_eq!(play(&src), vec!["aa!alpha\n"]);
}

/// No text before the slot was already right (nothing to reorder);
/// unchanged. Reference: `a!trueb`.
#[test]
fn no_prefix_control() {
    let src = format!("{{(f() == \"alpha\")}}b\n-> END\n{STR_F}");
    assert_eq!(play(&src), vec!["a!trueb\n"]);
}

/// A call-free compound slot is not composed, so an empty-slot line keeps
/// its existing shape. Reference: `a3c`.
#[test]
fn call_free_compound_slot_is_unchanged() {
    let src = "VAR n = 1\na{n + 2}c\n-> END\n";
    assert_eq!(play(src), vec!["a3c\n"]);
}
