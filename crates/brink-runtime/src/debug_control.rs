//! Debugger control seam (D8, issue #3186): breakpoints, pause/resume, and
//! step in/over/out — the part of the debugger epic (#452) that turns the
//! read-only [`crate::DebugSnapshot`] (D4, #3182) into something that can
//! actually halt and single-step a running story.
//!
//! **Why this is provably zero-cost when `debug-hooks` is off.** The
//! `effect-trace`/`bench-counters` precedent this feature follows
//! (`docs/debugger-spec.md` §1.4, `vm.rs:~1544-1620`) threads paired
//! `#[cfg(feature)]`/no-op-stub *call sites* directly into `vm::step_impl`'s
//! dispatch body, because that instrumentation (per-opcode fault/effect
//! attribution) genuinely needs to run inline with specific opcodes. A
//! breakpoint check does not: all it needs is the position
//! `(container_idx, offset)` *before* an instruction executes, which is
//! already fully available from outside the hot loop — [`Story`]'s own
//! `container_stack.last()` (the same read `debug_snapshot`/D4's
//! `debug_position` already do, `story/mod.rs`'s `build_debug_snapshot`).
//! So this module does not add anything to `vm::step_impl` or to
//! `FlowInstance::advance_with_limit` (the production per-turn loop) at
//! all — on or off. Instead it wraps the existing `pub(crate) vm::step`
//! (already used this way by the `testing`-gated `Story::step_once` probe)
//! in its **own** loop, entered only through the `Story::debug_run`/
//! `debug_step*` methods this module's types back — methods that exist at
//! all only when `debug-hooks` is enabled (declared behind
//! `#[cfg(feature = "debug-hooks")]` in `lib.rs` and `story/mod.rs`). With
//! the feature off: this module doesn't compile, those methods don't
//! exist, and every byte of `vm.rs`/`flow_instance.rs` that the production
//! path (`continue_single`/`continue_maximally`/…) actually executes is
//! untouched — not merely "the branch is cheap", there is no branch. This
//! is a *stronger* zero-cost property than the effect-trace template's own
//! (which does add cfg-compiled-out call sites inside the hot loop), and
//! it is exactly what CLAUDE.md's "instrumentation doesn't belong in the
//! production path" principle asks for: "if an `if observer` branch
//! appears in a hot loop, the abstraction boundary is wrong" — so this
//! seam doesn't put one there.
//!
//! **The step-limit ruling (issue #3186 decision comment, 2026-08-28).**
//! Debug stepping gets its own budget, entirely separate from the
//! production step limit (`FlowInstance::STEP_LIMIT`,
//! `Stats::steps`/`RuntimeError::StepLimitExceeded`):
//!
//! - Production step accounting is unchanged and unread from debug-hook
//!   code: [`Story::debug_run`]/[`Story::debug_step`] call `vm::step`
//!   directly (bypassing `advance_with_limit` entirely, per the module doc
//!   above), and count VM steps in a **local** loop variable — never
//!   `Stats::steps`. (`Stats` itself is still threaded through, because
//!   `vm::step`'s signature requires `&mut Stats` for its own
//!   *non*-step-limit bookkeeping — `frames_pushed`, `materializations`,
//!   `choices_presented` — real per-event counters, not the step-limit
//!   counter this ruling is about. Debug-hook code never reads or writes
//!   `Stats::steps` specifically.)
//! - [`DEFAULT_DEBUG_BUDGET`] is the debug-only ceiling: generous enough
//!   that ordinary single-stepping never trips it, low enough that a
//!   `debug_run` that never reaches an armed breakpoint (or a `debug_step`
//!   step-over/out that never returns to/leaves the target frame — a
//!   runaway loop between the two) surfaces promptly rather than hanging a
//!   studio UI. Callers may pass a tighter or looser ceiling per call.
//! - Exceeding it is [`RuntimeError::DebugBudgetExceeded`] — never
//!   [`RuntimeError::StepLimitExceeded`], which would misreport a debug
//!   budget as the production one.
//!
//! **Frame semantics** (breakpoint/step-into/step-over/step-out) are
//! derived from call-stack depth deltas per `docs/debugger-spec.md` §4 and
//! the issue's own framing — see [`Story::debug_step`]'s doc for exactly
//! how each [`StepMode`] maps to a depth comparison, and for what is
//! deliberately *not* attempted here (source-level/statement-boundary
//! stepping needs the `DebugInfo` section's `IS_STMT` entries, D6/#3184,
//! not shipped yet; this seam only has opcode-level positions to work
//! with, which is exactly what "derived from call-stack depth deltas" — a
//! phrase from the issue text itself — asks for).
//!
//! **Watchpoints reuse [`WriteObserver`]/[`ObservedContext`]** (`state.rs`)
//! rather than inventing a second observer, per the issue's own
//! instruction. [`WatchpointObserver`] is the whole addition: a
//! `WriteObserver` impl that records a hit when a *watched* global slot is
//! written. [`Story::debug_run_watching`] wraps the routing context in
//! [`ObservedContext`] around it, exactly as `Story::continue_single_observed`
//! already does for the production line-buffered path — no VM change
//! needed for this half either.

