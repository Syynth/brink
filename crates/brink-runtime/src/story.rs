//! Per-instance mutable story state.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use brink_format::{ChoiceFlags, DefinitionId, Value};

use crate::error::RuntimeError;
use crate::output::{OutputBuffer, clean_output_whitespace};
use crate::program::Program;
use crate::rng::{FastRng, StoryRng};
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
    ///
    /// `tags` is per-line: `tags[i]` holds the tags for the `i`-th line
    /// of `text` (split on `\n`).
    Done {
        text: String,
        tags: Vec<Vec<String>>,
    },
    /// Yielded text and choices; call [`choose`](Story::choose) then [`step`](Story::step).
    Choices {
        text: String,
        choices: Vec<Choice>,
        tags: Vec<Vec<String>>,
    },
    /// Story permanently ended.
    Ended {
        text: String,
        tags: Vec<Vec<String>>,
    },
}

/// A single choice presented to the player.
#[derive(Debug, Clone)]
pub struct Choice {
    pub text: String,
    pub index: usize,
    pub tags: Vec<String>,
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
/// - **Thread**: boundary frame pushed by `ThreadCall`. When this frame
///   exhausts, the thread is done — inherited frames below it are never
///   unwound into during normal execution. `->->` (`TunnelReturn`) strips
///   Thread frames to find the enclosing Tunnel.
/// - **External**: pushed by `CallExternal`. Holds popped arguments in
///   `temps` and the external function's [`DefinitionId`] in
///   `external_fn_id`. The orchestration layer resolves it (binding or
///   fallback) before the VM resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallFrameType {
    Root,
    Function,
    Tunnel,
    Thread,
    External,
}

#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    pub return_address: Option<ContainerPosition>,
    pub temps: Vec<Value>,
    pub container_stack: Vec<ContainerPosition>,
    pub frame_type: CallFrameType,
    /// For `External` frames: the `DefinitionId` of the external function,
    /// used to look up the fallback container if no binding is registered.
    pub external_fn_id: Option<DefinitionId>,
}

/// Two-part call stack: shared read-only prefix + owned mutable frames.
///
/// `fork_thread` snapshots the parent's frames into a cached `Rc<[CallFrame]>`
/// (one clone, amortized across all children). Children get `Rc::clone` — O(1).
/// The parent keeps its `own` vec unchanged and continues mutating freely.
#[derive(Debug, Clone)]
pub(crate) struct CallStack {
    /// Shared read-only prefix inherited from the parent thread.
    inherited: Option<Rc<[CallFrame]>>,
    /// Frames owned by this thread (above the fork point).
    own: Vec<CallFrame>,
    /// Cached snapshot so multiple forks from the same parent share one allocation.
    cached_snapshot: Option<Rc<[CallFrame]>>,
}

impl CallStack {
    pub fn new(frame: CallFrame) -> Self {
        Self {
            inherited: None,
            own: vec![frame],
            cached_snapshot: None,
        }
    }

    pub fn push(&mut self, frame: CallFrame) {
        self.cached_snapshot = None;
        self.own.push(frame);
    }

    pub fn pop(&mut self) -> Option<CallFrame> {
        self.cached_snapshot = None;
        if let Some(f) = self.own.pop() {
            return Some(f);
        }
        self.materialize();
        self.own.pop()
    }

    pub fn last(&self) -> Option<&CallFrame> {
        self.own
            .last()
            .or_else(|| self.inherited.as_ref().and_then(|h| h.last()))
    }

    pub fn last_mut(&mut self) -> Option<&mut CallFrame> {
        self.cached_snapshot = None;
        if !self.own.is_empty() {
            return self.own.last_mut();
        }
        self.materialize();
        self.own.last_mut()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inherited.as_ref().map_or(0, |h| h.len()) + self.own.len()
    }

    pub fn is_empty(&self) -> bool {
        self.own.is_empty() && self.inherited.as_ref().is_none_or(|h| h.is_empty())
    }

