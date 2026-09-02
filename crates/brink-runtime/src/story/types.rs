//! Small output/status types: [`StoryStatus`], [`Step`], [`OutputLine`],
//! [`BlockId`], [`Element`], [`StepOutcome`], [`Choice`], [`Stats`].

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
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
/// Compile-time-baked block ids (per §3.6's attachment mechanism) are a
/// superset of this. Issue #2108 (`docs/decision-log.md` 2026-08-03 "The
/// element output model") delivers the first real instance: an
/// `attach = StructName` convention handler's data is merged into the VM's
/// output buffer (`brink_runtime::vm`'s `Opcode::AttachElement`/
/// `Opcode::EndElementRun`) and every line materialized while it's live gets
/// a copy in [`Element::data`] — but `BlockId` itself is **not** re-derived
/// from that mechanism; it stays the plain terminator-counting value it
/// always was (this field's own `next_block_id` doc, `brink_runtime::story::
/// call_stack::Flow`). A run of adjacent lines sharing one attach group can
/// therefore span more than one `BlockId` if it also crosses a real
/// terminator — the two concepts have not been unified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

/// A line's classification — kind + an open, preset-defined data map
/// (`docs/prose-dialect-spec.md` §7/§3.5b, §3.6, sitting-5 ruling item 8:
/// "the output format bakes no scene-specific fields — element data is an
/// open map produced by conventions and handlers"). Deliberately a `String`
/// kind, not a closed enum: the vocabulary belongs to whichever preset or
/// `@[element]` handler classified the line, never to the runtime.
///
/// **`data` is real** (issue #2108, `docs/decision-log.md` 2026-08-03 "The
/// element output model: attachment is block-level metadata, delivery is
/// per-line"): an `attach = StructName` convention handler's claimed line
/// consumes itself (no event — item 6, "AN EVENT EXISTS IFF A LINE EXISTS")
/// and its returned struct's fields merge into `data` on every line in the
/// run that follows (item 3: multiple attach handlers, e.g. `cue` then
/// `parenthetical`, accumulate onto the same run). **`kind` stays the
/// degenerate [`Self::NARRATIVE`] regardless** — classifying `kind` itself
/// for a non-attach single-line handler (`heading`/`transition` reporting
/// their own handler name as `kind`) is a distinct, still-open gap; see
/// issue #2108's own follow-up notes. A line with no preceding attach
/// convention still reports the always-correct [`Element::narrative`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The classifying handler's name, or [`Self::NARRATIVE`] for an
    /// ordinary, unclassified line — always the latter today, see this
    /// type's own doc.
    pub kind: String,
    /// Open, handler-defined payload. Empty when no `attach` convention
    /// preceded this line.
    pub data: BTreeMap<String, String>,
}

impl Element {
    /// The degenerate kind every line reports today — `docs/prose-dialect-spec.md`
    /// §7's own superset check: "schema-less ink → `element: narrative,
    /// parts: [Text]`" (§1 puts it the same way: "the *degenerate case* —
    /// an untyped narrative element with no spans").
    pub const NARRATIVE: &'static str = "narrative";

    /// The always-correct default: no handler classified this line.
    #[must_use]
    pub fn narrative() -> Self {
        Self {
            kind: Self::NARRATIVE.to_string(),
            data: BTreeMap::new(),
        }
    }
}

/// One line of story content, carried inside [`Step::Line`].
///
/// `.text` stays a plain field rather than a `Vec<Part>` decomposition —
/// that structured-markup surface (`docs/prose-dialect-spec.md` §7/§9.1's
/// `Part::Span`) is still out of scope (issue #2108 populated
/// [`Element::data`], the other half of the spec's element/markup layer, but
/// deliberately not this one — see this issue's own tracked follow-up).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    /// The line's text content.
    pub text: String,
    /// Tags associated with this line.
    pub tags: Vec<String>,
    /// The run of adjacent content this line belongs to. See [`BlockId`].
    pub block_id: BlockId,
    /// Where this line came from in the author's source (W7/#3300
    /// transcript provenance): the first contributing line-table entry's
    /// `source_location` — file plus UTF-8 byte range as the compiler
    /// consumed it. `None` when no entry contributed one (pure
    /// interpolation, or a program compiled without locations).
    pub source: Option<brink_format::SourceLocation>,
    /// This line's classification. See [`Element`]'s own doc for what's
    /// populated today and what isn't.
    pub element: Element,
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
    /// `+` (sticky, offered again) vs `*` (once-only) in the source — the
    /// bytecode's `ChoiceFlags::once_only`, inverted. A host that echoes the
    /// taken choice can mark it the way it was written (#3435).
    pub sticky: bool,
    /// Where the choice's text came from in the author's source (#3435) —
    /// the first `LineRef` of the choice's display fragment, the same rule
    /// [`OutputLine::source`] uses. `None` when the display is not a
    /// fragment (a constant string, an empty label) or the line table
    /// carries no location.
    pub source: Option<brink_format::SourceLocation>,
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
