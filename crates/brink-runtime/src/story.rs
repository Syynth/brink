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

/// Distinguishes call frame types for container-stack-empty semantics:
///
/// - **Root**: the initial frame. Yields for pending choices.
/// - **Function**: `f()` calls. Output is captured as a return value.
/// - **Tunnel**: `->t->` calls. Yields for pending choices (the tunnel
///   needs the player's choice before it can continue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallFrameType {
    Root,
    Function,
    Tunnel,
}

#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    pub return_address: Option<ContainerPosition>,
    pub temps: Vec<Value>,
    pub container_stack: Vec<ContainerPosition>,
    pub frame_type: CallFrameType,
}

/// A single execution thread with its own call stack.
#[derive(Debug, Clone)]
pub(crate) struct Thread {
    pub call_stack: Vec<CallFrame>,
    #[expect(dead_code)]
    pub thread_index: u32,
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
    /// Snapshot of the current thread at choice creation time, so that
    /// selecting this choice can restore the execution context
    /// (including temp variables from enclosing tunnels/functions).
    pub thread_fork: Thread,
}

/// Per-flow execution context. Owns threads, eval stack, output, choices.
pub(crate) struct Flow {
    pub threads: Vec<Thread>,
    pub thread_counter: u32,
    pub value_stack: Vec<Value>,
    pub output: OutputBuffer,
    pub pending_choices: Vec<PendingChoice>,
    pub turn_index: u32,
    pub current_tags: Vec<String>,
    pub in_tag: bool,
    pub skipping_choice: bool,
}

impl Flow {
    /// Returns a reference to the current (topmost) thread.
    ///
    /// # Panics
    ///
    /// Panics if the thread stack is empty. This is a programming error —
    /// flows are always constructed with at least one thread.
    #[expect(clippy::expect_used)]
    pub fn current_thread(&self) -> &Thread {
        self.threads
            .last()
            .expect("flow must always have at least one thread")
    }

    /// Returns a mutable reference to the current (topmost) thread.
    ///
    /// # Panics
    ///
    /// Panics if the thread stack is empty. This is a programming error —
    /// flows are always constructed with at least one thread.
    #[expect(clippy::expect_used)]
    pub fn current_thread_mut(&mut self) -> &mut Thread {
        self.threads
            .last_mut()
            .expect("flow must always have at least one thread")
    }

    pub fn can_pop_thread(&self) -> bool {
        self.threads.len() > 1
    }

    pub fn pop_thread(&mut self) {
        self.threads.pop();
    }

    pub fn push_thread(&mut self, initial_frame: CallFrame) {
        self.thread_counter += 1;
        self.threads.push(Thread {
            call_stack: vec![initial_frame],
            thread_index: self.thread_counter,
        });
    }

    pub fn fork_thread(&mut self) -> Thread {
        self.thread_counter += 1;
        Thread {
            call_stack: self.current_thread().call_stack.clone(),
            thread_index: self.thread_counter,
        }
    }

    /// Pop a value from the value stack.
    pub fn pop_value(&mut self) -> Result<Value, RuntimeError> {
        self.value_stack.pop().ok_or(RuntimeError::StackUnderflow)
    }

    /// Peek at the top value without popping.
    pub fn peek_value(&self) -> Result<&Value, RuntimeError> {
        self.value_stack.last().ok_or(RuntimeError::StackUnderflow)
    }
}

