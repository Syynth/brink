//! Per-instance mutable story state.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use brink_format::{ChoiceFlags, DefinitionId, PluralResolver, Value};

use crate::error::RuntimeError;
use crate::output::OutputBuffer;
use crate::program::Program;
use crate::rng::{FastRng, StoryRng};
use crate::state::{ContextAccess, WriteObserver};
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

/// A single step of story output from [`Story::continue_single`].
///
/// The enum tells the caller what to do next:
/// - `Text` — more output may follow, keep calling `continue_single`.
/// - `Done` — this turn's output is complete. Call `continue_single`
///   again for the next turn (the story isn't over).
/// - `Choices` — pick a choice via [`Story::choose`], then resume.
/// - `End` — the story has permanently ended.
#[derive(Debug, Clone)]
pub enum Line {
    /// One line of story content. More may follow — keep calling
    /// [`Story::continue_single`].
    Text { text: String, tags: Vec<String> },
    /// This turn's output is complete (ink `-> DONE`). The story isn't
    /// over — call [`Story::continue_single`] again for more.
    Done { text: String, tags: Vec<String> },
    /// The story is presenting choices. Call [`Story::choose`] then
    /// resume with [`Story::continue_single`].
    Choices {
        text: String,
        tags: Vec<String>,
        choices: Vec<Choice>,
    },
    /// The story has permanently ended (ink `-> END`).
    End { text: String, tags: Vec<String> },
}

impl Line {
    /// The text content of this line, regardless of variant.
    pub fn text(&self) -> &str {
        match self {
            Self::Text { text, .. }
            | Self::Done { text, .. }
            | Self::Choices { text, .. }
            | Self::End { text, .. } => text,
        }
    }

    /// The tags associated with this line, regardless of variant.
    pub fn tags(&self) -> &[String] {
        match self {
            Self::Text { tags, .. }
            | Self::Done { tags, .. }
            | Self::Choices { tags, .. }
            | Self::End { tags, .. } => tags,
        }
    }

    /// Returns true if this is a terminal variant (`Done`, `Choices`, or `End`).
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Text { .. })
    }
}

/// A single choice presented to the player.
#[derive(Debug, Clone)]
pub struct Choice {
    pub text: String,
    pub index: usize,
    pub tags: Vec<String>,
}

// ── Stats ───────────────────────────────────────────────────────────────────

/// Lightweight counters tracking VM activity over a story's lifetime.
///
/// Always-on — incrementing a `u64` is effectively free compared to opcode
/// dispatch. Use [`Story::stats`] to read after a run.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// Total opcodes dispatched.
    pub opcodes: u64,
    /// Total `vm::step` calls from the outer loop.
    pub steps: u64,
    /// Threads forked (via `ThreadCall` and choice creation).
    pub threads_created: u64,
    /// Threads that completed and were popped.
    pub threads_completed: u64,
    /// Call frames pushed onto thread stacks.
    pub frames_pushed: u64,
    /// Call frames popped from thread stacks.
    pub frames_popped: u64,
    /// Choice sets presented to the player.
    pub choices_presented: u64,
    /// Individual choices selected.
    pub choices_selected: u64,
    /// `CallStack::snapshot` cache hits (reused existing `Rc`).
    pub snapshot_cache_hits: u64,
    /// `CallStack::snapshot` cache misses (new allocation).
    pub snapshot_cache_misses: u64,
    /// `CallStack::materialize` calls (flattened inherited prefix).
    pub materializations: u64,
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
    /// Count of materializations (flattening inherited prefix into own).
    pub(crate) materialization_count: u64,
}

impl CallStack {
    pub fn new(frame: CallFrame) -> Self {
        Self {
            inherited: None,
            own: vec![frame],
            cached_snapshot: None,
            materialization_count: 0,
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
        if !self.own.is_empty() {
            return self.own.last_mut();
        }
        self.materialize();
        self.own.last_mut()
    }

    pub fn len(&self) -> usize {
        self.inherited.as_ref().map_or(0, |h| h.len()) + self.own.len()
    }

    pub fn is_empty(&self) -> bool {
        self.own.is_empty() && self.inherited.as_ref().is_none_or(|h| h.is_empty())
    }

    /// Get a frame by absolute index (0 = bottom of stack).
    pub fn get(&self, index: usize) -> Option<&CallFrame> {
        let inherited_len = self.inherited.as_ref().map_or(0, |h| h.len());
        if index < inherited_len {
            self.inherited.as_ref().and_then(|h| h.get(index))
        } else {
            self.own.get(index - inherited_len)
        }
    }

    /// Get a mutable reference to a frame by absolute index.
    /// Materializes the inherited prefix if the target is in it.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut CallFrame> {
        let inherited_len = self.inherited.as_ref().map_or(0, |h| h.len());
        if index < inherited_len {
            self.materialize();
            self.own.get_mut(index)
        } else {
            self.own.get_mut(index - inherited_len)
        }
    }

