//! Per-instance mutable story state.

use std::collections::HashMap;

use brink_format::{ChoiceFlags, DefinitionId, Value};

use crate::error::RuntimeError;
use crate::output::OutputBuffer;
use crate::program::Program;
use crate::vm;

/// The current execution status of a story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryStatus {
    /// Ready to step.
    Active,
    /// Waiting for a choice selection via [`Story::choose`].
    WaitingForChoice,
    /// Hit a `done` opcode — can still resume after output is consumed.
    Done,
    /// Hit an `end` opcode — permanently finished.
    Ended,
}

/// Result of calling [`Story::step`].
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Yielded text; can resume with another [`step`](Story::step).
    Done { text: String },
    /// Yielded text and choices; call [`choose`](Story::choose) then [`step`](Story::step).
    Choices { text: String, choices: Vec<Choice> },
    /// Story permanently ended.
    Ended { text: String },
}

/// A single choice presented to the player.
#[derive(Debug, Clone)]
pub struct Choice {
    pub text: String,
    pub index: usize,
}

// ── Internal types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContainerPosition {
    pub container_idx: u32,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    pub return_address: Option<ContainerPosition>,
    pub temps: Vec<Value>,
    pub container_stack: Vec<ContainerPosition>,
    /// True for `f()` calls — output is captured as a return value.
    pub is_function_call: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingChoice {
    pub display_text: String,
    pub target_id: DefinitionId,
    pub target_idx: u32,
    pub target_offset: usize,
    pub flags: ChoiceFlags,
    #[expect(dead_code)]
    pub original_index: usize,
    #[expect(dead_code)]
    pub output_line_idx: Option<u16>,
}

/// Per-instance mutable state for executing stories.
///
/// Created from a [`Program`] via [`Story::new`]. Holds all mutable state
/// (stacks, globals, output buffer) while the immutable program data lives
/// in [`Program`].
pub struct Story {
    pub(crate) call_stack: Vec<CallFrame>,
    pub(crate) value_stack: Vec<Value>,
    pub(crate) globals: Vec<Value>,
    pub(crate) output: OutputBuffer,
    pub(crate) visit_counts: HashMap<DefinitionId, u32>,
    pub(crate) turn_index: u32,
    pub(crate) status: StoryStatus,
    pub(crate) pending_choices: Vec<PendingChoice>,
    pub(crate) current_tags: Vec<String>,
    pub(crate) in_tag: bool,
    pub(crate) skipping_choice: bool,
}

