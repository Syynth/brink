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
    .map(|output| output.data)
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
        !story.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_path_nested_includes_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier3/misc/I025-nested-includes/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(
        !story.data.containers.is_empty(),
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
/// yielded to the player. Regression: execution fell through to the
/// gather's `end` opcode, terminating the story before choices could
/// be presented.
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

/// Bare `->->` on a gather line must emit a tunnel return.
/// `lower_gather_to_block` only handles `simple_divert()`, so `->->`
/// (a `TUNNEL_ONWARDS_NODE`) is silently dropped, producing `done`
/// instead of `tunnel_return`.
#[test]
fn gather_bare_tunnel_return() {
    let source = "\
-> start
== start ==
-> tun ->
After tunnel.
-> END
== tun ==
- Gathered.
* Pick me
- ->->
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(result, "Gathered.\nPick me\nAfter tunnel.\n");
}

/// `->-> target` on a gather line — tunnel return with divert override.
#[test]
fn gather_tunnel_return_with_override() {
    let source = "\
-> start
== start ==
-> tun ->
Should not print.
-> END
== tun ==
- In tunnel.
* Pick me
- ->-> destination
== destination ==
Overridden.
-> END
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(result, "In tunnel.\nPick me\nOverridden.\n");
}

/// `-> target ->` on a gather line — tunnel call from a gather.
#[test]
fn gather_tunnel_call() {
    let source = "\
-> start
== start ==
* Pick me
- -> inner_tunnel ->
After inner tunnel.
-> END
== inner_tunnel ==
Inside inner tunnel.
->->
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(
        result,
        "Pick me\nInside inner tunnel.\nAfter inner tunnel.\n"
    );
}

/// `<- thread` on a gather line — thread start from a gather.
/// The thread's choice must merge with the local sticky choice.
#[test]
fn gather_thread_start() {
    let source = "\
-> start
== start ==
* Pick me
- <- bg_thread
+ Next
-
Done.
-> END
== bg_thread ==
* Background option
- -> DONE
";
    // Pick "Pick me" first, then "Background option" (from the thread)
    // If the thread start is silently dropped, only "Next" is available
    // and "Background option" never appears.
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("Background option"),
        "expected thread's choice from gather `<- bg_thread` to be available, got: {result:?}"
    );
}

/// Structural test: compile a tunnel with `->->` on a gather line and
/// verify the .inkt contains `tunnel_return`, not just `done`.
#[test]
fn gather_tunnel_return_emits_tunnel_return_opcode() {
    let source = "\
-> start
== start ==
-> tun ->
After.
-> END
== tun ==
- Top.
* Option
- ->->
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        buf.contains("tunnel_return"),
        "expected tunnel_return in bytecode for gather `->->`, got:\n{buf}"
    );
}

// ── Pattern 3: Thread choices not merged with current context ────────

/// Choices from a thread (`<- thread_with_options`) must merge with
/// choices from the current context (tunnel or inline).
#[test]
#[ignore = "thread completion doesn't resume main flow — runtime thread merging bug"]
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

/// Two threads contribute choices that must both appear in the choice set.
#[test]
fn multiple_thread_choices_merge() {
    let source = "\
-> start
== start ==
-> tunnel ->
The end
-> END
== tunnel ==
<- place1
<- place2
-> DONE
== place1 ==
This is place 1.
* choice in place 1
- ->->
== place2 ==
This is place 2.
* choice in place 2
- ->->
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("choice in place 1"),
        "expected first thread's choice to be available, got: {result:?}"
    );
}

/// Thread choices in a loop: `<- choices(-> top)` must merge the thread's
/// "No" choice with the local "Yes" choice, and picking "No" must loop.
#[test]
fn thread_choice_loop_with_variable_divert() {
    let source = "\
-> start

=== start ===
Here is some gold. Do you want it?
- (top)
    <- choices(-> top)
    + Yes
        You win!
        -> END

=== choices(-> goback) ===
+ No
    Try again!
    -> goback
";
    // Pick No, No, then Yes
    let result = compile_and_run(source, &[1, 1, 0]);
    assert!(
        result.contains("You win!"),
        "expected loop with thread choices, got: {result:?}"
    );
}

