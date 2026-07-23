//! Small output/status types: [`StoryStatus`], [`Line`], [`StepOutcome`],
//! [`Choice`], [`Stats`].

use alloc::string::String;
use alloc::vec::Vec;

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
    /// A flow parked at an `await` site (the `FlowFrame` model,
    /// `docs/flow-suspension-spec.md` §10.1). Like `Done`, a park is a
    /// **turn boundary**: text accumulated before the park flushes with
    /// it, so the pre-`await` state is never held hostage. The host wakes
    /// the flow via [`Story::wake_check`] and drives it when it wants
    /// output — a park never auto-continues.
    ///
    /// **Runtime-unreachable until FS-3r.** No code path in today's
    /// runtime constructs this variant — the E052 lowering fence
    /// (`docs/flow-suspension-spec.md` §11.4) keeps `await` from
    /// producing bytecode, so `park`/`spill`/`resume` do not yet exist.
    /// It ships now (FS-3w, the web-surface slice) purely so consumers
    /// migrate the API *shape* early: every marshal leg over `Line` names
    /// it, and adding it makes each missed leg a compile error by design.
    /// See `line_suspended_is_terminal_and_never_constructed_in_runtime`
    /// for the "nothing constructs it" guard.
    Suspended { text: String, tags: Vec<String> },
}

impl Line {
    /// The text content of this line, regardless of variant.
    pub fn text(&self) -> &str {
        match self {
            Self::Text { text, .. }
            | Self::Done { text, .. }
            | Self::Choices { text, .. }
            | Self::End { text, .. }
            | Self::Suspended { text, .. } => text,
        }
    }

    /// The tags associated with this line, regardless of variant.
    pub fn tags(&self) -> &[String] {
        match self {
            Self::Text { tags, .. }
            | Self::Done { tags, .. }
            | Self::Choices { tags, .. }
            | Self::End { tags, .. }
            | Self::Suspended { tags, .. } => tags,
        }
    }

    /// Returns true if this is a terminal variant (`Done`, `Choices`,
    /// `End`, or `Suspended`) — anything but `Text`. A park is a turn
    /// boundary, so `Suspended` is terminal.
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
