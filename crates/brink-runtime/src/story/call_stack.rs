//! Low-level flow mechanics: [`CallStack`], [`Flow`], and their supporting
//! types (call frames, threads, pending choices).

use alloc::string::String;
use alloc::vec::Vec;

use brink_format::{ChoiceFlags, DefinitionId, Value};

use core::ops::Range;

use crate::error::{RanOutOfContentCause, RuntimeError};
use crate::output::{OutputBuffer, OutputMark};

// ── Internal types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// - **External**: pushed by `CallExternal`. Holds popped arguments in
///   `temps` and the external function's [`DefinitionId`] in
///   `external_fn_id`. The orchestration layer resolves it (binding or
///   fallback) before the VM resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallFrameType {
    Root,
    Function,
    Tunnel,
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
/// the root frame remains) is the plain case; anything else (an in-progress
/// `FunctionEvalFromGame` frame, say) is the "unknown reason" backstop — a
/// call-stack shape well-formed compiler output should never produce. Called from [`crate::vm::handle_frame_exhaustion`] at the
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

/// One activation on a [`CallStack`]: the frame *header*. Its temp slots and
/// container positions live in the stack's shared, contiguous storage,
/// addressed by the two base offsets here (see [`CallStack`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallFrame {
    pub return_address: Option<ContainerPosition>,
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
    /// Start of this frame's temp slots in [`CallStack::temps`] (and, in
    /// parallel, [`CallStack::temps_written`]). Set by `CallStack::push`;
    /// the segment ends where the next frame's begins, or at the end of the
    /// storage for the top frame.
    temps_base: usize,
    /// Start of this frame's container positions in
    /// [`CallStack::containers`]; same segment rule as `temps_base`.
    containers_base: usize,
}

impl CallFrame {
    /// A frame header. The storage bases are assigned when the frame is
    /// pushed, so a header is inert until then.
    pub fn new(
        frame_type: CallFrameType,
        return_address: Option<ContainerPosition>,
        function_output_start: Option<OutputMark>,
    ) -> Self {
        Self {
            return_address,
            frame_type,
            external_fn_id: None,
            function_output_start,
            temps_base: 0,
            containers_base: 0,
        }
    }

    /// An `External` frame header, parked on `fn_id` until the host
    /// resolves the call.
    pub fn external(fn_id: DefinitionId, return_address: Option<ContainerPosition>) -> Self {
        Self {
            external_fn_id: Some(fn_id),
            ..Self::new(CallFrameType::External, return_address, None)
        }
    }
}

/// Initial reservation for a thread's temp-slot storage, in slots.
const TEMPS_RESERVE: usize = 64;
/// Initial reservation for a thread's container-position storage.
const CONTAINERS_RESERVE: usize = 16;
/// Initial reservation for a thread's frame headers.
const FRAMES_RESERVE: usize = 8;