    /// Build an `Rc<[CallFrame]>` snapshot of the full stack (inherited + own).
    /// The result is cached so multiple forks from the same parent share one
    /// allocation. Returns `(snapshot, cache_hit)`.
    pub fn snapshot(&mut self) -> (Rc<[CallFrame]>, bool) {
        if let Some(ref cached) = self.cached_snapshot {
            return (Rc::clone(cached), true);
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
        (rc, false)
    }

    /// Flatten inherited prefix into `own`. Returns `true` if work was done.
    fn materialize(&mut self) -> bool {
        self.cached_snapshot = None;
        if let Some(prefix) = self.inherited.take() {
            let mut combined = Vec::with_capacity(prefix.len() + self.own.len());
            combined.extend_from_slice(&prefix);
            combined.append(&mut self.own);
            self.own = combined;
            self.materialization_count += 1;
            true
        } else {
            false
        }
    }
}

/// A single execution thread with its own call stack.
#[derive(Debug, Clone)]
pub(crate) struct Thread {
    pub call_stack: CallStack,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingChoice {
    pub display_text: String,
    pub target_id: DefinitionId,
    pub target_idx: u32,
    pub target_offset: usize,
    pub flags: ChoiceFlags,
    #[expect(
        dead_code,
        reason = "needs research — likely needed for structured output / voice acting"
    )]
    pub original_index: usize,
    /// Tags collected during choice evaluation.
    pub tags: Vec<String>,
    /// Snapshot of the current thread at choice creation time, so that
    /// selecting this choice can restore the execution context
    /// (including temp variables from enclosing tunnels/functions).
    pub thread_fork: Thread,
}

/// Per-flow execution context. Owns threads, eval stack, output, choices.
#[derive(Clone)]
pub(crate) struct Flow {
    pub threads: Vec<Thread>,
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
#[derive(Clone)]
pub(crate) struct Context {
    pub globals: Vec<Value>,
    pub visit_counts: HashMap<DefinitionId, u32>,
    pub turn_counts: HashMap<DefinitionId, u32>,
    pub turn_index: u32,
    pub rng_seed: i32,
    pub previous_random: i32,
}

impl Context {
    pub fn global(&self, idx: u32) -> &Value {
        &self.globals[idx as usize]
    }

    pub fn set_global(&mut self, idx: u32, value: Value) {
        self.globals[idx as usize] = value;
    }

    pub fn visit_count(&self, id: DefinitionId) -> u32 {
        self.visit_counts.get(&id).copied().unwrap_or(0)
    }

    pub fn increment_visit(&mut self, id: DefinitionId) {
        *self.visit_counts.entry(id).or_insert(0) += 1;
    }

    pub fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.turn_counts.get(&id).copied()
    }