    /// Build an `Rc<[CallFrame]>` snapshot of the full stack (inherited + own).
    /// The result is cached so multiple forks from the same parent share one
    /// allocation.
    pub fn snapshot(&mut self) -> Rc<[CallFrame]> {
        if let Some(ref cached) = self.cached_snapshot {
            return Rc::clone(cached);
        }
        let rc = match &self.inherited {
            None => Rc::from(self.own.as_slice()),
            Some(prefix) if self.own.is_empty() => Rc::clone(prefix),
            Some(prefix) => {
                let mut combined = Vec::with_capacity(prefix.len() + self.own.len());
                combined.extend_from_slice(prefix);
                combined.extend_from_slice(&self.own);
                Rc::from(combined)
            }
        };
        self.cached_snapshot = Some(Rc::clone(&rc));
        rc
    }

    fn materialize(&mut self) {
        self.cached_snapshot = None;
        if let Some(prefix) = self.inherited.take() {
            let mut combined = Vec::with_capacity(prefix.len() + self.own.len());
            combined.extend_from_slice(&prefix);
            combined.append(&mut self.own);
            self.own = combined;
        }
    }
}

/// A single execution thread with its own call stack.
#[derive(Debug, Clone)]
pub(crate) struct Thread {
    pub call_stack: CallStack,
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
    /// Tags collected during choice evaluation.
    pub tags: Vec<String>,
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
    pub current_tags: Vec<String>,
    pub in_tag: bool,
    pub skipping_choice: bool,
}

/// Shared game state that lives above individual flows.
///
/// Holds globals, visit/turn tracking, and RNG state. This is the natural
/// serialization boundary for save/load (deferred).
pub(crate) struct Context<R: StoryRng = FastRng> {
    pub globals: Vec<Value>,
    pub visit_counts: HashMap<DefinitionId, u32>,
    pub turn_counts: HashMap<DefinitionId, u32>,
    pub turn_index: u32,
    pub rng_seed: i32,
    pub previous_random: i32,
    pub _rng: PhantomData<R>,
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

    pub fn fork_thread(&mut self) -> Thread {
        self.thread_counter += 1;
        let shared = self.current_thread_mut().call_stack.snapshot();
        Thread {
            call_stack: CallStack {
                inherited: Some(shared),
                own: Vec::new(),
                cached_snapshot: None,
            },
            thread_index: self.thread_counter,
        }
    }

    /// Read the arguments from the top External frame.
    pub fn external_args(&self) -> &[Value] {
        let frame = self.current_thread().call_stack.last();
        match frame {
            Some(f) if f.frame_type == CallFrameType::External => &f.temps,
            _ => &[],
        }
    }

    /// Read the external function's `DefinitionId` from the top External frame.
    pub fn external_fn_id(&self) -> Option<DefinitionId> {
        let frame = self.current_thread().call_stack.last()?;
        if frame.frame_type == CallFrameType::External {
            frame.external_fn_id
        } else {
            None
        }
    }

    /// Resolve an external call: pop the External frame and push the
    /// return value onto the value stack.
    pub fn resolve_external(&mut self, value: Value) {
        let thread = self.current_thread_mut();
        if let Some(frame) = thread.call_stack.last()
            && frame.frame_type == CallFrameType::External
        {
            let ret_addr = frame.return_address;
            thread.call_stack.pop();
            self.value_stack.push(value);
            // Restore position from return address (if any).
            if let Some(pos) = ret_addr
                && let Some(f) = self.current_thread_mut().call_stack.last_mut()
                && let Some(top) = f.container_stack.last_mut()
            {
                *top = pos;
            }
        }
    }

