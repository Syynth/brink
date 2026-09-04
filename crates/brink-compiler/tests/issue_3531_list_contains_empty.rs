//! Issue #3531: `l ? ()` was `true` and `l !? ()` `false` — brink's list
//! contains was the vacuous subset test, while ink's
//! `InkList.Contains(other)` returns false whenever either list is empty
//! (and `!?` is its negation). Reference outputs below are inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6); the corpus case with a C# golden
//! is owed (dotnet, maintainer-local).

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

/// Every empty/non-empty combination on both sides, `?` and `!?`.
/// Reference (inkjs): `l ? ()` false, `l !? ()` true, `() ? ()` false,
/// `() !? ()` true, `() ? l` false, `() !? l` true; the non-empty cases
/// unchanged.
#[test]
fn contains_is_false_when_either_operand_is_empty() {
    let src = "LIST l = (a), b\n-> k\n\n=== k ===\n~ temp e = l - a\n\
               {(l ? e)}\n{(l !? e)}\n{(e ? e)}\n{(e !? e)}\n{(e ? l)}\n{(e !? l)}\n\
               {(l ? a)}\n{(l !? b)}\n{(l ? (a, b))}\n{(l !? (a, b))}\n-> END\n";
    assert_eq!(
        play(src),
        [
            "false", "true", "false", "true", "false", "true", "true", "true", "false", "true"
        ]
        .iter()
        .map(|s| format!("{s}\n"))
        .collect::<Vec<_>>()
    );
}

/// The shrunk differential shape: a list emptied by subtraction, then
/// `!?` against itself. Reference: `true`.
#[test]
fn emptied_list_hasnt_itself() {
    let src = "LIST l = (a), (b)\n-> k\n\n=== k ===\n~ l -= (l ^ (l ^ l))\n{(l !? l)}\n-> END\n";
    assert_eq!(play(src), vec!["true\n"]);
}
