//! Issue #3519: a newline emitted by a function that had not yet printed
//! anything was kept whenever the line it was called from already held
//! content — so a second call in one expression to a function whose body
//! starts with a conditional block (whose branch begins with a newline)
//! broke the line: `x{f(f(true))}y` printed `xa` / `atruey` where ink
//! prints `xaatruey`. The C# runtime drops a newline while the current
//! function frame has produced no non-whitespace output
//! (`functionStartInOutputStream`); brink's `push_newline` only knew the
//! enclosing scope's content. Reference outputs below are inkjs 2.4.0 via
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

/// `f` prints `a` from inside a conditional block and returns its
/// argument.
const BLOCK_F: &str = "\n=== function f(p) ===\n{ true:\n    a\n}\n~ return p\n";

/// The shrunk differential counterexample: a nested call. (The sibling
/// shape `x{f(true) and f(true)}y` is issue #3525's — a compound slot
/// expression is not composed into the slot — and is pinned there.)
/// Reference: `xaatruey`.
#[test]
fn nested_call_keeps_the_line() {
    let src = format!("x{{f(f(true))}}y\n-> END\n{BLOCK_F}");
    assert_eq!(play(&src), vec!["xaatruey\n"]);
}

/// Two calls in a `~ temp` initialiser: their output shares one line, the
/// `~` line's own newline ends it. Reference: `aa` / `xtruey`.
#[test]
fn two_calls_in_a_temp_initialiser_share_one_line() {
    let src = format!("~ temp t = f(true) and f(true)\nx{{t}}y\n-> END\n{BLOCK_F}");
    assert_eq!(play(&src), vec!["aa\n", "xtruey\n"]);
}

/// Two calls in a lifted construct's prefix: the same rule inside the
/// hoisted evaluation. Reference: `aatruey`.
#[test]
fn two_calls_before_an_inline_conditional_keep_the_line() {
    let src = format!("{{f(true) and f(true)}}y\n-> END\n{BLOCK_F}");
    assert_eq!(play(&src), vec!["aatruey\n"]);
}

/// A single call was already right (the fragment was empty when the
/// newline arrived); unchanged. Reference: `xatruey`.
#[test]
fn single_call_control() {
    let src = format!("x{{f(true)}}y\n-> END\n{BLOCK_F}");
    assert_eq!(play(&src), vec!["xatruey\n"]);
}

/// Once the function HAS printed, its later newlines are real: a
/// two-line function body still breaks the line after its first line.
/// Reference: `a` / `xbtruey`.
#[test]
fn newline_after_the_function_printed_is_kept() {
    let src = "~ temp t = f(true)\nx{t}y\n-> END\n\n=== function f(p) ===\na\nb\n~ return p\n";
    assert_eq!(play(src), vec!["a\n", "b\n", "xtruey\n"]);
}
