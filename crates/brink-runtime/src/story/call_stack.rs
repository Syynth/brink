//! Low-level flow mechanics: [`CallStack`], [`Flow`], and their supporting
//! types (call frames, threads, pending choices).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use brink_format::{ChoiceFlags, DefinitionId, Value};

use crate::error::{RanOutOfContentCause, RuntimeError};
use crate::output::{OutputBuffer, OutputMark};

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

/// Classify *why* execution ran out of content, from the exhausted frame's
/// type and whether the call stack could pop at all at that instant.
/// Mirrors C#'s `Story.Continue()` selection (`Story.cs`): a tunnel or
/// function frame gets its own message; a stack that can't pop at all (only
/// the root frame remains) is the plain case; anything else (a `Thread`
/// boundary, an in-progress `FunctionEvalFromGame` frame) is the "unknown
/// reason" backstop — a call-stack shape well-formed compiler output should
/// never produce. Called from [`crate::vm::handle_frame_exhaustion`] at the
/// exact moment a frame's content is discovered exhausted — the same
/// instant C# reads `callStack.CanPop` — before this runtime's own
/// exhaustion recovery (which, unlike C#, always pops the exhausted frame)
/// can change the stack's shape out from under a later read.
pub(crate) fn classify_ran_out_of_content(
    frame_type: CallFrameType,
    can_pop: bool,
) -> RanOutOfContentCause {
    if can_pop && frame_type == CallFrameType::Tunnel {
        RanOutOfContentCause::Tunnel
    } else if can_pop && frame_type == CallFrameType::Function {
        RanOutOfContentCause::Function
    } else if can_pop {
        RanOutOfContentCause::Unknown
    } else {
        RanOutOfContentCause::Plain
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    pub return_address: Option<ContainerPosition>,
    pub temps: Vec<Value>,
    /// Parallel to `temps`: `temps_written[i]` is `true` once slot `i` has
    /// been the target of a real write (`DeclareTemp`, `SetTemp`, the
    /// `TempPointer` write-through target, or the `as`-binding store) —
    /// never set merely because `temps` grew to cover the index.
    ///
    /// Issue #3354's `GetTemp` fallback (see `vm.rs`) needs this because
    /// `Value::Null` is not a reliable "never written" marker: it is also
    /// the padding `temps.push(Value::Null)` uses when growing the vector
    /// to a new highest index, AND it is the value a real, completed
    /// `DeclareTemp` legitimately stores when its initializer itself
    /// evaluates to `Null` (a void-returning function assigned into a
    /// temp). Keying the fallback on the *value* conflated those two
    /// cases; keying on this bitmap does not.
    pub temps_written: Vec<bool>,
    pub container_stack: Vec<ContainerPosition>,
    pub frame_type: CallFrameType,
    /// For `External` frames: the `DefinitionId` of the external function,
    /// used to look up the fallback container if no binding is registered.
    pub external_fn_id: Option<DefinitionId>,
    /// For `Function` frames: where the active output target stood at call
    /// time ([`OutputMark`]). On return, trailing whitespace is trimmed
    /// back to this point — matching the C# runtime's
    /// `TrimWhitespaceFromFunctionEnd` — and while the function has
    /// produced no content past it, a newline it emits is dropped
    /// (`functionStartInOutputStream`, issue #3519).
    pub function_output_start: Option<OutputMark>,
}

impl CallFrame {
    /// Write `val` into temp slot `idx`, growing both `temps` and
    /// `temps_written` as needed, and marking the slot written. The single
    /// path every real temp-slot store in the VM funnels through, so
    /// `GetTemp`'s "was this ever written" check (issue #3354) stays
    /// accurate without every call site having to remember the bitmap.
    pub fn write_temp(&mut self, idx: usize, val: Value) {
        while self.temps.len() <= idx {
            self.temps.push(Value::Null);
        }
        while self.temps_written.len() <= idx {
            self.temps_written.push(false);
        }
        self.temps[idx] = val;
        self.temps_written[idx] = true;
    }

    /// Whether temp slot `idx` has ever been the target of [`Self::write_temp`].
    /// An index past the end of `temps_written` was never written.
    #[must_use]
    pub fn is_temp_written(&self, idx: usize) -> bool {
        self.temps_written.get(idx).copied().unwrap_or(false)
    }
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
        // A mutable frame is about to change: a snapshot taken before
        // this write no longer describes the stack (issue #3528 — a temp
        // written after `<- thread` forked the stack was missing from the
        // next choice's fork, served from the stale cache).
        self.cached_snapshot = None;
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
        // See `last_mut`: any mutable access stales the cached snapshot.
        self.cached_snapshot = None;
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
    /// Has a `Step::Line` been handed out since this turn began (issue
    /// #3533)? Set by `make_output_line`, cleared wherever `next_block_id`
    /// starts a fresh run (a choice, a resume from `Done`, a host jump).
    /// Decides whether trailing blank lines at a yield are ink's dropped
    /// lookahead (yes) or a turn's first — and kept — `Continue` (no).
    pub line_delivered_this_turn: bool,
    /// The call-stack-derived cause captured the moment execution hit the
    /// content-exhaustion boundary ([`crate::vm::handle_frame_exhaustion`])
    /// that produced the terminal `Done` — mirrors C#'s inline
    /// `CanPop(Tunnel)`/`CanPop(Function)`/`!canPop` selection (`Story.cs`)
    /// at the instant it happens, before this runtime's own frame unwinding
    /// (which, unlike C#, always pops the exhausted frame — see the type's
    /// own docs) can erase the evidence. Read by
    /// [`FlowInstance::advance_with_limit`](crate::story::FlowInstance::advance_with_limit)'s
    /// deferred "ran out of content" fault one `continue_single` call
    /// later. Written *only* on the exhaustion paths that themselves return
    /// `Done` — an exhaustion that instead resumes execution (a completed
    /// thread with a parent to fall back to, a popped frame with content
    /// still below it) never touches this field, so a transient exhaustion
    /// elsewhere on the same flow (e.g. a `Story::call_function` boundary
    /// evaluating a function that calls a void helper) can't clobber a
    /// cause an earlier, still-pending exhaustion already recorded. Not
    /// cleared between cycles like the two flags above it — it is
    /// meaningless unless `did_safe_exit` is `false` at the same `Done`,
    /// which is the only condition under which it is ever read.
    pub ran_out_of_content_cause: RanOutOfContentCause,
    /// The dev/prod execution mode (NS-A4, [`ExecMode`]). A host/build
    /// knob, not story state — never persisted; defaults to
    /// [`ExecMode::Dev`].
    pub exec_mode: ExecMode,
    /// In-flight nested **pure-callback** evaluation state — see
    /// [`PureCallbackState`].
    pub pure_callback: PureCallbackState,
    /// The [`super::types::BlockId`] stamped onto the next `Step::Line`
    /// produced by this flow — counts uninterrupted runs of adjacent
    /// content (`docs/prose-dialect-spec.md` §3.7/§8d.2). Bumped whenever a
    /// new run begins: after a choice is selected, after resuming from
    /// `Done`, and on a host-directed jump (`choose_path_string`, which is
    /// itself specified as force-completing the current flow like `->
    /// DONE`).
    ///
    /// ⚠ **Persistence is boundary-dependent, not uniformly "never" (the
    /// 2026-08-05 ruling on #2108, `docs/decision-log.md`).** This field's
    /// doc previously said outright that it is *never* persisted — that
    /// claim was true only for the *ordinary* save (`Story::save_state`/
    /// `load_state`, `brink_runtime::save`'s free functions): that save
    /// captures game state only, the host re-enters at a known knot on
    /// load, and a fresh `0` there is genuinely harmless because nothing
    /// ever compares a block id *across* that boundary — a brand-new run
    /// starting fresh is exactly what re-entering a knot means. **That part
    /// of the old claim still holds and this field is still not part of
    /// `SaveState` itself.**
    ///
    /// It stopped being true unconditionally once element attachment
    /// (`@[convention(..., attach = X)]`, #2108) could leave a run *open*
    /// across a suspension: a block is not just lines — executable
    /// statements can interleave with an attach-scoped dialogue run, and
    /// `Step::Suspended` is deliberately not one of the run-terminators
    /// above (an `await` can fire mid-run). A flow parked there is resumed
    /// from its exact execution position via `brink_format::SuspendedFlow`
    /// (the `FlowFrame`, `docs/flow-suspension-spec.md` §2/§9) — genuinely
    /// continuing the SAME run, not re-entering a knot from the top — so a
    /// numbering restart at `0` there would silently collide with (or just
    /// diverge from) the pre-park sequence, and the run's element data
    /// (`crate::output::OutputBuffer`'s carried-forward attachment state)
    /// would reset to empty, dropping the attributed speaker. For that
    /// boundary only, this value **is** persisted — see
    /// `SuspendedFlow::next_block_id`/`SuspendedFlow::pending_element`'s own
    /// docs — even though the ordinary game-state save still never touches
    /// it.
    pub next_block_id: u64,
    /// A terminal ([`super::types::Step`] variant with no line payload)
    /// computed but not yet delivered, because its trailing content needed
    /// to go out first as an ordinary `Step::Line` (terminals carry no
    /// text — `docs/prose-dialect-spec.md` §7). Consumed and returned bare
    /// on the very next `advance` call, with no VM stepping — see
    /// [`PendingTerminal`] for the invalidation invariant this type
    /// enforces.
    pub pending_terminal: PendingTerminal,
    /// Non-fatal conditions this flow reported while running (issue #3354),
    /// drained by the host through
    /// [`FlowInstance::take_runtime_warnings`](crate::FlowInstance::take_runtime_warnings).
    ///
    /// Lives on the flow rather than on [`super::Stats`] because it is
    /// execution *output*, not a counter, and because every VM site that
    /// can raise one already holds `&mut Flow` — no new parameter has to be
    /// threaded through `vm::step` for it. Capped at
    /// [`crate::RUNTIME_WARNING_CAP`] entries between drains
    /// ([`Self::warn`]).
    pub warnings: Vec<crate::error::RuntimeWarning>,
}

/// A terminal computed for the current run but held back because its
/// trailing content had to flush first as its own `Step::Line` (terminals
/// carry no text of their own — `docs/prose-dialect-spec.md` §7). Stamped
/// with the [`Flow::next_block_id`] value current at the moment it was
/// computed.
///
/// **Invariant this type exists to enforce** (the bug found in #1684's
/// review, filed as #2104): a stashed terminal must never be handed back
/// after a host-directed jump or choice has moved execution somewhere else
/// in the meantime — `choose`/`choose_path_string`/`choose_path_string_with_args`
/// all force-complete the current run and begin a fresh one (bumping
/// `next_block_id`, per that field's own doc comment: "Bumped whenever a
/// new run begins: after a choice is selected, after resuming from `Done`,
/// and on a host-directed jump"). [`take_if_current`](Self::take_if_current)
/// compares the stash's stamp against the block id current *at read time*
/// and silently discards a stale stash rather than returning it — so the
/// invariant holds **by construction**: any call site that begins a new run
/// only has to keep bumping `next_block_id` for its own reasons (block-id
/// correctness for `Step::Line`, already required whether or not this type
/// existed), and pending-terminal invalidation falls out for free. No call
/// site needs its own `= None` clear, so a future host-directed mutation
/// (a rewind, a fast-forward) that begins a new run cannot reintroduce this
/// bug by forgetting one.
///
/// `Story::load_state`/the free `load_state` function are **not** part of
/// this invariant's surface: they reconcile only game state (globals,
/// visit/turn counts) into a `ContextAccess`, never touching `Flow` or its
/// `next_block_id`/`pending_terminal` at all — so they cannot leave a stale
/// stash behind, and need no clear of their own. See
/// `docs/runtime-spec.md`'s pending-terminal section.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingTerminal(Option<(u64, super::types::Step)>);

impl PendingTerminal {
    /// Stash `terminal`, stamped with the run (`next_block_id`) it was
    /// computed under.
    pub(crate) fn stash(&mut self, block_id: u64, terminal: super::types::Step) {
        self.0 = Some((block_id, terminal));
    }

    /// Take the stashed terminal iff its stamp matches `current_block_id` —
    /// i.e. no new run has begun since it was stashed. Always empties the
    /// slot (fresh or stale), so a stale stash can never be read twice.
    pub(crate) fn take_if_current(&mut self, current_block_id: u64) -> Option<super::types::Step> {
        self.0
            .take()
            .and_then(|(stamp, terminal)| (stamp == current_block_id).then_some(terminal))
    }
}

/// Transient bookkeeping for in-flight nested callback evaluations: a
/// `sort_by`/`sorted_by` comparator (NS-A4), a pure fn-value verb's callback
/// (`map`/`filter`/`fold`/`filter_map` — `docs/stdlib-spec.md` §4, issue
/// #1679), or an effectful fn-value verb's callback (`each`/`map_each`,
/// issue #1679 slice 2). All three re-enter [`crate::vm::step`] from inside
/// a single opcode, so they share one counter — `effectful` is what splits
/// the two runtime contracts without splitting the bookkeeping.
///
/// Never persisted (a callback always completes within the opcode that
/// started it). The depth guards Rust stack recursion when a callback itself
/// runs a callback verb, regardless of which contract; the verb name is
/// what the dev-mode world-write guard reports.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PureCallbackState {
    /// Number of nested callback evaluations currently on the Rust stack
    /// (pure and effectful alike).
    pub depth: u16,
    /// The source spelling of the innermost verb whose callback is running
    /// (`"sort_by"`, `"map"`, `"each"`, …). Only meaningful while
    /// `depth > 0`; the default `""` is never read.
    pub verb: &'static str,
    /// Whether the innermost running callback is the **effectful** contract
    /// (`each`/`map_each`): output reaches the transcript instead of being
    /// captured, and [`crate::vm`]'s dev-mode world-write guard is disarmed
    /// for it. `false` for the pure quartet and for `sort_by`/`sorted_by`.
    /// Only meaningful while `depth > 0`.
    pub effectful: bool,
}

