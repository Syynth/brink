//! Issue #3386: three inline conditionals on one content line collided on
//! a `DefinitionId` (E060). Lifting nests the line's constructs, and a
//! clone's id was re-derived from `(original, host branch index)` with
//! branch 0 as the identity — so across two lift levels the clone at
//! (0, 1) and the clone at (1, 0) both derived to the same id. The salt
//! now mixes the lifting construct's own identity in (`lift_salt`), and
//! these cases compile real ink and play it against the C# reference's
//! output. Found by the story-level generator's expressions tier (#3378).

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
                let pick = picks.next().expect("more choices offered than supplied");
                story.choose(pick).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

#[test]
fn three_inline_conditionals_on_one_line_compile_and_play() {
    let src = "-> a\n=== a ===\n{false:p}{true:q|r}{true:s}\n-> END\n";
    assert_eq!(play(src, &[]), vec!["qs"]);
}

#[test]
fn four_inline_conditionals_on_one_line_compile_and_play() {
    let src = "-> a\n=== a ===\n{true:p|x}{false:q|r}{true:s|y}{false:t|z}\n-> END\n";
    assert_eq!(play(src, &[]), vec!["prsz"]);
}

#[test]
fn three_inline_conditionals_inside_a_choice_in_a_gather() {
    let src = "-> a\n=== a ===\n+ [x]\n    body\n-\n* [a]\n    {false:p}{true:q|r}{true:s}\n    -> END\n+ -> END\n";
    assert_eq!(play(src, &[0, 0]), vec!["body", "qs"]);
}
