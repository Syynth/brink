//! Low-level flow mechanics: [`CallStack`], [`Flow`], and their supporting
//! types (call frames, threads, pending choices).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use brink_format::{ChoiceFlags, DefinitionId, Value};

use crate::error::RuntimeError;
use crate::output::OutputBuffer;

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

/// The dev/prod execution mode (NS-A4, `docs/stdlib-spec.md` §4b, ruled
/// 2026-07-18): the knob that decides WHERE execution stops on an unordered
/// comparand — never WHAT values are fabricated.
///
/// The split is **fenced to placement**: it exists only where the prod
/// behavior is defined, total, and fabricates no data. Ordering contexts
/// qualify (`sort`/`sorted`/`min`/`max`; A7 adds `heap_push`): every element
/// is preserved, the order is deterministic, saves/replay are safe.
/// Fabrication never qualifies — `int("potato")`, OOB indexing stay
/// always-fault in both modes. Effect rows are mode-independent (the checker
/// doesn't know modes exist).
///
/// - [`Dev`](Self::Dev) (the default — the Rust dev-profile analogy, like
///   debug-build overflow checks): a float NaN comparand in an ordering
///   context is a turn-terminating [`RuntimeError::UnorderedComparand`]
///   fault, surfacing the upstream bug at its first ordering consumption.
/// - [`Prod`](Self::Prod): the pinned non-fabricating total order applies —
///   ordinary IEEE order with `-0 == +0` as a tie, NaN greater than
///   everything, NaN-vs-NaN ties (deliberately NOT IEEE `totalOrder`, whose
///   `-0 < +0` would split ordering from `==` on clean data). Execution
///   keeps moving.
///
/// On NaN-free data the modes agree exactly and cohere with `<`/`==`.
///
/// The knob's *home* is project config (`brink.toml` profile) with a
/// host-API override (ruled 2026-07-19; tooling wires the config side).
/// This runtime mechanism is the host-API leg: set it via
/// [`Story::set_exec_mode`] / [`FlowInstance::set_exec_mode`]. The mode is
/// a host/build knob, not story state — it is never embedded in `.inkb`
/// (mirroring `dialect`/`types`) and never persisted in saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecMode {
    /// Fault on unordered comparands (NaN) in ordering contexts.
    #[default]
    Dev,
    /// Keep moving: place NaN by the pinned non-fabricating total order.
    Prod,
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
    /// The dev/prod execution mode (NS-A4, [`ExecMode`]). A host/build
    /// knob, not story state — never persisted; defaults to
    /// [`ExecMode::Dev`].
    pub exec_mode: ExecMode,
    /// In-flight nested **pure-callback** evaluation state — see
    /// [`PureCallbackState`].
    pub pure_callback: PureCallbackState,
}

/// Transient bookkeeping for in-flight nested pure-callback evaluations:
/// a `sort_by`/`sorted_by` comparator (NS-A4) or a fn-value verb's callback
/// (`map`/`filter`/`fold` — `docs/stdlib-spec.md` §4, issue #1679). Both
/// re-enter [`crate::vm::step`] from inside a single opcode under the same
/// pure·silent contract, so they share one counter.
///
/// Never persisted (a callback always completes within the opcode that
/// started it). The depth guards Rust stack recursion when a callback itself
/// runs a callback verb; the verb name is what the dev-mode world-write
/// guard reports.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PureCallbackState {
    /// Number of pure-callback evaluations currently on the Rust stack.
    pub depth: u16,
    /// The source spelling of the innermost verb whose callback is running
    /// (`"sort_by"`, `"map"`, …). Only meaningful while `depth > 0`;
    /// the default `""` is never read.
    pub verb: &'static str,
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