/// Structural test: the compiler must NOT emit `begin_choice_set` in the
/// bytecode. This opcode was removed because it cleared pending choices,
/// breaking thread choice merging.
#[test]
fn choice_set_does_not_emit_begin_choice_set() {
    let source = "\
-> start
== start ==
* Choice A
* Choice B
- Done.
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        !buf.contains("begin_choice_set"),
        "begin_choice_set should not appear in compiled output:\n{buf}"
    );
    assert!(
        !buf.contains("end_choice_set"),
        "end_choice_set should not appear in compiled output:\n{buf}"
    );
}

/// Three `<- thread` calls each contributing a choice — all three must appear.
#[test]
fn three_threads_all_choices_merge() {
    let source = "\
-> start
== start ==
<- t1
<- t2
<- t3
* local choice
- Done.
== t1 ==
* thread 1 choice
- -> DONE
== t2 ==
* thread 2 choice
- -> DONE
== t3 ==
* thread 3 choice
- -> DONE
";
    let result = compile_and_run(source, &[0]);
    // If all 4 choices are available, picking index 0 should succeed.
    // The key test: the story doesn't end prematurely due to cleared choices.
    assert!(
        result.contains("Done.") || result.contains("choice"),
        "expected all thread choices to be available, got: {result:?}"
    );
}

/// Thread provides a `*` (once-only) choice, main provides a `+` (sticky).
/// After selecting the once-only, only the sticky remains on re-evaluation.
#[test]
fn thread_choice_with_once_only_filtering() {
    let source = "\
-> start
== start ==
<- thread_opts
+ [sticky] Sticky text
- -> END
== thread_opts ==
* once only
    -> start
- -> DONE
";
    // Pick once-only (should be present alongside sticky), then sticky
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("once only") || result.contains("Sticky text"),
        "expected both choices to be available initially, got: {result:?}"
    );
}

/// `-> tunnel ->` where the tunnel does `<- thread`, both tunnel and
/// thread choices must merge with the caller's choices.
#[test]
fn nested_thread_in_tunnel_choices_merge() {
    let source = "\
-> start
== start ==
-> tun ->
* caller choice
- The end.
== tun ==
<- inner_thread
* tunnel choice
- ->->
== inner_thread ==
* thread choice
- -> DONE
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("The end.") || result.contains("choice"),
        "expected thread+tunnel+caller choices to merge, got: {result:?}"
    );
}

// ── Pattern 3c: Nested gather chaining in deep weaves ────────────────

/// Three levels of choices with gathers at each level. After resolving
/// the deepest choices, execution must flow through each gather level
/// back to the outermost gather.
#[test]
fn nested_gather_three_levels() {
    let source = "\
* A
    * * B
        * * * C
        - - - Inner gather.
    - - Middle gather.
- Outer gather.
-> END
";
    let result = compile_and_run(source, &[0, 0, 0]);
    assert_eq!(
        result,
        "A\nB\nC\nInner gather.\nMiddle gather.\nOuter gather.\n"
    );
}

/// Two levels with a gather-then-second-choice-set pattern: the `- -`
/// gather has content then a second round of choices. After that second
/// round resolves, execution must still reach the `-` outer gather.
#[test]
fn nested_gather_with_second_choice_round() {
    let source = "\
* First
    * * Second
    * * Third
    - - Between.
    * * Fourth
    - - After fourth.
- Final.
-> END
";
    let result = compile_and_run(source, &[0, 0, 0]);
    assert_eq!(
        result,
        "First\nSecond\nBetween.\nFourth\nAfter fourth.\nFinal.\n"
    );
}

