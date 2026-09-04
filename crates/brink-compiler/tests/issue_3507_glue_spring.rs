//! Issue #3507: whitespace between an inline construct's `}` and `<>` was
//! dropped — `{0} <>` then `world` printed `0world` where ink prints
//! `0 world`. The lexer folds a space after TEXT into the TEXT token (so
//! `hello <>` always worked), but after `}` the space is a WHITESPACE
//! trivia token the parser skips before `GLUE_NODE`, and content lowering
//! only ever looked at node children. The fix lowers that whitespace to a
//! `ContentPart::Spring` — the runtime's conditional space: emitted once,
//! never doubled, trimmed at end of output — which is exactly what ink does
//! with it. Reference outputs below are inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in, `docs/program-generator-
//! spec.md` §6); the corpus case with a C# golden is owed (dotnet,
//! maintainer-local).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file and play it straight through (no choices),
/// returning every delivered line with only its trailing newline removed —
/// trailing spaces are part of what this issue is about, so they are kept.
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
            Step::Line(l) => lines.push(l.text.trim_end_matches('\n').to_owned()),
            Step::Choices(_) => panic!("unexpected choices in {source}"),
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

/// The shrunk differential counterexample. Reference: `0 world`.
#[test]
fn interpolation_space_glue_keeps_one_space() {
    assert_eq!(play("{0} <>\nworld\n-> END\n"), ["0 world"]);
}

/// A run of spaces collapses to one, as any whitespace run does in ink.
/// Reference: `0 world`.
#[test]
fn interpolation_spaces_glue_collapse_to_one() {
    assert_eq!(play("{0}  <>\nworld\n-> END\n"), ["0 world"]);
}

/// No whitespace, no space — the pre-fix behavior was right here and must
/// stay. Reference: `0world`.
#[test]
fn interpolation_glue_without_space_stays_joined() {
    assert_eq!(play("{0}<>\nworld\n-> END\n"), ["0world"]);
}

/// The same trivia shape follows an inline conditional and an inline
/// sequence. References: `x world`, `a world`.
#[test]
fn conditional_and_sequence_before_glue_keep_the_space() {
    assert_eq!(play("{true:x} <>\nworld\n-> END\n"), ["x world"]);
    assert_eq!(play("{a|b} <>\nworld\n-> END\n"), ["a world"]);
}

/// A spring at the very end of output is a trailing space, and ink trims
/// those. Reference: `0`.
#[test]
fn spring_at_end_of_output_is_trimmed() {
    assert_eq!(play("{0} <>\n-> END\n"), ["0"]);
    assert_eq!(play("{a|b} <>\n-> END\n"), ["a"]);
}

/// Two glues with a space between: still exactly one space, because the
/// second glue follows a glue, not content. Reference: `0 world`.
#[test]
fn space_before_a_second_glue_is_not_doubled() {
    assert_eq!(play("{0} <> <>\nworld\n-> END\n"), ["0 world"]);
}

/// Plain text before glue already kept its space (the lexer folds it into
/// the TEXT token) and must keep doing so. References: `hello world`.
#[test]
fn text_before_glue_is_unchanged() {
    assert_eq!(play("hello <>\nworld\n-> END\n"), ["hello world"]);
    assert_eq!(play("hello  <>\nworld\n-> END\n"), ["hello world"]);
    assert_eq!(play("{0} x <>\nworld\n-> END\n"), ["0 x world"]);
}

/// The same shape inside a multiline conditional's body — the branchless
/// (`{cond: … }`, no else) and the else-bearing forms lower through two
/// different loops, and the branchless one dropped the space too.
/// References: `0 world` for all three.
#[test]
fn glue_space_inside_multiline_conditional_bodies() {
    assert_eq!(
        play("{true:\n    {0} <>\n    world\n}\n-> END\n"),
        ["0 world"]
    );
    assert_eq!(play("{true:\n    {0} <>\n}\nworld\n-> END\n"), ["0 world"]);
    assert_eq!(
        play("{false:\n    x\n- else:\n    {0} <>\n    world\n}\n-> END\n"),
        ["0 world"]
    );
}

/// When the construct before the space renders EMPTY, ink's glue trims the
/// preceding newline and every whitespace-only string after it, so the
/// space dies with the newline — `ab`, not `a b`. Found by the inkjs
/// differential on the first fix attempt (a spring that always emitted).
/// References: `ab`, `b`, `ab`, `ay z`.
#[test]
fn empty_construct_before_glue_leaves_no_space() {
    assert_eq!(play("a\n{false:x} <>\nb\n-> END\n"), ["ab"]);
    assert_eq!(play("{false:x} <>\nb\n-> END\n"), ["b"]);
    assert_eq!(play("a\n{false:x} {false:y} <>\nb\n-> END\n"), ["ab"]);
    assert_eq!(play("a\n{false:x} <>\n{true:y} z\n-> END\n"), ["ay z"]);
}

/// Whitespace AFTER the glue is the next text's own leading space and is
/// never trimmed by that glue. References: `a c`, `a b`.
#[test]
fn whitespace_after_glue_is_kept() {
    assert_eq!(play("a\n{false:x} <> c\n-> END\n"), ["a c"]);
    assert_eq!(play("a\n<> b\n-> END\n"), ["a b"]);
}
