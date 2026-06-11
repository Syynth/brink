//! Per-instance mutable story state.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

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

/// Outcome of a single [`FlowInstance::advance`] step.
///
/// Like [`Line`], but with an extra variant for when a binding handler
/// deferred an external call ([`ExternalResult::Pending`]) — e.g. a
/// world-access query hit during normal playback. The flow is paused with
/// its state intact: inspect the pending call via
/// [`pending_external_name`](FlowInstance::pending_external_name) /
/// [`pending_external_args`](FlowInstance::pending_external_args), supply
/// the result with [`resolve_external`](FlowInstance::resolve_external),
/// then call [`advance`](FlowInstance::advance) again.
///
/// [`step_single_line`](FlowInstance::step_single_line) is the simpler API
/// for consumers whose handler never pauses — it maps `AwaitingExternal`
/// to an error.
#[derive(Debug, Clone)]
pub enum StepOutcome {
    /// A line of output, or a yield point (`Done`/`Choices`/`End`).
    Line(Line),
    /// The flow paused on a deferred external; resolve it and `advance`.
    AwaitingExternal,
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
    /// `CallStack::snapshot` cache hits (reused existing `Arc`).
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
    /// Boundary frame pushed by an engine→ink call
    /// ([`FlowInstance::begin_function_eval`]). Behaves like `Function`
    /// for output trimming and implicit-return purposes, but marks where
    /// a from-game evaluation began so the eval driver knows when the
    /// function has returned. Mirrors C#'s
    /// `PushPopType.FunctionEvaluationFromGame`.
    FunctionEvalFromGame,
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
    /// For `Function` frames: the length of the active output target at
    /// call time.  On return, trailing whitespace is trimmed back to this
    /// point — matching the C# runtime's `TrimWhitespaceFromFunctionEnd`.
    pub function_output_start: Option<usize>,
}

/// Two-part call stack: shared read-only prefix + owned mutable frames.
///
/// `fork_thread` snapshots the parent's frames into a cached `Arc<[CallFrame]>`
/// (one clone, amortized across all children). Children get `Arc::clone` — O(1).
/// The parent keeps its `own` vec unchanged and continues mutating freely.
#[derive(Debug, Clone)]
pub(crate) struct CallStack {
    /// Shared read-only prefix inherited from the parent thread.
    inherited: Option<Arc<[CallFrame]>>,
    /// Frames owned by this thread (above the fork point).
    own: Vec<CallFrame>,
    /// Cached snapshot so multiple forks from the same parent share one allocation.
    cached_snapshot: Option<Arc<[CallFrame]>>,
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

    /// Build an `Arc<[CallFrame]>` snapshot of the full stack (inherited + own).
    /// The result is cached so multiple forks from the same parent share one
    /// allocation. Returns `(snapshot, cache_hit)`.
    pub fn snapshot(&mut self) -> (Arc<[CallFrame]>, bool) {
        if let Some(ref cached) = self.cached_snapshot {
            return (Arc::clone(cached), true);
        }
        let rc = match &self.inherited {
            None => Arc::from(self.own.as_slice()),
            Some(prefix) if self.own.is_empty() => Arc::clone(prefix),
            Some(prefix) => {
                let mut combined = Vec::with_capacity(prefix.len() + self.own.len());
                combined.extend_from_slice(prefix);
                combined.extend_from_slice(&self.own);
                Arc::from(combined)
            }
        };
        self.cached_snapshot = Some(Arc::clone(&rc));
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

/// How the choice display text is stored internally.
#[derive(Debug, Clone)]
pub(crate) enum ChoiceDisplay {
    /// Eagerly resolved text (legacy path, converter, or non-fragment codegen).
    Text(String),
    /// Index into the output buffer's fragment store — resolved on demand.
    Fragment(u32),
}

#[derive(Debug, Clone)]
pub(crate) struct PendingChoice {
    pub display: ChoiceDisplay,
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
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "VM flags are inherently boolean"
)]
pub(crate) struct Flow {
    pub threads: Vec<Thread>,
    pub value_stack: Vec<Value>,
    pub output: OutputBuffer,
    pub pending_choices: Vec<PendingChoice>,
    pub current_tags: Vec<String>,
    pub in_tag: bool,
    pub skipping_choice: bool,
    /// Set to `true` when a `Done` opcode fires (explicit `-> DONE`).
    /// Cleared at the start of each `continue_single` call.
    pub did_safe_exit: bool,
    /// Set to `true` when a `Yield` opcode falls through with no
    /// pending choices — the story passed through an empty choice set.
    /// Cleared at the start of each `continue_single` call.
    pub did_unsafe_yield: bool,
}

/// Shared game state that lives above individual flows.
///
/// Holds globals, visit/turn tracking, and RNG state. This is the natural
/// serialization boundary for save/load (deferred).
///
/// Multiple [`FlowInstance`]s can share a single `Context` (matching
/// inklecate's semantics where flow writes are immediately visible to other
/// flows), or each flow can hold its own cloned `Context` if the consumer
/// wants fork/branch/rollback semantics. The runtime's step functions take
/// `&mut Context` (or any `&mut impl ContextAccess`) without prescribing
/// where it lives.
#[derive(Debug, Clone)]
pub struct Context {
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