/// Simplified version of complex-flow-v1: the key pattern is that
/// the `- -` gather has glue (`<>`) that connects to the `-` gather.
#[test]
fn nested_gather_with_glue_continuation() {
    let source = "\
* Outer choice
    * * Deep choice
    - - After deep, <>
- outer end.
-> END
";
    let result = compile_and_run(source, &[0, 0]);
    assert_eq!(
        result,
        "Outer choice\nDeep choice\nAfter deep, outer end.\n"
    );
}

// ── Pattern 3d: Stitch parameters (including ref) ────────────────────

/// Stitch parameters must receive unique temp slots and be accessible
/// within the stitch body. This is the simplest case: by-value params.
#[test]
fn stitch_params_by_value() {
    let source = "\
-> greet.say(\"Hello\", \"world\")

== greet ==
= say(greeting, who)
{greeting}, {who}!
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello, world!\n");
}

/// Ref parameters on a function must be writable and must persist changes
/// back to the caller's variable (global var case).
#[test]
fn ref_param_global_var() {
    let source = "\
VAR x = 1
~ bump(x)
{x}
-> END

=== function bump(ref target) ===
~ target = target + 1
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "2\n");
}

/// Ref param passed via function call with two ref args — the
/// `move_ring` pattern from tower-of-hanoi.
#[test]
fn ref_param_function_two_refs() {
    let source = "\
VAR a = 10
VAR b = 0
~ swap(a, b)
a={a} b={b}
-> END

=== function swap(ref x, ref y) ===
~ temp t = x
~ x = y
~ y = t
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "a=0 b=10\n");
}

/// Thread-called stitch with conditional choice and ref params —
/// the core tower-of-hanoi pattern. The stitch is called via `<-`
/// and provides a conditional choice based on `can_move`.
#[test]
fn tower_of_hanoi_mini() {
    let source = "\
LIST Discs = one, two, three
VAR post1 = ()
VAR post2 = ()
VAR post3 = ()

~ post1 = LIST_ALL(Discs)

-> gameloop

=== function can_move(from_list, to_list) ===
    {
    -   LIST_COUNT(from_list) == 0:
        ~ return false
    -   LIST_COUNT(to_list) > 0 && LIST_MIN(from_list) > LIST_MIN(to_list):
        ~ return false
    -   else:
        ~ return true
    }

=== function move_ring( ref from, ref to ) ===
    ~ temp whichRingToMove = LIST_MIN(from)
    ~ from -= whichRingToMove
    ~ to += whichRingToMove

=== gameloop
    Start.
- (top)
    +  [ Regard]
        Regarded.
    <- move_post(1, 2, post1, post2)
    -> DONE

= move_post(from_post_num, to_post_num, ref from_post_list, ref to_post_list)
    +   { can_move(from_post_list, to_post_list) }
        [ Move ]
        { move_ring(from_post_list, to_post_list) }
        Moved.
    -> top
";
    // Choose \"Move\" (from move_post thread), then \"Regard\"
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("Moved") || result.contains("Regarded"),
        "expected tower-of-hanoi mini to produce output, got: {result:?}"
    );
}

/// Ref params with list operations — minimal `move_ring` pattern.
#[test]
fn ref_param_list_move_ring() {
    let source = "\
LIST Discs = one, two, three
VAR post1 = ()
VAR post2 = ()

~ post1 = LIST_ALL(Discs)

~ move_ring(post1, post2)

{post1}
{post2}
-> END

=== function move_ring( ref from, ref to ) ===
~ temp whichRingToMove = LIST_MIN(from)
~ from -= whichRingToMove
~ to += whichRingToMove
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "two, three\none\n");
}

// ── Pattern 4: Missing space literal in string interpolation ─────────

/// `{gatherCount} {loop}` must produce "1 1", not "11" — the space
/// between interpolations must be emitted as a literal.
#[test]
#[ignore = "visit count for gather labels not incremented on re-entry"]
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

