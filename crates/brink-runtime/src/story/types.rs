//! Small output/status types: [`StoryStatus`], [`Step`], [`OutputLine`],
//! [`BlockId`], [`StepOutcome`], [`Choice`], [`Stats`].

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

/// Opaque identifier grouping a run of adjacent content lines.
///
/// `docs/prose-dialect-spec.md` §3.7/§8d.2 (RULED): "block id is universal"
/// — every run of same-element adjacent content lines carries one; hosts
/// aggregate consecutive [`OutputLine`]s sharing a `BlockId` or ignore it
/// entirely. Two `OutputLine`s carry the same id iff they belong to the same
/// uninterrupted run — a terminal ([`Step::Choices`]/[`Step::Done`]/
/// [`Step::End`]) or a host-directed jump always starts a new one.
///
/// Compile-time-baked block ids (per §3.6's attachment mechanism, once the
/// element/markup layer lands — issue #1683) are a superset of this: in
/// today's schema-less-ink degenerate case there is exactly one implicit
/// "narrative" element for the whole story, so a `BlockId` here simply
/// counts uninterrupted runs. The wire field is stable now so #1683 can
/// refine *how* it's assigned without another contract break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

/// One line of story content, carried inside [`Step::Line`].
///
/// `.text()`/`.tags()` intentionally stay plain fields rather than a
/// `Vec<Part>` decomposition — that structured-markup surface
/// (`docs/prose-dialect-spec.md` §7/§9.1's `Part::Span`) is out of scope
/// here; it rides its own follow-up (#1683) once the element/markup layer
/// exists. This shape is the information-identical degenerate case the
/// spec's "superset check" calls out: schema-less ink → one implicit
/// narrative element, flat text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    /// The line's text content.
    pub text: String,
    /// Tags associated with this line.
    pub tags: Vec<String>,
    /// The run of adjacent content this line belongs to. See [`BlockId`].
    pub block_id: BlockId,
}

/// A single step of story output from [`Story::continue_single`].
///
/// The enum tells the caller what to do next:
/// - `Line` — more output may follow, keep calling `continue_single`.
/// - `Done` — this turn's output is complete. Call `continue_single`
///   again for the next turn (the story isn't over).
/// - `Choices` — pick a choice via [`Story::choose`], then resume.
/// - `End` — the story has permanently ended.
///
/// **Terminals carry no payload** (`docs/prose-dialect-spec.md` §7, RULED —
/// this replaces the earlier `Line` enum, whose terminal variants fused
/// trailing text onto the outcome). Any trailing content that precedes a
/// terminal is always delivered first as its own `Step::Line` — a caller
/// draining `continue_single` in a loop sees the same total text either
/// way, just spread across one more step when a turn ends mid-line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// One line of story content. More may follow — keep calling
    /// [`Story::continue_single`].
    Line(OutputLine),
    /// The story is presenting choices. Call [`Story::choose`] then
    /// resume with [`Story::continue_single`].
    Choices(Vec<Choice>),
    /// This turn's output is complete (ink `-> DONE`). The story isn't
    /// over — call [`Story::continue_single`] again for more.
    Done,
    /// The story has permanently ended (ink `-> END`).
    End,
    /// A flow parked at an `await` site (the `FlowFrame` model,
    /// `docs/flow-suspension-spec.md` §10.1). Like `Done`, a park is a
    /// **turn boundary**: text accumulated before the park flushes with
    /// it (as its own preceding `Step::Line`), so the pre-`await` state is
    /// never held hostage. The host wakes the flow via
    /// [`Story::wake_check`] and drives it when it wants output — a park
    /// never auto-continues.
    ///
    /// **Runtime-unreachable until FS-3r.** No code path in today's
    /// runtime constructs this variant — the E052 lowering fence
    /// (`docs/flow-suspension-spec.md` §11.4) keeps `await` from
    /// producing bytecode, so `park`/`spill`/`resume` do not yet exist.
    /// See `step_suspended_is_terminal_and_never_constructed_in_runtime`
    /// for the "nothing constructs it" guard.
    Suspended,
}

impl Step {
    /// The text content of this step. Only [`Step::Line`] carries any —
    /// every other (terminal) variant returns the empty string, since
    /// terminals carry no payload.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Line(line) => &line.text,
            Self::Choices(_) | Self::Done | Self::End | Self::Suspended => "",
        }
    }

    /// The tags associated with this step. Only [`Step::Line`] carries
    /// any — every other (terminal) variant returns an empty slice.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        match self {
            Self::Line(line) => &line.tags,
            Self::Choices(_) | Self::Done | Self::End | Self::Suspended => &[],
        }
    }

    /// Returns true if this is a terminal variant (`Choices`, `Done`,
    /// `End`, or `Suspended`) — anything but `Line`. A park is a turn
    /// boundary, so `Suspended` is terminal.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Line(_))
    }
}

/// Outcome of a single [`FlowInstance::advance`] step.
///
/// Like [`Step`], but with an extra variant for when a binding handler
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// A step of output, or a yield point (`Done`/`Choices`/`End`).
    Step(Step),
    /// The flow paused on a deferred external; resolve it and `advance`.
    AwaitingExternal,
}

/// A single choice presented to the player.
#[derive(Debug, Clone, PartialEq, Eq)]
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