    /// Replace the External frame with a Function frame pointing at the
    /// fallback container. Args are pushed back onto the value stack so
    /// the fallback body's `temp=` opcodes can pop them.
    pub fn invoke_fallback(&mut self, container_idx: u32) {
        let thread = self.current_thread_mut();
        if let Some(frame) = thread.call_stack.last_mut()
            && frame.frame_type == CallFrameType::External
        {
            let args = core::mem::take(&mut frame.temps);
            frame.frame_type = CallFrameType::Function;
            frame.container_stack = vec![ContainerPosition {
                container_idx,
                offset: 0,
            }];
            frame.external_fn_id = None;
            // Push args back onto the value stack — the fallback body
            // starts with `temp=` instructions that pop them.
            self.value_stack.extend(args);
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

/// Result of an external function handler call.
pub enum ExternalResult {
    /// The handler resolved the call and returned a value.
    /// `Value::Null` is valid for fire-and-forget calls.
    Resolved(Value),
    /// The handler declined — use the ink fallback body if available.
    Fallback,
}

/// Trait for handling external function calls from ink.
///
/// Implement this to provide runtime-injected external function behavior.
/// The orchestration layer calls [`call`](ExternalFnHandler::call) when the
/// VM encounters a `CallExternal` opcode. The handler can resolve the call
/// immediately, decline to handle it (triggering fallback), or in the future,
/// indicate that resolution is pending (async/WASM).
pub trait ExternalFnHandler {
    /// Handle an external function call.
    ///
    /// `name` is the ink-declared function name. `args` are the values
    /// popped from the value stack, in declaration order.
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult;
}

/// Default handler that always falls back to the ink function body.
struct FallbackHandler;

impl ExternalFnHandler for FallbackHandler {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Fallback
    }
}

// ── FlowInstance ────────────────────────────────────────────────────────────

/// A paired (Flow, Context, Status) representing one independent execution
/// thread within a story. The default flow runs from the root container;
/// named flows can be spawned at arbitrary entry points.
pub(crate) struct FlowInstance<R: StoryRng = FastRng> {
    pub(crate) flow: Flow,
    pub(crate) context: Context<R>,
    pub(crate) status: StoryStatus,
}

/// Maximum opcodes per step to prevent infinite loops.
const MAX_OPS_PER_STEP: u32 = 100_000;

impl<R: StoryRng> FlowInstance<R> {
    /// Create a new flow instance starting at the root container.
    fn new_at_root(program: &Program) -> Self {
        let globals = program.global_defaults();
        let initial_frame = CallFrame {
            return_address: None,
            temps: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx: program.root_idx(),
                offset: 0,
            }],
            frame_type: CallFrameType::Root,
            external_fn_id: None,
        };
        let initial_thread = Thread {
            call_stack: CallStack::new(initial_frame),
            thread_index: 0,
        };
        Self {
            flow: Flow {
                threads: vec![initial_thread],
                thread_counter: 0,
                value_stack: Vec::new(),
                output: OutputBuffer::new(),
                pending_choices: Vec::new(),
                current_tags: Vec::new(),
                in_tag: false,
                skipping_choice: false,
            },
            context: Context {
                globals,
                visit_counts: HashMap::new(),
                turn_counts: HashMap::new(),
                turn_index: 0,
                rng_seed: 0,
                previous_random: 0,
                _rng: PhantomData,
            },
            status: StoryStatus::Active,
        }
    }

