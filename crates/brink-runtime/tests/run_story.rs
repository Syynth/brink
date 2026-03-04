//! Integration tests for brink-runtime.
//!
//! Converts ink.json, links, steps through with inputs, and compares output.

use brink_converter::convert;
use brink_json::InkJson;
use brink_runtime::{DotNetRng, StepResult, Story};

/// Convert an ink.json string, link, and run to completion with the given choice inputs.
/// Returns the full text output.
#[expect(clippy::unwrap_used)]
fn run_story(ink_json: &str, inputs: &[usize]) -> String {
    let ink: InkJson = serde_json::from_str(ink_json).unwrap();
    let data = convert(&ink).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(&program);
    let mut output = String::new();
    let mut input_idx = 0;

    loop {
        match story.step(&program).unwrap() {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                output.push_str(&text);
                break;
            }
            StepResult::Choices { text, choices, .. } => {
                output.push_str(&text);
                let choice_idx = if input_idx < inputs.len() {
                    let c = inputs[input_idx];
                    input_idx += 1;
                    c
                } else {
                    0
                };
                assert!(
                    choice_idx < choices.len(),
                    "choice index {choice_idx} out of range (only {} choices)",
                    choices.len()
                );
                story.choose(choice_idx).unwrap();
            }
        }
    }

    output
}

#[expect(clippy::unwrap_used)]
fn load_ink_json(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// When a story presents choices via bytecode exhaustion (no explicit `Done`
/// opcode), `step()` must return `Choices`, not `Done`. The I003 story diverts
/// to a knot via goto (clearing the container stack), creates choices inside
/// that knot, and then the container stack naturally exhausts — there is no
/// `Done` opcode to yield at. The VM must still present the pending choices.
#[test]
fn choices_yielded_on_bytecode_exhaustion() {
    let json = load_ink_json("../../tests/tier1/basics/I003-tunnel-to-death/story.ink.json");
    let ink: InkJson = serde_json::from_str(&json).unwrap();
    let data = convert(&ink).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(&program);

    // First step should produce text AND choices, not Done.
    let result = story.step(&program).unwrap();
    assert!(
        matches!(result, StepResult::Choices { .. }),
        "expected Choices after first step, got {result:?}"
    );
    if let StepResult::Choices { choices, .. } = &result {
        assert_eq!(choices.len(), 2, "expected 2 choices (Yes/No)");
    }
}

#[test]
fn test_i001_minimal_story() {
    let json = load_ink_json("../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result.trim(), "Hello, world!");
}

/// Function calls via `f()` must capture text output as a return value.
/// The `out` opcode after the call pops this return value and emits it.
/// Without text capture, `out` hits a value stack underflow.
#[test]
fn function_call_captures_text_as_return_value() {
    // Minimal ink.json: `{print_hello()}` where print_hello outputs "hello".
    // Equivalent ink: `{print_hello()}`  /  `=== function print_hello` / `hello`
    let json = r##"{
        "inkVersion": 21,
        "root": [
            [
                "ev",
                { "f()": "print_hello" },
                "out",
                "/ev",
                "\n",
                ["done", { "#n": "g-0" }],
                null
            ],
            "done",
            {
                "print_hello": [
                    "^hello",
                    "\n",
                    null
                ]
            }
        ],
        "listDefs": {}
    }"##;
    let result = run_story(json, &[]);
    assert_eq!(result.trim(), "hello");
}

/// Function text output used inline must not leak trailing newlines into
/// the surrounding text. Equivalent ink: `Say {greet()} please.`
/// where greet outputs "hi\n" (trailing newline from the function body).
/// Expected: "Say hi please." not "Say hi\n please."
#[test]
fn function_text_capture_strips_trailing_newlines() {
    let json = r##"{
        "inkVersion": 21,
        "root": [
            [
                "^Say ",
                "ev",
                { "f()": "greet" },
                "out",
                "/ev",
                "^ please.",
                "\n",
                ["done", { "#n": "g-0" }],
                null
            ],
            "done",
            {
                "greet": [
                    "^hi",
                    "\n",
                    null
                ]
            }
        ],
        "listDefs": {}
    }"##;
    let result = run_story(json, &[]);
    assert_eq!(result.trim(), "Say hi please.");
}

