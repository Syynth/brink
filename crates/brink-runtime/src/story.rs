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
}

#[derive(Debug, Clone)]
pub(crate) struct PendingChoice {
    pub display_text: String,
    pub target_idx: u32,
    pub target_offset: usize,
    #[expect(dead_code)]
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
    pub(crate) string_eval_stack: Vec<OutputBuffer>,
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
            string_eval_stack: Vec::new(),
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

        let yield_kind = vm::run(self, program)?;

        let text = self.output.flush();
        self.turn_index += 1;

        match yield_kind {
            vm::VmYield::Done => {
                if self.status == StoryStatus::WaitingForChoice {
                    let choices = self
                        .pending_choices
                        .iter()
                        .enumerate()
                        .map(|(i, pc)| Choice {
                            text: pc.display_text.clone(),
                            index: i,
                        })
                        .collect();
                    Ok(StepResult::Choices { text, choices })
                } else {
                    self.status = StoryStatus::Done;
                    Ok(StepResult::Done { text })
                }
            }
            vm::VmYield::End => {
                self.status = StoryStatus::Ended;
                Ok(StepResult::Ended { text })
            }
        }
    }

    /// Select a choice by index. Call [`step`](Story::step) afterward to continue.
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        if self.status != StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }

        let available = self.pending_choices.len();
        if index >= available {
            return Err(RuntimeError::InvalidChoiceIndex { index, available });
        }

        let choice = &self.pending_choices[index];
        let target_idx = choice.target_idx;
        let target_offset = choice.target_offset;

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