    /// Create a new flow instance starting at the given container.
    fn new_at(program: &Program, container_idx: u32) -> Self {
        let globals = program.global_defaults();
        let initial_frame = CallFrame {
            return_address: None,
            temps: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx,
                offset: 0,
            }],
            frame_type: CallFrameType::Root,
            external_fn_id: None,
        };
        let initial_thread = Thread {
            call_stack: CallStack::new(initial_frame),
            thread_index: 0,
        };
        Self {
            flow: Flow {
                threads: vec![initial_thread],
                thread_counter: 0,
                value_stack: Vec::new(),
                output: OutputBuffer::new(),
                pending_choices: Vec::new(),
                current_tags: Vec::new(),
                in_tag: false,
                skipping_choice: false,
            },
            context: Context {
                globals,
                visit_counts: HashMap::new(),
                turn_counts: HashMap::new(),
                turn_index: 0,
                rng_seed: 0,
                previous_random: 0,
                _rng: PhantomData,
            },
            status: StoryStatus::Active,
        }
    }

    /// Execute until the next yield point, using the given external
    /// function handler to resolve `CallExternal` opcodes.
    fn step_with(
        &mut self,
        program: &Program,
        handler: &dyn ExternalFnHandler,
    ) -> Result<StepResult, RuntimeError> {
        if self.status == StoryStatus::Ended {
            return Err(RuntimeError::StoryEnded);
        }

        // Reset status to Active if we were in Done (resuming after output).
        if self.status == StoryStatus::Done {
            self.status = StoryStatus::Active;
        }

        let mut all_lines: Vec<(String, Vec<String>)> = Vec::new();
        let mut op_count: u32 = 0;

        loop {
            op_count += 1;
            if op_count > MAX_OPS_PER_STEP {
                // Safety limit — treat as Done to avoid infinite loops.
                extend_lines(&mut all_lines, self.flow.output.flush_lines());
                self.context.turn_index += 1;
                self.status = StoryStatus::Done;
                let (text, tags) = finalize_lines(&all_lines);
                return Ok(StepResult::Done { text, tags });
            }

            match vm::step(&mut self.flow, &mut self.context, program)? {
                vm::Stepped::Continue | vm::Stepped::ThreadCompleted => {}

                vm::Stepped::ExternalCall => {
                    resolve_external_call(&mut self.flow, program, handler)?;
                }

                vm::Stepped::Done => {
                    extend_lines(&mut all_lines, self.flow.output.flush_lines());
                    self.context.turn_index += 1;

                    if self.flow.pending_choices.is_empty() {
                        self.status = StoryStatus::Done;
                        let (text, tags) = finalize_lines(&all_lines);
                        return Ok(StepResult::Done { text, tags });
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
                            tags: pc.tags.clone(),
                        })
                        .collect();
                    let (text, tags) = finalize_lines(&all_lines);
                    return Ok(StepResult::Choices {
                        text,
                        choices,
                        tags,
                    });
                }
                vm::Stepped::Ended => {
                    extend_lines(&mut all_lines, self.flow.output.flush_lines());
                    self.context.turn_index += 1;
                    self.status = StoryStatus::Ended;
                    let (text, tags) = finalize_lines(&all_lines);
                    return Ok(StepResult::Ended { text, tags });
                }
            }
        }
    }

    /// Select a choice by index. Call [`step_with`] afterward to continue.
    fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
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
        *self
            .context
            .visit_counts
            .entry(choice.target_id)
            .or_insert(0) += 1;
        self.context
            .turn_counts
            .insert(choice.target_id, self.context.turn_index);

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
}

/// Resolve an external function call using the handler and program metadata.
fn resolve_external_call(
    flow: &mut Flow,
    program: &Program,
    handler: &dyn ExternalFnHandler,
) -> Result<(), RuntimeError> {
    let fn_id = flow
        .external_fn_id()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    let entry = program.external_fn(fn_id);
    let fn_name = entry.map_or("?", |e| program.name(e.name));

    let result = handler.call(fn_name, flow.external_args());
    match result {
        ExternalResult::Resolved(value) => {
            flow.resolve_external(value);
        }
        ExternalResult::Fallback => {
            let fallback_id = entry.and_then(|e| e.fallback);
            if let Some(fb_id) = fallback_id {
                let container_idx = program
                    .resolve_container(fb_id)
                    .ok_or(RuntimeError::UnresolvedDefinition(fb_id))?;

                // Begin output capture — fallback is a function call whose
                // text output becomes the return value.
                flow.output.begin_capture();
                flow.invoke_fallback(container_idx);
            } else {
                return Err(RuntimeError::UnresolvedExternalCall(fn_id));
            }
        }
    }
    Ok(())
}

/// Extend accumulated lines with a new flush, stripping the trailing empty
/// entry from the previous flush to avoid double-newlines at flush boundaries.
///
/// `flush_lines` represents a trailing newline as `("", [])` — this is correct
/// when the lines are finalized alone, but when concatenating multiple flushes
/// (e.g., auto-selecting invisible default choices), the trailing empty entry
/// from one flush and the `\n` join from `finalize_lines` produce a double `\n`.
fn extend_lines(all: &mut Vec<(String, Vec<String>)>, new: Vec<(String, Vec<String>)>) {
    if !new.is_empty()
        && let Some(last) = all.last()
        && last.0.is_empty()
        && last.1.is_empty()
    {
        all.pop();
    }
    all.extend(new);
}