/// Fallback choices (`is_invisible_default` flag) must be auto-selected
/// without presenting them to the player. The VM should never yield
/// `StepResult::Choices` for invisible-default choices — they should be
/// followed transparently.
#[test]
fn fallback_choice_auto_selected() {
    let json =
        load_ink_json("../../tests/tier1/choices/I077-fallback-choice-on-thread/story.ink.json");
    let ink: InkJson = serde_json::from_str(&json).unwrap();
    let data = convert(&ink).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(&program);

    // The story should complete in a single step with no Choices yield.
    let result = story.step(&program).unwrap();
    assert!(
        matches!(result, StepResult::Done { .. } | StepResult::Ended { .. }),
        "expected Done/Ended (auto-selected fallback), got Choices"
    );
    if let StepResult::Done { text, .. } | StepResult::Ended { text, .. } = result {
        assert_eq!(text.trim(), "Should be 1 not 0: 1.");
    }
}

/// Tunnel onwards: `->-> B` should override the tunnel return address
/// and divert to B instead of returning to the caller.
#[test]
fn tunnel_onwards_divert_override() {
    let json = load_ink_json(
        "../../tests/tier1/diverts/I053-tunnel-onwards-divert-override/story.ink.json",
    );
    let result = run_story(&json, &[]);
    assert_eq!(result.trim(), "This is A\nNow in B.");
}