impl Story {
    /// Create a new story instance from a linked program.
    pub fn new(program: &Program) -> Self {
        // Initialize globals from program defaults.
        let globals = program.global_defaults();

        // Set up the initial call frame pointing at the root container.
        let initial_frame = CallFrame {
            return_address: None,
            temps: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx: program.root_idx(),
                offset: 0,
            }],
            is_function_call: false,
        };

        Self {
            call_stack: vec![initial_frame],
            value_stack: Vec::new(),
            globals,
            output: OutputBuffer::new(),
            visit_counts: HashMap::new(),
            turn_index: 0,
            status: StoryStatus::Active,
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
        }
    }

    /// Execute until the next yield point (done, choices, or end).
    pub fn step(&mut self, program: &Program) -> Result<StepResult, RuntimeError> {
        if self.status == StoryStatus::Ended {
            return Err(RuntimeError::StoryEnded);
        }

        // Reset status to Active if we were in Done (resuming after output).
        if self.status == StoryStatus::Done {
            self.status = StoryStatus::Active;
        }

        let mut full_text = String::new();

        loop {
            let yield_kind = vm::run(self, program)?;

            let text = self.output.flush();
            full_text.push_str(&text);
            self.turn_index += 1;

            match yield_kind {
                vm::VmYield::Done => {
                    if self.pending_choices.is_empty() {
                        self.status = StoryStatus::Done;
                        return Ok(StepResult::Done { text: full_text });
                    }

                    // If all pending choices are invisible defaults (fallback
                    // choices), auto-select the first one and keep running.
                    let all_invisible = self
                        .pending_choices
                        .iter()
                        .all(|pc| pc.flags.is_invisible_default);

                    if all_invisible {
                        self.select_choice(0)?;
                        continue;
                    }

                    // Filter out invisible defaults — they shouldn't be
                    // presented to the player.
                    self.status = StoryStatus::WaitingForChoice;
                    let choices = self
                        .pending_choices
                        .iter()
                        .enumerate()
                        .filter(|(_, pc)| !pc.flags.is_invisible_default)
                        .map(|(i, pc)| Choice {
                            text: pc.display_text.clone(),
                            index: i,
                        })
                        .collect();
                    return Ok(StepResult::Choices {
                        text: full_text,
                        choices,
                    });
                }
                vm::VmYield::End => {
                    self.status = StoryStatus::Ended;
                    return Ok(StepResult::Ended { text: full_text });
                }
            }
        }
    }

    /// Select a choice by index. Call [`step`](Story::step) afterward to continue.
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        if self.status != StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }
        self.select_choice(index)
    }

    /// Internal: set execution position to the given choice target, clear
    /// pending choices, and set status to Active. No status precondition.
    fn select_choice(&mut self, index: usize) -> Result<(), RuntimeError> {
        let available = self.pending_choices.len();
        if index >= available {
            return Err(RuntimeError::InvalidChoiceIndex { index, available });
        }

        let choice = &self.pending_choices[index];
        let target_id = choice.target_id;
        let target_idx = choice.target_idx;
        let target_offset = choice.target_offset;

        // Increment visit count for the choice target container so that
        // once-only choices can be filtered on subsequent passes.
        *self.visit_counts.entry(target_id).or_insert(0) += 1;

        // Set execution position to the choice target.
        let frame = self
            .call_stack
            .last_mut()
            .ok_or(RuntimeError::CallStackUnderflow)?;

        if let Some(top) = frame.container_stack.last_mut() {
            top.container_idx = target_idx;
            top.offset = target_offset;
        } else {
            frame.container_stack.push(ContainerPosition {
                container_idx: target_idx,
                offset: target_offset,
            });
        }

        self.pending_choices.clear();
        self.status = StoryStatus::Active;

        Ok(())
    }

    /// Get the current execution status.
    pub fn status(&self) -> StoryStatus {
        self.status
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Pop a value from the value stack.
    pub(crate) fn pop_value(&mut self) -> Result<Value, RuntimeError> {
        self.value_stack.pop().ok_or(RuntimeError::StackUnderflow)
    }

    /// Peek at the top value without popping.
    pub(crate) fn peek_value(&self) -> Result<&Value, RuntimeError> {
        self.value_stack.last().ok_or(RuntimeError::StackUnderflow)
    }
}

#[cfg(test)]
#[expect(clippy::panic, clippy::needless_continue)]
mod tests {
    use super::*;
    use crate::link;

    fn load_i079() -> (crate::Program, Story) {
        let json_str = std::fs::read_to_string(
            "../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink.json",
        )
        .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    /// Step a story until it yields choices, panicking if it ends first.
    fn step_until_choices(story: &mut Story, program: &Program) -> Vec<Choice> {
        loop {
            match story.step(program).unwrap() {
                StepResult::Choices { choices, .. } => return choices,
                StepResult::Done { .. } => continue,
                StepResult::Ended { .. } => panic!("story ended before presenting choices"),
            }
        }
    }

    /// After selecting a once-only choice, the visit count for its target
    /// container must be > 0. Without this, the once-only filter in
    /// `handle_begin_choice` can never fire.
    #[test]
    fn select_choice_increments_visit_count_for_target() {
        let (program, mut story) = load_i079();
        let choices = step_until_choices(&mut story, &program);

        assert!(!choices.is_empty(), "expected at least one choice");

        // Record the target_id of the first pending choice BEFORE selecting.
        let target_id = story.pending_choices[0].target_id;
        let visit_before = story.visit_counts.get(&target_id).copied().unwrap_or(0);

        story.choose(0).unwrap();

        // After selection, the visit count for this target must have increased.
        let visit_after = story.visit_counts.get(&target_id).copied().unwrap_or(0);
        assert!(
            visit_after > visit_before,
            "visit count for choice target should increment after selection: \
             before={visit_before}, after={visit_after}"
        );
    }

    /// On the second pass through a choice set with once-only choices,
    /// a choice whose target has already been visited must NOT appear
    /// in `pending_choices`.
    #[test]
    fn once_only_choice_excluded_on_second_pass() {
        let (program, mut story) = load_i079();

        let first_choices = step_until_choices(&mut story, &program);
        assert!(
            first_choices
                .iter()
                .any(|c| c.text.contains("First choice")),
            "first pass should contain 'First choice', got: {first_choices:?}"
        );

        story.choose(0).unwrap();

        let second_choices = step_until_choices(&mut story, &program);
        assert!(
            !second_choices
                .iter()
                .any(|c| c.text.contains("First choice")),
            "second pass should NOT contain 'First choice' (once-only, already visited), \
             got: {second_choices:?}"
        );
    }
}
