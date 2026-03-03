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

#[test]
fn test_simple_divert() {
    let json = load_ink_json("../../tests/tier1/divert/simple-divert/story.ink.json");
    let result = run_story(&json, &[]);
    let expected = "We arrived into London at 9.45pm exactly.\nWe hurried home to Savile Row as fast as we could.";
    assert_eq!(result.trim(), expected);
}
