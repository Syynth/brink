//! Issue #3536: a function whose output ends in a value that renders as
//! whitespace — an empty list, `""`, a `none` — left a blank line behind.
//! ink stringifies values into the output stream as they are pushed, so
//! by the time `TrimWhitespaceFromFunctionEnd` runs an empty
//! interpolation is an inline-whitespace `StringValue` and is trimmed
//! with the newline behind it; brink resolves values later, and its
//! `trim_function_end` had no `ValueRef` arm at all, so the value stopped
//! the trim. Reference outputs below are inkjs 2.4.0 via
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

const PRELUDE: &str = "LIST l = li\nVAR e = ()\n-> k\n\n=== k ===\n";

/// A void function printing an empty list contributes nothing — not even
/// a blank line — whether or not lines surround the call.
#[test]
fn a_functions_empty_output_is_trimmed() {
    let f = "\n=== function f() ===\n{l}\n";
    assert_eq!(
        play(&format!("{PRELUDE}a\n~ f()\nb\n-> END\n{f}")),
        ["a\n", "b\n"]
    );
    assert_eq!(play(&format!("{PRELUDE}a\n~ f()\n-> END\n{f}")), ["a\n"]);
    assert_eq!(
        play(&format!("{PRELUDE}~ f()\n-> END\n{f}")),
        Vec::<String>::new()
    );
}

/// The same for the other values that render as whitespace, and for a
/// function whose whole body is several of them.
#[test]
fn every_whitespace_rendering_value_is_trimmed() {
    for body in ["{e}", "{\"\"}", "{\" \"}", "{l}\n{e}\n{\" \"}"] {
        let src = format!("{PRELUDE}a\n~ f()\nb\n-> END\n\n=== function f() ===\n{body}\n");
        assert_eq!(play(&src), ["a\n", "b\n"], "body was {body}");
    }
}

/// A value that renders visibly still stops the trim, and still ends the
/// function's line.
#[test]
fn a_visible_value_is_not_trimmed() {
    let src = format!("{PRELUDE}a\n~ f()\nb\n-> END\n\n=== function f() ===\n{{l}}x\n");
    assert_eq!(play(&src), ["a\n", "x\n", "b\n"]);
    let src =
        format!("{PRELUDE}a\n~ f()\nb\n-> END\n\n=== function f() ===\n~ temp t = (li)\n{{t}}\n");
    assert_eq!(play(&src), ["a\n", "li\n", "b\n"]);
}

/// A value function's empty output is trimmed the same way — the returned
/// value still reaches the caller's line.
#[test]
fn a_value_functions_empty_output_is_trimmed() {
    let src =
        format!("{PRELUDE}a\n{{f()}}\nb\n-> END\n\n=== function f() ===\n{{l}}\n~ return 7\n");
    assert_eq!(play(&src), ["a\n", "7\n", "b\n"]);
}