use alloc::string::String;
use alloc::vec::Vec;

use brink_format::Value;

use crate::debug::DebugPosition;
use crate::state::WriteObserver;

/// Debug stepping's own step budget ceiling — separate from the
/// production step limit (`FlowInstance::STEP_LIMIT` = 1,000,000 per
/// call). See the module doc's "step-limit ruling" section for why this
/// exists and what it does and doesn't share with production accounting.
///
/// Chosen generous relative to a single step-into/step-over/breakpoint-run
/// (ordinary stepping and running to a breakpoint a handful of frames away
/// stays orders of magnitude under this), low enough that a predicate/loop
/// that never satisfies its stop condition reports back in well under a
/// second of VM-step work instead of hanging a studio UI indefinitely.
pub const DEFAULT_DEBUG_BUDGET: u64 = 200_000;

/// Identifies one breakpoint within a [`BreakpointSet`].
pub type BreakpointId = u32;

/// One breakpoint: an unconditional halt at a `(container_idx, offset)`
/// bytecode position, checked *before* that instruction executes.
///
/// v1 breakpoints are position-only (no source expression condition) —
/// scoped this way deliberately: an ink-expression *conditional*
/// breakpoint would need to evaluate an expression inside the paused
/// frame, which is exactly the "evaluate expression in frame" facility
/// [`crate::Speculation`] exists for for a *later* slice of this seam, not
/// re-derived here. A `run`/`debug_run` that never reaches an armed
/// breakpoint is still bounded — that's what [`DEFAULT_DEBUG_BUDGET`] is
/// for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: BreakpointId,
    pub container_idx: u32,
    pub offset: usize,
    /// Author-facing name/label, surfaced in
    /// [`DebugStopReason::Breakpoint`] and in
    /// [`RuntimeError::DebugBudgetExceeded`](crate::RuntimeError::DebugBudgetExceeded)
    /// when a conditional evaluation (a future slice) burns the debug
    /// budget on this breakpoint specifically. Never empty in practice —
    /// [`BreakpointSet::insert`] defaults it to the position if the caller
    /// passes an empty string.
    pub name: String,
    pub enabled: bool,
}

/// A caller-owned collection of breakpoints, checked by position. Not tied
/// to any particular [`crate::Story`] — the same set can be handed to
/// consecutive `debug_run` calls, or across flows compiled from the same
/// [`crate::Program`].
#[derive(Debug, Clone, Default)]
pub struct BreakpointSet {
    breakpoints: Vec<Breakpoint>,
    next_id: BreakpointId,
}

impl BreakpointSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an enabled breakpoint at `(container_idx, offset)`, returning its
    /// id. An empty `name` is replaced with a `container:offset` label so
    /// every breakpoint has something non-empty to report.
    pub fn insert(
        &mut self,
        container_idx: u32,
        offset: usize,
        name: impl Into<String>,
    ) -> BreakpointId {
        let id = self.next_id;
        self.next_id += 1;
        let mut name = name.into();
        if name.is_empty() {
            use alloc::format;
            name = format!("{container_idx}:{offset}");
        }
        self.breakpoints.push(Breakpoint {
            id,
            container_idx,
            offset,
            name,
            enabled: true,
        });
        id
    }

    /// Remove a breakpoint by id. Returns `false` if no breakpoint with
    /// that id exists.
    pub fn remove(&mut self, id: BreakpointId) -> bool {
        let before = self.breakpoints.len();
        self.breakpoints.retain(|b| b.id != id);
        self.breakpoints.len() != before
    }

    /// Enable/disable a breakpoint without removing it. Returns `false` if
    /// no breakpoint with that id exists.
    pub fn set_enabled(&mut self, id: BreakpointId, enabled: bool) -> bool {
        if let Some(bp) = self.breakpoints.iter_mut().find(|b| b.id == id) {
            bp.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Breakpoint> {
        self.breakpoints.iter()
    }

    /// The first enabled breakpoint at `pos`, if any. Deterministic:
    /// breakpoints are checked in insertion order (`Vec`, not a hash map),
    /// so a position with more than one enabled breakpoint always reports
    /// the earliest-inserted one.
    #[must_use]
    pub(crate) fn hit(&self, pos: DebugPosition) -> Option<&Breakpoint> {
        self.breakpoints
            .iter()
            .find(|b| b.enabled && b.container_idx == pos.container_idx && b.offset == pos.offset)
    }
}

/// How a `debug_step` call derives its "run until" target from call-stack
/// depth deltas (`docs/debugger-spec.md` §4). See
/// [`Story::debug_step`](crate::Story::debug_step) for the exact
/// per-variant depth comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    /// Execute exactly one instruction, descending into any newly-entered
    /// frame. Uniform across every `CallFrameType` — table §4's own
    /// framing ("This is uniform across frame types").
    Into,
    /// Run through any call the next instruction makes without stopping
    /// inside it; stop once back at (or still at) the starting depth.
    Over,
    /// Run until the current frame returns to its caller (depth strictly
    /// less than the starting depth).
    Out,
}