/// Per-instance mutable state for executing stories.
///
/// Created from a [`Program`] via [`Story::new`]. Holds all mutable state
/// (stacks, globals, output buffer) while the immutable program data lives
/// in [`Program`].
pub struct Story {
    pub(crate) flow: Flow,
    pub(crate) globals: Vec<Value>,
    pub(crate) visit_counts: HashMap<DefinitionId, u32>,
    pub(crate) status: StoryStatus,
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
            frame_type: CallFrameType::Root,
        };

        let initial_thread = Thread {
            call_stack: vec![initial_frame],
            thread_index: 0,
        };

        Self {
            flow: Flow {
                threads: vec![initial_thread],
                thread_counter: 0,
                value_stack: Vec::new(),
                output: OutputBuffer::new(),
                pending_choices: Vec::new(),
                turn_index: 0,
                current_tags: Vec::new(),
                in_tag: false,
                skipping_choice: false,
            },
            globals,
            visit_counts: HashMap::new(),
            status: StoryStatus::Active,
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

            let text = self.flow.output.flush();
            full_text.push_str(&text);
            self.flow.turn_index += 1;

            match yield_kind {
                vm::VmYield::Done => {
                    if self.flow.pending_choices.is_empty() {
                        self.status = StoryStatus::Done;
                        return Ok(StepResult::Done { text: full_text });
                    }

                    // If all pending choices are invisible defaults (fallback
                    // choices), auto-select the first one and keep running.
                    let all_invisible = self
                        .flow
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
                        .flow
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
        let available = self.flow.pending_choices.len();
        if index >= available {
            return Err(RuntimeError::InvalidChoiceIndex { index, available });
        }

        let choice = self.flow.pending_choices.swap_remove(index);

        // Increment visit count for the choice target container so that
        // once-only choices can be filtered on subsequent passes.
        *self.visit_counts.entry(choice.target_id).or_insert(0) += 1;

        // Replace the current thread with the fork from choice creation
        // time. By selection time, all spawned threads should have
        // completed — only the main thread remains.
        let current = self.flow.current_thread_mut();
        *current = choice.thread_fork;

        // Set execution position to the choice target. We reset the top
        // frame's container_stack to just the target — the snapshot may
        // have captured stale nesting from inside the choice eval block.
        let frame = current
            .call_stack
            .last_mut()
            .ok_or(RuntimeError::CallStackUnderflow)?;

        frame.container_stack.clear();
        frame.container_stack.push(ContainerPosition {
            container_idx: choice.target_idx,
            offset: choice.target_offset,
        });

        self.flow.pending_choices.clear();
        self.status = StoryStatus::Active;

        Ok(())
    }

    /// Get the current execution status.
    pub fn status(&self) -> StoryStatus {
        self.status
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
        let target_id = story.flow.pending_choices[0].target_id;
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

    // ── Choice thread forking ──────────────────────────────────────────

    fn load_i083() -> (crate::Program, Story) {
        let json_str = std::fs::read_to_string(
            "../../tests/tier1/choices/I083-choice-thread-forking/story.ink.json",
        )
        .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    /// When a choice is created inside a tunnel, the call stack at that
    /// moment (including the tunnel frame with its temps) must be captured.
    /// After the tunnel returns and the choice is presented, the snapshot
    /// should still reflect the tunnel-era call stack depth (>= 2 frames).
    #[test]
    fn pending_choice_captures_tunnel_call_stack() {
        let (program, mut story) = load_i083();
        let _choices = step_until_choices(&mut story, &program);

        // At this point the tunnel has returned, so the live call_stack
        // has only the root frame.
        let current_thread = story.flow.current_thread();
        assert_eq!(
            current_thread.call_stack.len(),
            1,
            "live call stack should be 1 frame (root) after tunnel return"
        );

        // But the pending choice's fork should have captured the
        // call stack from inside the tunnel (root + tunnel = 2 frames).
        assert!(!story.flow.pending_choices.is_empty());
        let fork = &story.flow.pending_choices[0].thread_fork;
        assert!(
            fork.call_stack.len() >= 2,
            "choice fork should have >= 2 frames (root + tunnel), got {}",
            fork.call_stack.len()
        );
    }

    /// After selecting a choice that was created inside a tunnel,
    /// `select_choice` must restore the tunnel's call frame so that
    /// temp variables from the tunnel scope are accessible.
    #[test]
    fn select_choice_restores_tunnel_frame_with_temps() {
        let (program, mut story) = load_i083();
        let _choices = step_until_choices(&mut story, &program);

        // Before choosing: only root frame, no tunnel temps.
        assert_eq!(story.flow.current_thread().call_stack.len(), 1);

        story.choose(0).unwrap();

        // After choosing: the tunnel frame should be restored.
        // The call stack should have at least 2 frames (root + tunnel).
        let call_stack = &story.flow.current_thread().call_stack;
        assert!(
            call_stack.len() >= 2,
            "call stack should be restored to tunnel depth after choice selection, \
             got {} frame(s)",
            call_stack.len()
        );

        // The tunnel frame (last frame) should have temp x = Int(1).
        let tunnel_frame = call_stack.last().unwrap();
        assert!(
            !tunnel_frame.temps.is_empty(),
            "tunnel frame should have temp variables"
        );
        assert_eq!(
            tunnel_frame.temps[0],
            Value::Int(1),
            "tunnel frame temps[0] should be Int(1) (the parameter x)"
        );
    }

    // ── Thread support ──────────────────────────────────────────────────

    fn load_i091() -> (crate::Program, Story) {
        let json_str =
            std::fs::read_to_string("../../tests/tier1/choices/I091-choice-count/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    /// `<- choices` (thread) must create choices AND return to the main
    /// flow so that `CHOICE_COUNT()` can evaluate. The thread body
    /// should be called like a tunnel — when its container stack empties,
    /// execution returns to the caller. Non-root frames must always pop
    /// back to their caller, even when pending choices exist.
    #[test]
    fn thread_call_returns_to_main_flow() {
        let (program, mut story) = load_i091();

        let result = story.step(&program).unwrap();

        // The story should yield Choices (not Done/Ended) because the
        // thread creates two choice points.
        assert!(
            matches!(result, StepResult::Choices { .. }),
            "expected Choices after thread creates choices, got {result:?}"
        );

        // The text output should include "2" (CHOICE_COUNT()) which
        // proves execution returned to the main flow after the thread.
        if let StepResult::Choices { text, choices } = result {
            assert!(
                text.contains('2'),
                "text should contain '2' from CHOICE_COUNT(), got: {text:?}"
            );
            assert_eq!(choices.len(), 2, "should have 2 choices (one/two)");
        }
    }
}