    pub fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.turn_counts.insert(id, turn);
    }

    pub fn turn_index(&self) -> u32 {
        self.turn_index
    }

    pub fn increment_turn_index(&mut self) {
        self.turn_index += 1;
    }

    pub fn rng_seed(&self) -> i32 {
        self.rng_seed
    }

    pub fn set_rng_seed(&mut self, seed: i32) {
        self.rng_seed = seed;
    }

    pub fn previous_random(&self) -> i32 {
        self.previous_random
    }

    pub fn set_previous_random(&mut self, val: i32) {
        self.previous_random = val;
    }

    pub fn next_random<R: StoryRng>(seed: i32) -> i32 {
        let mut rng = R::from_seed(seed);
        rng.next_int()
    }

    pub fn random_sequence<R: StoryRng>(seed: i32, count: usize) -> Vec<i32> {
        let mut rng = R::from_seed(seed);
        (0..count).map(|_| rng.next_int()).collect()
    }
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

    /// Fork a new thread from the current one. Returns `(thread, snapshot_cache_hit)`.
    pub fn fork_thread(&mut self) -> (Thread, bool) {
        let (shared, cache_hit) = self.current_thread_mut().call_stack.snapshot();
        (
            Thread {
                call_stack: CallStack {
                    inherited: Some(shared),
                    own: Vec::new(),
                    cached_snapshot: None,
                    materialization_count: 0,
                },
            },
            cache_hit,
        )
    }

    /// Drain materialization counts from all thread call stacks.
    pub fn drain_materializations(&mut self) -> u64 {
        let mut total = 0;
        for thread in &mut self.threads {
            total += thread.call_stack.materialization_count;
            thread.call_stack.materialization_count = 0;
        }
        total
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
    /// The handler cannot resolve the call yet (async resolution).
    /// The VM freezes with the `External` frame intact. The caller must
    /// resolve via `story.resolve_external(value)` before continuing.
    Pending,
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
#[derive(Clone)]
pub(crate) struct FlowInstance {
    pub(crate) flow: Flow,
    pub(crate) status: StoryStatus,
    pub(crate) stats: Stats,
}

impl FlowInstance {
    /// Create a new flow instance starting at the root container.
    fn new_at_root(program: &Program) -> (Self, Context) {
        Self::new_at(program, program.root_idx())
    }

    fn new_at(program: &Program, container_idx: u32) -> (Self, Context) {
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
        };
        let flow_instance = Self {
            flow: Flow {
                threads: vec![initial_thread],
                value_stack: Vec::new(),
                output: OutputBuffer::new(),
                pending_choices: Vec::new(),
                current_tags: Vec::new(),
                in_tag: false,
                skipping_choice: false,
            },
            status: StoryStatus::Active,
            stats: Stats::default(),
        };
        let context = Context {
            globals,
            visit_counts: HashMap::new(),
            turn_counts: HashMap::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
        };
        (flow_instance, context)
    }

    /// Maximum VM steps per `continue_maximally` call before erroring.
    /// Prevents infinite loops from malformed bytecode.
    const STEP_LIMIT: u64 = 1_000_000;

    /// Execute until one complete line of output is available, or until
    /// a yield point (choices/done/ended) if no newline occurs first.
    ///
    /// Returns a [`Line`] variant that tells the caller what happened:
    /// - `Line::Text` — mid-stream content, more may follow.
    /// - `Line::Done` — this turn is complete, call again for more.
    /// - `Line::Choices` — the story needs a choice selection.
    /// - `Line::End` — the story has permanently ended.
    fn step_single_line<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<Line, RuntimeError> {
        // 1. If buffer already has a completed line from a previous step,
        //    take it immediately (no VM stepping needed).
        if self.flow.output.has_completed_line()
            && let Some((text, tags)) =
                self.flow
                    .output
                    .take_first_line(program, line_tables, resolver)
        {
            return Ok(Line::Text { text, tags });
        }

        // 2. If buffer has partial content but VM has already yielded
        //    (any non-Active state), flush it. At a yield point, no more
        //    output is coming, so trailing Newlines are committed.
        if !self.flow.output.parts.is_empty() && self.status != StoryStatus::Active {
            let (text, tags) = flush_remaining(&mut self.flow, program, line_tables, resolver);
            return Ok(make_yield_line(self.status, text, tags, &self.flow));
        }

        // 3. Status checks.
        if self.status == StoryStatus::Ended {
            return Err(RuntimeError::StoryEnded);
        }
        if self.status == StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }

        // 4. Reset Done → Active (resuming after output).
        if self.status == StoryStatus::Done {
            self.status = StoryStatus::Active;
        }

        // 5. Step VM loop.
        let Self {
            flow,
            status,
            stats,
            ..
        } = self;
        let step_start = stats.steps;

        loop {
            stats.steps += 1;

            if stats.steps - step_start > Self::STEP_LIMIT {
                return Err(RuntimeError::StepLimitExceeded(Self::STEP_LIMIT));
            }

            let stepped = vm::step::<R>(flow, program, line_tables, context, stats, resolver)?;
            stats.materializations += flow.drain_materializations();

            match stepped {
                vm::Stepped::Continue | vm::Stepped::ThreadCompleted => {
                    if flow.output.has_completed_line()
                        && let Some((text, tags)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(Line::Text { text, tags });
                    }
                }

                vm::Stepped::ExternalCall => {
                    resolve_external_call(flow, program, handler)?;
                    if flow.output.has_completed_line()
                        && let Some((text, tags)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(Line::Text { text, tags });
                    }
                }

                vm::Stepped::Done => {
                    context.increment_turn_index();

                    // Handle invisible default choices: auto-select and keep running.
                    if !flow.pending_choices.is_empty() {
                        let all_invisible = flow
                            .pending_choices
                            .iter()
                            .all(|pc| pc.flags.is_invisible_default);
                        if all_invisible {
                            select_choice(flow, context, status, stats, 0)?;
                            if flow.output.has_completed_line()
                                && let Some((text, tags)) =
                                    flow.output.take_first_line(program, line_tables, resolver)
                            {
                                return Ok(Line::Text { text, tags });
                            }
                            continue;
                        }
                    }

                    // Set status based on remaining choices.
                    if flow.pending_choices.is_empty() {
                        *status = StoryStatus::Done;
                    } else {
                        *status = StoryStatus::WaitingForChoice;
                        stats.choices_presented += 1;
                    }

                    if flow.output.has_completed_line()
                        && let Some((text, tags)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(Line::Text { text, tags });
                    }

                    let (text, tags) = flush_remaining(flow, program, line_tables, resolver);
                    return Ok(make_yield_line(*status, text, tags, flow));
                }

                vm::Stepped::Ended => {
                    context.increment_turn_index();
                    *status = StoryStatus::Ended;

                    if flow.output.has_completed_line()
                        && let Some((text, tags)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(Line::Text { text, tags });
                    }

                    let (text, tags) = flush_remaining(flow, program, line_tables, resolver);
                    return Ok(Line::End { text, tags });
                }
            }
        }
    }

    /// Select a choice by index. Call [`step_with`] afterward to continue.
    fn choose(
        &mut self,
        context: &mut (impl ContextAccess + ?Sized),
        index: usize,
    ) -> Result<(), RuntimeError> {
        if self.status != StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }
        select_choice(
            &mut self.flow,
            context,
            &mut self.status,
            &mut self.stats,
            index,
        )
    }
}

