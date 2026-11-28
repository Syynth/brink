//! Issue #3523: a multi-line conditional's `- cond:` / `- else:` arm lacked
//! the leading newline inklecate gives every multi-line arm (`{"b": ["\n",
//! "^b", …]}`), so content before the arm that did not end in a newline —
//! a printing function called in the condition, whose trailing newline
//! the function-end trim removes — glued onto the arm's first line.
//! Reference outputs below are inkjs 2.4.0 via `tools/inkjs-oracle` (the
//! sanctioned stand-in, `docs/program-generator-spec.md` §6); the corpus
//! case with a C# golden is owed (dotnet, maintainer-local).

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

const PRINTING_F: &str = "\n=== function f() ===\na\n~ return 0\n";

/// The shrunk differential counterexample: the else arm of `{ cond:`.
/// Reference: `a` / `b`.
#[test]
fn else_arm_of_a_branchless_conditional_starts_a_new_line() {
    let src = format!("{{ (1 < f()):\n    x\n- else:\n    b\n}}\n-> END\n{PRINTING_F}");
    assert_eq!(play(&src), vec!["a\n", "b\n"]);
}

/// The taken first arm already started a new line (its own first-newline
/// rule); unchanged. Reference: `a` / `x`.
#[test]
fn first_arm_of_a_branchless_conditional_starts_a_new_line() {
    let src = format!("{{ (0 < f() + 1):\n    x\n- else:\n    b\n}}\n-> END\n{PRINTING_F}");
    assert_eq!(play(&src), vec!["a\n", "x\n"]);
}

/// A `- cond:` list conditional: every arm starts a new line. Reference:
/// `a` / `b`.
#[test]
fn cond_list_arms_start_a_new_line() {
    let src = format!("{{\n- f() == 1:\n    x\n- else:\n    b\n}}\n-> END\n{PRINTING_F}");
    assert_eq!(play(&src), vec!["a\n", "b\n"]);
}

/// A switch on a value: every arm starts a new line. Reference: `a` / `z`.
#[test]
fn switch_arms_start_a_new_line() {
    let src = format!("{{ f():\n- 1:\n    x\n- 0:\n    z\n}}\n-> END\n{PRINTING_F}");
    assert_eq!(play(&src), vec!["a\n", "z\n"]);
}

/// Content on the marker line (`- else: b`) gets the newline too.
/// Reference: `a` / `b`.
#[test]
fn content_on_the_marker_line_starts_a_new_line() {
    let src = format!("{{ (1 < f()):\n    x\n- else: b\n}}\n-> END\n{PRINTING_F}");
    assert_eq!(play(&src), vec!["a\n", "b\n"]);
}

/// The common case — output before the block already ends in a newline —
/// is unchanged: the runtime dedupes the arm's leading newline.
/// Reference: `before` / `b` / `after`.
#[test]
fn leading_newline_dedupes_after_a_full_line() {
    let src = "before\n{ false:\n    x\n- else:\n    b\n}\nafter\n-> END\n";
    assert_eq!(play(src), vec!["before\n", "b\n", "after\n"]);
}