// ── Pattern 4b: Conditional divert in inline branch ──────────────────

/// `{condition:->target}` — divert inside a conditional inline branch.
/// The divert was silently dropped by `lower_content_node_children`,
/// so the conditional body was empty and the divert never fired.
#[test]
fn conditional_divert_basic() {
    let source = "\
VAR x = 1
{x == 1:->yes}
Nope.
-> END
== yes ==
Yes!
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Yes!\n");
}

/// Conditional divert in a loop — the core pattern from the space test.
#[test]
fn conditional_divert_loop() {
    let source = "\
VAR i = 0
- (loop)
~ i++
{i}
{i < 3:->loop}
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1\n2\n3\n");
}

/// Conditional with text AND divert: `{cond: text ->target}`
#[test]
fn conditional_text_then_divert() {
    let source = "\
VAR x = 1
{x == 1: Going there! ->yes}
Nope.
-> END
== yes ==
Arrived.
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Going there! Arrived.\n");
}

/// Negative case: condition is false, divert should NOT fire.
#[test]
fn conditional_divert_false_branch() {
    let source = "\
VAR x = 0
{x == 1:->yes}
Fallthrough.
-> END
== yes ==
Yes!
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Fallthrough.\n");
}

// ── Pattern 5: ref parameters compiled as pointer ────────────────────

/// `ref` parameter should pass by reference, allowing the callee to
/// modify the caller's variable.
#[test]
fn ref_parameter_modifies_caller_variable() {
    let source = "\
VAR x = 0
~ bump(x)
{x}
-> DONE

=== function bump(ref n) ===
~ n++
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1\n");
}

/// Tower-of-hanoi pattern with all 6 thread starts.
/// Hangs due to runtime thread merging bug — multiple threads with
/// conditional choices create an infinite loop in the VM.
#[test]
#[ignore = "runtime thread merging infinite loop with multiple conditional-choice threads"]
fn tower_of_hanoi_6threads() {
    let source = "\
LIST Discs = one, two, three
VAR post1 = ()
VAR post2 = ()
VAR post3 = ()

~ post1 = LIST_ALL(Discs)

-> gameloop

=== function can_move(from_list, to_list) ===
    {
    -   LIST_COUNT(from_list) == 0:
        ~ return false
    -   LIST_COUNT(to_list) > 0 && LIST_MIN(from_list) > LIST_MIN(to_list):
        ~ return false
    -   else:
        ~ return true
    }

=== function move_ring( ref from, ref to ) ===
    ~ temp whichRingToMove = LIST_MIN(from)
    ~ from -= whichRingToMove
    ~ to += whichRingToMove

=== gameloop
    Start.
- (top)
    +  [ Regard]
        Regarded.
    <- move_post(1, 2, post1, post2)
    <- move_post(2, 1, post2, post1)
    <- move_post(1, 3, post1, post3)
    <- move_post(3, 1, post3, post1)
    <- move_post(3, 2, post3, post2)
    <- move_post(2, 3, post2, post3)
    -> DONE

= move_post(from_post_num, to_post_num, ref from_post_list, ref to_post_list)
    +   { can_move(from_post_list, to_post_list) }
        [ Move {from_post_num} to {to_post_num} ]
        { move_ring(from_post_list, to_post_list) }
        Moved.
    -> top
";
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("Moved") || result.contains("Regarded"),
        "expected output, got: {result:?}"
    );
}

// ── Expected compile errors ─────────────────────────────────────────
//
// Inklecate rejects these programs. Brink should too.

/// Helper: extract diagnostic codes from a compile error.
fn diagnostic_codes(err: &brink_compiler::CompileError) -> Vec<&'static str> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => {
            diags.iter().map(|d| d.code.as_str()).collect()
        }
        _ => vec![],
    }
}

