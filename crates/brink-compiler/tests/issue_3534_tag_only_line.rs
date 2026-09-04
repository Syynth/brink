//! Issue #3534: a tag-only line (`# tag` on a line of its own) lowered to
//! a blank content line plus a newline, so its tags rode a line ink never
//! has: ink's parser appends a line's `"\n"` only when the line is not
//! pure tags, and the runtime attaches a tag-only line's tags to the next
//! line's newline. Reference outputs below are inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6); `tests/tests_github/
//! bobon4uto__dream_on` carries the C# golden.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile `source` and play it to the end, always taking the first
/// choice; each delivered line lands as `(text, tags)`, a choice point as
/// `("<choices>", [])`.
fn play(source: &str) -> Vec<(String, Vec<String>)> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut steps = Vec::new();
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => steps.push((l.text.clone(), l.tags.clone())),
            Step::Choices(_) => {
                steps.push(("<choices>".to_owned(), Vec::new()));
                story.choose(0).expect("choose");
            }
            Step::Done | Step::End => return steps,
            Step::Suspended => panic!("unexpected suspension in {source}"),
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

fn line(text: &str, tags: &[&str]) -> (String, Vec<String>) {
    (
        text.to_owned(),
        tags.iter().map(|t| (*t).to_owned()).collect(),
    )
}

/// A tag line between two content lines tags the second; two stacked tag
/// lines both land there, in order.
#[test]
fn tag_line_tags_the_next_line() {
    assert_eq!(
        play("a\n# tag\nb\n-> END\n"),
        [line("a\n", &[]), line("b\n", &["tag"])]
    );
    assert_eq!(
        play("a\n# t1\n# t2\nb\n-> END\n"),
        [line("a\n", &[]), line("b\n", &["t1", "t2"])]
    );
}

/// A tag line at the top of a knot tags its first line; inside a choice
/// body it tags the body's first line.
#[test]
fn tag_line_at_a_block_start() {
    assert_eq!(play("# tag\na\n-> END\n"), [line("a\n", &["tag"])]);
    assert_eq!(
        play("a\n* x\n  # tag\n  b\n  -> END\n"),
        [
            line("a\n", &[]),
            line("<choices>", &[]),
            line("x\n", &[]),
            line("b\n", &["tag"])
        ]
    );
}

/// A tag line before a blank interpolation line: the tags ride the blank
/// line (it is the next newline), and #3533's boundary rule leaves the
/// tagged line alone because a tagged line is never blank.
#[test]
fn tag_line_before_a_blank_line() {
    assert_eq!(
        play("VAR e = ()\na\n# tag\n{e}\nb\n-> END\n"),
        [line("a\n", &[]), line("\n", &["tag"]), line("b\n", &[])]
    );
}

/// The corpus shape (`tests_github/bobon4uto__dream_on`): three tag lines
/// inside a conditional block, then the block's text.
#[test]
fn stacked_tag_lines_inside_a_conditional_block() {
    let src =
        "VAR n = 0\n-> k\n=== k ===\n{ n == 0:\n# CLEAR\n# CLASS: end\nThey are here.\n}\n-> END\n";
    assert_eq!(
        play(src),
        [line("They are here.\n", &["CLEAR", "CLASS: end"])]
    );
}