/// Build the final `text` string and per-line `tags` from structured lines.
fn finalize_lines(lines: &[(String, Vec<String>)]) -> (String, Vec<Vec<String>>) {
    let text = lines
        .iter()
        .map(|(t, _)| clean_output_whitespace(t))
        .collect::<Vec<_>>()
        .join("\n");
    let tags = lines.iter().map(|(_, t)| t.clone()).collect();
    (text, tags)
}

// ── Story ───────────────────────────────────────────────────────────────────

/// Per-instance mutable state for executing stories.
///
/// Created from a [`Program`] via [`Story::new`]. Holds all mutable state
/// (stacks, globals, output buffer) while the immutable program data lives
/// in [`Program`].
///
/// Generic over `R: StoryRng` — defaults to [`FastRng`]. Use
/// [`DotNetRng`](crate::DotNetRng) for .NET-compatible deterministic output.
pub struct Story<R: StoryRng = FastRng> {
    pub(crate) default: FlowInstance<R>,
    instances: HashMap<String, FlowInstance<R>>,
}

impl<R: StoryRng> Story<R> {
    /// Create a new story instance from a linked program.
    pub fn new(program: &Program) -> Self {
        Self {
            default: FlowInstance::new_at_root(program),
            instances: HashMap::new(),
        }
    }

    // ── Default flow API (unchanged public interface) ───────────────

    /// Execute until the next yield point (done, choices, or end).
    ///
    /// External functions use fallback ink bodies when available,
    /// or error if no fallback exists. Use [`step_with`](Self::step_with)
    /// to provide a custom external function handler.
    pub fn step(&mut self, program: &Program) -> Result<StepResult, RuntimeError> {
        self.default.step_with(program, &FallbackHandler)
    }

    /// Execute until the next yield point, using the given external
    /// function handler to resolve `CallExternal` opcodes.
    pub fn step_with(
        &mut self,
        program: &Program,
        handler: &dyn ExternalFnHandler,
    ) -> Result<StepResult, RuntimeError> {
        self.default.step_with(program, handler)
    }

