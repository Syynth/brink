//! Issue #3698: a divert written on a choice line runs before the line's
//! newline in every form ink allows there, not only `-> target`. brink
//! used to lift a plain `-> k` (keeping just its first target) and leave
//! `->->`, `-> t ->` and `-> a -> b` to the body, after the newline: the
//! line ended early, and a multi-target divert lost every target after
//! the first.
//!
//! Every expected output is inkjs 2.4.0 via `tools/inkjs-oracle` (the
//! sanctioned stand-in, `docs/program-generator-spec.md` §6). The tier4
//! cases `choice-condition-call-tunnel-return`, `choice-line-tunnel-call`,
//! `choice-line-tunnel-call-bracketed` and `choice-line-multi-target-divert`
//! pin the episode-level shapes; these cover the rest.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Play `source`, taking the first choice whenever one is offered, and
/// return the lines it delivers.
fn lines(source: &str) -> Vec<String> {
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
    for _ in 0..50 {
        match story.continue_single().expect("plays") {
            Step::Line(l) => lines.push(l.text),
            Step::Choices(_) => story.choose(0).expect("choose"),
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("no end in 50 steps\n{source}");
}

#[test]
fn a_tunnel_return_on_a_choice_line_keeps_the_line_open() {
    assert_eq!(
        lines("-> t ->\nafter\n-> END\n=== t\n+ hi ->->\n"),
        ["hi after\n"]
    );
}

/// The original report: a choice condition's call prints, the condition
/// is false, and the fallback's `->->` returns before the newline.
#[test]
fn a_condition_call_glues_to_the_text_after_the_tunnel() {
    assert_eq!(
        lines(
            "-> t ->\nafter\n-> END\n=== t\n* {f()} [x]\n    ->->\n+ ->->\n\
             === function f()\n{\"golf\"}\n~ return false\n"
        ),
        ["golfafter\n"]
    );
}

#[test]
fn every_target_of_a_multi_target_divert_runs() {
    assert_eq!(
        lines("+ hi -> a -> b\n=== a\n<>A<>\n->->\n=== b\nB\n-> END\n"),
        ["hi AB\n"]
    );
}

#[test]
fn a_tunnel_call_ends_the_line_when_it_returns() {
    assert_eq!(
        lines("+ hi -> t ->\n  after\n  -> END\n=== t\nin\n->->\n"),
        ["hi in\n", "after\n"]
    );
}

#[test]
fn a_thread_on_a_choice_line_runs_before_the_newline() {
    assert_eq!(
        lines("* hi <- t\n  after\n  -> END\n=== t\n<>T<>\n-> DONE\n"),
        ["hi Tafter\n"]
    );
}

/// The form that already worked keeps working.
#[test]
fn a_plain_divert_still_glues_to_its_target() {
    assert_eq!(
        lines("+ hi -> k\n=== k\nthere\n-> END\n"),
        ["hi there\n"]
    );
}