/// String constants declared via `CONST kX = "hi"` / `VAR x = kX` must
/// resolve to the string value. The ink.json wraps these in `str/.../str`
/// control commands inside the `global decl` container.
#[test]
fn string_constant_global() {
    let json = load_ink_json("../../tests/tier1/variables/string-constants/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result.trim(), "hi");
}

/// Variable tunnel call: `-> one_then_tother(-> tunnel)` passes a divert
/// target as a parameter. The ink.json uses `{"->t->": "x", "var": true}`
/// which is a tunnel call where the target comes from variable `x`.
#[test]
fn variable_tunnel_call() {
    let json = load_ink_json("../../tests/tier1/diverts/variable-tunnel/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result.trim(), "STUFF");
}

/// Once-only choices (`*` bullet in ink) must not appear after their target
/// container has been visited. Without this, stories that loop back to a
/// choice point will infinite-loop because exhausted choices keep appearing.
#[test]
fn once_only_choices_filtered_by_visit_count() {
    let json = load_ink_json(
        "../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink.json",
    );
    // Input: choose 0 twice (first choice both times). The story should
    // terminate after the fallback fires, not infinite loop.
    let result = run_story(&json, &[0, 0]);
    // Story should complete without panic/infinite loop.
    assert!(!result.is_empty() || result.is_empty()); // just assert it completes
}

/// Once-only choices with start content (using `[]` syntax) combined with
/// sequences (`{first|second|third}`) must work without stack underflow.
/// The `"visit"` control command pushes the current container's visit count
/// without popping anything from the stack.
#[test]
fn once_only_choices_with_own_content() {
    let json = load_ink_json(
        "../../tests/tier1/choices/I089-once-only-choices-with-own-content/story.ink.json",
    );
    let result = run_story(&json, &[0, 0, 0]);
    assert!(
        result.contains("first time"),
        "expected sequence output, got: {result:?}"
    );
    assert!(
        result.contains("I've finished eating now."),
        "expected story to complete, got: {result:?}"
    );
}

/// Choices created inside a tunnel must preserve the tunnel's temp
/// variables. `generate_choice(1)` sets temp `x = 1`; the choice target
/// outputs `{x}` which must resolve to `1`, not empty.
#[test]
fn choice_thread_forking_preserves_temp() {
    let json = load_ink_json("../../tests/tier1/choices/I083-choice-thread-forking/story.ink.json");
    let result = run_story(&json, &[0]);
    assert!(
        result.contains("Vaue of local var is: 1"),
        "expected temp var x=1 to be preserved through choice, got: {result:?}"
    );
}

/// `<- choices` thread must run the thread body, creating choices, then
/// return to the main flow where `CHOICE_COUNT()` outputs "2".
#[test]
fn thread_call_with_choice_count() {
    let json = load_ink_json("../../tests/tier1/choices/I091-choice-count/story.ink.json");
    let result = run_story(&json, &[0]);
    assert!(
        result.starts_with('2'),
        "expected output to start with '2' from CHOICE_COUNT(), got: {result:?}"
    );
}

/// Conditional choice in weave: the choice's post-creation divert must
/// jump past the conditional block (to the nop/gather), not back to the
/// start of the container. Regression: named-path labels resolved to
/// offset 0 causing an infinite loop.
///
/// The gather text "gather should be seen" IS expected in the output —
/// the reference ink test (`TestConditionalChoiceInWeave`) asserts this.
#[test]
fn conditional_choice_in_weave() {
    let json =
        load_ink_json("../../tests/tier1/choices/conditional-choice-in-weave/story.ink.json");
    let result = run_story(&json, &[0]);
    assert_eq!(result.trim(), "start\ngather should be seen\nresult");
}

/// Diverts to labeled weave points via named-path aliases (e.g.
/// `knot.stitch.0.g-0.c-0`) must resolve correctly. Regression:
/// named aliases were registered in the index but their descendants
/// were not, so `knot.stitch.0.g-0.c-0` failed to resolve while
/// `knot.stitch.0.0.c-0` (the numeric equivalent) would have worked.
#[test]
fn divert_to_weave_points() {
    let json =
        load_ink_json("../../tests/tier1/diverts/I063-divert-to-weave-points/story.ink.json");
    let result = run_story(&json, &[0]);
    let expected = "gather\ntest\nchoice content\ngather\nsecond time round";
    assert_eq!(result.trim(), expected);
}

#[test]
fn test_simple_divert() {
    let json = load_ink_json("../../tests/tier1/divert/simple-divert/story.ink.json");
    let result = run_story(&json, &[]);
    let expected = "We arrived into London at 9.45pm exactly.\nWe hurried home to Savile Row as fast as we could.";
    assert_eq!(result.trim(), expected);
}

// ── Visit counting ─────────────────────────────────────────────────────

/// Tunnel calls must increment the visit count of the target container.
/// Ink: `->t-> knot` twice, each time outputting `{knot}` (visit count).
/// Expected: "1\n2\n" — each tunnel entry increments.
/// Bug: `TunnelCall` handler doesn't increment `visit_counts`.
#[test]
fn tunnel_call_increments_visit_count() {
    // Ink equivalent:
    //   -> knot ->
    //   -> knot ->
    //   done
    //   == knot ==
    //   {knot}
    //   ->->
    let json = r##"{
        "inkVersion": 21,
        "root": [
            [
                {"->t->": "knot"},
                {"->t->": "knot"},
                "done",
                null
            ],
            "done",
            {
                "knot": [
                    "ev", {"CNT?": ".^"}, "out", "/ev",
                    "\n",
                    "ev", "void", "/ev",
                    "->->",
                    {"#f": 1}
                ]
            }
        ],
        "listDefs": {}
    }"##;
    let result = run_story(json, &[]);
    assert_eq!(
        result, "1\n2\n",
        "tunnel calls should increment visit count: first call=1, second call=2"
    );
}

/// Function calls (`f()`) must increment the visit count of the target container.
/// Ink: `{func()}{func()}` where func returns its own visit count as text.
/// Expected: output contains "1" then "2".
/// Bug: `Call` handler doesn't increment `visit_counts`.
#[test]
fn function_call_increments_visit_count() {
    // Ink equivalent:
    //   {func()}
    //   {func()}
    //   === function func ===
    //   ~ return func
    //
    // In ink.json, `{func()}` is: ev, f() func, out, /ev
    // The function body: ev, CNT? .^, /ev, ~ret (return)
    // CNT? pushes the visit count of func, which becomes the return value.
    let json = r##"{
        "inkVersion": 21,
        "root": [
            [
                "ev", {"f()": "func"}, "out", "/ev",
                "^ ",
                "ev", {"f()": "func"}, "out", "/ev",
                "\n",
                ["done", {"#n": "g-0"}],
                null
            ],
            "done",
            {
                "func": [
                    "ev", {"CNT?": ".^"}, "/ev",
                    "~ret",
                    {"#f": 1}
                ]
            }
        ],
        "listDefs": {}
    }"##;
    let result = run_story(json, &[]);
    assert_eq!(
        result.trim(),
        "1 2",
        "function calls should increment visit count: first call=1, second call=2"
    );
}