/// A thread's call stack, laid out as one contiguous stack per kind of
/// per-frame data.
///
/// Every frame's temp slots sit end to end in `temps` (with `temps_written`
/// in parallel), and every frame's container positions sit end to end in
/// `containers`; a [`CallFrame`] records only where its segments begin.
/// Pushing a frame records the current lengths as its bases and allocates
/// nothing; popping truncates back to them. Each thread reserves its
/// storage once at creation, so a function call at steady state is three
/// integer stores and no allocator traffic at all.
///
/// This replaced a `Vec<CallFrame>` whose every frame owned three
/// separately heap-allocated `Vec`s — a `container_stack` of one element,
/// `temps`, and `temps_written` — which made every function call three
/// `malloc`s and three `free`s: 83% of all heap blocks on `crucible-8`
/// and 13.7% of its instructions in the allocator (DHAT and callgrind,
/// measured after #3569). The per-step instruction-pointer advance also
/// loses a pointer chase: the top container is the last element of one
/// `Vec` instead of the last element of a `Vec` inside the last element of
/// another.
///
/// Frame segments are sized lazily, as temps are declared by slot index
/// (the format carries no per-container slot count). The top frame grows
/// by pushing; a lower frame — reachable only through a `ref` parameter's
/// [`Value::TempPointer`] — grows by inserting at its segment's end and
/// shifting the bases of every frame above it. That path is essentially
/// never taken (a `ref` names a slot its owner has already declared) and
/// is O(stack) when it is; it exists so the shared layout is exact in every
/// case, not just the common one.
///
/// A thread fork clones this whole structure: four `Vec` clones per fork,
/// where the old layout paid one per frame per `Vec`.
#[derive(Debug, Clone)]
pub(crate) struct CallStack {
    frames: Vec<CallFrame>,
    /// Every frame's temp slots, end to end.
    temps: Vec<Value>,
    /// Parallel to `temps`: `temps_written[i]` is `true` once slot `i` has
    /// been the target of a real write (`DeclareTemp`, `SetTemp`, the
    /// `TempPointer` write-through target, or the `as`-binding store) —
    /// never set merely because a segment grew to cover the index.
    ///
    /// Issue #3354's `GetTemp` fallback (see `vm.rs`) needs this because
    /// `Value::Null` is not a reliable "never written" marker: it is also
    /// the padding a segment grows with to reach a new highest index, AND
    /// it is the value a real, completed `DeclareTemp` legitimately stores
    /// when its initializer itself evaluates to `Null` (a void-returning
    /// function assigned into a temp). Keying the fallback on the *value*
    /// conflated those two cases; keying on this bitmap does not.
    temps_written: Vec<bool>,
    /// Every frame's container positions, end to end.
    containers: Vec<ContainerPosition>,
}

impl CallStack {
    /// A stack holding `root` as its only frame, executing at `entry` (or
    /// nowhere, for a reset stack that is done).
    pub fn new(root: CallFrame, entry: Option<ContainerPosition>) -> Self {
        let mut stack = Self {
            frames: Vec::with_capacity(FRAMES_RESERVE),
            temps: Vec::with_capacity(TEMPS_RESERVE),
            temps_written: Vec::with_capacity(TEMPS_RESERVE),
            containers: Vec::with_capacity(CONTAINERS_RESERVE),
        };
        stack.push(root, entry);
        stack
    }

    // ── Frames ─────────────────────────────────────────────────────────

    /// Push `frame`, executing at `entry` if given. Its segments begin at
    /// the current end of the shared storage.
    pub fn push(&mut self, mut frame: CallFrame, entry: Option<ContainerPosition>) {
        frame.temps_base = self.temps.len();
        frame.containers_base = self.containers.len();
        self.frames.push(frame);
        if let Some(pos) = entry {
            self.containers.push(pos);
        }
    }

    /// Push an `External` frame holding `args` as its temp slots, every one
    /// of them written by construction (they are supplied values, not
    /// padding). It executes nowhere until resolved.
    pub fn push_with_args(&mut self, frame: CallFrame, args: Vec<Value>) {
        self.push(frame, None);
        self.temps_written
            .resize(self.temps.len() + args.len(), true);
        self.temps.extend(args);
    }

    /// Pop the top frame, releasing its segments.
    pub fn pop(&mut self) -> Option<CallFrame> {
        let frame = self.frames.pop()?;
        self.temps.truncate(frame.temps_base);
        self.temps_written.truncate(frame.temps_base);
        self.containers.truncate(frame.containers_base);
        Some(frame)
    }

    pub fn last(&self) -> Option<&CallFrame> {
        self.frames.last()
    }

