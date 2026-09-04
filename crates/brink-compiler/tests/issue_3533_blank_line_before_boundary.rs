//! Issue #3533: a blank line (an empty-list or empty-string interpolation
//! on a line of its own) between a delivered line and a turn boundary was
//! delivered as its own `Step::Line`, where ink drops it. ink evaluates
//! the lines after a delivered one inside the same `Continue`: the blank
//! line's newline is dropped because the stream still ends in the
//! delivered newline, and it comes back only if non-whitespace content
//! follows (the state snapshot rewinds). At `END`, `DONE`, a choice point
//! or running out of content nothing follows, so trailing blank lines
//! vanish — unless nothing was delivered this turn yet, in which case the
//! turn's first `Continue` keeps exactly one of them. Reference outputs
//! below are inkjs 2.4.0 via `tools/inkjs-oracle` (the sanctioned
//! stand-in, `docs/program-generator-spec.md` §6); the corpus case with a
//! C# golden is owed (dotnet, maintainer-local).

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

/// The differential's find: a blank line right before `-> END` after a
/// delivered line is not delivered. Reference: `a\n` then end.
#[test]
fn blank_line_before_end_is_dropped() {
    assert_eq!(play("a\n{e}\n-> END"), ["a\n", "<end>"]);
    assert_eq!(play("a\n{e}\n{e}\n-> END"), ["a\n", "<end>"]);
    assert_eq!(play("a\n{\" \"}\n-> END"), ["a\n", "<end>"]);
    assert_eq!(play("a\n{e}\n\n-> END"), ["a\n", "<end>"]);
}

/// The same before `-> DONE`, a choice point, a tunnel return and after
/// logic that prints nothing.
#[test]
fn blank_line_before_other_boundaries_is_dropped() {
    assert_eq!(play("a\n{e}\n-> DONE"), ["a\n", "<done>"]);
    assert_eq!(
        play("a\n{e}\n* x\n  y\n  -> END"),
        ["a\n", "<choices>", "x\n", "y\n", "<end>"]
    );
    assert_eq!(
        play("a\n{e}\n{e}\n* x\n  y\n  -> END"),
        ["a\n", "<choices>", "x\n", "y\n", "<end>"]
    );
    assert_eq!(play("a\n{e}\n~ temp q = 1\n-> END"), ["a\n", "<end>"]);
    assert_eq!(
        play("a\n-> f ->\n-> END\n=== f ===\n{e}\n->->"),
        ["a\n", "<end>"]
    );
}

/// After a choice whose text echoes, the echo is the delivered line and a
/// blank line before the boundary is dropped; after a `[x]` choice with
/// no echo the blank line is the turn's first and is kept.
#[test]
fn blank_line_after_a_choice() {
    assert_eq!(play("* x\n  {e}\n  -> END"), ["<choices>", "x\n", "<end>"]);
    assert_eq!(play("* [x]\n  {e}\n  -> END"), ["<choices>", "\n", "<end>"]);
    assert_eq!(
        play("* [x]\n  {e}\n  {e}\n  b\n  -> END"),
        ["<choices>", "\n", "\n", "b\n", "<end>"]
    );
}

/// A turn's first `Continue` keeps exactly one blank line: at the story
/// start, one `{e}` and two `{e}` both print a single empty line.
#[test]
fn leading_blank_lines_collapse_to_one() {
    assert_eq!(play("{e}\n-> END"), ["\n", "<end>"]);
    assert_eq!(play("{e}\n{e}\n-> END"), ["\n", "<end>"]);
}

/// Blank lines followed by content are all delivered, each on its own —
/// including across a divert. (The glue row of the same table, `a` /
/// `{e}` / `<> b`, is #3535's: ink walks the glue back past the blank
/// line and prints `a b`, brink prints two lines.)
#[test]
fn blank_lines_followed_by_content_are_delivered() {
    assert_eq!(play("a\n{e}\nb\n-> END"), ["a\n", "\n", "b\n", "<end>"]);
    assert_eq!(
        play("a\n{e}\n{e}\nb\n-> END"),
        ["a\n", "\n", "\n", "b\n", "<end>"]
    );
    assert_eq!(play("a\n{\" \"}\nb\n-> END"), ["a\n", "\n", "b\n", "<end>"]);
    assert_eq!(
        play("a\n{e}\n-> k2\n=== k2 ===\nb\n-> END"),
        ["a\n", "\n", "b\n", "<end>"]
    );
}

/// The generator's shape verbatim: a knot diverting into its stitch,
/// which prints a line and then an empty `LIST_INVERT`.
#[test]
fn differential_shape() {
    let source = "LIST l0_a = li0_0\n\n-> a_k0\n\n=== a_k0 ===\n-> a_k0.a_s0\n\n= a_s0\na\n{LIST_INVERT((li0_0))}\n-> END\n";
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned())).expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let steps: Vec<Step> = story.continue_maximally().expect("runtime");
    let texts: Vec<&str> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(l) => Some(l.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["a\n"]);
    assert!(matches!(steps.last(), Some(Step::End)));
}