impl Flow {
    /// Record a non-fatal runtime condition, up to
    /// [`crate::RUNTIME_WARNING_CAP`] entries between drains. Beyond the
    /// cap the warning is dropped rather than growing the list without
    /// bound — the earlier entries already say what the author needs.
    pub fn warn(&mut self, warning: crate::error::RuntimeWarning) {
        if self.warnings.len() < crate::RUNTIME_WARNING_CAP {
            self.warnings.push(warning);
        }
    }

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
        let output_start = self.output.mark();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::Step;

    // ── PendingTerminal ───────────────────────────────────────────────────
    //
    // Unit-level coverage for the invalidation invariant itself (see
    // `PendingTerminal`'s own doc comment): a stash is only ever handed
    // back if the caller's current block id still matches the one it was
    // stamped with, and a read — fresh or stale — always empties the slot.
    // The `FlowInstance`-level regression tests for the actual bug this
    // guards against (a stashed terminal replaying after a host jump or a
    // choice) live in `brink-test-harness/tests/jump_to_path.rs`
    // (`jump_right_after_content_line_does_not_replay_stale_terminal`,
    // `choose_right_after_content_line_does_not_replay_stale_terminal`).

    /// A stash read back under the SAME block id it was stamped with (the
    /// ordinary case: content flushes, then the terminal is delivered on
    /// the very next call, with no run boundary in between) is returned.
    #[test]
    fn take_if_current_returns_a_fresh_stash() {
        let mut pending = PendingTerminal::default();
        pending.stash(3, Step::Done);
        assert_eq!(pending.take_if_current(3), Some(Step::Done));
    }