    pub fn last_mut(&mut self) -> Option<&mut CallFrame> {
        self.frames.last_mut()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Depth index of the top frame, if any.
    pub fn top_depth(&self) -> Option<usize> {
        self.frames.len().checked_sub(1)
    }

    pub fn get(&self, depth: usize) -> Option<&CallFrame> {
        self.frames.get(depth)
    }

    // ── Temp slots ─────────────────────────────────────────────────────

    /// The storage range of frame `depth`'s temp segment.
    fn temp_range(&self, depth: usize) -> Option<Range<usize>> {
        let start = self.frames.get(depth)?.temps_base;
        let end = self
            .frames
            .get(depth + 1)
            .map_or(self.temps.len(), |next| next.temps_base);
        Some(start..end)
    }

    /// Frame `depth`'s temp slots; empty for a frame that does not exist.
    pub fn temps(&self, depth: usize) -> &[Value] {
        self.temp_range(depth)
            .and_then(|r| self.temps.get(r))
            .unwrap_or(&[])
    }

    /// Temp slot `slot` of frame `depth`, if the segment covers it.
    pub fn temp(&self, depth: usize, slot: usize) -> Option<&Value> {
        self.temps(depth).get(slot)
    }

    /// Whether temp `slot` of frame `depth` has ever been the target of
    /// [`Self::write_temp`]. A slot past the segment's end was never written.
    #[must_use]
    pub fn is_temp_written(&self, depth: usize, slot: usize) -> bool {
        self.temp_range(depth)
            .and_then(|r| self.temps_written.get(r))
            .and_then(|written| written.get(slot))
            .copied()
            .unwrap_or(false)
    }

    /// Grow frame `depth`'s segment so that `slot` is covered, padding with
    /// `Value::Null` (unwritten). Returns the storage index of `slot`, or
    /// `None` if the frame does not exist.
    fn ensure_temp(&mut self, depth: usize, slot: usize) -> Option<usize> {
        let range = self.temp_range(depth)?;
        if slot < range.len() {
            return Some(range.start + slot);
        }
        let grow = slot + 1 - range.len();
        if depth + 1 == self.frames.len() {
            // The top frame: its segment ends at the end of the storage.
            self.temps.resize(range.end + grow, Value::Null);
            self.temps_written.resize(range.end + grow, false);
        } else {
            // A lower frame, reached through a `ref` parameter: open a gap
            // at its segment's end and shift everything above it. See the
            // type doc — exact, and effectively never taken.
            self.temps.splice(
                range.end..range.end,
                core::iter::repeat_n(Value::Null, grow),
            );
            self.temps_written
                .splice(range.end..range.end, core::iter::repeat_n(false, grow));
            for frame in &mut self.frames[depth + 1..] {
                frame.temps_base += grow;
            }
        }
        Some(range.start + slot)
    }

    /// Write `val` into temp `slot` of frame `depth`, growing the segment
    /// as needed, and mark the slot written. The single path every real
    /// temp-slot store in the VM funnels through, so `GetTemp`'s "was this
    /// ever written" check (issue #3354) stays accurate without every call
    /// site having to remember the bitmap. A write to a frame that does
    /// not exist is dropped.
    pub fn write_temp(&mut self, depth: usize, slot: usize, val: Value) {
        if let Some(i) = self.ensure_temp(depth, slot) {
            self.temps[i] = val;
            self.temps_written[i] = true;
        }
    }

    /// Move temp `slot` of frame `depth` out, leaving `Value::Null` behind
    /// (the written bit is untouched — `TakeTemp` leaves `Null` by design).
    /// The segment grows to cover the slot exactly as a write would; a
    /// slot that never existed yields `Null`.
    pub fn take_temp(&mut self, depth: usize, slot: usize) -> Value {
        match self.ensure_temp(depth, slot) {
            Some(i) => core::mem::replace(&mut self.temps[i], Value::Null),
            None => Value::Null,
        }
    }

    /// Move the top frame's whole temp segment out, leaving it empty.
    pub fn take_top_temps(&mut self) -> Vec<Value> {
        let Some(base) = self.frames.last().map(|f| f.temps_base) else {
            return Vec::new();
        };
        self.temps_written.truncate(base);
        self.temps.drain(base..).collect()
    }

    /// Test seam: clear the written bit of one slot, to stage the
    /// "declared but never written" state a real program reaches only
    /// through `DeclareTemp`'s padding.
    #[cfg(test)]
    pub fn clear_temp_written(&mut self, depth: usize, slot: usize) {
        if let Some(r) = self.temp_range(depth)
            && let Some(bit) = self.temps_written.get_mut(r.start + slot)
        {
            *bit = false;
        }
    }

    // ── Container positions ────────────────────────────────────────────

    /// The storage range of frame `depth`'s container segment.
    fn container_range(&self, depth: usize) -> Option<Range<usize>> {
        let start = self.frames.get(depth)?.containers_base;
        let end = self
            .frames
            .get(depth + 1)
            .map_or(self.containers.len(), |next| next.containers_base);
        Some(start..end)
    }

    /// Frame `depth`'s container positions, outermost first; empty for a
    /// frame that does not exist.
    pub fn containers(&self, depth: usize) -> &[ContainerPosition] {
        self.container_range(depth)
            .and_then(|r| self.containers.get(r))
            .unwrap_or(&[])
    }

    /// Where the top frame is executing: the innermost position of its
    /// container segment. `None` when the stack is empty or the top frame's
    /// segment is (the frame is exhausted).
    pub fn top_container(&self) -> Option<ContainerPosition> {
        let base = self.frames.last()?.containers_base;
        (self.containers.len() > base).then(|| self.containers[self.containers.len() - 1])
    }

    /// Mutable form of [`Self::top_container`].
    pub fn top_container_mut(&mut self) -> Option<&mut ContainerPosition> {
        let base = self.frames.last()?.containers_base;
        (self.containers.len() > base).then(|| {
            let last = self.containers.len() - 1;
            &mut self.containers[last]
        })
    }

    /// The top frame's container positions, outermost first.
    pub fn top_containers(&self) -> &[ContainerPosition] {
        self.frames
            .last()
            .and_then(|f| self.containers.get(f.containers_base..))
            .unwrap_or(&[])
    }

    /// Enter a nested container in the top frame.
    pub fn push_container(&mut self, pos: ContainerPosition) {
        if !self.frames.is_empty() {
            self.containers.push(pos);
        }
    }

    /// Leave the top frame's innermost container. A no-op when the frame's
    /// segment is already empty — never reaches into the frame below.
    pub fn pop_container(&mut self) -> Option<ContainerPosition> {
        let base = self.frames.last()?.containers_base;
        (self.containers.len() > base)
            .then(|| self.containers.pop())
            .flatten()
    }

    /// Replace the top frame's whole container segment with `pos`: a jump
    /// out of whatever nesting it was in.
    pub fn reset_top_containers(&mut self, pos: ContainerPosition) {
        if let Some(base) = self.frames.last().map(|f| f.containers_base) {
            self.containers.truncate(base);
            self.containers.push(pos);
        }
    }

    /// Unwind the top frame's container segment to its first `keep`
    /// positions, then set the innermost remaining position's offset — a
    /// break divert back into an enclosing container.
    pub fn unwind_top_containers(&mut self, keep: usize, offset: usize) {
        if let Some(base) = self.frames.last().map(|f| f.containers_base) {
            self.containers.truncate(base + keep);
            if let Some(top) = self.containers.get_mut(base..).and_then(<[_]>::last_mut) {
                top.offset = offset;
            }
        }
    }
}

/// A single execution thread with its own call stack.
#[derive(Debug, Clone)]
pub(crate) struct Thread {
    pub call_stack: CallStack,
    /// How many frames at the bottom of `call_stack` belong to the parent
    /// this thread was forked from — the mark that says where *this*
    /// thread's own execution begins. `0` for the root thread; for a
    /// thread spawned by `<-`, the parent's depth at the fork.
    ///
    /// This replaces the boundary `CallFrameType::Thread` frame `<-` used
    /// to push (issue #3561). That frame was never released: selecting a
    /// choice raised inside a thread installs the thread's fork wholesale
    /// (`FlowInstance::select_choice`), so the boundary rode into the main
    /// call stack and stayed there — one retained frame per turn in the
    /// ordinary `<- thread`-as-choice game loop, with every subsequent
    /// fork O(depth) against a depth that rose with the turn count. The
    /// mark belongs on the thread, which is popped whole, rather than in
    /// the stack, which gets copied and installed elsewhere.
    pub base_depth: usize,
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
    ///
    /// The fork starts with `base_depth: 0` — the root-thread value, which
    /// is what a choice fork needs, since selecting a choice installs it as
    /// the flow's only thread. `Opcode::ThreadCall` overwrites it with the
    /// parent's depth for a `<-` spawn.
    pub fn fork_thread(&mut self) -> Thread {
        Thread {
            call_stack: self.current_thread().call_stack.clone(),
            base_depth: 0,
        }
    }

