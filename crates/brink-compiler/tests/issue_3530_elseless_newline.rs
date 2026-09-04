//! Issue #3530: a whole-line `{cond:then}` with no else arm lost its line's
//! newline when the condition itself printed. The lift gives the then-arm a
//! line clone plus an end-of-line, but synthesizes an else arm only when
//! there is prefix or suffix text to carry — so with the construct alone on
//! its line, the all-false path emitted no end-of-line at all.
//!
//! ink keeps the line's `\n` whichever arm ran; it is suppressed only when
//! the line produced no content, and here the condition's call printed.
//!
//! Reference outputs below are inkjs 2.4.0 via `tools/inkjs-oracle` (the
//! sanctioned stand-in, `docs/program-generator-spec.md` §6).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file and play it, returning each delivered line's
/// text verbatim (newlines included — they are what this issue is about).
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

/// The issue's own repro. The condition's call prints `a`, the condition is
/// false, so the then-arm's `a` never runs — but the line's newline is real
/// and ink keeps it. inkjs: `a\n`, `b\n`. brink printed `ab`.
#[test]
fn elseless_conditional_keeps_its_newline_when_the_condition_prints() {
    let src = concat!(
        "-> k\n",
        "\n",
        "=== k ===\n",
        "{f():a}\n",
        "b\n",
        "-> END\n",
        "\n",
        "=== function f() ===\n",
        "a\n",
        "~ return false\n",
    );
    assert_eq!(play(src), ["a\n", "b\n"]);
}

/// The true arm of the same shape: the condition prints `a`, is true, so the
/// then-arm's `c` runs too. One line, then `b`.
#[test]
fn elseless_conditional_keeps_its_newline_when_the_condition_is_true() {
    let src = concat!(
        "-> k\n",
        "\n",
        "=== k ===\n",
        "{f():c}\n",
        "b\n",
        "-> END\n",
        "\n",
        "=== function f() ===\n",
        "a\n",
        "~ return true\n",
    );
    assert_eq!(play(src), ["ac\n", "b\n"]);
}

/// The guard against over-correcting: a silent false condition still emits
/// nothing at all. Synthesizing an else arm that holds only an end-of-line
/// must not turn `{false:a}` into a blank line — the runtime suppresses a
/// newline with no content before it, and this pins that.
#[test]
fn a_silent_false_conditional_still_emits_no_line() {
    let src = concat!(
        "-> k\n",
        "\n",
        "=== k ===\n",
        "{false:a}\n",
        "b\n",
        "-> END\n",
    );
    assert_eq!(play(src), ["b\n"]);
}

/// And a silent true condition is unchanged: one line, its own newline.
#[test]
fn a_silent_true_conditional_emits_its_line() {
    let src = concat!(
        "-> k\n",
        "\n",
        "=== k ===\n",
        "{true:a}\n",
        "b\n",
        "-> END\n",
    );
    assert_eq!(play(src), ["a\n", "b\n"]);
}
