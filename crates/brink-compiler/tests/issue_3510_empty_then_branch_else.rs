//! Issue #3510: a multi-line conditional whose then-branch is empty and
//! whose only marked arm is `- else:` ran the else arm when the condition
//! was TRUE. The lone `- else:` made every branch "have a condition", so
//! the block lowered as a switch on the condition with the else arm as its
//! default — taken whatever the value. ink reads the shape as an `if` with
//! an empty true branch. Reference outputs below are inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6).

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

/// The issue's repro: the empty then-branch is taken and prints nothing.
/// Reference: `a` / `b`.
#[test]
fn a_true_condition_takes_the_empty_then_branch() {
    let src = "a\n{true:\n- else:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a\n", "b\n"]);
}

/// A whitespace-only then-branch is the same shape. Reference: `a` / `b`.
#[test]
fn a_blank_then_branch_is_empty_too() {
    let src = "a\n{true:\n    \n- else:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a\n", "b\n"]);
}

/// Content on the `- else:` marker line. Reference: `a` / `b`.
#[test]
fn else_content_on_the_marker_line_is_still_the_else_arm() {
    let src = "a\n{true:\n- else: x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a\n", "b\n"]);
}

/// A false condition takes the else arm. Reference: `a` / `x` / `b`.
#[test]
fn a_false_condition_takes_the_else_arm() {
    let src = "a\n{false:\n- else:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a\n", "x\n", "b\n"]);
}

/// The condition is truthiness, not a switch value: `0` is false.
/// Reference: `a` / `x` / `b`.
#[test]
fn the_condition_is_truthiness_not_a_switch_value() {
    let src = "VAR v = 0\na\n{v:\n- else:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a\n", "x\n", "b\n"]);
}

/// Glue before the block: the empty arm prints nothing between `a` and
/// `b`. Reference: `a b`.
#[test]
fn glue_runs_through_the_empty_then_branch() {
    let src = "a <>\n{true:\n- else:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a b\n"]);
}

/// Glue before the block, condition false: the else arm joins `a`.
/// Reference: `a x` / `b`.
#[test]
fn glue_joins_the_else_arm_when_the_condition_is_false() {
    let src = "a <>\n{false:\n- else:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a x\n", "b\n"]);
}

/// Control: a real switch with one value arm is unchanged. Reference:
/// `a` / `x` / `b`.
#[test]
fn a_one_arm_switch_stays_a_switch() {
    let src = "a\n{1:\n- 1:\n    x\n}\nb\n-> END\n";
    assert_eq!(play(src), vec!["a\n", "x\n", "b\n"]);
}