/// Internal: set execution position to the given choice target, clear
/// pending choices, and set status to Active. No status precondition.
#[expect(clippy::similar_names)]
/// Returns the `DefinitionId` of the selected choice target, so the
/// caller can notify observers if needed.
fn select_choice(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
    status: &mut StoryStatus,
    stats: &mut Stats,
    index: usize,
) -> Result<(), RuntimeError> {
    let available = flow.pending_choices.len();
    if index >= available {
        return Err(RuntimeError::InvalidChoiceIndex { index, available });
    }

    let choice = flow.pending_choices.swap_remove(index);
    let target_id = choice.target_id;

    // Increment visit count for the choice target container so that
    // once-only choices can be filtered on subsequent passes.
    context.increment_visit(target_id);
    context.set_turn_count(target_id, context.turn_index());

    // Replace the current thread with the fork from choice creation
    // time. By selection time, all spawned threads should have
    // completed — only the main thread remains.
    let current = flow.current_thread_mut();
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

    flow.pending_choices.clear();
    *status = StoryStatus::Active;
    stats.choices_selected += 1;

    Ok(())
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
                    .resolve_target(fb_id)
                    .map(|(idx, _)| idx)
                    .ok_or(RuntimeError::UnresolvedDefinition(fb_id))?;

                // Begin output capture — fallback is a function call whose
                // text output becomes the return value.
                flow.output.begin_capture();
                flow.invoke_fallback(container_idx);
            } else {
                return Err(RuntimeError::UnresolvedExternalCall(fn_id));
            }
        }
        ExternalResult::Pending => {
            // Leave the External frame intact — the caller must resolve
            // via story.resolve_external(value) before continuing.
            return Err(RuntimeError::UnresolvedExternalCall(fn_id));
        }
    }
    Ok(())
}

/// Flush remaining output buffer content into `(text, tags)`.
///
/// At a yield point (Done/Choices/Ended), no more output is coming, so
/// trailing newlines are committed. Lines are joined with `\n` and tags
/// are flattened into a single vec.
fn flush_remaining(
    flow: &mut Flow,
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
) -> (String, Vec<String>) {
    let lines = flow.output.flush_lines(program, line_tables, resolver);
    let mut text = String::new();
    let mut tags = Vec::new();
    for (i, (line_text, line_tags)) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.push_str(line_text);
        tags.extend_from_slice(line_tags);
    }
    (text, tags)
}