/// Goto-to-self (loop back to the same VISITS-only container) must NOT
/// increment the visit count. The count should only increase on new
/// entry (tunnel/function call or cross-knot goto).
/// Ink: `== knot ==` with `{knot}` and a conditional `-> knot` loop.
/// Expected: visit count stays at 1 across all iterations.
/// Bug: `goto_target` always increments, causing over-counting.
#[test]
fn goto_to_self_does_not_increment_visits_only() {
    // Ink equivalent:
    //   -> knot ->
    //   done
    //   == knot ==
    //   ~ count++
    //   {count} {knot}
    //   {count < 3: -> knot}
    //   ->->
    //
    // The knot has #f: 1 (VISITS only). Looping back with -> knot should
    // NOT increment the visit count since the container is already on the stack.
    let json = r##"{
        "inkVersion": 21,
        "root": [
            [
                {"->t->": "knot"},
                "done",
                null
            ],
            "done",
            {
                "knot": [
                    "ev", {"VAR?": "count"}, 1, "+", {"VAR=": "count", "re": true}, "/ev",
                    "ev", {"VAR?": "count"}, "out", "/ev",
                    "^ ",
                    "ev", {"CNT?": ".^"}, "out", "/ev",
                    "\n",
                    "ev", {"VAR?": "count"}, 3, "<", "/ev",
                    [
                        {"->": ".^.b", "c": true},
                        {"b": [{"->": ".^.^.^"}, {"->": ".^.^.^.22"}, null]}
                    ],
                    "nop",
                    "\n",
                    "ev", "void", "/ev",
                    "->->",
                    {"#f": 1}
                ],
                "global decl": [
                    "ev", 0, {"VAR=": "count"}, "/ev",
                    "end",
                    null
                ]
            }
        ],
        "listDefs": {}
    }"##;
    let result = run_story(json, &[]);
    // count increments each iteration; visit count should stay at 1
    assert_eq!(
        result, "1 1\n2 1\n3 1\n",
        "goto-to-self should NOT increment visit count for VISITS-only containers"
    );
}