    /// Select a choice by index. Call [`step`](Story::step) afterward to continue.
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        self.default.choose(index)
    }

    /// Get the current execution status.
    pub fn status(&self) -> StoryStatus {
        self.default.status
    }

    // ── Named flow API ──────────────────────────────────────────────

    /// Spawn a new flow instance starting at the given entry point.
    ///
    /// `entry_point` is the `DefinitionId` of the target container
    /// (e.g., a knot). Each flow instance gets its own globals, visit
    /// counts, and execution state.
    pub fn spawn_flow(
        &mut self,
        name: &str,
        entry_point: DefinitionId,
        program: &Program,
    ) -> Result<(), RuntimeError> {
        if self.instances.contains_key(name) {
            return Err(RuntimeError::FlowAlreadyExists(name.to_owned()));
        }
        let container_idx = program
            .resolve_container(entry_point)
            .ok_or(RuntimeError::UnresolvedDefinition(entry_point))?;
        self.instances.insert(
            name.to_owned(),
            FlowInstance::new_at(program, container_idx),
        );
        Ok(())
    }

    /// Step a named flow instance.
    pub fn step_flow(&mut self, name: &str, program: &Program) -> Result<StepResult, RuntimeError> {
        self.step_flow_with(name, program, &FallbackHandler)
    }

    /// Step a named flow instance with an external function handler.
    pub fn step_flow_with(
        &mut self,
        name: &str,
        program: &Program,
        handler: &dyn ExternalFnHandler,
    ) -> Result<StepResult, RuntimeError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        instance.step_with(program, handler)
    }

    /// Select a choice in a named flow.
    pub fn choose_flow(&mut self, name: &str, index: usize) -> Result<(), RuntimeError> {
        let instance = self
            .instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        instance.choose(index)
    }

    /// Destroy a named flow instance.
    pub fn destroy_flow(&mut self, name: &str) -> Result<(), RuntimeError> {
        if self.instances.remove(name).is_none() {
            return Err(RuntimeError::UnknownFlow(name.to_owned()));
        }
        Ok(())
    }

    /// Query the status of a named flow.
    pub fn flow_status(&self, name: &str) -> Option<StoryStatus> {
        self.instances.get(name).map(|i| i.status)
    }

    /// List active flow names.
    pub fn flow_names(&self) -> Vec<&str> {
        self.instances.keys().map(String::as_str).collect()
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
        let target_id = story.default.flow.pending_choices[0].target_id;
        let visit_before = story
            .default
            .context
            .visit_counts
            .get(&target_id)
            .copied()
            .unwrap_or(0);

        story.choose(0).unwrap();

        // After selection, the visit count for this target must have increased.
        let visit_after = story
            .default
            .context
            .visit_counts
            .get(&target_id)
            .copied()
            .unwrap_or(0);
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
        let current_thread = story.default.flow.current_thread();
        assert_eq!(
            current_thread.call_stack.len(),
            1,
            "live call stack should be 1 frame (root) after tunnel return"
        );

        // But the pending choice's fork should have captured the
        // call stack from inside the tunnel (root + tunnel = 2 frames).
        assert!(!story.default.flow.pending_choices.is_empty());
        let fork = &story.default.flow.pending_choices[0].thread_fork;
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
        assert_eq!(story.default.flow.current_thread().call_stack.len(), 1);

        story.choose(0).unwrap();

        // After choosing: the tunnel frame should be restored.
        // The call stack should have at least 2 frames (root + tunnel).
        let call_stack = &story.default.flow.current_thread().call_stack;
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

    // ── Tags ──────────────────────────────────────────────────────────

    fn load_tags() -> (crate::Program, Story) {
        let json_str =
            std::fs::read_to_string("../../tests/tier3/tags/tags/story.ink.json").unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    fn load_tags_in_choice() -> (crate::Program, Story) {
        let json_str =
            std::fs::read_to_string("../../tests/tier3/tags/tagsInChoice/story.ink.json").unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    #[test]
    fn step_result_exposes_tags() {
        let (program, mut story) = load_tags();
        let result = story.step(&program).unwrap();
        match result {
            StepResult::Done { tags, .. } | StepResult::Ended { tags, .. } => {
                // Tags are per-line; the first line should have both tags.
                assert_eq!(tags[0], vec!["author: Joe", "title: My Great Story"]);
            }
            other @ StepResult::Choices { .. } => panic!("expected Done or Ended, got {other:?}"),
        }
    }

    #[test]
    fn choice_exposes_tags() {
        let (program, mut story) = load_tags_in_choice();
        let choices = step_until_choices(&mut story, &program);
        assert!(!choices.is_empty());
        // The choice in tagsInChoice has tags "one" and "two"
        assert!(
            !choices[0].tags.is_empty(),
            "choice should have tags, got: {choices:?}"
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
        if let StepResult::Choices { text, choices, .. } = result {
            assert!(
                text.contains('2'),
                "text should contain '2' from CHOICE_COUNT(), got: {text:?}"
            );
            assert_eq!(choices.len(), 2, "should have 2 choices (one/two)");
        }
    }

    // ── External functions ──────────────────────────────────────────

    fn load_external_0_arg() -> (crate::Program, Story) {
        let json_str = std::fs::read_to_string(
            "../../tests/tier3/runtime/external-function-0-arg/story.ink.json",
        )
        .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    fn load_external_binding() -> (crate::Program, Story) {
        let json_str =
            std::fs::read_to_string("../../tests/tier3/bindings/external-binding/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    /// When an external function has a fallback body and no binding is
    /// registered, the VM should invoke the fallback without error.
    #[test]
    fn external_function_uses_fallback_when_no_binding() {
        let (program, mut story) = load_external_0_arg();
        // The fallback returns "" so the output should be "The value is ."
        let result = story.step(&program);
        assert!(
            result.is_ok(),
            "external function with fallback should not error, got: {result:?}"
        );
        match result.unwrap() {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                assert!(
                    text.contains("The value is"),
                    "expected 'The value is' in output, got: {text:?}"
                );
            }
            StepResult::Choices { .. } => panic!("expected Done or Ended, got Choices"),
        }
    }

    /// When an external function handler is provided via `step_with`,
    /// it should be called instead of the fallback.
    #[test]
    fn external_function_binding_is_called() {
        use super::ExternalResult;

        struct TestHandler;
        impl super::ExternalFnHandler for TestHandler {
            #[expect(clippy::cast_precision_loss, clippy::cast_sign_loss)]
            fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
                match name {
                    "multiply" => {
                        let a = match args[0] {
                            Value::Float(f) => f,
                            Value::Int(i) => i as f32,
                            _ => 0.0,
                        };
                        let b = match args[1] {
                            Value::Float(f) => f,
                            Value::Int(i) => i as f32,
                            _ => 0.0,
                        };
                        #[expect(clippy::cast_possible_truncation)]
                        ExternalResult::Resolved(Value::Int((a * b) as i32))
                    }
                    "message" => ExternalResult::Resolved(Value::Null),
                    "times" => {
                        let count = match args[0] {
                            Value::Int(i) => i as usize,
                            _ => 0,
                        };
                        let s = match &args[1] {
                            Value::String(s) => &**s,
                            _ => "",
                        };
                        ExternalResult::Resolved(Value::String(s.repeat(count).into()))
                    }
                    _ => ExternalResult::Fallback,
                }
            }
        }

        let (program, mut story) = load_external_binding();
        let handler = TestHandler;

        let result = story.step_with(&program, &handler);
        assert!(
            result.is_ok(),
            "bound external functions should not error, got: {result:?}"
        );
        match result.unwrap() {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                assert!(
                    text.contains("15"),
                    "expected '15' from multiply(5.0, 3), got: {text:?}"
                );
                assert!(
                    text.contains("knock knock knock"),
                    "expected 'knock knock knock' from times(3, 'knock '), got: {text:?}"
                );
            }
            StepResult::Choices { .. } => panic!("expected Done or Ended, got Choices"),
        }
    }

    // ── Multi-thread tunnel return ──────────────────────────────────

    fn load_multi_thread() -> (crate::Program, Story) {
        let json_str =
            std::fs::read_to_string("../../tests/tier2/threads/multi-thread/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let story = Story::new(&program);
        (program, story)
    }

    /// When threads are spawned inside a tunnel (`<- place1` inside
    /// `->t->`), selecting a choice created by a thread must return
    /// to the tunnel caller after `->->`. The thread fork must
    /// preserve the enclosing tunnel frame so that `TunnelReturn` can
    /// resume at `start` where "The end" text lives.
    #[test]
    fn multi_thread_tunnel_return_outputs_the_end() {
        let (program, mut story) = load_multi_thread();

        // Step to choices — should present choices from place1 and place2.
        let choices = step_until_choices(&mut story, &program);
        assert_eq!(choices.len(), 2, "expected 2 choices from place1 + place2");

        // Select the first choice (choice in place 1).
        story.choose(0).unwrap();

        // Step again — should output "The end" from the tunnel caller.
        let mut found_the_end = false;
        loop {
            match story.step(&program).unwrap() {
                StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                    if text.contains("The end") {
                        found_the_end = true;
                    }
                    if matches!(story.status(), StoryStatus::Done | StoryStatus::Ended) {
                        break;
                    }
                }
                StepResult::Choices { .. } => {
                    panic!("unexpected choices after selecting first choice");
                }
            }
        }

        assert!(
            found_the_end,
            "after selecting a choice created in a thread inside a tunnel, \
             the story should output 'The end' from the tunnel caller"
        );
    }

    // ── Named flow instances ────────────────────────────────────────

    /// Spawn a named flow at a known knot, step it, verify output.
    #[test]
    fn spawn_and_step_named_flow() {
        // Load a simple story — I001 is a minimal "Hello, world!" story.
        let json_str =
            std::fs::read_to_string("../../tests/tier1/basics/I001-minimal-story/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let mut story: Story = Story::new(&program);

        // Spawn a named flow at the root container (same entry point).
        let root_id = program.containers[program.root_idx() as usize].id;
        story.spawn_flow("side", root_id, &program).unwrap();

        // Step the named flow — should produce the same output as the default.
        let result = story.step_flow("side", &program).unwrap();
        match result {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                assert!(
                    text.contains("Hello"),
                    "named flow should produce output, got: {text:?}"
                );
            }
            StepResult::Choices { .. } => panic!("expected Done or Ended, got Choices"),
        }
    }

    /// Verify the existing API still works identically after restructure.
    #[test]
    fn default_flow_unchanged() {
        let json_str =
            std::fs::read_to_string("../../tests/tier1/basics/I001-minimal-story/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let mut story: Story = Story::new(&program);

        let result = story.step(&program).unwrap();
        match result {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                assert!(
                    text.contains("Hello"),
                    "default flow should produce output, got: {text:?}"
                );
            }
            StepResult::Choices { .. } => panic!("expected Done or Ended, got Choices"),
        }
    }

    /// Spawn, destroy, verify gone.
    #[test]
    fn destroy_flow_removes_instance() {
        let json_str =
            std::fs::read_to_string("../../tests/tier1/basics/I001-minimal-story/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let mut story: Story = Story::new(&program);

        let root_id = program.containers[program.root_idx() as usize].id;
        story.spawn_flow("temp", root_id, &program).unwrap();
        assert!(story.flow_status("temp").is_some());
        assert!(story.flow_names().contains(&"temp"));

        story.destroy_flow("temp").unwrap();
        assert!(story.flow_status("temp").is_none());
        assert!(!story.flow_names().contains(&"temp"));

        // Destroying again should error.
        let err = story.destroy_flow("temp");
        assert!(err.is_err());
    }

    /// Spawn two flows at the same entry point, step them independently.
    #[test]
    fn independent_state() {
        let json_str =
            std::fs::read_to_string("../../tests/tier1/basics/I001-minimal-story/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let mut story: Story = Story::new(&program);

        let root_id = program.containers[program.root_idx() as usize].id;
        story.spawn_flow("a", root_id, &program).unwrap();
        story.spawn_flow("b", root_id, &program).unwrap();

        // Step "a" only — "b" should still be Active.
        let _ = story.step_flow("a", &program).unwrap();
        assert_eq!(story.flow_status("b"), Some(StoryStatus::Active));

        // Step "b" — should also produce output independently.
        let result_b = story.step_flow("b", &program).unwrap();
        match result_b {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                assert!(
                    text.contains("Hello"),
                    "flow 'b' should produce output independently, got: {text:?}"
                );
            }
            StepResult::Choices { .. } => panic!("expected Done or Ended, got Choices"),
        }

        // Duplicate name should error.
        let err = story.spawn_flow("a", root_id, &program);
        assert!(err.is_err());
    }

    // ── CallStack shared-prefix optimization ───────────────────────

    #[test]
    fn fork_thread_shares_snapshot_rc() {
        let json_str =
            std::fs::read_to_string("../../tests/tier1/choices/I091-choice-count/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        let program = link(&data).unwrap();
        let mut story: Story = Story::new(&program);

        let parent_len = story.default.flow.current_thread().call_stack.len();
        assert!(
            parent_len >= 1,
            "main thread should have at least one frame"
        );

        // Fork — children share the cached snapshot Rc.
        let child = story.default.flow.fork_thread();
        let child2 = story.default.flow.fork_thread();

        assert_eq!(child.call_stack.len(), parent_len);
        assert_eq!(child2.call_stack.len(), parent_len);

        // Both children reference the same inherited Rc.
        let rc1 = child.call_stack.inherited.as_ref().unwrap();
        let rc2 = child2.call_stack.inherited.as_ref().unwrap();
        assert!(
            Rc::ptr_eq(rc1, rc2),
            "children should share the same snapshot Rc"
        );
    }
}