    /// A stash read back under a LATER block id than the one it was
    /// stamped with — exactly what happens after a host-directed jump or
    /// choice, both of which bump `next_block_id` before the flow is ever
    /// asked to advance again — is discarded rather than replayed. This is
    /// the mechanism that makes the invariant hold **without** a jump/choice
    /// call site needing its own explicit `= None` clear.
    #[test]
    fn take_if_current_discards_a_stash_stamped_for_an_earlier_block() {
        let mut pending = PendingTerminal::default();
        pending.stash(3, Step::Done);
        assert_eq!(
            pending.take_if_current(4),
            None,
            "a stash stamped for block 3 must not surface once the current \
             block has moved to 4"
        );
    }

    /// Reading the slot always empties it — a stale stash discarded by one
    /// read can't somehow surface on a later read even if that later read
    /// happens to use the original stamp again.
    #[test]
    fn take_if_current_always_empties_the_slot_even_when_stale() {
        let mut pending = PendingTerminal::default();
        pending.stash(3, Step::Done);
        assert_eq!(pending.take_if_current(4), None, "first (stale) read");
        assert_eq!(
            pending.take_if_current(3),
            None,
            "the slot was already emptied by the stale read above — it must \
             not resurrect the old value just because the stamp is asked \
             for again"
        );
    }