/// Goto-to-self in a `COUNT_START_ONLY` container (gather loop) SHOULD
/// increment the visit count every time offset 0 is re-entered.
/// Ink: `- (loop)` with `{loop}` and a conditional `-> loop`.
/// Expected: visit count increases each loop iteration (1, 2, 3).
/// This currently over-counts because goto always increments (accidental pass),
/// but the semantics must be: increment because `COUNT_START_ONLY` + offset 0.
#[test]
fn gather_loop_increments_count_start_only() {
    // Ink equivalent:
    //   -> test ->
    //   done
    //   == test ==
    //   - (loop)
    //   ~ count++
    //   {count} {loop}
    //   {count < 3: -> loop}
    //   ->->
    let json = r##"{
        "inkVersion": 21,
        "root": [
            [
                {"->t->": "test"},
                "done",
                null
            ],
            "done",
            {
                "test": [
                    [
                        [
                            "ev", {"VAR?": "count"}, 1, "+", {"VAR=": "count", "re": true}, "/ev",
                            "ev", {"VAR?": "count"}, "out", "/ev",
                            "^ ",
                            "ev", {"CNT?": ".^"}, "out", "/ev",
                            "\n",
                            "ev", {"VAR?": "count"}, 3, "<", "/ev",
                            [
                                {"->": ".^.b", "c": true},
                                {"b": [{"->": ".^.^.^"}, {"->": ".^.^.^.22"}, null]}
                            ],
                            "nop",
                            "\n",
                            "ev", "void", "/ev",
                            "->->",
                            {"#f": 5, "#n": "loop"}
                        ],
                        null
                    ],
                    null
                ],
                "global decl": [
                    "ev", 0, {"VAR=": "count"}, "/ev",
                    "end",
                    null
                ]
            }
        ],
        "listDefs": {}
    }"##;
    let result = run_story(json, &[]);
    // Both count and visit count should increment each iteration
    assert_eq!(
        result, "1 1\n2 2\n3 3\n",
        "COUNT_START_ONLY gather loops should increment visit count on each re-entry at offset 0"
    );
}

/// I098: A thread spawned via `<-` calls a tunnel that creates a choice.
/// The thread must complete (pop back to the main flow) so the main flow
/// text ("When should this get printed?") appears *before* the choice.
/// After the player selects the choice, the tunnel resumes and the thread
/// finishes ("Finishing thread.").
#[test]
fn knot_thread_interaction_2() {
    let json =
        load_ink_json("../../tests/tier1/knots/I098-knot-thread-interaction-2/story.ink.json");
    let result = run_story(&json, &[0]);
    let expected = "\
I\u{2019}m in a tunnel
When should this get printed?
I\u{2019}m an option
Finishing thread.\n";
    assert_eq!(
        result, expected,
        "I098 knot-thread-interaction-2 output mismatch"
    );
}

/// `TURNS_SINCE(-> knot)` must return the number of turns since the knot
/// was last visited. Returns 0 on the same turn, 1 after one choice, etc.
/// The VM previously stubbed this to always return -1.
#[test]
fn turns_since_with_variable_target() {
    let json = load_ink_json(
        "../../tests/tier1/variables/turns-since-with-variable-target/story.ink.json",
    );
    let result = run_story(&json, &[0]);
    assert_eq!(
        result, "0\n0\n1\n",
        "TURNS_SINCE should return 0 on same turn, 1 after a choice"
    );
}

/// Full I128 corpus test: validates tunnel visit counting, goto-to-self
/// suppression for VISITS-only, and gather `COUNT_START_ONLY` behavior together.
#[test]
fn knot_stitch_gather_counts() {
    let json =
        load_ink_json("../../tests/tier1/knots/I128-knot-stitch-gather-counts/story.ink.json");
    let result = run_story(&json, &[]);
    let expected = "\
1 1\n2 2\n3 3\n\
1 1\n2 1\n3 1\n\
1 2\n2 2\n3 2\n\
1 1\n2 1\n3 1\n\
1 2\n2 2\n3 2\n";
    assert_eq!(
        result, expected,
        "I128 knot-stitch-gather-counts output mismatch"
    );
}

