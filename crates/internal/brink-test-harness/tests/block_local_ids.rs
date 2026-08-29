//! Block-local anonymous-id regression suite (2026-08-29 re-scoping
//! ruling; `brink-ir::hir::stamp`'s "Counter scoping and edit stability").
//!
//! The unit-level pins (counter locality, label anchoring, distinct
//! sequence-branch ids) live in `brink-ir`'s `stamp::tests`. This file
//! carries the full-pipeline half of the E060 fix: a block-level
//! `{stopping:}` with a once-only choice in TWO branches is legal ink
//! that failed to compile at all before the fix — branch bodies recursed
//! under the wrapper's scope with fresh per-branch counters, stamping
//! both choices `{wrapper}.c-0` and tripping the #1673 duplicate-id
//! guard. It must compile AND play correctly: each branch's choice is
//! its own container with its own once-only state.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

fn compile(src: &str) -> brink_compiler::CompileOutput {
    brink_compiler::compile("t.ink", |p| {
        assert_eq!(p, "t.ink", "single-file fixture");
        Ok(src.to_string())
    })
    .expect("fixture compiles")
}

#[test]
fn choices_in_two_sequence_branches_compile_and_keep_separate_state() {
    let out = compile(
        "-> start\n\
         === start ===\n\
         {stopping:\n\
         - first visit\n\
           * choice A\n\
             picked A\n\
             -> start\n\
         - later visit\n\
           * choice B\n\
             picked B\n\
             -> DONE\n\
         }\n\
         -> DONE\n",
    );

    let (program, line_tables) = brink_runtime::link(&out.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    // Drive: start → "first visit" → [choice A] → "picked A" → start →
    // "later visit" → [choice B] → "picked B". Choice A's once-only state
    // must not alias choice B's (one shared id made this fixture
    // uncompilable before; distinct ids are what make the second visit
    // offer B, not nothing).
    let mut transcript = Vec::new();
    let mut picks = 0usize;
    loop {
        match story.continue_single().expect("runtime step") {
            Step::Line(line) => transcript.push(line.text.trim().to_string()),
            Step::Choices(choices) => {
                let texts: Vec<&str> = choices.iter().map(|c| c.text.trim()).collect();
                match picks {
                    0 => assert_eq!(texts, ["choice A"], "first visit offers branch 1's choice"),
                    1 => assert_eq!(
                        texts,
                        ["choice B"],
                        "second visit offers branch 2's choice — its own container, \
                         its own once-only state"
                    ),
                    n => panic!("unexpected third choice point (pick {n}): {texts:?}"),
                }
                picks += 1;
                story.choose(0).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => break,
        }
    }
    assert_eq!(
        transcript,
        [
            "first visit",
            "choice A", // chosen text echoes as a line (ink semantics)
            "picked A",
            "later visit",
            "choice B",
            "picked B"
        ],
        "both branches' choice bodies must run"
    );
}
