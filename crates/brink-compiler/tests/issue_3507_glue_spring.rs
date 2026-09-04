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

/// Compile one in-memory file and play it, choosing `choices` in order,
/// returning every delivered line's text VERBATIM — trailing spaces and
/// the trailing newline are part of what this issue is about (ink delivers
/// `0` with no newline when glue ends the story, `0 world\n` otherwise), so
/// nothing is trimmed.
fn play_choosing(source: &str, choices: &[usize]) -> Vec<String> {
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
            Step::Line(l) => lines.push(l.text.clone()),
            Step::Choices(_) => {
                let pick = picks
                    .next()
                    .unwrap_or_else(|| panic!("unexpected choices in {source}"));
                story.choose(pick).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

fn play(source: &str) -> Vec<String> {
    play_choosing(source, &[])
}

/// The shrunk differential counterexample. Reference: `0 world`.
#[test]
fn interpolation_space_glue_keeps_one_space() {
    assert_eq!(play("{0} <>\nworld\n-> END\n"), ["0 world\n"]);
}

/// A run of spaces collapses to one, as any whitespace run does in ink.
/// Reference: `0 world`.
#[test]
fn interpolation_spaces_glue_collapse_to_one() {
    assert_eq!(play("{0}  <>\nworld\n-> END\n"), ["0 world\n"]);
}

/// No whitespace, no space — the pre-fix behavior was right here and must
/// stay. Reference: `0world`.
#[test]
fn interpolation_glue_without_space_stays_joined() {
    assert_eq!(play("{0}<>\nworld\n-> END\n"), ["0world\n"]);
}

/// The same trivia shape follows an inline conditional and an inline
/// sequence. References: `x world`, `a world`.
#[test]
fn conditional_and_sequence_before_glue_keep_the_space() {
    assert_eq!(play("{true:x} <>\nworld\n-> END\n"), ["x world\n"]);
    assert_eq!(play("{a|b} <>\nworld\n-> END\n"), ["a world\n"]);
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
    assert_eq!(play("{0} <> <>\nworld\n-> END\n"), ["0 world\n"]);
}

/// Plain text before glue already kept its space (the lexer folds it into
/// the TEXT token) and must keep doing so. References: `hello world`.
#[test]
fn text_before_glue_is_unchanged() {
    assert_eq!(play("hello <>\nworld\n-> END\n"), ["hello world\n"]);
    assert_eq!(play("hello  <>\nworld\n-> END\n"), ["hello world\n"]);
    assert_eq!(play("{0} x <>\nworld\n-> END\n"), ["0 x world\n"]);
}

/// The same shape inside a multiline conditional's body — the branchless
/// (`{cond: … }`, no else) and the else-bearing forms lower through two
/// different loops, and the branchless one dropped the space too.
/// References: `0 world` for all three.
#[test]
fn glue_space_inside_multiline_conditional_bodies() {
    assert_eq!(
        play("{true:\n    {0} <>\n    world\n}\n-> END\n"),
        ["0 world\n"]
    );
    assert_eq!(
        play("{true:\n    {0} <>\n}\nworld\n-> END\n"),
        ["0 world\n"]
    );
    assert_eq!(
        play("{false:\n    x\n- else:\n    {0} <>\n    world\n}\n-> END\n"),
        ["0 world\n"]
    );
}

/// When the construct before the space renders EMPTY, ink's glue trims the
/// preceding newline and every whitespace-only string after it, so the
/// space dies with the newline — `ab`, not `a b`. Found by the inkjs
/// differential on the first fix attempt (a spring that always emitted).
/// References: `ab`, `b`, `ab`, `ay z`.
#[test]
fn empty_construct_before_glue_leaves_no_space() {
    assert_eq!(play("a\n{false:x} <>\nb\n-> END\n"), ["ab\n"]);
    assert_eq!(play("{false:x} <>\nb\n-> END\n"), ["b\n"]);
    assert_eq!(play("a\n{false:x} {false:y} <>\nb\n-> END\n"), ["ab\n"]);
    assert_eq!(play("a\n{false:x} <>\n{true:y} z\n-> END\n"), ["ay z\n"]);
}

/// Whitespace AFTER the glue is the next text's own leading space and is
/// never trimmed by that glue. References: `a c`, `a b`.
#[test]
fn whitespace_after_glue_is_kept() {
    assert_eq!(play("a\n{false:x} <> c\n-> END\n"), ["a c\n"]);
    assert_eq!(play("a\n<> b\n-> END\n"), ["a b\n"]);
}

/// An empty construct then glue as the LAST content of the story: ink's
/// glue trims the preceding newline, so the previous line is delivered
/// WITHOUT one. Found by the 512-case CI run: inside an else arm the
/// deferred whitespace used to lower as `Text(" ")`, which the lift folded
/// into a whitespace-only line (`emit_line " "; glue`) that the runtime's
/// glue scan took for content. References: `a` (no newline) for all four
/// (a choice step carries no text of its own, so only the line shows).
#[test]
fn empty_construct_glue_at_end_of_story_trims_the_previous_newline() {
    assert_eq!(play("a\n{false:a} <>\n-> END\n"), ["a"]);
    assert_eq!(
        play("a\n{ false:\n    a\n- else:\n    {false:a} <>\n}\n-> END\n"),
        ["a"]
    );
    assert_eq!(
        play_choosing("* [a]\n    a\n    {false:a} <>\n    -> END\n", &[0]),
        ["a"]
    );
    // The shrunk CI counterexample itself.
    assert_eq!(
        play_choosing(
            "VAR v0_a = 0\n-> a_k0\n=== a_k0 ===\n-> a_k0.a_s0\n= a_s0\n* [a]\n    * * [a]\n        a\n        { (0 == (0 + 1)):\n            a\n        - else:\n            {false:a} <>\n        }\n        -> END\n    + + -> END\n+ -> END\n",
            &[0, 0]
        ),
        ["a"]
    );
}