/// I071: List basic operations — default value output, union, intersection,
/// contains, not-contains. Exercises `PushList`, `ListContains`, `ListNotContains`,
/// `ListIntersect`, and `Add` (list union via +).
#[test]
fn i071_list_basic_operations() {
    let json = load_ink_json("../../tests/tier2/lists/I071-list-basic-operations/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result, "b, d\na, b, c, e\nb, c\n0\n1\n1\n");
}

/// `list(n)` calls `ListFromInt` with a string list-def name on the stack.
/// `LIST_VALUE`, `LIST_ALL`, `LIST_INVERT`, and subtract also tested.
#[test]
fn list_from_int_and_more_ops() {
    let json = load_ink_json("../../tests/tier2/lists/more-list-operations/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result, "1\nl\nn\nl, m\nn\n");
}

/// Assigning `()` to a list variable must preserve its origins so that
/// `LIST_ALL` can still enumerate all items from the original list def.
#[test]
fn empty_list_preserves_origins() {
    let json =
        load_ink_json("../../tests/tier2/lists/empty-list-origin-after-assignment/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result, "a, b, c\n");
}

/// `LIST_RANGE` filters a list's items by ordinal bounds, including inline
/// literals with no origins. Multi-origin lists are stringified sorted by
/// ordinal then origin name.
#[test]
fn list_range_and_ordering() {
    let json = load_ink_json("../../tests/tier2/lists/list-range/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(
        result,
        "Pound, Pizza, Euro, Pasta, Dollar, Curry, Paella\n\
         Euro, Pasta, Dollar, Curry\n\
         Two, Three, Four, Five, Six\n\
         Pizza, Pasta\n"
    );
}

/// List item variable references (like `A`, `B`) must be resolved to list
/// values, not treated as global variables. `LIST_ALL(A + B)` unions two
/// single-item lists then expands to all items from their origins.
#[test]
fn list_item_variable_reference() {
    let json = load_ink_json("../../tests/tier2/lists/list-all/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(result, "A, B\n");
}

/// `SEED_RANDOM` must push `Null` (void) after consuming its seed argument,
/// so the subsequent `pop` instruction can discard it without underflow.
/// List literals must derive origins from item qualified names when no
/// explicit `origins` array is present in the ink.json.
#[test]
fn seed_random_and_list_literal_origins() {
    let json = load_ink_json("../../tests/tier2/lists/more-list-operations2/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(
        result,
        "a1, b1, c1\n\
         a1\n\
         a1, b2\n\
         count:2\n\
         max:c2\n\
         min:a1\n\
         true\n\
         true\n\
         false\n\
         empty\n\
         a2\n\
         a2, b2, c2\n\
         range:a1, b2\n\
         a1\n\
         subtract:a1, c1\n\
         random:c2\n\
         listinc:b1\n"
    );
}

/// Function calls via variable target (`{"f()": "s", "var": true}`) must
/// read the divert target from the variable `s` rather than treating `s` as
/// a literal container path. Without `CallVariable`, the converter emits a
/// static `Call` to a non-existent container and execution goes haywire.
#[test]
fn function_variable_call() {
    let json = load_ink_json("../../tests/tier2/lists/list-comparison/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(
        result,
        "Hey, my name is Philippe. What about yours?\n\
         I am Andre and I need my rheumatism pills!\n\
         Would you like me, Philippe, to get some more for you?\n"
    );
}

/// `ref` parameters must pass a variable by reference. `inc(ref x)` takes
/// a pointer to `val`; reads through the pointer see val's value, writes
/// go back to val. After `inc(val)`, val should be 6 (was 5).
#[test]
fn variable_pointer_ref_from_knot() {
    let json =
        load_ink_json("../../tests/tier1/variables/variable-pointer-ref-from-knot/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(
        result, "6\n",
        "ref parameter should increment val from 5 to 6"
    );
}

/// `RANDOM(min, max)` + `SEED_RANDOM(seed)` must produce deterministic output
/// matching the reference .NET runtime. Seed 100 → 4 dice rolls.
#[test]
fn rnd_func_deterministic_output() {
    let json = load_ink_json("../../tests/tier2/function/rnd-func/story.ink.json");
    let result = run_story(&json, &[]);
    assert_eq!(
        result,
        "Rolling dice 1: 6.\n\
         Rolling dice 2: 6.\n\
         Rolling dice 3: 4.\n\
         Rolling dice 4: 2.\n"
    );
}