/// Build the appropriate [`Line`] variant for a yield point based on
/// the current story status.
fn make_yield_line(status: StoryStatus, text: String, tags: Vec<String>, flow: &Flow) -> Line {
    match status {
        StoryStatus::WaitingForChoice => {
            let choices = flow
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
            Line::Choices {
                text,
                tags,
                choices,
            }
        }
        StoryStatus::Ended => Line::End { text, tags },
        StoryStatus::Done => Line::Done { text, tags },
        StoryStatus::Active => Line::Text { text, tags },
    }
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
pub struct Story<'p, R: StoryRng = FastRng> {
    program: &'p Program,
    pub(crate) default: FlowInstance,
    pub(crate) default_context: Context,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    instances: HashMap<String, (FlowInstance, Context)>,
    resolver: Option<Box<dyn PluralResolver>>,
    _rng: PhantomData<R>,
}

impl<R: StoryRng> Clone for Story<'_, R> {
    fn clone(&self) -> Self {
        Self {
            program: self.program,
            default: self.default.clone(),
            default_context: self.default_context.clone(),
            line_tables: self.line_tables.clone(),
            instances: self.instances.clone(),
            resolver: None,
            _rng: PhantomData,
        }
    }
}

/// Owned story state that can be detached from a `Program` and reattached later.
///
/// Created by [`Story::into_snapshot`], consumed by [`Story::from_snapshot`].
/// This enables locale hot-swapping: detach state, mutate the program's line
/// tables, then reattach.
pub struct StorySnapshot<R: StoryRng = FastRng> {
    default: FlowInstance,
    default_context: Context,
    instances: HashMap<String, (FlowInstance, Context)>,
    _rng: PhantomData<R>,
}

impl<'p, R: StoryRng> Story<'p, R> {
    /// Create a new story instance from a linked program and its line tables.
    pub fn new(program: &'p Program, line_tables: Vec<Vec<brink_format::LineEntry>>) -> Self {
        let (default, default_context) = FlowInstance::new_at_root(program);
        Self {
            program,
            default,
            default_context,
            line_tables,
            instances: HashMap::new(),
            resolver: None,
            _rng: PhantomData,
        }
    }

    /// Set the plural resolver for Select resolution in localized lines.
    pub fn set_plural_resolver(&mut self, resolver: Box<dyn PluralResolver>) {
        self.resolver = Some(resolver);
    }

    /// Replace the active line tables (e.g. for locale swapping).
    pub fn set_line_tables(&mut self, tables: Vec<Vec<brink_format::LineEntry>>) {
        self.line_tables = tables;
    }

    /// Read-only access to the current line tables.
    pub fn line_tables(&self) -> &[Vec<brink_format::LineEntry>] {
        &self.line_tables
    }

    /// Detach story state from the program, consuming the story.
    pub fn into_snapshot(self) -> (StorySnapshot<R>, Vec<Vec<brink_format::LineEntry>>) {
        let snapshot = StorySnapshot {
            default: self.default,
            default_context: self.default_context,
            instances: self.instances,
            _rng: PhantomData,
        };
        (snapshot, self.line_tables)
    }

    /// Reattach a snapshot to a program with line tables.
    pub fn from_snapshot(
        program: &'p Program,
        snapshot: StorySnapshot<R>,
        line_tables: Vec<Vec<brink_format::LineEntry>>,
    ) -> Self {
        Self {
            program,
            default: snapshot.default,
            default_context: snapshot.default_context,
            line_tables,
            instances: snapshot.instances,
            resolver: None,
            _rng: PhantomData,
        }
    }

    // ── Execution API ──────────────────────────────────────────────

    /// Execute until one line of content (up to newline), or until a
    /// yield point (choices/end) if no newline occurs first.
    ///
    /// The returned [`Line`] variant tells you what to do next:
    /// - [`Line::Text`] — more output may follow, keep calling.
    /// - [`Line::Choices`] — call [`choose`](Self::choose) then resume.
    /// - [`Line::End`] — the story has permanently ended.
    pub fn continue_single(&mut self) -> Result<Line, RuntimeError> {
        let resolver = self.resolver.as_deref();
        self.default.step_single_line::<R>(
            self.program,
            &self.line_tables,
            &mut self.default_context,
            &FallbackHandler,
            resolver,
        )
    }

