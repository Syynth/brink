//! Issue #3395: lifting an inline construct evaluated its condition (or a
//! sequence's selection) BEFORE the prefix cloned around it, so a prefix
//! side effect (`{bump()}{n == 1:yes|no}`) or a prefix read the condition
//! then mutates (`{n}{bump() == 1:yes|no}`) observed the wrong state.
//! RULED 2026-09-02 (option B): the lift hoists every prefix interpolation
//! into a hidden synthetic temp first, in source order, and the clones read
//! the temp. These cases compile real ink and play it against the ink
//! reference's output (inkjs 2.4.0, cross-checked against the C# oracle
//! cases in `tests/tier2/evaluation/lift-order-*`).
//!
//! The text-emitting case is the one a plain hoist gets wrong: `~ temp t =
//! shout()` emits `shout`'s text ahead of the line in ink and brink alike,
//! so codegen gives a synthetic temp's direct-call value the slot
//! composition (its printed output is captured into the value) and
//! `{$lift0}` replays it where the original `{shout()}` stood.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file, play it choosing `choices` in order, and
/// return every line of text.
fn play(source: &str, choices: &[usize]) -> Vec<String> {
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
    let mut picks = choices.iter().copied();
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => lines.push(l.text.trim().to_owned()),
            Step::Choices(_) => {
                let Some(pick) = picks.next() else {
                    return lines;
                };
                story.choose(pick).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

const BUMP: &str = "VAR n = 0\n-> k\n=== function bump() ===\n~ n = n + 1\n~ return n\n";

/// `tests/tier2/evaluation/lift-order-fn-then-cond`: the call runs first,
/// the condition sees its side effect. Reference: `1yes`.
#[test]
fn call_then_condition_reading_its_effect() {
    let src = format!("{BUMP}=== k ===\n{{bump()}}{{n == 1:yes|no}}\n-> END\n");
    assert_eq!(play(&src, &[]), vec!["1yes"]);
}

/// The reverse shape the ruling named as the one a "hoist only calls" fix
/// would miss: the prefix READ must see the value before the condition's
/// own call mutates it. Reference: `0yes`.
#[test]
fn read_then_effectful_condition() {
    let src = format!("{BUMP}=== k ===\n{{n}}{{bump() == 1:yes|no}}\n-> END\n");
    assert_eq!(play(&src, &[]), vec!["0yes"]);
}

/// `tests/tier2/evaluation/lift-order-seq-fn-cond`: a stateful sequence,
/// a call, and a conditional on one line, viewed three times. Reference:
/// `a1small / b2big / c3big` — the second view is the one the old order
/// got wrong (`b2small`).
#[test]
fn sequence_call_and_condition_across_three_views() {
    let src = format!(
        "{BUMP}=== k ===\n+ [again]\n    {{a|b|c}}{{bump()}}{{n > 1:big|small}}\n    -> k\n"
    );
    assert_eq!(play(&src, &[0, 0, 0]), vec!["a1small", "b2big", "c3big"]);
}

/// The control the ruling asked for: condition first, then the call — was
/// already right, must stay right. Reference: `yes1`.
#[test]
fn condition_then_call_is_unchanged() {
    let src = format!("{BUMP}=== k ===\n{{n == 0:yes|no}}{{bump()}}\n-> END\n");
    assert_eq!(play(&src, &[]), vec!["yes1"]);
}

/// A prefix call that PRINTS: its text must stay inline where the call
/// stood (`aXyes`), not land ahead of the line — the hole a plain `~ temp`
/// hoist opens (measured: `X` / `ayes`), closed by the synthetic temp's
/// slot composition in codegen. Reference: `aXyes`.
#[test]
fn text_emitting_prefix_call_keeps_its_text_inline() {
    let src = "VAR n = 0\n-> k\n=== function shout() ===\n~ n = n + 1\nX\n=== k ===\na{shout()}{n == 1:yes|no}\n-> END\n";
    assert_eq!(play(src, &[]), vec!["aXyes"]);
}

/// A prefix call with no return value prints nothing, and its side effect
/// is still ordered before the condition. Reference: `yes`.
#[test]
fn void_prefix_call_prints_nothing_and_orders_its_effect() {
    let src = "VAR n = 0\n-> k\n=== function bumpv() ===\n~ n = n + 1\n=== k ===\n{bumpv()}{n == 1:yes|no}\n-> END\n";
    assert_eq!(play(src, &[]), vec!["yes"]);
}

/// Two constructs on one line, a call between them: the inner lift hoists
/// the middle call inside each outer branch, so all three evaluate in
/// source order. Reference: `1yes2yes`.
#[test]
fn two_constructs_with_calls_between_evaluate_in_source_order() {
    let src = format!(
        "{BUMP}=== k ===\n{{bump()}}{{n == 1:yes|no}}{{bump()}}{{n == 2:yes|no}}\n-> END\n"
    );
    assert_eq!(play(&src, &[]), vec!["1yes2yes"]);
}

/// A `once` sequence lifted with a prefix call: the synthesized exhausted
/// branch reads the same hoisted temp, so the call still runs exactly once
/// per view after the sequence is spent. Reference: `1a / 2b / 3`.
#[test]
fn once_sequence_exhausted_branch_reads_the_hoisted_temp() {
    let src = format!("{BUMP}=== k ===\n+ [again]\n    {{bump()}}{{!a|b}}\n    -> k\n");
    assert_eq!(play(&src, &[0, 0, 0]), vec!["1a", "2b", "3"]);
}