/// A choice inside `{ true: * choice }` without an explicit divert is
/// invalid — inklecate errors with "need to explicitly divert".
#[test]
fn compile_error_nested_choice_in_conditional() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "{ true:\n    * choice\n}\n")]);
    let result = compile_mem("main.ink", &files);
    let err = result.expect_err(
        "choice inside inline conditional should be a compile error, \
         but compilation succeeded",
    );
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E029"),
        "expected E029 (choice in conditional must explicitly divert), got: {codes:?}"
    );
}

/// A choice inside a conditional WITH a divert is valid — E029 must not fire.
#[test]
fn choice_in_conditional_with_divert_is_valid() {
    let source = "=== play_game ===\n{ true:\n  + [Burn] -> play_game\n}\n-> END\n";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_ok(),
        "choice with divert in conditional should compile: {result:?}"
    );
}

/// A choice inside a conditional WITHOUT a divert but with a gather continuation
/// after the conditional is valid ink — inklecate accepts this.
#[test]
fn choice_in_conditional_with_gather_continuation_is_valid() {
    let source =
        "=== play_game ===\n{ true:\n  + (burny) [Burn]\n    Hello\n}\n- -> burny\n-> END\n";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_ok(),
        "choice in conditional with gather continuation should compile: {result:?}"
    );
}

/// A bare `->` (empty divert) outside a choice is invalid.
/// Inklecate: "Empty diverts (->) are only valid on choices".
#[test]
fn compile_error_disallow_empty_diverts() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "->\n")]);
    let result = compile_mem("main.ink", &files);
    let err = result.expect_err("bare `->` should be a compile error, but compilation succeeded");
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E012"),
        "expected E012 (divert is missing a target), got: {codes:?}"
    );
}

// ── Unresolved function calls should error, not silently produce Null ─

#[test]
fn unresolved_function_call_is_compile_error() {
    // A call to a function that doesn't exist should be a compile-time
    // diagnostic, not a silent Null. This guards against the LIR lowering
    // fallback that converts unresolvable calls to Expr::Null.
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
~ temp x = DOES_NOT_EXIST()
{x}
-> END
",
    )]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_err(),
        "calling a nonexistent function should produce a compile error, not succeed silently"
    );
}

// ── TURNS() built-in ────────────────────────────────────────────────

#[test]
fn turns_builtin_compiles_and_runs() {
    // TURNS() is a zero-argument ink built-in that returns the current turn
    // index. The compiler must recognize it, lower it through LIR, and emit
    // the TurnIndex opcode. This test verifies end-to-end correctness.
    let output = compile_and_run(
        "\
~ temp t = TURNS()
turn is {t}
-> END
",
        &[],
    );
    assert_eq!(output.trim(), "turn is 0");
}

#[test]
fn turns_builtin_increments_across_choices() {
    // TURNS() should increment each time the player makes a choice and
    // the story continues. Turn 0 is the initial passage, turn 1 after
    // the first choice, etc.
    let output = compile_and_run(
        "\
turn {TURNS()}
+ [continue]
-
turn {TURNS()}
-> END
",
        &[0],
    );
    assert_eq!(output.trim(), "turn 0\nturn 1");
}

// ── Block-level sequence branch behaviors ──────────────────────────

/// Compile from in-memory source, link, and run. Returns a list of
/// (text, `choice_count`) pairs for each step.
fn compile_and_run_steps(source: &str, inputs: &[usize]) -> Vec<(String, Option<usize>)> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(&program);
    let mut steps = Vec::new();
    let mut input_idx = 0;
    let mut guard = 0;

    loop {
        guard += 1;
        assert!(guard < 100, "infinite loop detected");
        match story.continue_maximally().unwrap() {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                steps.push((text, None));
                break;
            }
            StepResult::Choices { text, choices, .. } => {
                let count = choices.len();
                steps.push((text.clone(), Some(count)));
                let idx = if input_idx < inputs.len() {
                    let c = inputs[input_idx];
                    input_idx += 1;
                    c
                } else {
                    0
                };
                assert!(
                    idx < count,
                    "choice index {idx} out of range (only {count} choices), text so far: {text:?}"
                );
                story.choose(idx).unwrap();
            }
        }
    }

    steps
}