    /// Like [`continue_single`](Self::continue_single) but with a custom
    /// external function handler.
    pub fn continue_single_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Line, RuntimeError> {
        let resolver = self.resolver.as_deref();
        self.default.step_single_line::<R>(
            self.program,
            &self.line_tables,
            &mut self.default_context,
            handler,
            resolver,
        )
    }

    /// Execute until the next yield point, collecting all lines.
    ///
    /// Returns a `Vec<Line>` where the last element is always
    /// [`Line::Choices`] or [`Line::End`], and all preceding elements
    /// are [`Line::Text`].
    pub fn continue_maximally(&mut self) -> Result<Vec<Line>, RuntimeError> {
        self.continue_maximally_impl(&FallbackHandler)
    }

    /// Like [`continue_maximally`](Self::continue_maximally) but with a
    /// custom external function handler.
    pub fn continue_maximally_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Line>, RuntimeError> {
        self.continue_maximally_impl(handler)
    }

    /// Maximum lines per `continue_maximally` call. Safety net against
    /// infinite loops from malformed bytecode.
    const LINE_LIMIT: usize = 10_000;

    fn continue_maximally_impl(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Line>, RuntimeError> {
        let mut lines = Vec::new();
        loop {
            let resolver = self.resolver.as_deref();
            let line = self.default.step_single_line::<R>(
                self.program,
                &self.line_tables,
                &mut self.default_context,
                handler,
                resolver,
            )?;
            let terminal = line.is_terminal();
            lines.push(line);
            if terminal {
                return Ok(lines);
            }
            if lines.len() >= Self::LINE_LIMIT {
                return Err(RuntimeError::LineLimitExceeded(Self::LINE_LIMIT));
            }
        }
    }

    /// Execute until the next yield point with a [`WriteObserver`] that
    /// receives notifications for every state mutation.
    pub fn continue_maximally_observed(
        &mut self,
        observer: &mut dyn WriteObserver,
    ) -> Result<Vec<Line>, RuntimeError> {
        use crate::state::ObservedContext;
        let mut obs_ctx = ObservedContext::new(&mut self.default_context, observer);
        let mut lines = Vec::new();
        loop {
            let resolver = self.resolver.as_deref();
            let line = self.default.step_single_line::<R>(
                self.program,
                &self.line_tables,
                &mut obs_ctx,
                &FallbackHandler,
                resolver,
            )?;
            let terminal = line.is_terminal();
            lines.push(line);
            if terminal {
                return Ok(lines);
            }
            if lines.len() >= Self::LINE_LIMIT {
                return Err(RuntimeError::LineLimitExceeded(Self::LINE_LIMIT));
            }
        }
    }

    /// Select a choice by index, then resume with
    /// [`continue_single`](Self::continue_single) or
    /// [`continue_maximally`](Self::continue_maximally).
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        self.default.choose(&mut self.default_context, index)
    }

    /// Read-only access to the default flow's VM statistics.
    pub fn stats(&self) -> &Stats {
        &self.default.stats
    }

    /// Returns `true` if the default flow has a pending external call
    /// (an `External` frame on top of the call stack).
    pub fn has_pending_external(&self) -> bool {
        self.default.flow.external_fn_id().is_some()
    }

    /// Resolve a pending external call on the default flow by providing
    /// the return value. For fire-and-forget calls, pass `Value::Null`.
    ///
    /// After resolving, call [`continue_maximally`](Story::continue_maximally)
    /// to continue execution.
    pub fn resolve_external(&mut self, value: Value) {
        self.default.flow.resolve_external(value);
    }

