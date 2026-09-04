//! Issue #3508: runs of whitespace inside a choice's presented text were
//! collapsed to one space — `* [a  0]` presented `a 0` where ink presents
//! `a  0`. inklecate keeps text verbatim (`"^a  0"` in the compiled JSON),
//! the C# runtime collapses runs only when rendering an OUTPUT line
//! (`CleanOutputWhitespace`), and a choice's text is the evaluated string
//! trimmed of leading/trailing spaces and tabs — interior runs untouched.
//! brink collapses at compile time, into the line table, which is
//! observably the same for output lines and wrong for choice text; codegen
//! now leaves a choice's display entries verbatim. Reference outputs are
//! inkjs 2.4.0 via `tools/inkjs-oracle`; the capture-tier case is
//! `tests/tier4-generated/choice-text-whitespace-run`.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// What a host sees: presented choice texts (as `choice:<text>`) and
/// delivered lines (verbatim, newline included), in order. Always picks the
/// first choice.
fn observe(source: &str) -> Vec<String> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut seen = Vec::new();
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => seen.push(l.text.clone()),
            Step::Choices(choices) => {
                for c in &choices {
                    seen.push(format!("choice:{}", c.text));
                }
                story.choose(0).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => return seen,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

/// The shrunk differential counterexample. Reference: choice `a  0`.
#[test]
fn bracket_choice_text_keeps_the_run() {
    assert_eq!(observe("* [a  0]\n-> END\n"), ["choice:a  0"]);
}

/// Start text is BOTH the choice text (verbatim) and, after choosing, an
/// output line (collapsed, like every output line). References: `a  0`
/// then `a 0`.
#[test]
fn start_text_is_verbatim_as_choice_and_collapsed_as_output() {
    assert_eq!(observe("* a  0\n-> END\n"), ["choice:a  0", "a 0\n"]);
    assert_eq!(
        observe("* a  0[b  1]c  2\n-> END\n"),
        ["choice:a  0b  1", "a 0c 2\n"]
    );
}

/// Tabs are kept too, and leading/trailing whitespace is still trimmed.
/// References: `a\t\t0`, `a  0`.
#[test]
fn tabs_are_kept_and_the_ends_are_trimmed() {
    assert_eq!(observe("* [a\t\t0]\n-> END\n"), ["choice:a\t\t0"]);
    assert_eq!(observe("* [  a  0  ]\n-> END\n"), ["choice:a  0"]);
}

/// A templated display (interpolation inside the brackets) keeps its
/// literal's run. Reference: `a  1`.
#[test]
fn templated_choice_text_keeps_the_run() {
    assert_eq!(observe("VAR x = 1\n* [a  {x}]\n-> END\n"), ["choice:a  1"]);
}

/// Conditioned and tagged choices go through the same display path.
/// References: `a  0` both.
#[test]
fn conditioned_and_tagged_choices_keep_the_run() {
    assert_eq!(observe("* {true} [a  0]\n-> END\n"), ["choice:a  0"]);
    assert_eq!(observe("* [a  0] #t  ag\n-> END\n"), ["choice:a  0", "\n"]);
}

/// Output lines still collapse, exactly as before. Reference: `a 0`.
#[test]
fn output_lines_still_collapse() {
    assert_eq!(observe("a  0\n-> END\n"), ["a 0\n"]);
}
