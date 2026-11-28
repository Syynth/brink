//! Issue #3522: a function ending in a spring before glue (`{x} <>`)
//! printed `x ` where ink prints `x` glued to what follows. The C#
//! runtime's `TrimWhitespaceFromFunctionEnd` walks the output stream
//! backwards from the function's end and `continue`s past every non-text
//! object — glue included — removing whitespace until it meets text;
//! brink's `trim_function_end` stopped at the glue and left the spring
//! beneath it. Reference outputs below are inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in, `docs/program-generator-
//! spec.md` §6); the corpus case with a C# golden is owed (dotnet,
//! maintainer-local).

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

/// The shrunk differential counterexample. Reference: `0a`.
#[test]
fn spring_before_glue_at_function_end_is_trimmed() {
    let src = "~ f()\na\n-> END\n\n=== function f() ===\n{0} <>\n";
    assert_eq!(play(src), vec!["0a\n"]);
}

/// Text before the glue keeps its own trailing space: the lexer folds it
/// into the TEXT token, and the C# trim stops at non-whitespace text the
/// same way. Reference: `x a`.
#[test]
fn text_space_before_glue_at_function_end_stays() {
    let src = "~ f()\na\n-> END\n\n=== function f() ===\nx <>\n";
    assert_eq!(play(src), vec!["x a\n"]);
}

/// A newline under the glue goes too; the glue then joins the lines.
/// Reference: `0a`.
#[test]
fn newline_under_glue_at_function_end_is_trimmed() {
    let src = "~ f()\na\n-> END\n\n=== function f() ===\n{0}\n<>\n";
    assert_eq!(play(src), vec!["0a\n"]);
}

/// Glue at the end of a function called in display position. Reference:
/// `x0y`.
#[test]
fn spring_before_glue_in_a_display_position_call() {
    let src = "x{f()}y\n-> END\n\n=== function f() ===\n{0} <>\n";
    assert_eq!(play(src), vec!["x0y\n"]);
}
