//! Issue #3383: a divert (or any authored terminator) at the end of a
//! *nested* gather was silently replaced by the exit to the enclosing
//! gather — `patch_innermost_gather` overwrote the inner gather's last
//! statement instead of leaving a body that already transfers control
//! alone. Found by the story-level generator's first smoke run (#3378).
//!
//! Each case compiles real `.ink` through the pipeline and plays it,
//! asserting the text the C# reference produces for the same choices.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file, play it choosing `choices` in order, and
/// return every line of text plus the terminal step name.
fn play(source: &str, choices: &[usize]) -> (Vec<String>, &'static str) {
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
    let mut terminal = None;
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => lines.push(l.text.trim().to_owned()),
            Step::Choices(_) => {
                let pick = picks.next().expect("more choices offered than supplied");
                story.choose(pick).expect("choose");
            }
            Step::Done => terminal = Some("done"),
            Step::End => terminal = Some("end"),
            Step::Suspended => terminal = Some("suspended"),
        }
        if terminal.is_some() {
            break;
        }
    }
    assert!(
        terminal.is_some(),
        "story did not reach a terminal step in 200 steps"
    );
    (lines, terminal.expect("just asserted above"))
}

#[test]
fn divert_after_nested_gather_is_taken() {
    let src = "-> a\n=== a ===\n* [q7]\n    + + [a6med]\n    - -\n    -> b\n* -> END\n=== b ===\nlast line\n-> DONE\n";
    let (lines, terminal) = play(src, &[0, 0]);
    assert_eq!(lines, vec!["last line"], "terminal={terminal}");
    assert_eq!(terminal, "done");
}

#[test]
fn divert_after_nested_gather_with_text_is_taken() {
    let src = "-> a\n=== a ===\n* [q7]\n    + + [a6med]\n    - -\n    gathered text\n    -> b\n* -> END\n=== b ===\nlast line\n-> DONE\n";
    let (lines, _) = play(src, &[0, 0]);
    assert_eq!(lines, vec!["gathered text", "last line"]);
}

#[test]
fn authored_done_at_nested_gather_ends_the_flow() {
    // `-> DONE` inside the nested gather must NOT be rewritten into a
    // jump to the outer gather: the outer gather's text is never reached.
    let src = "-> a\n=== a ===\n* [q7]\n    + + [a6med]\n    - -\n    inner done\n    -> DONE\n- outer gather\n-> END\n";
    let (lines, terminal) = play(src, &[0, 0]);
    assert_eq!(lines, vec!["inner done"]);
    assert_eq!(terminal, "done");
}

#[test]
fn empty_nested_gather_falls_into_the_outer_gather() {
    // The one synthesized terminator a continuation can carry: an empty
    // nested gather in root content flows on to the enclosing gather.
    let src = "* [q7]\n    + + [a6med]\n    - -\n- outer gather\n-> END\n";
    let (lines, terminal) = play(src, &[0, 0]);
    assert_eq!(lines, vec!["outer gather"]);
    assert_eq!(terminal, "end");
}
