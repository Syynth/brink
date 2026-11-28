//! Issue #3535: glue reaches back across a blank line. A line that
//! renders empty (an empty list, an empty or whitespace-only string)
//! leaves a newline behind it; a glue after it removes that newline AND
//! keeps walking back over whitespace to the newline before it, so the
//! lines either side of the blank one join. ink's
//! `TrimNewlinesFromOutputStream` walks back until it meets non-whitespace
//! text or a control command, and removes every newline on the way.
//! Forward glue (`a <>` then a blank line) had the same defect from the
//! other side: a blank value after the glue ended its reach, so the blank
//! line's newline closed the line.
//! Reference outputs below are inkjs 2.4.0 via `tools/inkjs-oracle` (the
//! sanctioned stand-in, `docs/program-generator-spec.md` §6).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

const PRELUDE: &str = "LIST l = li\nVAR e = ()\n-> k\n\n=== k ===\n";

/// Compile `PRELUDE` + `body` and play it to the end, always taking the
/// first choice; every delivered line's text lands verbatim, a choice
/// point as `<choices>`, and the terminal as `<done>` / `<end>`.
fn play(body: &str) -> Vec<String> {
    let source = format!("{PRELUDE}{body}\n");
    let output = brink_compiler::compile("story.ink", |_| Ok(source.clone()));
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
            Step::Line(l) => steps.push(l.text.clone()),
            Step::Choices(_) => {
                steps.push("<choices>".to_owned());
                story.choose(0).expect("choose");
            }
            Step::Done => {
                steps.push("<done>".to_owned());
                return steps;
            }
            Step::End => {
                steps.push("<end>".to_owned());
                return steps;
            }
            Step::Suspended => panic!("unexpected suspension in {source}"),
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

/// The issue's shape: `a`, a blank line, then `<> b` — one line.
#[test]
fn glue_reaches_back_across_a_blank_line() {
    assert_eq!(play("a\n{e}\n<> b\n-> END"), ["a b\n", "<end>"]);
}

/// Two blank lines, and a whitespace-only one, are walked back over too.
#[test]
fn glue_reaches_back_across_several_blank_lines() {
    assert_eq!(play("a\n{e}\n{e}\n<> b\n-> END"), ["a b\n", "<end>"]);
    assert_eq!(play("a\n{\" \"}\n<> b\n-> END"), ["a b\n", "<end>"]);
}

/// The space is the glued text's own: without it the lines butt up —
/// including across a whitespace-only line, whose space goes with it.
#[test]
fn glue_across_a_blank_line_adds_no_space() {
    assert_eq!(play("a\n{e}\n<>b\n-> END"), ["ab\n", "<end>"]);
    assert_eq!(play("a\n{\" \"}\n<>b\n-> END"), ["ab\n", "<end>"]);
}

/// A blank value after the glue does not end its forward reach.
#[test]
fn glue_reaches_forward_over_a_blank_value() {
    assert_eq!(play("a\n{e}\n<> {e}\nb\n-> END"), ["a b\n", "<end>"]);
}

/// The lines after the join, and before it, are untouched.
#[test]
fn glue_across_a_blank_line_joins_only_its_neighbours() {
    assert_eq!(play("a\n{e}\n<> b\nc\n-> END"), ["a b\n", "c\n", "<end>"]);
    assert_eq!(play("a\nb\n{e}\n<> c\n-> END"), ["a\n", "b c\n", "<end>"]);
}

/// Across a divert, and inside a choice's output.
#[test]
fn glue_across_a_blank_line_through_a_divert_and_a_choice() {
    assert_eq!(
        play("a\n{e}\n-> k2\n=== k2 ===\n<> b\n-> END"),
        ["a b\n", "<end>"]
    );
    assert_eq!(
        play("+ [x] a\n  {e}\n  <> b\n  -> END"),
        ["<choices>", "a b\n", "<end>"]
    );
}

/// A tag between the glue and the newline stops the walk: ink's tags are
/// control commands in the output stream, and the trim stops at the first
/// one. A tag behind the newline does not: the walk has already passed it.
#[test]
fn a_tag_stops_the_glue() {
    assert_eq!(play("a\n{e} #t\n<> b\n-> END"), ["a\n", "b\n", "<end>"]);
    assert_eq!(play("a\n#t\n<> b\n-> END"), ["a\n", "b\n", "<end>"]);
    assert_eq!(
        play("a\n{e}\n#t\n<> b\n-> END"),
        ["a\n", "\n", "b\n", "<end>"]
    );
    assert_eq!(play("a #t\n<> b\n-> END"), ["a b\n", "<end>"]);
    assert_eq!(play("a #t\n{e}\n<> b\n-> END"), ["a b\n", "<end>"]);
}

/// Forward glue: every newline between the glue and the next visible
/// text is dropped, blank lines' included.
#[test]
fn forward_glue_across_a_blank_line() {
    assert_eq!(play("a <>\n{l - l}\nb\n-> END"), ["a b\n", "<end>"]);
    assert_eq!(play("a <>\n{e}\n{e}\nb\n-> END"), ["a b\n", "<end>"]);
    assert_eq!(play("a <>\n{e}\n<> b\n-> END"), ["a b\n", "<end>"]);
}

/// Issue #3558, the same rule at a function's end: `f` prints `1` and then
/// a blank line. The blank value does not commit the `1` line, so the
/// function-end trim removes the blank line's newline and the return value
/// joins `1` — inside an interpolation and outside one alike.
#[test]
fn a_blank_line_at_a_function_end_does_not_commit_the_line_before_it() {
    let f = "\n=== function f() ===\n1\n{e}\n~ return 2";
    assert_eq!(play(&format!("{{f()}}\n-> END{f}")), ["12\n", "<end>"]);
    assert_eq!(
        play(&format!("x {{f()}} y\n-> END{f}")),
        ["x 12 y\n", "<end>"]
    );
    assert_eq!(
        play(&format!("{{ f(): a }}\n-> END{f}")),
        ["1 a\n", "<end>"]
    );
    assert_eq!(
        play(&format!("~ f()\nb\n-> END{f}")),
        ["1\n", "b\n", "<end>"]
    );
    assert_eq!(
        play(&format!("~ temp t = f()\nb {{t}}\n-> END{f}")),
        ["1\n", "b 2\n", "<end>"]
    );
}
