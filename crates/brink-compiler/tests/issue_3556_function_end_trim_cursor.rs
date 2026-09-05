//! Issue #3556: `trim_function_end` walked behind the read cursor.
//!
//! A function whose body spans a yield point — it printed a line, the
//! consumer took it, and only then did the function return — has an
//! `OutputMark` recorded before parts that have since been delivered.
//! Trimming those left `transcript.len() < cursor` and the next reader of
//! `transcript[cursor..]` panicked with a slice-range error.
//!
//! The hole predates #3536; #3536's widening of the trimmable set (a
//! `ValueRef` that renders as whitespace) is what made the walk reach far
//! enough to fall through it. Every story below panicked on `main` before
//! the cursor floor landed.
//!
//! Reference outputs are inkjs 2.4.0 via `tools/inkjs-oracle` (the
//! sanctioned stand-in, `docs/program-generator-spec.md` §6).

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

/// The function every case below calls: it prints a line, which the
/// consumer takes mid-call, and then ends with an empty list — output the
/// trim considers trimmable. Its `OutputMark` is 0 and the cursor is 2 by
/// the time it returns.
const F: &str = "\n=== function f() ===\n1\n{l}\n~ return true\n";

const PRELUDE: &str = "LIST l = li\n-> k\n\n=== k ===\n";

/// Called as a statement.
#[test]
fn a_statement_call_that_spans_a_yield_point() {
    assert_eq!(
        play(&format!("{PRELUDE}~ f()\nb\n-> END\n{F}")),
        ["1\n", "b\n"]
    );
}

/// Called as a conditional's condition. This is the shape `brink-gen`'s
/// `both_roads_agree` found (seed `cc 8def00a8…`).
#[test]
fn a_condition_call_that_spans_a_yield_point() {
    assert_eq!(
        play(&format!("{PRELUDE}{{ f():\n    a\n}}\n-> END\n{F}")),
        ["1\n", "a\n"]
    );
}

/// The function's trailing whitespace output is still trimmed — the floor
/// stops the walk at the cursor, it does not disable the trim. Without
/// #3536 these carried a spurious blank line between the two.
#[test]
fn the_unread_tail_is_still_trimmed() {
    for src in [
        format!("{PRELUDE}~ f()\nb\n-> END\n{F}"),
        format!("{PRELUDE}{{ f():\n    a\n}}\n-> END\n{F}"),
    ] {
        let lines = play(&src);
        assert!(
            !lines.iter().any(|l| l.trim().is_empty()),
            "a blank line survived the trim: {lines:?}\n{src}"
        );
    }
}

/// A function that yields mid-call and then ends *visibly* keeps that
/// output — the floor must not swallow the tail it was never meant to.
#[test]
fn a_visible_tail_after_a_yield_point_survives() {
    let f = "\n=== function f() ===\n1\nx\n~ return true\n";
    assert_eq!(
        play(&format!("{PRELUDE}~ f()\nb\n-> END\n{f}")),
        ["1\n", "x\n", "b\n"]
    );
}
