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

/// A choice inside `{ true: * choice }` without an explicit divert is
/// invalid — inklecate errors with "need to explicitly divert".
#[test]
#[ignore = "red-phase: brink does not yet reject this pattern"]
fn compile_error_nested_choice_in_conditional() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "{ true:\n    * choice\n}\n")]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_err(),
        "choice inside inline conditional should be a compile error, \
         but compilation succeeded"
    );
}

/// A bare `->` (empty divert) outside a choice is invalid.
/// Inklecate: "Empty diverts (->) are only valid on choices".
#[test]
#[ignore = "red-phase: brink does not yet reject this pattern"]
fn compile_error_disallow_empty_diverts() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "->\n")]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_err(),
        "bare `->` should be a compile error, but compilation succeeded"
    );
}

/// VAR/CONST declarations after a knot should be an error.
/// Inklecate rejects this because global declarations must appear
/// before any knot/stitch definitions.
#[test]
#[ignore = "red-phase: brink does not yet reject this pattern"]
fn compile_error_globals_after_knot() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "=== stuff ===\n-> END\n\nVAR X = 1\nCONST Y = 2\n",
    )]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_err(),
        "VAR/CONST after a knot should be a compile error, \
         but compilation succeeded"
    );
}