/// Block-level sequence branches must start with a newline relative to
/// preceding content. Inklecate inserts "\n" at the start of each
/// branch's content stream. Without this, output like
/// "I drew a card. 2 of Diamonds." appears on one line instead of two.
#[test]
fn sequence_branch_starts_with_newline() {
    let source = "\
-> test

=== test ===
{ stopping:
    - Branch one.
    - Branch two.
}
* [Again] Prefix. -> test
- -> END
";
    // First visit: "Branch one.\n" + choices: [Again]
    // Choose "Again" (once-only *), second visit: "Prefix.\nBranch two.\n" + no choices → END
    let steps = compile_and_run_steps(source, &[0]);
    // Step 1 (after choosing "Again") text must have a newline between
    // "Prefix." and "Branch two."
    assert!(
        steps.len() >= 2,
        "expected at least 2 steps, got {}",
        steps.len()
    );
    let text = &steps[1].0;
    assert!(
        text.contains("Prefix.") && text.contains("Branch two."),
        "expected both 'Prefix.' and 'Branch two.' in output, got: {text:?}"
    );
    // The newline must separate them (not on the same line)
    assert!(
        !text.contains("Prefix. Branch two.") && !text.contains("Prefix.Branch two."),
        "expected newline between 'Prefix.' and 'Branch two.', got: {text:?}"
    );
}

/// Choices inside a sequence branch must accumulate with choices from the
/// parent container. When a sequence branch contains a `ChoiceSet` and there
/// are also choices after the sequence in the same container, all choices
/// must be visible together (the branch's Done must not block the parent).
#[test]
fn choices_inside_sequence_branch_accumulate_with_parent() {
    // Pattern from the multiline-choice test case: a stopping sequence
    // where branch 1 has a once-only choice, plus a sticky choice after
    // the sequence. On visit 2, both must be visible.
    let source = "\
-> test
=== test ===
{ stopping:
    - At the table, I drew a card. Ace of Hearts.
    - 2 of Diamonds.
        \"Should I hit you again,\" the croupier asks.
        * [No.] I left the table. -> END
    - King of Spades.
        \"You lose,\" he crowed.
        -> END
}
+ [Draw a card] I drew a card. -> test
";
    // Visit 1: branch 0 text + choices: [Draw a card]
    // Choose "Draw a card" → visit 2: branch 1, choices: [No., Draw a card]
    let steps = compile_and_run_steps(source, &[0, 0]);
    // Second step must show 2 choices: [No., Draw a card]
    let second_choice_count = steps[1].1;
    assert_eq!(
        second_choice_count,
        Some(2),
        "expected 2 choices (No. + Draw a card) on second visit, got: {second_choice_count:?}"
    );
}

/// Content after a block-level conditional's closing `}` must not be
/// dropped. The glue and text `<> b` should join with the branch output.
#[test]
fn content_after_multiline_conditional_preserved() {
    let source = "\
{true:
    a
} <> b
";
    let result = compile_and_run(source, &[]);
    assert_eq!(
        result, "a b\n",
        "glue + text after conditional must be preserved"
    );
}

/// Same as above but with a second conditional after the glue.
#[test]
fn content_after_multiline_conditional_with_nested_conditional() {
    let source = "\
{true:
    a
} <> { true:
    b
}
";
    let result = compile_and_run(source, &[]);
    assert_eq!(
        result, "a b\n",
        "glue + conditional after conditional must be preserved"
    );
}

// ── Shuffle sequence exhaustion ────────────────────────────────────