    /// Resolve a pending external call on the default flow by invoking
    /// the ink-defined fallback body. The fallback is a function call
    /// whose output becomes the return value.
    ///
    /// After invoking, call [`continue_maximally`](Story::continue_maximally)
    /// to continue execution.
    pub fn invoke_fallback(&mut self) -> Result<(), RuntimeError> {
        let fn_id = self
            .default
            .flow
            .external_fn_id()
            .ok_or(RuntimeError::CallStackUnderflow)?;
        let entry = self.program.external_fn(fn_id);
        let fallback_id = entry
            .and_then(|e| e.fallback)
            .ok_or(RuntimeError::UnresolvedExternalCall(fn_id))?;
        let container_idx = self
            .program
            .resolve_target(fallback_id)
            .map(|(idx, _)| idx)
            .ok_or(RuntimeError::UnresolvedDefinition(fallback_id))?;
        self.default.flow.output.begin_capture();
        self.default.flow.invoke_fallback(container_idx);
        Ok(())
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
    ) -> Result<(), RuntimeError> {
        if self.instances.contains_key(name) {
            return Err(RuntimeError::FlowAlreadyExists(name.to_owned()));
        }
        let container_idx = self
            .program
            .resolve_target(entry_point)
            .map(|(idx, _)| idx)
            .ok_or(RuntimeError::UnresolvedDefinition(entry_point))?;
        let (flow, ctx) = FlowInstance::new_at(self.program, container_idx);
        self.instances.insert(name.to_owned(), (flow, ctx));
        Ok(())
    }

    /// Run a named flow instance until the next yield point.
    pub fn continue_flow_maximally(&mut self, name: &str) -> Result<Vec<Line>, RuntimeError> {
        self.continue_flow_maximally_with(name, &FallbackHandler)
    }

    /// Run a named flow instance with an external function handler.
    pub fn continue_flow_maximally_with(
        &mut self,
        name: &str,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Vec<Line>, RuntimeError> {
        let (instance, ctx) = self
            .instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        let mut lines = Vec::new();
        loop {
            let resolver = self.resolver.as_deref();
            let line = instance.step_single_line::<R>(
                self.program,
                &self.line_tables,
                ctx,
                handler,
                resolver,
            )?;
            let terminal = line.is_terminal();
            lines.push(line);
            if terminal {
                return Ok(lines);
            }
            if lines.len() >= Self::LINE_LIMIT {
                return Err(RuntimeError::LineLimitExceeded(Self::LINE_LIMIT));
            }
        }
    }

    /// Select a choice in a named flow.
    pub fn choose_flow(&mut self, name: &str, index: usize) -> Result<(), RuntimeError> {
        let (instance, ctx) = self
            .instances
            .get_mut(name)
            .ok_or_else(|| RuntimeError::UnknownFlow(name.to_owned()))?;
        instance.choose(ctx, index)
    }

    /// Destroy a named flow instance.
    pub fn destroy_flow(&mut self, name: &str) -> Result<(), RuntimeError> {
        if self.instances.remove(name).is_none() {
            return Err(RuntimeError::UnknownFlow(name.to_owned()));
        }
        Ok(())
    }

    /// List active flow names.
    pub fn flow_names(&self) -> Vec<&str> {
        self.instances.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
#[expect(clippy::panic)]
mod tests {
    use super::*;
    use crate::link;

    fn load_i079_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let json_str = std::fs::read_to_string(
            "../../tests/tier1/choices/I079-once-only-choices-can-link-back-to-self/story.ink.json",
        )
        .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        link(&data).unwrap()
    }

    /// Step a story until it yields choices, panicking if it ends first.
    fn step_until_choices(story: &mut Story) -> Vec<Choice> {
        loop {
            match story.continue_single().unwrap() {
                Line::Choices { choices, .. } => return choices,
                Line::Text { .. } => {}
                Line::Done { .. } => panic!("story hit Done before presenting choices"),
                Line::End { .. } => panic!("story ended before presenting choices"),
            }
        }
    }

    /// After selecting a once-only choice, the visit count for its target
    /// container must be > 0. Without this, the once-only filter in
    /// `handle_begin_choice` can never fire.
    #[test]
    fn select_choice_increments_visit_count_for_target() {
        let (program, line_tables) = load_i079_program();
        let mut story = Story::new(&program, line_tables);
        let choices = step_until_choices(&mut story);

        assert!(!choices.is_empty(), "expected at least one choice");

        // Record the target_id of the first pending choice BEFORE selecting.
        let target_id = story.default.flow.pending_choices[0].target_id;
        let visit_before = story
            .default_context
            .visit_counts
            .get(&target_id)
            .copied()
            .unwrap_or(0);

        story.choose(0).unwrap();

        // After selection, the visit count for this target must have increased.
        let visit_after = story
            .default_context
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
        let (program, line_tables) = load_i079_program();
        let mut story = Story::new(&program, line_tables);

        let first_choices = step_until_choices(&mut story);
        assert!(
            first_choices
                .iter()
                .any(|c| c.text.contains("First choice")),
            "first pass should contain 'First choice', got: {first_choices:?}"
        );

        story.choose(0).unwrap();

        let second_choices = step_until_choices(&mut story);
        assert!(
            !second_choices
                .iter()
                .any(|c| c.text.contains("First choice")),
            "second pass should NOT contain 'First choice' (once-only, already visited), \
             got: {second_choices:?}"
        );
    }

    // ── Choice thread forking ──────────────────────────────────────────

    fn load_i083_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let json_str = std::fs::read_to_string(
            "../../tests/tier1/choices/I083-choice-thread-forking/story.ink.json",
        )
        .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        link(&data).unwrap()
    }

    /// When a choice is created inside a tunnel, the call stack at that
    /// moment (including the tunnel frame with its temps) must be captured.
    /// After the tunnel returns and the choice is presented, the snapshot
    /// should still reflect the tunnel-era call stack depth (>= 2 frames).
    #[test]
    fn pending_choice_captures_tunnel_call_stack() {
        let (program, line_tables) = load_i083_program();
        let mut story = Story::new(&program, line_tables);
        let _choices = step_until_choices(&mut story);

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
        let (program, line_tables) = load_i083_program();
        let mut story = Story::new(&program, line_tables);
        let _choices = step_until_choices(&mut story);

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

    fn load_tags_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let json_str =
            std::fs::read_to_string("../../tests/tier3/tags/tags/story.ink.json").unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        link(&data).unwrap()
    }

    fn load_tags_in_choice_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let json_str =
            std::fs::read_to_string("../../tests/tier3/tags/tagsInChoice/story.ink.json").unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        link(&data).unwrap()
    }