    /// Is the current thread one spawned by `<-`, standing at its own base
    /// — holding no frame above the ones it inherited from its parent?
    ///
    /// This is the question the boundary `CallFrameType::Thread` frame used
    /// to answer by being on top of the stack (issue #3561, see
    /// [`Thread::base_depth`]). Two callers need it: content exhaustion
    /// here means the *thread* is done (pop it whole; never unwind into the
    /// parent's frames below), and the debugger refuses step-out there —
    /// `docs/debugger-spec.md` §4's ruled `Thread` row, "a thread is not a
    /// frame you can return from".
    pub fn at_thread_base(&self) -> bool {
        let thread = self.current_thread();
        self.can_pop_thread() && thread.call_stack.len() <= thread.base_depth
    }

    /// Read the arguments from the top External frame.
    pub fn external_args(&self) -> &[Value] {
        let stack = &self.current_thread().call_stack;
        match stack.last() {
            Some(f) if f.frame_type == CallFrameType::External => stack.temps(stack.len() - 1),
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
                && let Some(top) = self.current_thread_mut().call_stack.top_container_mut()
            {
                *top = pos;
            }
        }
    }

    /// Replace the External frame with a Function frame pointing at the
    /// fallback container. Args are pushed back onto the value stack so
    /// the fallback body's `temp=` opcodes can pop them.
    pub fn invoke_fallback(&mut self, container_idx: u32, param_slots: &[u16]) {
        let output_start = self.output.mark();
        let Some(thread) = self.threads.last_mut() else {
            return;
        };
        let stack = &mut thread.call_stack;
        if stack
            .last()
            .is_some_and(|frame| frame.frame_type == CallFrameType::External)
        {
            let args = stack.take_top_temps();
            if let Some(frame) = stack.last_mut() {
                frame.frame_type = CallFrameType::Function;
                frame.external_fn_id = None;
                frame.function_output_start = Some(output_start);
            }
            stack.reset_top_containers(ContainerPosition {
                container_idx,
                offset: 0,
            });
            // Push the external call's arguments back onto the value stack
            // and bind them into the fallback's parameter slots. `.inkb` v10
            // removed the `DeclareTemp` prologue that used to do the second
            // half; doing it here rather than writing the list straight into
            // the slots keeps the old behaviour when the argument count and
            // the fallback's arity disagree — the surplus stays on the stack.
            let depth = stack.top_depth();
            self.value_stack.extend(args);
            let last = self.threads.len() - 1;
            if let Some(depth) = depth {
                let stack = &mut self.threads[last].call_stack;
                for slot in param_slots.iter().rev() {
                    let Some(val) = self.value_stack.pop() else {
                        break;
                    };
                    stack.write_temp(depth, usize::from(*slot), val);
                }
            }
        }
    }