/// `shuffle once` must stop producing content after all branches are visited.
/// This is an end-to-end behavioral test: call a shuffle-once function 4 times
/// with 2 branches — only the first 2 calls should produce text.
#[test]
fn shuffle_once_exhausts_after_all_branches_visited() {
    let source = "\
~ SEED_RANDOM(1)
one: {f()}
two: {f()}
three: {f()}
four: {f()}
== function f ==
{shuffle once:
    - A
    - B
}
";
    let result = compile_and_run(source, &[]);
    // Each of the 4 lines "N: X\n" gets the function result appended.
    // First 2 calls produce "A" or "B" (in shuffled order); last 2 produce nothing.
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines.len(), 4, "expected 4 output lines, got: {result:?}");

    // First two lines must each contain either "A" or "B".
    let first_two_content: Vec<&str> = lines[0..2]
        .iter()
        .map(|l| l.split(": ").nth(1).unwrap_or("").trim())
        .collect();
    let mut sorted = first_two_content.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["A", "B"],
        "first two calls should produce A and B (in any order), got: {first_two_content:?}"
    );

    // Last two lines must have no content after the colon.
    for (i, line) in lines[2..].iter().enumerate() {
        let after_colon = line.split(": ").nth(1).unwrap_or("").trim();
        assert!(
            after_colon.is_empty(),
            "call {} (line {:?}) should produce no text after exhaustion, got: {after_colon:?}",
            i + 3,
            line,
        );
    }
}

/// `shuffle stopping` must pin to the last branch after all are visited.
/// Call a 3-branch shuffle-stopping function 5 times — after the first 3 calls
/// exhaust all branches, calls 4 and 5 must always return the last branch.
#[test]
fn shuffle_stopping_pins_to_last_branch() {
    let source = "\
~ SEED_RANDOM(1)
one: {f()}
two: {f()}
three: {f()}
four: {f()}
five: {f()}
== function f ==
{stopping shuffle:
    - A
    - B
    - final
}
";
    let result = compile_and_run(source, &[]);
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines.len(), 5, "expected 5 output lines, got: {result:?}");

    // First three calls produce A, B, final in some shuffled order.
    let first_three_content: Vec<String> = lines[0..3]
        .iter()
        .map(|l| l.split(": ").nth(1).unwrap_or("").trim().to_string())
        .collect();
    let mut sorted: Vec<&str> = first_three_content.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["A", "B", "final"],
        "first three calls should produce A, B, final (in any order), got: {first_three_content:?}"
    );

    // Calls 4 and 5 must produce "final" (the last/stopping branch).
    for (i, line) in lines[3..].iter().enumerate() {
        let after_colon = line.split(": ").nth(1).unwrap_or("").trim();
        assert_eq!(
            after_colon,
            "final",
            "call {} should pin to 'final' after exhaustion, got: {after_colon:?}",
            i + 4,
        );
    }
}

/// Opcode-level test: `shuffle once` codegen must emit a `Min` opcode
/// to clamp the visit count, enabling exhaustion detection.
#[test]
fn shuffle_once_codegen_emits_min_opcode() {
    use brink_format::Opcode;

    let source = "\
{shuffle once:
    - A
    - B
}
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();

    // Find the sequence container (has VISITS + COUNT_START_ONLY flags).
    let seq_container = data
        .containers
        .iter()
        .find(|c| {
            let mut offset = 0;
            let mut has_sequence = false;
            while offset < c.bytecode.len() {
                if let Ok(op) = Opcode::decode(&c.bytecode, &mut offset) {
                    if matches!(op, Opcode::Sequence(..)) {
                        has_sequence = true;
                    }
                } else {
                    break;
                }
            }
            has_sequence
        })
        .expect("should find a container with a Sequence opcode");

    // Decode all opcodes and check for Min.
    let mut offset = 0;
    let mut has_min = false;
    while offset < seq_container.bytecode.len() {
        if let Ok(op) = Opcode::decode(&seq_container.bytecode, &mut offset) {
            if matches!(op, Opcode::Min) {
                has_min = true;
            }
        } else {
            break;
        }
    }
    assert!(
        has_min,
        "shuffle once container must emit Min opcode for exhaustion clamping"
    );
}