    #[test]
    fn line_exposes_tags() {
        let (program, line_tables) = load_tags_program();
        let mut story = Story::<crate::FastRng>::new(&program, line_tables);
        let lines = story.continue_maximally().unwrap();
        // The first line should have both tags.
        let first = lines.first().expect("expected at least one line");
        assert!(
            !matches!(first, Line::Choices { .. }),
            "expected Text or End, got Choices"
        );
        assert_eq!(first.tags(), &["author: Joe", "title: My Great Story"],);
    }

    #[test]
    fn choice_exposes_tags() {
        let (program, line_tables) = load_tags_in_choice_program();
        let mut story = Story::new(&program, line_tables);
        let choices = step_until_choices(&mut story);
        assert!(!choices.is_empty());
        // The choice in tagsInChoice has tags "one" and "two"
        assert!(
            !choices[0].tags.is_empty(),
            "choice should have tags, got: {choices:?}"
        );
    }

    // ── Thread support ──────────────────────────────────────────────────

    fn load_i091_program() -> (crate::Program, Vec<Vec<brink_format::LineEntry>>) {
        let json_str =
            std::fs::read_to_string("../../tests/tier1/choices/I091-choice-count/story.ink.json")
                .unwrap();
        let ink: brink_json::InkJson = serde_json::from_str(&json_str).unwrap();
        let data = brink_converter::convert(&ink).unwrap();
        link(&data).unwrap()
    }

    /// `<- choices` (thread) must create choices AND return to the main
    /// flow so that `CHOICE_COUNT()` can evaluate. The thread body
    /// should be called like a tunnel — when its container stack empties,
    /// execution returns to the caller. Non-root frames must always pop
    /// back to their caller, even when pending choices exist.
    #[test]
    fn thread_call_returns_to_main_flow() {
        let (program, line_tables) = load_i091_program();
        let mut story = Story::<crate::FastRng>::new(&program, line_tables);

        let lines = story.continue_maximally().unwrap();
        // I091 should output "2\n" (CHOICE_COUNT) then present 2 choices.
        let full_text: String = lines.iter().map(Line::text).collect();
        assert!(
            full_text.starts_with('2'),
            "output should start with '2' from CHOICE_COUNT(), got: {full_text:?}"
        );
        let last = lines.last().expect("expected at least one line");
        match last {
            Line::Choices { choices, .. } => {
                assert_eq!(choices.len(), 2, "expected 2 choices");
            }
            other => panic!("expected Choices, got {other:?}"),
        }
    }
}