    /// Pop a value from the value stack.
    pub fn pop_value(&mut self) -> Result<Value, RuntimeError> {
        self.value_stack
            .pop()
            .ok_or_else(|| RuntimeError::StackUnderflow)
    }

    /// Peek at the top value without popping.
    pub fn peek_value(&self) -> Result<&Value, RuntimeError> {
        self.value_stack
            .last()
            .ok_or_else(|| RuntimeError::StackUnderflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::Step;

    // ── CallStack: the contiguous layout ─────────────────────────────────
    //
    // Every frame's temps and container positions live in one shared
    // `Vec` per kind (see `CallStack`'s doc). These pin the segment
    // arithmetic: a frame sees exactly its own slots, popping releases
    // exactly its own storage, and the one slow path — growing a frame
    // that is not on top — shifts the frames above it correctly.

    fn pos(container_idx: u32, offset: usize) -> ContainerPosition {
        ContainerPosition {
            container_idx,
            offset,
        }
    }

    fn function_frame() -> CallFrame {
        CallFrame::new(CallFrameType::Function, Some(pos(0, 7)), None)
    }

    #[test]
    fn frames_see_only_their_own_temp_segments() {
        let mut stack = CallStack::new(
            CallFrame::new(CallFrameType::Root, None, None),
            Some(pos(0, 0)),
        );
        stack.write_temp(0, 1, Value::Int(10));
        stack.push(function_frame(), Some(pos(1, 0)));
        stack.write_temp(1, 0, Value::Int(20));

        assert_eq!(stack.temps(0), &[Value::Null, Value::Int(10)]);
        assert_eq!(stack.temps(1), &[Value::Int(20)]);
        assert!(!stack.is_temp_written(0, 0), "padding is not a write");
        assert!(stack.is_temp_written(0, 1));
        assert!(stack.is_temp_written(1, 0));
        assert!(
            !stack.is_temp_written(1, 1),
            "past the segment's end is unwritten"
        );
        assert_eq!(stack.temp(1, 1), None);
        assert_eq!(
            stack.temps(2),
            &[],
            "a frame that does not exist has no slots"
        );
    }

    #[test]
    fn pop_releases_exactly_the_top_frame_storage() {
        let mut stack = CallStack::new(
            CallFrame::new(CallFrameType::Root, None, None),
            Some(pos(0, 0)),
        );
        stack.write_temp(0, 0, Value::Int(1));
        stack.push(function_frame(), Some(pos(1, 0)));
        stack.write_temp(1, 3, Value::Int(2));
        stack.push_container(pos(2, 5));

        let popped = stack.pop().expect("a frame to pop");
        assert_eq!(popped.frame_type, CallFrameType::Function);
        assert_eq!(popped.return_address, Some(pos(0, 7)));
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.temps(0), &[Value::Int(1)]);
        assert_eq!(stack.top_containers(), &[pos(0, 0)]);
        assert_eq!(stack.top_container(), Some(pos(0, 0)));
    }

    /// The slow path: a `ref` parameter writing a slot its owning frame
    /// never declared, while a callee frame sits above it. The lower
    /// segment grows in place and the callee's slots move with it, intact.
    #[test]
    fn growing_a_lower_frame_shifts_the_frames_above_it() {
        let mut stack = CallStack::new(
            CallFrame::new(CallFrameType::Root, None, None),
            Some(pos(0, 0)),
        );
        stack.write_temp(0, 0, Value::Int(1));
        stack.push(function_frame(), Some(pos(1, 0)));
        stack.write_temp(1, 0, Value::Int(100));
        stack.write_temp(1, 1, Value::Int(101));
        stack.push(function_frame(), Some(pos(2, 0)));
        stack.write_temp(2, 0, Value::Int(200));

        stack.write_temp(0, 3, Value::Int(4));

        assert_eq!(
            stack.temps(0),
            &[Value::Int(1), Value::Null, Value::Null, Value::Int(4)]
        );
        assert_eq!(stack.temps(1), &[Value::Int(100), Value::Int(101)]);
        assert_eq!(stack.temps(2), &[Value::Int(200)]);
        assert!(stack.is_temp_written(0, 3));
        assert!(!stack.is_temp_written(0, 2));
        assert!(stack.is_temp_written(1, 1));
        assert!(stack.is_temp_written(2, 0));

        // And the same through `take_temp`, which grows identically.
        assert_eq!(stack.take_temp(1, 4), Value::Null);
        assert_eq!(stack.temps(1).len(), 5);
        assert_eq!(stack.temps(2), &[Value::Int(200)]);
        assert_eq!(stack.take_temp(2, 0), Value::Int(200));
        assert_eq!(stack.temps(2), &[Value::Null]);
        assert!(
            stack.is_temp_written(2, 0),
            "a take leaves the written bit alone"
        );
    }

    #[test]
    fn external_frame_args_are_written_by_construction_and_movable() {
        let mut stack = CallStack::new(
            CallFrame::new(CallFrameType::Root, None, None),
            Some(pos(0, 0)),
        );
        stack.write_temp(0, 0, Value::Int(1));
        stack.push_with_args(
            CallFrame::external(
                DefinitionId::new(brink_format::DefinitionTag::Address, 9),
                Some(pos(0, 3)),
            ),
            vec![Value::Int(7), Value::Bool(true)],
        );
        assert_eq!(stack.temps(1), &[Value::Int(7), Value::Bool(true)]);
        assert!(stack.is_temp_written(1, 1));
        assert_eq!(
            stack.top_container(),
            None,
            "an external frame executes nowhere"
        );

        let args = stack.take_top_temps();
        assert_eq!(args, vec![Value::Int(7), Value::Bool(true)]);
        assert_eq!(stack.temps(1), &[]);
        assert_eq!(
            stack.temps(0),
            &[Value::Int(1)],
            "the caller's slots are untouched"
        );
    }

    #[test]
    fn container_operations_never_reach_the_frame_below() {
        let mut stack = CallStack::new(
            CallFrame::new(CallFrameType::Root, None, None),
            Some(pos(0, 0)),
        );
        stack.push_container(pos(1, 0));
        stack.push(function_frame(), None);

        assert_eq!(stack.top_container(), None);
        assert_eq!(
            stack.pop_container(),
            None,
            "nothing to pop in an empty segment"
        );
        assert_eq!(stack.containers(0), &[pos(0, 0), pos(1, 0)]);

        stack.push_container(pos(5, 0));
        stack.push_container(pos(6, 2));
        assert_eq!(stack.top_containers(), &[pos(5, 0), pos(6, 2)]);
        stack.unwind_top_containers(1, 9);
        assert_eq!(stack.top_containers(), &[pos(5, 9)]);
        stack.reset_top_containers(pos(8, 1));
        assert_eq!(stack.top_containers(), &[pos(8, 1)]);
        if let Some(top) = stack.top_container_mut() {
            top.offset = 4;
        }
        assert_eq!(stack.top_container(), Some(pos(8, 4)));
        assert_eq!(stack.containers(0), &[pos(0, 0), pos(1, 0)]);

        stack.pop();
        assert_eq!(stack.top_container(), Some(pos(1, 0)));
    }

    #[test]
    fn a_fork_is_an_independent_copy() {
        let mut stack = CallStack::new(
            CallFrame::new(CallFrameType::Root, None, None),
            Some(pos(0, 0)),
        );
        stack.push(function_frame(), Some(pos(1, 0)));
        stack.write_temp(1, 0, Value::Int(1));
        let mut fork = stack.clone();
        fork.write_temp(1, 0, Value::Int(2));
        fork.push(function_frame(), Some(pos(2, 0)));
        assert_eq!(stack.temps(1), &[Value::Int(1)]);
        assert_eq!(stack.len(), 2);
        assert_eq!(fork.temps(1), &[Value::Int(2)]);
        assert_eq!(fork.len(), 3);
    }

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

    /// Any other frame type that can still pop (a `FunctionEvalFromGame`
    /// boundary, even `Root`/`External`) falls to the "unknown reason"
    /// backstop — mirrors C#'s final `else` arm.
    #[test]
    fn classify_other_frame_types_with_can_pop_is_unknown() {
        for frame_type in [
            CallFrameType::Root,
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
