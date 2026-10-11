//! Issue #3695: a glue in the middle of a line, after an interpolation,
//! was dropped by LIR's `strip_boundary_glue` when the line became a
//! template. That glue is a no-op only when visible text comes before it
//! on the line. When everything before it can render blank, it walks back
//! over that and removes the previous line's newline (#3535's rule), so
//! `a` / `{e}<> b` prints `a b`. Reference outputs below are inkjs 2.4.0
//! via `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

const PRELUDE: &str = "VAR e = ()\nVAR s = \"\"\n-> k\n\n=== k ===\n";

/// Compile `PRELUDE` + `body` and play it to the end; every delivered
/// line's text lands verbatim, and the terminal as `<done>` / `<end>`.
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
            Step::Done => {
                steps.push("<done>".to_owned());
                return steps;
            }
            Step::End => {
                steps.push("<end>".to_owned());
                return steps;
            }
            Step::Choices(_) | Step::Suspended => panic!("unexpected step in {source}"),
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

/// The issue's shape, and the line after it is untouched.
#[test]
fn glue_after_a_blank_interpolation_reaches_the_line_before() {
    assert_eq!(play("a\n{e}<> b\n-> END"), ["a b\n", "<end>"]);
    assert_eq!(play("a\n{e}<> b\nc\n-> END"), ["a b\n", "c\n", "<end>"]);
}

/// Whitespace text and several blank values before the glue are walked
/// over too; the space before the glue goes with them.
#[test]
fn glue_walks_over_whitespace_and_several_blank_values() {
    assert_eq!(play("a\n{e} <>b\n-> END"), ["ab\n", "<end>"]);
    assert_eq!(play("a\n{e}{s}<> b\n-> END"), ["a b\n", "<end>"]);
}

/// A second glue later on the same line, after visible text, is inert.
#[test]
fn a_later_glue_after_visible_text_is_inert() {
    assert_eq!(play("a\n{e}<>b {e}<> c\n-> END"), ["ab c\n", "<end>"]);
}

/// Controls: visible text or a visible value before the glue stops it.
#[test]
fn visible_content_before_the_glue_stops_it() {
    assert_eq!(play("a\nx{e}<> b\n-> END"), ["a\n", "x b\n", "<end>"]);
    assert_eq!(play("a\nx<>y\n-> END"), ["a\n", "xy\n", "<end>"]);
    assert_eq!(play("a\n{1}<> b\n-> END"), ["a\n", "1 b\n", "<end>"]);
}