/// Why a `debug_run`/`debug_step` call stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugStopReason {
    /// An enabled breakpoint's position was reached, checked before the
    /// matching instruction executed.
    Breakpoint { id: BreakpointId, name: String },
    /// A watched global was written (`debug_run_watching` only).
    Watchpoint { global_idx: u32 },
    /// A choice point was reached (`vm::Stepped::Done` with non-empty
    /// pending choices) — the flow is now `WaitingForChoice`, distinct
    /// from [`DebugStopReason::Terminal`]: unlike an actual `-> DONE`/
    /// `-> END`, [`Story::choose`](crate::Story::choose) followed by
    /// [`Story::continue_single`](crate::Story::continue_single) can
    /// resume the story from here. Turn-index bump and invisible-default
    /// auto-select have already been applied (the same bookkeeping
    /// `advance_with_limit` performs on this outcome), so a caller that
    /// hands control back to the production API sees consistent state
    /// (issue #3186 review).
    Choices,
    /// The requested step (into/over/out) completed normally.
    Step,
    /// The flow reached a terminal VM outcome (`-> DONE`/`-> END`, or
    /// content otherwise exhausted) before the requested stop condition —
    /// breakpoint, watchpoint, or step target — was reached.
    Terminal,
    /// `StepMode::Out` was requested from the outermost (`Root`) frame,
    /// which has no caller to return to — `docs/debugger-spec.md` §4:
    /// "The debugger must disable step-out... exactly as GDB disables
    /// `finish` in the outermost frame." Reported instead of running the
    /// story to its own natural end, which would be a misleading way to
    /// answer "step out of a frame with no caller."
    NoStepOutTarget,
}

/// The result of a `debug_run`/`debug_step*` call: why it stopped, the
/// resulting position (mirrors [`DebugPosition`] semantics — `None` for a
/// frame with an empty container stack, e.g. after a terminal step, or a
/// parked/`External`-frame position; see `debug.rs`'s own doc), and the
/// resulting call-stack depth (the innermost thread's frame count) at the
/// moment execution stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugRunOutcome {
    pub reason: DebugStopReason,
    pub position: Option<DebugPosition>,
    pub depth: usize,
}

/// One recorded watchpoint hit: a watched global slot was written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchHit {
    pub global_idx: u32,
}

/// A [`WriteObserver`] that watches a fixed set of global slot indices and
/// records a hit whenever one is written — the entire watchpoint
/// implementation. Reuses the existing production `WriteObserver`/
/// `ObservedContext` seam (`state.rs`) rather than a second observer
/// mechanism, per the issue's own instruction: this struct is the only
/// piece of new plumbing watchpoints need.
///
/// Composes with [`crate::Story::debug_run_watching`] for pausing
/// mid-step-loop on a hit, or with the existing
/// `Story::continue_single_observed` (unaffected by this feature) for
/// production-path logging without pausing — the observer doesn't care
/// which loop drives it.
#[derive(Debug, Clone, Default)]
pub struct WatchpointObserver {
    watched: Vec<u32>,
    pending: Vec<WatchHit>,
}

impl WatchpointObserver {
    #[must_use]
    pub fn new(watched_globals: Vec<u32>) -> Self {
        Self {
            watched: watched_globals,
            pending: Vec::new(),
        }
    }

    #[must_use]
    pub fn watches(&self, global_idx: u32) -> bool {
        self.watched.contains(&global_idx)
    }

    /// Every hit recorded since the last `take_hits`/`take_hit` call.
    #[must_use]
    pub fn hits(&self) -> &[WatchHit] {
        &self.pending
    }

    /// Pop the earliest pending hit, if any — the FIFO half of the
    /// pause-on-write loop `debug_run_watching` drives.
    pub(crate) fn take_hit(&mut self) -> Option<WatchHit> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    /// Drain every hit recorded since the last `take_hits`/`clear` call, in
    /// the order they were recorded. The public counterpart to `take_hit`
    /// — for a consumer that composes this observer with
    /// `Story::continue_single_observed`/`continue_maximally_observed`
    /// (non-pausing logging, per this module's doc), `take_hit`'s
    /// `pub(crate)` FIFO pop is not reachable from outside the crate, so
    /// without this method `pending` would accumulate for the observer's
    /// entire lifetime with no way for a consumer to clear it — the
    /// unbounded-growth guard this method (and `clear`) close.
    pub fn take_hits(&mut self) -> Vec<WatchHit> {
        core::mem::take(&mut self.pending)
    }

    /// Discard every pending hit without returning them — the other half
    /// of the unbounded-growth guard `take_hits` provides.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

impl WriteObserver for WatchpointObserver {
    fn on_set_global(&mut self, idx: u32, _value: &Value) {
        if self.watched.contains(&idx) {
            self.pending.push(WatchHit { global_idx: idx });
        }
    }
}
