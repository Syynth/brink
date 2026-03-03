//! Integration tests for brink-runtime.
//!
//! Converts ink.json, links, steps through with inputs, and compares output.

use brink_converter::convert;
use brink_json::InkJson;
use brink_runtime::{StepResult, Story};

/// Convert an ink.json string, link, and run to completion with the given choice inputs.
/// Returns the full text output.
#[expect(clippy::unwrap_used)]
fn run_story(ink_json: &str, inputs: &[usize]) -> String {
    let ink: InkJson = serde_json::from_str(ink_json).unwrap();
    let data = convert(&ink).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    let mut story = Story::new(&program);
    let mut output = String::new();
    let mut input_idx = 0;

    loop {
        match story.step(&program).unwrap() {
            StepResult::Done { text } | StepResult::Ended { text } => {
                output.push_str(&text);
                break;
            }
            StepResult::Choices { text, choices } => {
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
    let mut story = Story::new(&program);

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
    let mut story = Story::new(&program);

    // The story should complete in a single step with no Choices yield.
    let result = story.step(&program).unwrap();
    assert!(
        matches!(result, StepResult::Done { .. } | StepResult::Ended { .. }),
        "expected Done/Ended (auto-selected fallback), got Choices"
    );
    if let StepResult::Done { text } | StepResult::Ended { text } = result {
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

#[test]
fn test_simple_divert() {
    let json = load_ink_json("../../tests/tier1/divert/simple-divert/story.ink.json");
    let result = run_story(&json, &[]);
    let expected = "We arrived into London at 9.45pm exactly.\nWe hurried home to Savile Row as fast as we could.";
    assert_eq!(result.trim(), expected);
}
