//! Issue #3527: `Choice.index` counted invisible fallback choices, so a
//! fallback ahead of a visible choice — a thread's `+ ->` merged in front
//! of the main flow's choices, or a `+ ->` written before its siblings —
//! made the visible indices skip (`0, 2` where ink numbers `0, 1`), and
//! `choose(1)` addressed the fallback. ink's `Choice.index` numbers
//! `currentChoices`, which holds visible choices only, and
//! `ChooseChoiceIndex` takes that number. Reference outputs below are
//! inkjs 2.4.0 via `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6); the corpus case with a C# golden
//! is owed (dotnet, maintainer-local).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file, play to the first choice point, return the
/// presented `(text, index)` pairs, then choose `pick` and return the
/// lines that follow.
fn choices_then(source: &str, pick: usize) -> (Vec<(String, usize)>, Vec<String>) {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut presented = Vec::new();
    let mut after = Vec::new();
    let mut chosen = false;
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => {
                if chosen {
                    after.push(l.text.clone());
                }
            }
            Step::Choices(choices) => {
                assert!(!chosen, "a second choice point in {source}");
                presented = choices.iter().map(|c| (c.text.clone(), c.index)).collect();
                story.choose(pick).expect("choose");
                chosen = true;
            }
            Step::Done | Step::End | Step::Suspended => return (presented, after),
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

fn pairs(v: &[(&str, usize)]) -> Vec<(String, usize)> {
    v.iter().map(|(t, i)| ((*t).to_owned(), *i)).collect()
}

const THREAD_WITH_FALLBACK: &str = "-> k\n\n=== k ===\n<- t\n* [x]\n    x\n    -> END\n+ -> END\n\n=== t ===\n* [a]\n    a\n    -> DONE\n+ -> END\n";

/// The shrunk differential counterexample: the thread's fallback sits
/// ahead of the main flow's choice. Reference: `a` 0, `x` 1; choosing 1
/// prints `x`.
#[test]
fn thread_fallback_does_not_occupy_an_index() {
    let (presented, after) = choices_then(THREAD_WITH_FALLBACK, 1);
    assert_eq!(presented, pairs(&[("a", 0), ("x", 1)]));
    assert_eq!(after, vec!["x\n"]);
}

/// …and choosing 0 takes the thread's choice. Reference: `a`.
#[test]
fn thread_choice_is_index_zero() {
    let (presented, after) = choices_then(THREAD_WITH_FALLBACK, 0);
    assert_eq!(presented, pairs(&[("a", 0), ("x", 1)]));
    assert_eq!(after, vec!["a\n"]);
}

/// A fallback written first in a plain weave. Reference: `x` 0, `y` 1;
/// choosing 1 prints `y`.
#[test]
fn fallback_written_first_does_not_occupy_an_index() {
    let src = "-> k\n\n=== k ===\n+ -> END\n* [x]\n    x\n    -> END\n* [y]\n    y\n    -> END\n";
    let (presented, after) = choices_then(src, 1);
    assert_eq!(presented, pairs(&[("x", 0), ("y", 1)]));
    assert_eq!(after, vec!["y\n"]);
}

/// An index past the visible choices is an error naming the visible
/// count, not the pending count.
#[test]
fn index_past_the_visible_choices_is_rejected() {
    let output = brink_compiler::compile("story.ink", |_| Ok(THREAD_WITH_FALLBACK.to_owned()))
        .expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    loop {
        match story.continue_single().expect("runtime") {
            Step::Choices(_) => break,
            Step::Line(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    let err = story
        .choose(2)
        .expect_err("index 2 is past the two visible choices");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::InvalidChoiceIndex {
                index: 2,
                available: 2
            }
        ),
        "{err:?}"
    );
}
