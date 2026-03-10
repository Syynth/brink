#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::Path;

use brink_runtime::{DotNetRng, StepResult, Story};

/// Helper: compile from an in-memory file system (`HashMap` of path to source).
fn compile_mem(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_format::StoryData, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
}

// ── Single file ─────────────────────────────────────────────────────

#[test]
fn compile_minimal_story() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "Hello, world!\n")]);

    let story = compile_mem("main.ink", &files).unwrap();
    // The driver ran without errors (parsed, lowered, analyzed, codegen).
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_story_with_knots() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
Hello!
-> greet

== greet ==
Welcome to the story.
-> END
",
    )]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── INCLUDE discovery ───────────────────────────────────────────────

#[test]
fn compile_follows_includes() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE helpers.ink\nHello!\n-> greet\n"),
        ("helpers.ink", "== greet ==\nWelcome.\n-> END\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_nested_includes() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE a.ink\nMain content.\n"),
        ("a.ink", "INCLUDE b.ink\n"),
        ("b.ink", "VAR x = 5\n== knot_b ==\nHello from b.\n-> END\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_circular_includes_detected() {
    // Each file includes the other — should be detected as a circular dependency.
    let files: HashMap<&str, &str> = HashMap::from([
        ("a.ink", "INCLUDE b.ink\nContent A.\n"),
        ("b.ink", "INCLUDE a.ink\nContent B.\n"),
    ]);

    let err = compile_mem("a.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::CircularInclude(_)),
        "expected CircularInclude variant, got: {err}"
    );
}

// ── Relative path resolution ────────────────────────────────────────

#[test]
fn compile_resolves_relative_include_paths() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("src/main.ink", "INCLUDE utils/helpers.ink\nHello!\n"),
        ("src/utils/helpers.ink", "== greet ==\nHi.\n-> END\n"),
    ]);

    let story = compile_mem("src/main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn compile_missing_entry_file() {
    let files: HashMap<&str, &str> = HashMap::new();

    let err = compile_mem("nonexistent.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Io(_)),
        "expected I/O error for missing entry file, got: {err}"
    );
}

#[test]
fn compile_missing_included_file() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "INCLUDE missing.ink\nHello!\n")]);

    let err = compile_mem("main.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Io(_)),
        "expected I/O error for missing included file, got: {err}"
    );
}

// ── compile_path (disk-based) ───────────────────────────────────────

#[test]
fn compile_path_reads_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier1/basics/I001-minimal-story/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_path_nested_includes_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier3/misc/I025-nested-includes/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── Compile + run (end-to-end) ─────────────────────────────────────

/// Compile from in-memory source, link, and run with given choice inputs.
fn compile_and_run(source: &str, inputs: &[usize]) -> String {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(&program);
    let mut output = String::new();
    let mut input_idx = 0;

    loop {
        match story.continue_maximally().unwrap() {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                output.push_str(&text);
                break;
            }
            StepResult::Choices { text, choices, .. } => {
                output.push_str(&text);
                let idx = if input_idx < inputs.len() {
                    let c = inputs[input_idx];
                    input_idx += 1;
                    c
                } else {
                    0
                };
                assert!(
                    idx < choices.len(),
                    "choice index {idx} out of range (only {} choices available)",
                    choices.len()
                );
                story.choose(idx).unwrap();
            }
        }
    }

    output
}

/// After a tunnel call returns, choices in the same container must be
/// yielded to the player. Regression: `EndChoiceSet` was a no-op and
/// execution fell through to the gather's `end` opcode, terminating
/// the story before choices could be presented.
#[test]
fn choices_after_tunnel_call_are_yielded() {
    let source = "\
-> main

=== function is_alive ===
~ return true

=== check ===
{ is_alive():
    ->->
}
-> END

=== main ===
Before choices.
-> check ->
*   [Option A]
    Chose A.
*   [Option B]
    Chose B.
- -> END
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("Chose A"),
        "expected 'Chose A' after tunnel return, got: {result:?}"
    );
}

/// Choices after a tunnel call with arguments must be yielded.
/// Same regression as above but with parameter passing.
#[test]
fn choices_after_tunnel_call_with_args_are_yielded() {
    let source = "\
VAR hp = 2

-> main

=== function is_alive ===
~ return hp > 0

=== get_hit(x) ===
~ hp = hp - x
{ is_alive():
    ->->
}
-> END

=== main ===
Start.
-> get_hit(1) ->
*   [Fight]
    You fight.
*   [Flee]
    You flee.
- -> END
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("You fight"),
        "expected 'You fight' after tunnel return, got: {result:?}"
    );
}

/// Nested choices with tunnel calls: outer choice leads to tunnel call,
/// tunnel returns, then inner choices must be presented. Mimics I003's
/// structure where the first choice leads to a stitch with a tunnel call
/// followed by sub-choices.
#[test]
fn nested_choices_after_tunnel_in_stitch() {
    let source = "\
VAR hp = 2

-> main

=== function is_alive ===
~ return hp > 0

=== get_hit(x) ===
~ hp = hp - x
{ is_alive():
    ->->
}
-> END

=== main ===
Choose:
*   [Yes]
    You chose yes.
    -> END
*   [No]
    You chose no.
    -> get_hit(1) ->
    **  [Fight]
        You fight.
    **  [Flee]
        You flee.
    - -> END
";
    let result = compile_and_run(source, &[1, 0]);
    assert!(
        result.contains("You fight"),
        "expected inner choice after tunnel return, got: {result:?}"
    );
}