    /// Returns `true` if a `FunctionEvalFromGame` boundary frame is present
    /// in the current thread's call stack — i.e. an engine→ink function
    /// evaluation is still in progress. Functions don't fork threads, so
    /// the current thread is where the boundary lives. The eval driver
    /// uses this to detect when the function has returned (boundary popped).
    pub fn has_eval_boundary(&self) -> bool {
        let cs = &self.current_thread().call_stack;
        (0..cs.len())
            .filter_map(|i| cs.get(i))
            .any(|f| f.frame_type == CallFrameType::FunctionEvalFromGame)
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
        let output_start = self.output.target_len();
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
            frame.function_output_start = Some(output_start);
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
#[derive(Debug, Clone)]
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
///
/// Use this as the `handler` argument to [`FlowInstance::step_single_line`]
/// or [`FlowInstance::choose`] when you don't want to provide a custom
/// external-function binding registry. Every external call returns
/// [`ExternalResult::Fallback`], delegating to the in-story fallback
/// container declared on the `EXTERNAL` declaration.
pub struct FallbackHandler;

impl ExternalFnHandler for FallbackHandler {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Fallback
    }
}

/// Outcome of an engine→ink function evaluation
/// ([`FlowInstance::begin_function_eval`] / [`resume_function_eval`](FlowInstance::resume_function_eval)).
///
/// Evaluating an ink function from engine code does not advance the
/// player-visible story: its output is isolated and discarded, and the
/// transcript is untouched. The only result is the function's return
/// value — unless the function calls an external that can't be resolved
/// synchronously.
#[derive(Debug, Clone)]
pub enum FunctionEval {
    /// The function returned this value and evaluation is complete.
    /// (Functions with no explicit `~ return` yield [`Value::Null`].)
    Returned(Value),
    /// The function called an external whose handler returned
    /// [`ExternalResult::Pending`] — typically a binding that needs
    /// engine/World access resolved out-of-band. Evaluation is paused
    /// with its full state intact. Inspect the pending call via
    /// [`pending_external_name`](FlowInstance::pending_external_name) /
    /// [`pending_external_args`](FlowInstance::pending_external_args),
    /// supply the result with
    /// [`resolve_external`](FlowInstance::resolve_external), then call
    /// [`resume_function_eval`](FlowInstance::resume_function_eval).
    AwaitingExternal,
}

// ── FlowInstance ────────────────────────────────────────────────────────────

/// A single independent execution context within a story. The default flow
/// runs from the root container; named flows can be spawned at arbitrary
/// entry points via [`FlowInstance::new_at`].
///
/// A `FlowInstance` is opaque from outside the crate: its internal fields
/// (`flow`, `status`, `stats`) are crate-private, but consumers can hold,
/// clone, serialize, and pass `&mut FlowInstance` to the runtime's step
/// functions. Use the inherent methods ([`step_single_line`](Self::step_single_line),
/// [`choose`](Self::choose), [`transcript`](Self::transcript),
/// [`status`](Self::status), etc.) for all interaction.
#[derive(Clone, Debug)]
pub struct FlowInstance {
    pub(crate) flow: Flow,
    pub(crate) status: StoryStatus,
    pub(crate) stats: Stats,
    /// Transient state for an in-progress engine→ink function evaluation
    /// ([`begin_function_eval`](Self::begin_function_eval)). `Some` only
    /// while a from-game call is mid-flight (possibly paused on an
    /// external); `None` during normal play. Not meaningful to persist.
    pub(crate) eval: Option<EvalState>,
}

/// Bookkeeping for an in-progress engine→ink function evaluation.
#[derive(Debug, Clone)]
pub(crate) struct EvalState {
    /// Value-stack length recorded before arguments were pushed, so the
    /// return value (and any leftover args) can be reclaimed on return.
    pub value_floor: usize,
    /// Pending-choice count when the eval began. A function that *grows*
    /// this presented a choice — illegal, and distinct from choices the
    /// main story may already have waiting.
    pub choice_floor: usize,
}

impl FlowInstance {
    /// Create a new flow instance starting at the program's root container,
    /// along with a fresh [`Context`] initialized from the program's global
    /// defaults.
    pub fn new_at_root(program: &Program) -> (Self, Context) {
        Self::new_at(program, program.root_idx())
    }

    /// Create a new flow instance starting at an arbitrary container index,
    /// along with a fresh [`Context`]. Use this to spawn a named flow at a
    /// specific entry point. The caller is responsible for deciding whether
    /// to share the returned `Context` with other flows or discard it and
    /// reuse an existing one.
    pub fn new_at(program: &Program, container_idx: u32) -> (Self, Context) {
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
            function_output_start: None,
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
                did_safe_exit: false,
                did_unsafe_yield: false,
            },
            status: StoryStatus::Active,
            stats: Stats::default(),
            eval: None,
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

    /// Execute until one complete line of output is available, or until a
    /// yield point (choices/done/ended) if no newline occurs first.
    ///
    /// Returns a [`Line`] telling the caller what happened (`Text`/`Done`/
    /// `Choices`/`End`). This is the simple API for consumers whose
    /// external handler never defers: if the handler returns
    /// [`ExternalResult::Pending`], this errors with
    /// [`UnresolvedExternalCall`](RuntimeError::UnresolvedExternalCall).
    /// For pausable world-access bindings, use [`advance`](Self::advance).
    pub fn step_single_line<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<Line, RuntimeError> {
        match self.advance::<R>(program, line_tables, context, handler, resolver)? {
            StepOutcome::Line(line) => Ok(line),
            StepOutcome::AwaitingExternal => {
                // Preserve historical behavior for consumers using this
                // (non-pausing) API: a deferred external they can't resolve
                // is an error.
                let id = self
                    .flow
                    .external_fn_id()
                    .ok_or(RuntimeError::CallStackUnderflow)?;
                Err(RuntimeError::UnresolvedExternalCall(id))
            }
        }
    }

    /// Like [`step_single_line`](Self::step_single_line), but surfaces a
    /// deferred external ([`ExternalResult::Pending`]) as
    /// [`StepOutcome::AwaitingExternal`] instead of an error — so a
    /// world-access binding hit during normal playback can pause cleanly.
    /// Resolve the pending external and call `advance` again to continue.
    #[expect(clippy::too_many_lines)]
    pub fn advance<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<StepOutcome, RuntimeError> {
        // 1. If buffer already has a completed line from a previous step,
        //    take it immediately (no VM stepping needed).
        if self.flow.output.has_completed_line()
            && let Some((text, tags)) =
                self.flow
                    .output
                    .take_first_line(program, line_tables, resolver)
        {
            return Ok(StepOutcome::Line(Line::Text { text, tags }));
        }

        // 2. If buffer has partial content but VM has already yielded
        //    (any non-Active state), flush it. At a yield point, no more
        //    output is coming, so trailing Newlines are committed.
        if self.flow.output.has_unread() && self.status != StoryStatus::Active {
            let (text, tags) = flush_remaining(&mut self.flow, program, line_tables, resolver);
            return Ok(StepOutcome::Line(make_yield_line(
                self.status,
                text,
                tags,
                &self.flow,
                program,
                line_tables,
                resolver,
            )));
        }

        // 3. Status checks.
        if self.status == StoryStatus::Ended {
            return Err(RuntimeError::StoryEnded);
        }
        if self.status == StoryStatus::WaitingForChoice {
            return Err(RuntimeError::NotWaitingForChoice);
        }

        // 4. Reset Done → Active (resuming after output).
        //    If the previous cycle ended without a safe exit (no explicit
        //    -> DONE opcode), the story ran out of content. The previous
        //    call delivered the text — error now.
        if self.status == StoryStatus::Done {
            if !self.flow.did_safe_exit {
                return Err(RuntimeError::RanOutOfContent);
            }
            self.status = StoryStatus::Active;
        }

        // Clear flags — will be set during this cycle if relevant.
        self.flow.did_safe_exit = false;
        self.flow.did_unsafe_yield = false;

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
                        return Ok(StepOutcome::Line(Line::Text { text, tags }));
                    }
                }