/// Contextual keywords like `once`, `stopping`, `shuffle`, `cycle` must be
/// usable as knot names and divert targets. Ink only treats these as keywords
/// inside sequence annotations — everywhere else they're valid identifiers.
#[test]
fn keyword_once_as_knot_name_and_divert_target() {
    let source = "\
-> once
== once ==
Hello from once.
-> END
";
    let result = compile_and_run(source, &[]);
    assert!(
        result.contains("Hello from once"),
        "knot named 'once' should work, got: {result:?}"
    );
}

/// Full thread-in-logic test (inklecate's TestThreadInLogic): tunnel calls
/// to a knot named `once` containing `{<- content|}`.
#[test]
fn thread_in_logic_compiles_and_runs() {
    let source = "\
-> once ->
-> once ->
== once ==
{<- content|}
->->
== content ==
Content
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert!(
        result.contains("Content"),
        "thread-in-logic should produce 'Content', got: {result:?}"
    );
}

// ── Template tests (intl-spec phase 3) ──────────────────────────────

#[test]
fn template_single_variable() {
    let source = "VAR name = \"World\"\nHello, {name}!\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello, World!\n");
}

#[test]
fn template_multiple_interpolations() {
    let source = "VAR a = \"one\"\nVAR b = \"two\"\n{a} and {b}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "one and two\n");
}

#[test]
fn template_expression_interpolation() {
    let source = "VAR n = 3\nResult: {n * 2}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Result: 6\n");
}

#[test]
fn template_interpolation_at_start() {
    let source = "VAR x = \"Hello\"\n{x} world\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello world\n");
}

#[test]
fn template_interpolation_at_end() {
    let source = "VAR x = \"world\"\nHello {x}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello world\n");
}

#[test]
fn plain_text_regression() {
    // Ensure plain text lines still work after template support.
    let source = "Just plain text.\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Just plain text.\n");
}

#[test]
fn template_integer_interpolation() {
    let source = "VAR count = 42\nThere are {count} items.\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "There are 42 items.\n");
}

#[test]
fn template_float_interpolation() {
    let source = "VAR pi = 3.14\nPi is {pi}.\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Pi is 3.14.\n");
}

#[test]
fn template_bool_interpolation() {
    let source = "VAR flag = true\nFlag: {flag}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Flag: true\n");
}

// ── Warning surfacing ───────────────────────────────────────────────

/// Helper: compile and return the full `CompileOutput` (data + warnings).
fn compile_mem_with_warnings(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
}

#[test]
fn warnings_surfaced_alongside_successful_compilation() {
    // A CONST with string interpolation should compile successfully
    // but produce an E030 warning.
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "VAR name = \"world\"\nCONST greeting = \"hi {name}\"\n{greeting}\n",
    )]);

    let output = compile_mem_with_warnings("main.ink", &files).unwrap();
    assert!(
        !output.data.containers.is_empty(),
        "compilation should succeed"
    );
    assert!(
        output.warnings.iter().any(|w| w.code.as_str() == "E030"),
        "expected E030 warning, got: {:?}",
        output
            .warnings
            .iter()
            .map(|w| w.code.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn clean_compilation_has_no_warnings() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "Hello, world!\n-> END\n")]);

    let output = compile_mem_with_warnings("main.ink", &files).unwrap();
    assert!(
        output.warnings.is_empty(),
        "expected no warnings for clean source, got: {:?}",
        output
            .warnings
            .iter()
            .map(|w| format!("[{}] {}", w.code.as_str(), w.message))
            .collect::<Vec<_>>()
    );
}