    /// An empty slot never produces a terminal, regardless of which block
    /// id is asked for.
    #[test]
    fn take_if_current_on_an_empty_slot_is_always_none() {
        let mut pending = PendingTerminal::default();
        assert_eq!(pending.take_if_current(0), None);
    }

    /// A tunnel frame that can still pop classifies as `Tunnel` — mirrors
    /// C#'s `callStack.CanPop(PushPopType.Tunnel)` arm.
    #[test]
    fn classify_tunnel_with_can_pop_is_tunnel() {
        assert_eq!(
            classify_ran_out_of_content(CallFrameType::Tunnel, true),
            RanOutOfContentCause::Tunnel
        );
    }

    /// A function frame that can still pop classifies as `Function` —
    /// mirrors C#'s `callStack.CanPop(PushPopType.Function)` arm.
    #[test]
    fn classify_function_with_can_pop_is_function() {
        assert_eq!(
            classify_ran_out_of_content(CallFrameType::Function, true),
            RanOutOfContentCause::Function
        );
    }

    /// Any other frame type that can still pop (a `Thread` boundary, a
    /// `FunctionEvalFromGame` boundary, even `Root`/`External`) falls to
    /// the "unknown reason" backstop — mirrors C#'s final `else` arm.
    #[test]
    fn classify_other_frame_types_with_can_pop_is_unknown() {
        for frame_type in [
            CallFrameType::Root,
            CallFrameType::Thread,
            CallFrameType::External,
            CallFrameType::FunctionEvalFromGame,
        ] {
            assert_eq!(
                classify_ran_out_of_content(frame_type, true),
                RanOutOfContentCause::Unknown,
                "frame type {frame_type:?} with can_pop=true should classify as Unknown"
            );
        }
    }

    /// A call stack that can't pop at all — regardless of the exhausted
    /// frame's type — is the plain "story fell off the end" case. Mirrors
    /// C#'s `!callStack.canPop` arm, which is checked before frame-type
    /// distinctions are even considered.
    #[test]
    fn classify_cannot_pop_is_always_plain() {
        for frame_type in [
            CallFrameType::Root,
            CallFrameType::Function,
            CallFrameType::Tunnel,
            CallFrameType::Thread,
            CallFrameType::External,
            CallFrameType::FunctionEvalFromGame,
        ] {
            assert_eq!(
                classify_ran_out_of_content(frame_type, false),
                RanOutOfContentCause::Plain,
                "frame type {frame_type:?} with can_pop=false should classify as Plain"
            );
        }
    }
}
