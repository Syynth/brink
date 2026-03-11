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

// ── List display names ───────────────────────────────────────────────

/// List items should display without their origin prefix.
/// e.g. `{myList}` should output "a, b" not "myList.a, myList.b".
#[test]
fn list_items_display_without_origin_prefix() {
    let source = "\
LIST colors = (red), green, (blue)
{colors}
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "red, blue\n");
}

/// Multi-list display: items from different lists show unqualified names.
#[test]
fn multi_list_display_without_origin_prefix() {
    let source = "\
LIST a = (x), y
LIST b = (p), q
{a + b}
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "x, p\n");
}

// ── External function fallback ───────────────────────────────────────

/// EXTERNAL declaration with ink fallback function should use the fallback
/// when no external binding is provided.
#[test]
fn external_function_uses_ink_fallback() {
    let source = "\
EXTERNAL greet()

The value is {greet()}.
-> END

=== function greet() ===
~ return \"hello\"
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "The value is hello.\n");
}

/// EXTERNAL with arguments should pass args to the ink fallback.
#[test]
fn external_function_fallback_with_args() {
    let source = "\
EXTERNAL add(x, y)

The value is {add(3, 4)}.
-> END

=== function add(x, y) ===
~ return x + y
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "The value is 7.\n");
}

// ── Include file ordering ────────────────────────────────────────────

/// Content from included files should appear before the including file's
/// content, matching ink's INCLUDE-as-paste semantics.
#[test]
fn include_content_appears_before_main() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE a.ink\nINCLUDE b.ink\nThis is main.\n"),
        ("a.ink", "This is A.\n"),
        ("b.ink", "This is B.\n"),
    ]);
    let data = compile_mem("main.ink", &files).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(&program);
    let result = match story.continue_maximally().unwrap() {
        StepResult::Done { text, .. }
        | StepResult::Ended { text, .. }
        | StepResult::Choices { text, .. } => text,
    };
    assert_eq!(
        result, "This is A.\nThis is B.\nThis is main.\n",
        "included file content must appear before main file content"
    );
}

// ── Divert to standalone labeled gather ──────────────────────────────

/// Diverting to a labeled gather within a knot (e.g. `-> knot.gather`)
/// must work. The gather needs its own container to be a divert target.
#[test]
fn divert_to_standalone_labeled_gather() {
    let source = "\
-> knot
=== knot ===
-> knot.gather
- (gather) g
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "g\n");
}

// ── Pattern 1: Divert/tunnel parameters not pushed onto stack ────────

/// Variable divert with parameter: `->x (5)` where x holds a divert target.
/// The argument must be pushed onto the value stack before the call.
#[test]
fn divert_target_with_parameter() {
    let source = "\
VAR x = ->place
->x (5)
== place (a) ==
{a}
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "5\n");
}

/// Tunnel onwards with argument: `->-> b (5 + 3)` must evaluate the
/// expression and pass the result to the target knot.
#[test]
fn tunnel_onwards_with_arg() {
    let source = "\
-> a ->
=== a ===
->-> b (5 + 3)
=== b (x) ===
{x}
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "8\n");
}

/// Tunnel onwards with parameter inside a default choice:
/// `* ->-> elsewhere (8)` — the default choice auto-fires and passes the arg.
#[test]
fn tunnel_onwards_with_param_default_choice() {
    let source = "\
-> tunnel ->
== tunnel ==
* ->-> elsewhere (8)
== elsewhere (x) ==
{x}
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "8\n");
}

/// Variable tunnel: `-> x ->` where x is a divert parameter.
/// Must use `tunnel_call_variable`, not a literal `tunnel_call`.
#[test]
fn variable_tunnel_call() {
    let source = "\
-> one_then_tother(-> tunnel)

=== one_then_tother(-> x) ===
    -> x -> end

=== tunnel ===
    STUFF
    ->->

=== end ===
    -> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "STUFF\n");
}

// ── Pattern 2: Tunnel gather emits done instead of tunnel_return ─────

/// After choosing inside a tunnel, execution should return to the caller
/// via `tunnel_return`, not terminate with `done`.
#[test]
fn tunnel_return_at_gather_with_thread() {
    let source = "\
-> knot
=== knot
    <- threadA
    When should this get printed?
    -> DONE
=== threadA
    -> tunnel ->
    Finishing thread.
    -> DONE
=== tunnel
    -   I'm in a tunnel
    *   I'm an option
    -   ->->
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(
        result,
        "I'm in a tunnel\nWhen should this get printed?\nI'm an option\nFinishing thread.\n"
    );
}

// ── Pattern 3: Thread choices not merged with current context ────────

/// Choices from a thread (`<- thread_with_options`) must merge with
/// choices from the current context (tunnel or inline).
#[test]
fn tunnel_and_thread_choices_merge() {
    let source = "\
-> knot_with_options ->
Finished tunnel.
Starting thread.
<- thread_with_options
* E
-
Done.
== knot_with_options ==
* A
* B
-
->->
== thread_with_options ==
* C
* D
- -> DONE
";
    // Episode e0: choose A (idx 0), then C (idx 0 of remaining thread choices)
    let result = compile_and_run(source, &[0, 0]);
    assert_eq!(result, "A\nFinished tunnel.\nStarting thread.\nC\nDone.\n");
}

/// Thread choices must merge with tunnel choices in an interleaved scenario.
#[test]
fn thread_choices_merge_with_tunnel() {
    let source = "\
-> knot
=== knot
    <- threadB
    -> tunnel ->
    THE END
    -> END
=== tunnel
    - blah blah
    * wigwag
    - ->->
=== threadB
    *   option
    -   something
        -> DONE
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(result, "blah blah\noption\nsomething\n");
}

// ── Pattern 4: Missing space literal in string interpolation ─────────

/// `{gatherCount} {loop}` must produce "1 1", not "11" — the space
/// between interpolations must be emitted as a literal.
#[test]
fn space_between_interpolations_preserved() {
    let source = "\
VAR gatherCount = 0
- (loop)
~ gatherCount++
{gatherCount} {loop}
{gatherCount<3:->loop}
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1 1\n2 2\n3 3\n");
}

// ── Pattern 5: ref parameters compiled as pointer ────────────────────

/// `ref` parameter should pass by reference, allowing the callee to
/// modify the caller's variable.
#[test]
fn ref_parameter_modifies_caller_variable() {
    let source = "\
~temp x = 0
-> bump(x)
{x}
-> DONE

=== bump(ref n) ===
~n++
->->
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1\n");
}