                vm::Stepped::ExternalCall => {
                    // `false` means the handler deferred (Pending): pause
                    // cleanly so the caller can resolve it out-of-band.
                    if !resolve_external_call(flow, program, handler)? {
                        return Ok(StepOutcome::AwaitingExternal);
                    }
                    if flow.output.has_completed_line()
                        && let Some((text, tags)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(StepOutcome::Line(Line::Text { text, tags }));
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
                                return Ok(StepOutcome::Line(Line::Text { text, tags }));
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
                        return Ok(StepOutcome::Line(Line::Text { text, tags }));
                    }

                    let (text, tags) = flush_remaining(flow, program, line_tables, resolver);
                    return Ok(StepOutcome::Line(make_yield_line(
                        *status,
                        text,
                        tags,
                        flow,
                        program,
                        line_tables,
                        resolver,
                    )));
                }

                vm::Stepped::Ended => {
                    context.increment_turn_index();
                    *status = StoryStatus::Ended;

                    if flow.output.has_completed_line()
                        && let Some((text, tags)) =
                            flow.output.take_first_line(program, line_tables, resolver)
                    {
                        return Ok(StepOutcome::Line(Line::Text { text, tags }));
                    }

                    let (text, tags) = flush_remaining(flow, program, line_tables, resolver);
                    return Ok(StepOutcome::Line(Line::End { text, tags }));
                }
            }
        }
    }

    /// Select a choice by index. Call [`step_single_line`](Self::step_single_line)
    /// afterward to continue execution from the chosen branch.
    pub fn choose(
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

    /// Move the play head to a named knot/stitch path — the equivalent of
    /// ink's `Story.ChoosePathString(path)` (with its default
    /// `resetCallstack: true`). Call [`step_single_line`](Self::step_single_line)
    /// (or any continue method) afterward to run from there.
    ///
    /// `path` is a dot-separated runtime path: a knot (`intro`), a qualified
    /// stitch (`intro.dock`), or — for programs compiled by `brink-compiler` —
    /// an author label (`knot.label`, `knot.stitch.label`; an extension over
    /// C#, which cannot address labels).
    ///
    /// Mirroring the C# reference (`Story.ChoosePathString` →
    /// `ResetCallstack`/`ForceEnd` → `ChoosePath` → `state.SetChosenPath` +
    /// `VisitChangedContainersDueToDivert`):
    ///
    /// - The current flow is **force-completed** first: the call stack
    ///   collapses to a single fresh root frame (abandoning any tunnels,
    ///   threads, or in-progress weave), pending choices are cleared, and
    ///   the jump counts as a safe exit (as if the story had hit `-> DONE`).
    /// - The jump **counts as a visit** to the target, with exactly the
    ///   semantics of an in-story `-> path` divert (it goes through the same
    ///   goto machinery, so counting flags are honored identically).
    /// - Output already produced but not yet consumed is **kept** (C# leaves
    ///   the output stream untouched); it is delivered before content from
    ///   the new location. The value stack is likewise left as-is.
    /// - A permanently **ended** story (`-> END`) may be re-entered by
    ///   jumping, matching C# where `ChoosePathString` + `Continue` works
    ///   after the story has ended.
    ///
    /// # Errors
    /// - [`UnknownPath`](RuntimeError::UnknownPath) if `path` resolves to no
    ///   target (the message names the path).
    /// - [`JumpWhileAwaitingExternal`](RuntimeError::JumpWhileAwaitingExternal)
    ///   if the flow is parked on an unresolved external call — a pending
    ///   host call must be resolved, not silently abandoned.
    /// - [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    ///   if an engine→ink function evaluation is in progress (C# likewise
    ///   refuses to redirect mid-function).
    pub fn choose_path_string(
        &mut self,
        program: &Program,
        context: &mut (impl ContextAccess + ?Sized),
        path: &str,
    ) -> Result<(), RuntimeError> {
        // A parked host call cannot be silently abandoned: erroring is the
        // strictest safe behavior (brink-specific — C# has no pausable
        // externals during normal playback).
        if let Some(id) = self.flow.external_fn_id() {
            let external = program
                .external_fn(id)
                .map_or_else(|| format!("{id}"), |e| program.name(e.name).to_owned());
            return Err(RuntimeError::JumpWhileAwaitingExternal {
                path: path.to_owned(),
                external,
            });
        }
        // An in-flight engine→ink evaluation (possibly paused on an external)
        // must finish or be aborted before the flow can be redirected.
        if self.eval.is_some() {
            return Err(RuntimeError::AlreadyEvaluatingFunction);
        }

        let target_id = program
            .find_path_target(path)
            .ok_or_else(|| RuntimeError::UnknownPath(path.to_owned()))?;

        // Force-end the current flow, mirroring C# `ResetCallstack` →
        // `StoryState.ForceEnd`: a single fresh root frame (callStack.Reset),
        // cleared choices, null pointers (the empty container stack), and
        // didSafeExit = true. The output buffer and value stack are
        // deliberately left untouched — C# `ForceEnd` does not clear the
        // output stream or the evaluation stack.
        let root_frame = CallFrame {
            return_address: None,
            temps: Vec::new(),
            container_stack: Vec::new(),
            frame_type: CallFrameType::Root,
            external_fn_id: None,
            function_output_start: None,
        };
        self.flow.threads = vec![Thread {
            call_stack: CallStack::new(root_frame),
        }];
        self.flow.pending_choices.clear();
        // Transient intra-step flags. Both are false at any point a host can
        // observe (between lines / at a yield), but the jump abandons whatever
        // produced them, so clear defensively.
        self.flow.skipping_choice = false;
        self.flow.in_tag = false;
        self.flow.did_safe_exit = true;

        // Jump via the same divert machinery as an in-story `-> path`
        // (mirrors C# `ChoosePath` → `SetChosenPath` +
        // `VisitChangedContainersDueToDivert`): sets the position and
        // increments the target's visit/turn counts per its counting flags.
        vm::goto_target(&mut self.flow, program, context, target_id)?;

        self.status = StoryStatus::Active;
        Ok(())
    }

    /// The current execution status of this flow.
    #[must_use]
    pub fn status(&self) -> StoryStatus {
        self.status
    }

    /// Runtime statistics (instructions, materialization counts, etc.)
    /// accumulated over this flow's execution.
    #[must_use]
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// The full append-only transcript of all output parts produced so far.
    ///
    /// The transcript stores structural references (e.g. `LineRef`) rather
    /// than resolved strings, so it can be re-rendered in any locale by
    /// passing a different set of line tables to
    /// [`transcript::render_transcript`](crate::transcript::render_transcript).
    #[must_use]
    pub fn transcript(&self) -> &[crate::output::OutputPart] {
        self.flow.output.transcript()
    }

    /// Number of parts in the transcript.
    #[must_use]
    pub fn transcript_len(&self) -> usize {
        self.flow.output.transcript_len()
    }

    /// Reset the transcript read cursor to the beginning (for re-rendering,
    /// e.g. after a locale swap).
    pub fn reset_cursor(&mut self) {
        self.flow.output.reset_cursor();
    }

    /// The fragments captured during execution (for re-rendering choice
    /// display text and computed substrings in a different locale).
    #[must_use]
    pub fn fragments(&self) -> &[crate::output::Fragment] {
        self.flow.output.fragments()
    }

    // ── External calls (ink → engine) ────────────────────────────────

    /// Returns `true` if this flow is frozen on an unresolved external
    /// call — i.e. the VM hit a `CallExternal` opcode and the handler
    /// returned [`ExternalResult::Pending`], leaving the `External` frame
    /// on top of the call stack.
    ///
    /// The orchestration layer (e.g. a Bevy resolver system) polls this to
    /// decide whether the flow needs an external resolved before it can be
    /// driven further. Resolve via [`resolve_external`](Self::resolve_external).
    #[must_use]
    pub fn has_pending_external(&self) -> bool {
        self.flow.external_fn_id().is_some()
    }

    /// The [`DefinitionId`] of the pending external function, if this flow
    /// is frozen on one. Returns `None` otherwise.
    #[must_use]
    pub fn pending_external_fn_id(&self) -> Option<DefinitionId> {
        self.flow.external_fn_id()
    }

    /// The arguments to the pending external call, in declaration order.
    /// Empty if no external call is pending.
    #[must_use]
    pub fn pending_external_args(&self) -> &[Value] {
        self.flow.external_args()
    }

    /// The ink-declared name of the pending external function, resolved
    /// against `program`'s name table. Returns `None` if no external is
    /// pending (or the entry is missing, which would indicate a malformed
    /// program).
    ///
    /// The orchestration layer uses this to look up the binding registered
    /// for this name.
    #[must_use]
    pub fn pending_external_name<'p>(&self, program: &'p Program) -> Option<&'p str> {
        let id = self.flow.external_fn_id()?;
        let entry = program.external_fn(id)?;
        Some(program.name(entry.name))
    }

    /// Resolve a pending external call by supplying its return value. Pops
    /// the `External` frame and pushes `value` onto the value stack so the
    /// VM can resume. For fire-and-forget externals, pass [`Value::Null`].
    ///
    /// No-op if no external call is pending. After resolving, drive the
    /// flow forward with [`step_single_line`](Self::step_single_line).
    pub fn resolve_external(&mut self, value: Value) {
        self.flow.resolve_external(value);
    }

    // ── Engine → ink calls ───────────────────────────────────────────

    /// Evaluate an ink function from engine code, returning its value.
    ///
    /// This does **not** advance the player-visible story: a
    /// `FunctionEvalFromGame` boundary frame is pushed, `args` are passed
    /// in declaration order (exactly as a normal call site would), output
    /// is captured and discarded, and the function runs until it returns.
    ///
    /// If the function calls an external whose handler returns
    /// [`ExternalResult::Pending`] (e.g. a binding that needs Bevy World
    /// access), evaluation pauses and returns
    /// [`FunctionEval::AwaitingExternal`]; the caller resolves the
    /// external (see [`resolve_external`](Self::resolve_external)) and
    /// calls [`resume_function_eval`](Self::resume_function_eval).
    ///
    /// `container_idx` is the function's container, typically obtained from
    /// [`Program::find_address`](crate::Program::find_address) on the
    /// function name. Unlike a normal `Call`, this does not increment the
    /// function's visit count — an engine query is out-of-band, matching
    /// C#'s `EvaluateFunction`.
    ///
    /// # Errors
    /// - [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    ///   if a function evaluation is already in progress on this flow.
    /// - [`FunctionYielded`](RuntimeError::FunctionYielded) if the function
    ///   presents choices or ends the story (functions must not yield).
    /// - [`UnresolvedExternalCall`](RuntimeError::UnresolvedExternalCall)
    ///   if an external has neither a binding nor a fallback.
    #[expect(
        clippy::too_many_arguments,
        reason = "the VM environment (program, line tables, context, handler, resolver) plus the call target and args"
    )]
    pub fn begin_function_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        container_idx: u32,
        args: &[Value],
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<FunctionEval, RuntimeError> {
        if self.eval.is_some() {
            return Err(RuntimeError::AlreadyEvaluatingFunction);
        }

        // Record floors BEFORE pushing args: the value-stack length (so the
        // return value and any leftover args can be reclaimed), and the
        // pending-choice count (so we can tell a choice the function
        // presents from choices the main story already has waiting).
        let value_floor = self.flow.value_stack.len();
        let choice_floor = self.flow.pending_choices.len();

        // Isolate output: anything the function emits routes to the
        // capture scratch space and never reaches the transcript.
        self.flow.output.begin_capture();

        let output_start = self.flow.output.target_len();
        let boundary = CallFrame {
            return_address: None,
            temps: Vec::new(),
            container_stack: vec![ContainerPosition {
                container_idx,
                offset: 0,
            }],
            frame_type: CallFrameType::FunctionEvalFromGame,
            external_fn_id: None,
            function_output_start: Some(output_start),
        };
        self.flow.current_thread_mut().call_stack.push(boundary);
        self.stats.frames_pushed += 1;

        // Pass arguments onto the value stack in declaration order — the
        // function's prologue (`DeclareTemp`) binds them exactly as it
        // would for an in-story call.
        self.flow.value_stack.extend_from_slice(args);

        self.eval = Some(EvalState {
            value_floor,
            choice_floor,
        });
        self.drive_function_eval::<R>(program, line_tables, context, handler, resolver)
    }

    /// Resume a function evaluation that paused on
    /// [`FunctionEval::AwaitingExternal`], after the pending external has
    /// been resolved via [`resolve_external`](Self::resolve_external).
    ///
    /// # Errors
    /// - [`NotEvaluatingFunction`](RuntimeError::NotEvaluatingFunction) if
    ///   no evaluation is in progress.
    /// - Same evaluation errors as
    ///   [`begin_function_eval`](Self::begin_function_eval).
    pub fn resume_function_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<FunctionEval, RuntimeError> {
        if self.eval.is_none() {
            return Err(RuntimeError::NotEvaluatingFunction);
        }
        self.drive_function_eval::<R>(program, line_tables, context, handler, resolver)
    }

    /// Returns `true` if a function evaluation is in progress (possibly
    /// paused awaiting an external).
    #[must_use]
    pub fn is_evaluating_function(&self) -> bool {
        self.eval.is_some()
    }

    /// Step the VM until the in-progress function evaluation returns or
    /// pauses on a pending external. Shared by `begin`/`resume`.
    fn drive_function_eval<R: StoryRng>(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        resolver: Option<&dyn PluralResolver>,
    ) -> Result<FunctionEval, RuntimeError> {
        let step_start = self.stats.steps;
        loop {
            self.stats.steps += 1;
            if self.stats.steps - step_start > Self::STEP_LIMIT {
                self.abort_eval(program, line_tables, resolver);
                return Err(RuntimeError::StepLimitExceeded(Self::STEP_LIMIT));
            }

            let stepped = vm::step::<R>(
                &mut self.flow,
                program,
                line_tables,
                context,
                &mut self.stats,
                resolver,
            )?;
            self.stats.materializations += self.flow.drain_materializations();

            match stepped {
                vm::Stepped::Done | vm::Stepped::Ended => {
                    // A function reached `-> DONE`/`-> END` — illegal.
                    self.abort_eval(program, line_tables, resolver);
                    return Err(RuntimeError::FunctionYielded);
                }
                vm::Stepped::ExternalCall => {
                    if let Some(pending) =
                        self.resolve_eval_external(program, line_tables, resolver, handler)?
                    {
                        return Ok(pending);
                    }
                }
                vm::Stepped::Continue | vm::Stepped::ThreadCompleted => {}
            }

            // Did the boundary frame pop? Then the function has returned
            // (via `~ return` or implicit exhaustion).
            if !self.flow.has_eval_boundary() {
                let _captured = self.flow.output.end_capture(program, line_tables, resolver);
                let floor = self.eval.take().map_or(0, |e| e.value_floor);
                let mut ret: Option<Value> = None;
                while self.flow.value_stack.len() > floor {
                    let v = self.flow.value_stack.pop();
                    if ret.is_none() {
                        ret = v; // first popped = top of stack = the return value
                    }
                }
                return Ok(FunctionEval::Returned(ret.unwrap_or(Value::Null)));
            }

            // A function must not present choices. Compare against the
            // count when the eval began — the main story may already have
            // choices waiting, which are none of our concern.
            let choice_floor = self.eval.as_ref().map_or(0, |e| e.choice_floor);
            if self.flow.pending_choices.len() > choice_floor {
                self.abort_eval(program, line_tables, resolver);
                return Err(RuntimeError::FunctionYielded);
            }
        }
    }

    /// Resolve an external hit during function evaluation, mirroring the
    /// normal step path but surfacing [`ExternalResult::Pending`] as
    /// [`FunctionEval::AwaitingExternal`] (returned as `Some`) rather than
    /// an error. Returns `None` when the external resolved and stepping
    /// should continue.
    fn resolve_eval_external(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        resolver: Option<&dyn PluralResolver>,
        handler: &dyn ExternalFnHandler,
    ) -> Result<Option<FunctionEval>, RuntimeError> {
        let fn_id = self
            .flow
            .external_fn_id()
            .ok_or(RuntimeError::CallStackUnderflow)?;
        let entry = program.external_fn(fn_id);
        let fn_name = entry.map_or("?", |e| program.name(e.name));
        match handler.call(fn_name, self.flow.external_args()) {
            ExternalResult::Resolved(value) => {
                self.flow.resolve_external(value);
                Ok(None)
            }
            ExternalResult::Fallback => {
                if let Some(fb_id) = entry.and_then(|e| e.fallback) {
                    let container_idx = program
                        .resolve_target(fb_id)
                        .map(|(idx, _)| idx)
                        .ok_or(RuntimeError::UnresolvedDefinition(fb_id))?;
                    self.flow.invoke_fallback(container_idx);
                    Ok(None)
                } else {
                    self.abort_eval(program, line_tables, resolver);
                    Err(RuntimeError::UnresolvedExternalCall(fn_id))
                }
            }
            ExternalResult::Pending => Ok(Some(FunctionEval::AwaitingExternal)),
        }
    }

    /// Tear down an aborted/failed evaluation: end the output capture and
    /// clear the eval marker. Leaves the call stack as-is (the caller is
    /// erroring out).
    fn abort_eval(
        &mut self,
        program: &Program,
        line_tables: &[Vec<brink_format::LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) {
        if self.eval.take().is_some() {
            let _ = self.flow.output.end_capture(program, line_tables, resolver);
        }
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
///
/// Returns `Ok(true)` if the call was resolved (a value was supplied or the
/// in-story fallback was invoked) and stepping should continue; `Ok(false)`
/// if the handler deferred ([`ExternalResult::Pending`]), leaving the
/// `External` frame intact for the caller to resolve out-of-band. Errors
/// only when the handler declined and no fallback exists.
fn resolve_external_call(
    flow: &mut Flow,
    program: &Program,
    handler: &dyn ExternalFnHandler,
) -> Result<bool, RuntimeError> {
    let fn_id = flow
        .external_fn_id()
        .ok_or(RuntimeError::CallStackUnderflow)?;

    let entry = program.external_fn(fn_id);
    let fn_name = entry.map_or("?", |e| program.name(e.name));

    let result = handler.call(fn_name, flow.external_args());
    match result {
        ExternalResult::Resolved(value) => {
            flow.resolve_external(value);
            Ok(true)
        }
        ExternalResult::Fallback => {
            let fallback_id = entry.and_then(|e| e.fallback);
            if let Some(fb_id) = fallback_id {
                let container_idx = program
                    .resolve_target(fb_id)
                    .map(|(idx, _)| idx)
                    .ok_or(RuntimeError::UnresolvedDefinition(fb_id))?;

                flow.invoke_fallback(container_idx);
                Ok(true)
            } else {
                Err(RuntimeError::UnresolvedExternalCall(fn_id))
            }
        }
        ExternalResult::Pending => {
            // Leave the External frame intact — the caller resolves it
            // out-of-band (via resolve_external) before continuing.
            Ok(false)
        }
    }
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
fn make_yield_line(
    status: StoryStatus,
    text: String,
    tags: Vec<String>,
    flow: &Flow,
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
) -> Line {
    match status {
        StoryStatus::WaitingForChoice => {
            let choices = flow
                .pending_choices
                .iter()
                .enumerate()
                .filter(|(_, pc)| !pc.flags.is_invisible_default)
                .map(|(i, pc)| {
                    let display_text = match &pc.display {
                        ChoiceDisplay::Text(s) => s.clone(),
                        ChoiceDisplay::Fragment(idx) => {
                            flow.output
                                .resolve_fragment(*idx, program, line_tables, resolver)
                        }
                    };
                    // Trim spaces/tabs from choice display text, matching C#:
                    // choice.text = (startText + choiceOnlyText).Trim(' ', '\t');
                    let display_text = display_text
                        .trim_matches(|c: char| c == ' ' || c == '\t')
                        .to_string();
                    Choice {
                        text: display_text,
                        index: i,
                        tags: pc.tags.clone(),
                    }
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

    /// The full append-only transcript of all output parts produced so far.
    pub fn transcript(&self) -> &[crate::output::OutputPart] {
        self.default.flow.output.transcript()
    }

    /// Number of parts in the transcript.
    pub fn transcript_len(&self) -> usize {
        self.default.flow.output.transcript_len()
    }

    /// Reset the transcript read cursor to the beginning (for re-rendering).
    pub fn reset_cursor(&mut self) {
        self.default.flow.output.reset_cursor();
    }

    /// Resolve a slice of the transcript against the current line tables.
    /// Returns `(text, tags)` tuples — one per line in the resolved output.
    pub fn resolve_transcript_slice(
        &self,
        range: std::ops::Range<usize>,
    ) -> Vec<(String, Vec<String>)> {
        let transcript = self.default.flow.output.transcript();
        let end = range.end.min(transcript.len());
        let start = range.start.min(end);
        let slice = &transcript[start..end];
        let fragments = self.default.flow.output.fragments();
        crate::output::resolve_lines(
            slice,
            self.program,
            &self.line_tables,
            self.resolver.as_deref(),
            fragments,
        )
    }

    /// Re-resolve all pending choices against the current line tables.
    /// Returns the same choices that would appear in `Line::Choices`,
    /// but freshly resolved (useful after locale switch).
    pub fn pending_choices(&self) -> Vec<Choice> {
        self.default
            .flow
            .pending_choices
            .iter()
            .enumerate()
            .filter(|(_, pc)| !pc.flags.is_invisible_default)
            .map(|(i, pc)| {
                let display_text = match &pc.display {
                    ChoiceDisplay::Text(s) => s.clone(),
                    ChoiceDisplay::Fragment(idx) => self.default.flow.output.resolve_fragment(
                        *idx,
                        self.program,
                        &self.line_tables,
                        self.resolver.as_deref(),
                    ),
                };
                let display_text = display_text
                    .trim_matches(|c: char| c == ' ' || c == '\t')
                    .to_string();
                Choice {
                    text: display_text,
                    index: i,
                    tags: pc.tags.clone(),
                }
            })
            .collect()
    }

    /// Resolve a fragment against the current line tables.
    pub fn resolve_fragment(&self, idx: u32) -> String {
        self.default.flow.output.resolve_fragment(
            idx,
            self.program,
            &self.line_tables,
            self.resolver.as_deref(),
        )
    }

    /// Get the fragment index for a pending choice's display text, if any.
    pub fn choice_fragment_idx(&self, choice_index: usize) -> Option<u32> {
        self.default
            .flow
            .pending_choices
            .get(choice_index)
            .and_then(|pc| match &pc.display {
                ChoiceDisplay::Fragment(idx) => Some(*idx),
                ChoiceDisplay::Text(_) => None,
            })
    }

    /// Read-only access to the fragment store (for transcript serialization).
    pub fn fragments(&self) -> &[crate::output::Fragment] {
        self.default.flow.output.fragments()
    }

    /// Read-only access to the program.
    pub fn program(&self) -> &Program {
        self.program
    }

    // ── Variable access (host-facing) ───────────────────────────────

    /// Read a global variable's current value by name. `None` if no global
    /// with that name is declared. Reads the default flow's context.
    pub fn variable(&self, name: &str) -> Option<&Value> {
        let idx = self.program.global_index(name)?;
        Some(self.default_context.global(idx))
    }

    /// Set a global variable by name, returning `false` (no-op) if no global
    /// with that name is declared. Ink globals are dynamically typed, so the
    /// host is responsible for passing a sensibly-typed value.
    pub fn set_variable(&mut self, name: &str, value: Value) -> bool {
        match self.program.global_index(name) {
            Some(idx) => {
                self.default_context.set_global(idx, value);
                true
            }
            None => false,
        }
    }

    /// Set the RNG seed for the default flow's context. Seeding makes
    /// `RANDOM`/shuffle output reproducible — set it before running (or after
    /// a reset) so two runs of the same story on different machines match.
    pub fn set_rng_seed(&mut self, seed: i32) {
        self.default_context.set_rng_seed(seed);
    }

    // ── Pausable stepping (async externals) ─────────────────────────

    /// Advance the default flow by one step with a custom handler, surfacing a
    /// deferred external as [`StepOutcome::AwaitingExternal`] rather than
    /// erroring (unlike [`continue_single_with`](Self::continue_single_with)).
    ///
    /// On `AwaitingExternal`, resolve the pending call
    /// ([`resolve_external`](Self::resolve_external), or
    /// [`invoke_fallback`](Self::invoke_fallback)) and call `advance_with` again
    /// to resume. Inspect the pending call via
    /// [`pending_external_name`](Self::pending_external_name) /
    /// [`pending_external_args`](Self::pending_external_args).
    pub fn advance_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<StepOutcome, RuntimeError> {
        let resolver = self.resolver.as_deref();
        self.default.advance::<R>(
            self.program,
            &self.line_tables,
            &mut self.default_context,
            handler,
            resolver,
        )
    }

    /// Name of the external the default flow is paused on, if any.
    #[must_use]
    pub fn pending_external_name(&self) -> Option<&str> {
        self.default.pending_external_name(self.program)
    }

    /// Arguments of the external the default flow is paused on.
    #[must_use]
    pub fn pending_external_args(&self) -> &[Value] {
        self.default.pending_external_args()
    }

    /// Evaluate an ink function by name from engine code, returning its value.
    ///
    /// Runs out-of-band on the default flow: output is isolated (the visible
    /// story is untouched), and the call completes synchronously. Externals the
    /// function calls are resolved inline by `handler`; an external the handler
    /// defers ([`ExternalResult::Pending`]) can't be resolved in a synchronous
    /// call and yields [`RuntimeError::AsyncExternalInCall`] (the paused eval is
    /// cleaned up first).
    ///
    /// # Errors
    /// [`RuntimeError::FunctionNotFound`] for an unknown name;
    /// [`RuntimeError::AsyncExternalInCall`] if a called external defers; plus
    /// any runtime error raised during evaluation.
    pub fn call_function(
        &mut self,
        name: &str,
        args: &[Value],
        handler: &dyn ExternalFnHandler,
    ) -> Result<Value, RuntimeError> {
        let container_idx = self
            .program
            .find_address(name)
            .ok_or_else(|| RuntimeError::FunctionNotFound(name.to_owned()))?
            .0;
        let resolver = self.resolver.as_deref();
        let outcome = self.default.begin_function_eval::<R>(
            self.program,
            &self.line_tables,
            &mut self.default_context,
            handler,
            container_idx,
            args,
            resolver,
        )?;
        match outcome {
            FunctionEval::Returned(value) => Ok(value),
            FunctionEval::AwaitingExternal => {
                let name = self
                    .default
                    .pending_external_name(self.program)
                    .map_or_else(|| name.to_owned(), ToOwned::to_owned);
                self.default
                    .abort_eval(self.program, &self.line_tables, resolver);
                Err(RuntimeError::AsyncExternalInCall(name))
            }
        }
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

    /// Like [`continue_single`](Self::continue_single) but with a
    /// [`WriteObserver`] that receives notifications for every state mutation.
    pub fn continue_single_observed(
        &mut self,
        observer: &mut dyn WriteObserver,
    ) -> Result<Line, RuntimeError> {
        use crate::state::ObservedContext;
        let mut obs_ctx = ObservedContext::new(&mut self.default_context, observer);
        let resolver = self.resolver.as_deref();
        self.default.step_single_line::<R>(
            self.program,
            &self.line_tables,
            &mut obs_ctx,
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

    /// Move the default flow's play head to a named knot/stitch path — ink's
    /// `ChoosePathString` equivalent. The current flow is force-completed
    /// (callstack reset, pending choices cleared), the jump counts as a visit
    /// to the target exactly like a `-> path` divert, and subsequent
    /// [`continue_single`](Self::continue_single) /
    /// [`continue_maximally`](Self::continue_maximally) calls run from there.
    /// See [`FlowInstance::choose_path_string`] for full semantics.
    ///
    /// # Errors
    /// [`UnknownPath`](RuntimeError::UnknownPath) for an unknown path;
    /// [`JumpWhileAwaitingExternal`](RuntimeError::JumpWhileAwaitingExternal)
    /// if the flow is parked on an unresolved external call;
    /// [`AlreadyEvaluatingFunction`](RuntimeError::AlreadyEvaluatingFunction)
    /// if an engine→ink function evaluation is in progress.
    pub fn choose_path_string(&mut self, path: &str) -> Result<(), RuntimeError> {
        self.default
            .choose_path_string(self.program, &mut self.default_context, path)
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

    /// A structured, name-resolved snapshot of the current runtime state for
    /// the studio State View: status, current location, globals, call stack,
    /// visit counts, pending choices, and rng. Read-only; built on demand and
    /// not on any hot path. See [`DebugSnapshot`](crate::DebugSnapshot).
    #[must_use]
    pub fn debug_snapshot(&self) -> crate::DebugSnapshot {
        use crate::debug::{
            DebugChoice, DebugFrame, DebugGlobal, DebugRng, DebugSnapshot, DebugVisit, NameResolver,
        };

        let flow = &self.default.flow;
        let ctx = &self.default_context;
        let resolver = NameResolver::new(self.program);

        let status = match self.default.status {
            StoryStatus::Active => "active",
            StoryStatus::WaitingForChoice => "waiting_for_choice",
            StoryStatus::Done => "done",
            StoryStatus::Ended => "ended",
        };

        let thread = flow.current_thread();

        // Nearest named container the cursor is currently in (innermost-first).
        let resolve_frame_location = |frame: &CallFrame| {
            frame
                .container_stack
                .iter()
                .rev()
                .find_map(|cp| resolver.container_path(cp.container_idx))
                .map(str::to_owned)
        };

        let current_location = thread.call_stack.last().and_then(resolve_frame_location);

        // Globals, skipping unnamed slots.
        let globals = ctx
            .globals
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                self.program.global_slot_name(i).map(|name| DebugGlobal {
                    name: name.to_owned(),
                    value: resolver.format_value(value),
                })
            })
            .collect();

        // Call stack, innermost (current) frame first.
        let depth = thread.call_stack.len();
        let mut call_stack = Vec::with_capacity(depth);
        for i in (0..depth).rev() {
            if let Some(frame) = thread.call_stack.get(i) {
                let kind = match frame.frame_type {
                    CallFrameType::Root => "root",
                    CallFrameType::Function => "function",
                    CallFrameType::Tunnel => "tunnel",
                    CallFrameType::Thread => "thread",
                    CallFrameType::External => "external",
                    CallFrameType::FunctionEvalFromGame => "eval",
                };
                call_stack.push(DebugFrame {
                    kind,
                    location: resolve_frame_location(frame),
                    temps: frame.temps.len(),
                });
            }
        }

        // Visit counts, resolved and sorted by path for determinism.
        let mut visit_counts: Vec<DebugVisit> = ctx
            .visit_counts
            .iter()
            .filter_map(|(id, &count)| {
                resolver.def_path(*id).map(|path| DebugVisit {
                    path: path.to_owned(),
                    count,
                })
            })
            .collect();
        visit_counts.sort_by(|a, b| a.path.cmp(&b.path));

        // Pending choices: visible texts (resolved) paired with target paths.
        let visible_targets: Vec<DefinitionId> = flow
            .pending_choices
            .iter()
            .filter(|pc| !pc.flags.is_invisible_default)
            .map(|pc| pc.target_id)
            .collect();
        let pending_choices = self
            .pending_choices()
            .into_iter()
            .enumerate()
            .map(|(i, ch)| DebugChoice {
                text: ch.text,
                target: visible_targets
                    .get(i)
                    .and_then(|id| resolver.def_path(*id))
                    .map(str::to_owned),
            })
            .collect();

        DebugSnapshot {
            status,
            current_location,
            turn_index: ctx.turn_index,
            globals,
            call_stack,
            visit_counts,
            pending_choices,
            rng: DebugRng {
                seed: ctx.rng_seed,
                previous: ctx.previous_random,
            },
        }
    }

    // ── Testing / instrumentation API ───────────────────────────────

    /// Dump the current execution state for debugging.
    ///
    /// Returns a human-readable summary of the call stack, current position,
    /// value stack, output buffer, globals, and pending choices.
    #[cfg(feature = "testing")]
    pub fn debug_state(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let flow = &self.default.flow;
        let ctx = &self.default_context;

        let _ = writeln!(out, "=== Story Debug State ===");
        let _ = writeln!(out, "status: {:?}", self.default.status);

        // Current position
        let thread = flow.current_thread();
        if let Some(frame) = thread.call_stack.last()
            && let Some(cp) = frame.container_stack.last()
        {
            let id = self.program.container(cp.container_idx).id;
            let _ = writeln!(
                out,
                "position: container_idx={} id={id:?} offset={}",
                cp.container_idx, cp.offset,
            );
        }

        // Call stack
        let depth = thread.call_stack.len();
        let _ = writeln!(out, "\ncall stack ({depth} frames):");
        for i in 0..depth {
            if let Some(frame) = thread.call_stack.get(i) {
                let ret = frame
                    .return_address
                    .map(|r| format!("idx={} off={}", r.container_idx, r.offset));
                let _ = writeln!(
                    out,
                    "  [{i}] {:?} ret={} temps={} containers={}",
                    frame.frame_type,
                    ret.as_deref().unwrap_or("none"),
                    frame.temps.len(),
                    frame.container_stack.len(),
                );
                for (j, cp) in frame.container_stack.iter().enumerate() {
                    let id = self.program.container(cp.container_idx).id;
                    let _ = writeln!(
                        out,
                        "       container_stack[{j}]: idx={} id={id:?} off={}",
                        cp.container_idx, cp.offset,
                    );
                }
            }
        }

        // Value stack
        let _ = writeln!(out, "\nvalue stack ({}):", flow.value_stack.len());
        for (i, v) in flow.value_stack.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {v:?}");
        }

        // Output buffer (unread transcript)
        let unread_start = flow.output.cursor;
        let transcript = &flow.output.transcript[unread_start..];
        let _ = writeln!(
            out,
            "\noutput buffer (cursor={unread_start}, {} unread parts):",
            transcript.len(),
        );
        for (i, part) in transcript.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {part:?}");
        }

        // Globals
        let _ = writeln!(out, "\nglobals:");
        for (i, v) in ctx.globals.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "global count fits in u32")]
            if let Some(name) = self.program.global_name(i as u32) {
                let _ = writeln!(out, "  {name} = {v:?}");
            }
        }

        // Flow flags
        let _ = writeln!(out, "\nskipping_choice: {}", flow.skipping_choice);

        // Pending choices
        let _ = writeln!(out, "\npending choices ({}):", flow.pending_choices.len());
        for (i, c) in flow.pending_choices.iter().enumerate() {
            let _ = writeln!(out, "  [{i}] {:?} -> {:?}", c.display, c.target_id);
        }

        out
    }

    /// Returns whether the last execution cycle ended with a safe exit
    /// (explicit `-> DONE` opcode). If false after a `Done` line, the
    /// story ran out of content.
    #[cfg(feature = "testing")]
    pub fn did_safe_exit(&self) -> bool {
        self.default.flow.did_safe_exit
    }

    /// Returns whether the last execution cycle passed through an empty
    /// choice set (a `Yield` opcode with no pending choices).
    #[cfg(feature = "testing")]
    pub fn did_unsafe_yield(&self) -> bool {
        self.default.flow.did_unsafe_yield
    }

    /// Execute a single VM step and return a debug trace of what happened.
    ///
    /// Returns `(opcode_description, container_idx, offset_before)` or None
    /// if the step didn't decode an opcode (frame exhaustion, thread completion, etc).
    #[cfg(feature = "testing")]
    pub fn step_once(&mut self) -> Result<Option<(String, u32, usize)>, RuntimeError> {
        use brink_format::Opcode;

        let flow = &self.default.flow;
        let thread = flow.current_thread();

        // Capture position before step
        let pre_info = thread.call_stack.last().and_then(|frame| {
            frame.container_stack.last().map(|pos| {
                let container = self.program.container(pos.container_idx);
                if pos.offset < container.bytecode.len() {
                    let mut off = pos.offset;
                    let op = Opcode::decode(&container.bytecode, &mut off).ok();
                    (pos.container_idx, pos.offset, op)
                } else {
                    (pos.container_idx, pos.offset, None)
                }
            })
        });

        // Execute one step
        let _result = vm::step::<R>(
            &mut self.default.flow,
            self.program,
            &self.line_tables,
            &mut self.default_context,
            &mut self.default.stats,
            self.resolver.as_deref(),
        )?;

        match pre_info {
            Some((ci, off, Some(op))) => Ok(Some((format!("{op:?}"), ci, off))),
            Some((ci, off, None)) => Ok(Some(("(end of container)".to_string(), ci, off))),
            None => Ok(None),
        }
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
