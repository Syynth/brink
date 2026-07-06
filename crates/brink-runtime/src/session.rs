//! Story sessions: a journaling, replayable wrapper around [`Story`].
//!
//! [`StorySession`] *composes* a [`Story`] (which itself wraps a
//! [`FlowInstance`](crate::FlowInstance) + [`Context`](crate::Context)) with a
//! serializable [`SessionJournal`]. The VM never learns about journaling — the
//! journal observes inputs at the session boundary (the same place the VM
//! receives them), so instrumentation composes instead of threading an
//! `if observer` branch through the stepping hot loop. This generalizes the
//! in-memory [`ReplayRecorder`](crate::ReplayRecorder): where the recorder
//! captured only external results for hot-reload, the journal captures every
//! *input* that entered the VM (start, choices, externals, mutations) as
//! durable, serde-serializable data.
//!
//! Consumers:
//! - **`bevy-brink`** — first-class sessions/replay/save-load.
//! - **`brink-web`** — wasm bindings expose [`StorySession`] on the web.
//! - **`@brink/studio-store`** — `LocalSessionProvider` migrates onto this.
//!
//! There is no JS-side journal: the journal serializes to JSON via serde and
//! that JSON is the durable save artifact. See `docs/story-session-spec.md`
//! (#370, plus the snapshot half of #371).
//!
//! ## Turn-boundary contract
//!
//! `set_var` / `go_to_path` / `load_state` are **turn-boundary only**. The
//! session rejects them mid-turn (status [`Active`](crate::StoryStatus::Active),
//! i.e. more content is pending) with
//! [`SessionError::MutationMidTurn`], rather than queuing them. This is the
//! documented "one behavior" the spec permits — reject, not queue. A caller
//! drains the current turn (to `Done`/`Choices`/`End`) before mutating, which
//! keeps the journal event order unambiguous. The schema reserves a per-event
//! `anchor` so exact mid-turn replay can arrive additively without a format
//! break.
//!
//! ## Escape hatch
//!
//! [`StorySession::story`] / [`StorySession::story_mut`] expose the wrapped
//! [`Story`]. Anything done through them **bypasses the journal** — the
//! documented journal-bypass contract. Foreign / shared flows (#200) reached
//! this way never journal, matching the "journaling window" gate: only the
//! session's own `advance` / `choose` / `resolve_external` frames record.

use std::collections::BTreeMap;

use brink_format::{SaveState, Value};
use serde::{Deserialize, Serialize};

use crate::error::RuntimeError;
use crate::rng::{FastRng, StoryRng};
use crate::story::Story;
use crate::story::{
    ExternalFnHandler, ExternalResult, FallbackHandler, Line, StepOutcome, StoryStatus,
};

/// Current [`SessionJournal`] format version.
pub const SESSION_JOURNAL_VERSION: u32 = 1;

/// Upper bound on journal events (unbounded-growth guard, mirroring
/// [`RECORDING_CAP`](crate::RECORDING_CAP)). Beyond it, appends are dropped and
/// [`SessionJournal::truncated`] is set — the journal degrades honestly and
/// restore falls back to the embedded [`checkpoint`](SessionJournal::checkpoint).
pub const SESSION_JOURNAL_CAP: usize = 65_536;

// ── Journal ──────────────────────────────────────────────────────────────────

/// One ordered log of every input that entered the VM during a session, plus a
/// terminal fast-restore [`checkpoint`](Self::checkpoint).
///
/// Serde-serializable; the canonical durable save artifact. Values serialize
/// **tagged** (via [`Value`]'s derived enum representation and
/// [`SaveState`]'s `BTreeMap<String, Value>`) — no lossy `List`/`Divert` → null
/// mapping. Deterministic: event order is insertion order; embedded maps are
/// `BTreeMap`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionJournal {
    /// Format version (see [`SESSION_JOURNAL_VERSION`]).
    pub version: u32,
    /// Checksum of the program this journal was recorded against, so replay can
    /// detect a recompile and decide fast-restore vs full replay.
    pub program_checksum: u32,
    /// RNG seed applied at session start, if the host seeded one.
    pub seed: Option<u64>,
    /// Ordered inputs, in the order they entered the VM.
    pub events: Vec<JournalEvent>,
    /// Set when the cap was hit or a divergence truncated the log.
    pub truncated: bool,
    /// Fast-restore: terminal game-state snapshot (ruling: embedded `SaveState`
    /// in v1). Present once the session has produced state worth checkpointing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<SaveState>,
}

impl SessionJournal {
    /// A fresh, empty journal bound to `program_checksum`.
    #[must_use]
    pub fn new(program_checksum: u32, seed: Option<u64>) -> Self {
        Self {
            version: SESSION_JOURNAL_VERSION,
            program_checksum,
            seed,
            events: Vec::new(),
            truncated: false,
            checkpoint: None,
        }
    }

    /// Append `event`, respecting [`SESSION_JOURNAL_CAP`]. Beyond the cap the
    /// event is dropped and [`truncated`](Self::truncated) is set.
    fn push(&mut self, event: JournalEvent) {
        if self.events.len() >= SESSION_JOURNAL_CAP {
            self.truncated = true;
            return;
        }
        self.events.push(event);
    }

    /// Number of recorded events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// One input that entered the VM.
///
/// The reserved `anchor` / `flow` dimensions are serialized (as `Option`, both
/// defaulting to `None`) but **not interpreted** in v1 — they let mid-turn
/// anchoring (`anchor`) and multi-flow journaling (`flow`) arrive additively
/// without a format break. See the module-level turn-boundary contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalEvent {
    /// The input kind + payload.
    pub kind: EventKind,
    /// Reserved: per-event position ordinal for future mid-turn anchoring.
    /// Serialized, never interpreted in v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<u64>,
    /// Reserved: flow tag for future multi-flow journaling. v1 is
    /// default-flow-only. Serialized, never interpreted in v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
}

impl JournalEvent {
    /// A v1 event with the reserved dimensions left `None`.
    #[must_use]
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            anchor: None,
            flow: None,
        }
    }
}

/// The kind + payload of a [`JournalEvent`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// Session start / play-from-here. `path` is `None` for the default root
    /// entry, `Some` for a `ChoosePathString` start.
    Start {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<Value>,
    },
    /// A choice selection. `label` is the choice text as seen (advisory — used
    /// only for label-drift warnings, never as the selection key).
    Choice {
        index: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// An external-function result, captured where the session's own frame
    /// received it (the journaling-window gate).
    External {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<Value>,
        result: Value,
    },
    /// A host `set_var` (turn-boundary only).
    SetVar { name: String, value: Value },
    /// A host `go_to_path` / `ChoosePathString` (turn-boundary only).
    GoToPath {
        path: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<Value>,
    },
    /// A host `load_state` (turn-boundary only).
    LoadState { state: SaveState },
    /// A journaled `call_function`. The function's *own* externals are resolved
    /// through the isolated (non-journaling) handler path — only the top-level
    /// call is journaled here.
    Call {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<Value>,
    },
}

// ── Replay ───────────────────────────────────────────────────────────────────

/// How replay obtains external values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExternalReplayMode {
    /// Default. Serve `External` events from the journal (recorded results). No
    /// effect re-fires; reads stay faithful.
    #[default]
    Recorded,
    /// Re-invoke externals live against the supplied handler. A handler that
    /// defers ([`ExternalResult::Pending`]) parks the replay as
    /// [`ReplayOutcome::Failed`] with [`FailReason::AwaitingExternal`]; resume
    /// via [`StorySession::continue_replay`].
    Live,
}

/// Outcome of replaying a journal prefix against a program.
///
/// Typed, never silent, never panicking.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayOutcome {
    /// The prefix replayed successfully. `warnings` collects soft issues (e.g.
    /// choice label drift at a matching index).
    Replayed { warnings: Vec<ReplayWarning> },
    /// Replay diverged at `at_event`: the recorded event could not be applied
    /// against the current program. The journal is truncated at that point and
    /// the session is parked at the reached position.
    Diverged {
        at_event: usize,
        expected: Box<JournalEvent>,
        found: DivergenceFound,
    },
    /// Replay failed at `at_event` for a non-divergence reason.
    Failed { at_event: usize, reason: FailReason },
}

/// A non-fatal replay observation.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplayWarning {
    /// A choice replayed by index, but its recorded label differs from the
    /// label now presented at that index (a soft signal the story text drifted
    /// under the same choice ordering).
    ChoiceLabelDrift {
        at_event: usize,
        index: u32,
        recorded: String,
        found: String,
    },
}

/// What was found at a divergence point instead of the recorded event.
#[derive(Debug, Clone, PartialEq)]
pub enum DivergenceFound {
    /// A choice index the current program does not present (out of range).
    ChoiceIndexOutOfRange { index: u32, available: usize },
    /// The session was not waiting for a choice when a `Choice` event replayed.
    NotWaitingForChoice,
    /// A path that no longer resolves in the current program.
    UnknownPath { path: String },
    /// The event kind cannot be applied from the reached state (e.g. a `Start`
    /// after the session already started).
    UnexpectedEvent,
}

/// Why replay stopped without diverging.
#[derive(Debug, Clone, PartialEq)]
pub enum FailReason {
    /// A runtime error surfaced during stepping.
    RuntimeError(String),
    /// A step/line budget was exceeded (the caller can restart fresh).
    Budget,
    /// Live replay hit a deferred external and parked. Resolve it and call
    /// [`StorySession::continue_replay`].
    AwaitingExternal { name: String },
}

// ── Snapshot / diff ──────────────────────────────────────────────────────────

/// A typed, name-resolved snapshot of a session's game state.
///
/// A NEW typed serialization path — distinct from the string-valued
/// [`DebugSnapshot`](crate::DebugSnapshot). Globals keep their [`Value`]s (list
/// membership included via [`SnapshotList`]); callstack is summarized to frame
/// kinds + resolved locations. Deterministic (`BTreeMap` / sorted vectors).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Global variables by name, typed. `BTreeMap` for determinism.
    pub globals: BTreeMap<String, Value>,
    /// Resolved list memberships for any `List`-valued global, keyed by
    /// variable name (item names, sorted). Complements `globals` for consumers
    /// that want membership without decoding `DefinitionId`s.
    pub lists: BTreeMap<String, SnapshotList>,
    /// Global turn index.
    pub turn_index: u32,
    /// Per-knot/stitch visit counts, keyed by resolved path, sorted.
    ///
    /// Path-keyed projection: counts for scopes with no resolvable author
    /// path (anonymous counted containers — gathers, choice points — keyed
    /// only by hash id) are **omitted** here. This is the known projection
    /// limit of the typed snapshot; the full id-keyed counts remain available
    /// via [`StorySession::save_state`].
    pub visit_counts: BTreeMap<String, u32>,
    /// Per-knot/stitch turn-since counts, keyed by resolved path, sorted.
    /// Same path-keyed projection limit as
    /// [`visit_counts`](Self::visit_counts).
    pub turn_counts: BTreeMap<String, u32>,
    /// Callstack summary of the default flow, innermost frame first.
    pub call_stack: Vec<SnapshotFrame>,
    /// Execution status.
    pub status: SnapshotStatus,
}

/// Resolved membership of a `List`-valued global.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotList {
    /// Active item names, sorted for determinism.
    pub items: Vec<String>,
}

/// One summarized call frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFrame {
    /// Frame kind: `root` / `function` / `tunnel` / `thread` / `external` / `eval`.
    pub kind: String,
    /// Nearest named container for this frame, if resolvable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Number of temporaries in this frame.
    pub temps: usize,
}

/// Session execution status, serde-friendly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    Active,
    WaitingForChoice,
    Done,
    Ended,
}

impl From<StoryStatus> for SnapshotStatus {
    fn from(s: StoryStatus) -> Self {
        match s {
            StoryStatus::Active => Self::Active,
            StoryStatus::WaitingForChoice => Self::WaitingForChoice,
            StoryStatus::Done => Self::Done,
            StoryStatus::Ended => Self::Ended,
        }
    }
}

/// A pure diff between two [`StateSnapshot`]s (see [`diff`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateDiff {
    /// Globals present in `b` but not `a`.
    pub added_globals: BTreeMap<String, Value>,
    /// Globals present in `a` but not `b`.
    pub removed_globals: BTreeMap<String, Value>,
    /// Globals whose value changed, mapped to `(before, after)`.
    pub changed_globals: BTreeMap<String, (Value, Value)>,
    /// Per-list membership deltas for lists that changed, keyed by var name.
    pub list_deltas: BTreeMap<String, ListDelta>,
    /// `turn_index` delta `(before, after)` if it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<(u32, u32)>,
    /// Callstack frames pushed in `b` relative to `a` (by innermost-first
    /// comparison): frames appended beyond the common prefix.
    pub pushed_frames: Vec<SnapshotFrame>,
    /// Callstack frames popped in `b` relative to `a`.
    pub popped_frames: Vec<SnapshotFrame>,
}

impl StateDiff {
    /// Whether the two snapshots were identical in every compared dimension.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added_globals.is_empty()
            && self.removed_globals.is_empty()
            && self.changed_globals.is_empty()
            && self.list_deltas.is_empty()
            && self.turn_index.is_none()
            && self.pushed_frames.is_empty()
            && self.popped_frames.is_empty()
    }
}

/// Membership delta for one list-valued global.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListDelta {
    /// Item names present in `b` but not `a`, sorted.
    pub added: Vec<String>,
    /// Item names present in `a` but not `b`, sorted.
    pub removed: Vec<String>,
}

/// Pure diff of two snapshots. `a` is "before", `b` is "after".
#[must_use]
pub fn diff(a: &StateSnapshot, b: &StateSnapshot) -> StateDiff {
    let mut d = StateDiff::default();

    for (name, av) in &a.globals {
        match b.globals.get(name) {
            None => {
                d.removed_globals.insert(name.clone(), av.clone());
            }
            Some(bv) if bv != av => {
                d.changed_globals
                    .insert(name.clone(), (av.clone(), bv.clone()));
            }
            Some(_) => {}
        }
    }
    for (name, bv) in &b.globals {
        if !a.globals.contains_key(name) {
            d.added_globals.insert(name.clone(), bv.clone());
        }
    }

    // List deltas over the union of list-valued globals.
    let mut list_names: Vec<&String> = a.lists.keys().chain(b.lists.keys()).collect();
    list_names.sort_unstable();
    list_names.dedup();
    for name in list_names {
        let empty = SnapshotList { items: Vec::new() };
        let al = a.lists.get(name).unwrap_or(&empty);
        let bl = b.lists.get(name).unwrap_or(&empty);
        if al == bl {
            continue;
        }
        let added: Vec<String> = bl
            .items
            .iter()
            .filter(|i| !al.items.contains(i))
            .cloned()
            .collect();
        let removed: Vec<String> = al
            .items
            .iter()
            .filter(|i| !bl.items.contains(i))
            .cloned()
            .collect();
        if !added.is_empty() || !removed.is_empty() {
            d.list_deltas
                .insert(name.clone(), ListDelta { added, removed });
        }
    }

    if a.turn_index != b.turn_index {
        d.turn_index = Some((a.turn_index, b.turn_index));
    }

    // Callstack: compare from the outermost (root) end. The frames are stored
    // innermost-first, so reverse to find the common outer prefix.
    let a_outer: Vec<&SnapshotFrame> = a.call_stack.iter().rev().collect();
    let b_outer: Vec<&SnapshotFrame> = b.call_stack.iter().rev().collect();
    let common = a_outer
        .iter()
        .zip(b_outer.iter())
        .take_while(|(x, y)| x == y)
        .count();
    // Frames beyond the common prefix in b were pushed; in a were popped.
    d.pushed_frames = b_outer[common..].iter().map(|f| (*f).clone()).collect();
    d.popped_frames = a_outer[common..].iter().map(|f| (*f).clone()).collect();

    d
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors from session-level operations (distinct from VM [`RuntimeError`]s,
/// which are wrapped).
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// A turn-boundary-only mutation (`set_var` / `go_to_path` / `load_state`)
    /// was attempted mid-turn (status `Active`). Drain the turn first.
    #[error(
        "mutation `{op}` attempted mid-turn; set_var/go_to_path/load_state are turn-boundary only"
    )]
    MutationMidTurn { op: &'static str },
    /// The program checksum in a journal does not match the program being
    /// restored/replayed against, and no fast-restore checkpoint was usable.
    #[error("journal program checksum {journal} does not match program {program}")]
    ChecksumMismatch { journal: u32, program: u32 },
    /// A wrapped VM error.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

// ── Journaling handler ───────────────────────────────────────────────────────

/// Composes a caller's [`ExternalFnHandler`] and journals every inline-resolved
/// external where the session's own frame receives it. Generalizes
/// [`RecordingHandler`](crate::RecordingHandler) from in-memory recording to the
/// durable journal.
///
/// Deferred externals ([`ExternalResult::Pending`]) resolve out-of-band; the
/// session journals those in [`StorySession::resolve_external`] where it has the
/// name/args/result.
struct JournalingHandler<'a, H: ExternalFnHandler + ?Sized> {
    inner: &'a H,
    // Interior-mutability: the trait method is `&self`, but we need to append.
    sink: std::cell::RefCell<&'a mut Vec<(String, Vec<Value>, Value)>>,
}

impl<'a, H: ExternalFnHandler + ?Sized> JournalingHandler<'a, H> {
    fn new(inner: &'a H, sink: &'a mut Vec<(String, Vec<Value>, Value)>) -> Self {
        Self {
            inner,
            sink: std::cell::RefCell::new(sink),
        }
    }
}

impl<H: ExternalFnHandler + ?Sized> ExternalFnHandler for JournalingHandler<'_, H> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        let result = self.inner.call(name, args);
        if let ExternalResult::Resolved(value) = &result {
            self.sink
                .borrow_mut()
                .push((name.to_owned(), args.to_vec(), value.clone()));
        }
        result
    }
}

/// Serves external values from a recorded journal prefix during replay
/// (`ExternalReplayMode::Recorded`). Consumes `External` events in order; on
/// mismatch it falls through to the ink fallback body (never re-invokes).
struct RecordedReplayHandler<'a> {
    // (name, args, result) queue, consumed front-to-back.
    queue: std::cell::RefCell<&'a mut std::collections::VecDeque<(String, Vec<Value>, Value)>>,
}

impl ExternalFnHandler for RecordedReplayHandler<'_> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        let mut q = self.queue.borrow_mut();
        match q.front() {
            Some((n, a, _)) if n == name && a.as_slice() == args => {
                let (_, _, result) = q.pop_front().unwrap_or_else(|| {
                    // Unreachable: we just matched `front`. Fall back safely.
                    (String::new(), Vec::new(), Value::Null)
                });
                ExternalResult::Resolved(result)
            }
            _ => ExternalResult::Fallback,
        }
    }
}

// ── Session ──────────────────────────────────────────────────────────────────

/// A journaling, replayable session wrapping a [`Story`].
///
/// Owns a [`Story`] + a [`SessionJournal`]. Stepping mirrors
/// [`Story::advance_with`] exactly ([`StepOutcome`]), recording inputs at the
/// session boundary. The wrapped story is reachable via [`story`](Self::story) /
/// [`story_mut`](Self::story_mut) for the documented journal-bypass escape
/// hatch.
pub struct StorySession<'p, R: StoryRng = FastRng> {
    story: Story<'p, R>,
    journal: SessionJournal,
    started: bool,
    /// The un-replayed tail of an in-progress replay that parked on a deferred
    /// external ([`FailReason::AwaitingExternal`]). `Some` only between the
    /// park and the [`continue_replay`](Self::continue_replay) that resumes
    /// it — resuming consumes the tail from this cursor instead of dropping
    /// the remaining recorded inputs.
    pending_replay: Option<PendingReplay>,
}

/// Cursor state for a parked, resumable replay: the remaining source events
/// (with their original indices, for `at_event` reporting), the
/// recorded-externals queue, the external mode, warnings accumulated so far,
/// and the source journal's checkpoint to carry over on completion.
struct PendingReplay {
    /// `(original_source_index, event)` pairs not yet applied.
    remaining: std::collections::VecDeque<(usize, JournalEvent)>,
    /// Recorded externals still unserved (`ExternalReplayMode::Recorded`).
    ext_queue: std::collections::VecDeque<(String, Vec<Value>, Value)>,
    mode: ExternalReplayMode,
    warnings: Vec<ReplayWarning>,
    /// The source journal's terminal checkpoint, applied to the rebuilt
    /// journal when the replay completes.
    source_checkpoint: Option<SaveState>,
    /// Total events in the source journal (for final-step `at_event`).
    total_events: usize,
}

/// Internal outcome of a replay stepping burst: parked on a deferred external
/// (resumable) or failed terminally.
enum StepPark {
    Awaiting { name: String },
    Fail(FailReason),
}

impl<'p, R: StoryRng> StorySession<'p, R> {
    /// Wrap `story` in a fresh session. `seed` is advisory metadata recorded in
    /// the journal (the host is responsible for actually seeding the story via
    /// [`Story::set_rng_seed`] before/at start).
    #[must_use]
    pub fn new(story: Story<'p, R>, seed: Option<u64>) -> Self {
        let checksum = story.program().source_checksum();
        Self {
            journal: SessionJournal::new(checksum, seed),
            story,
            started: false,
            pending_replay: None,
        }
    }

    /// Read-only access to the journal (for export / persistence).
    #[must_use]
    pub fn journal(&self) -> &SessionJournal {
        &self.journal
    }

    /// Take the journal by value, refreshing its checkpoint first so the
    /// exported artifact can fast-restore. The session keeps a fresh empty
    /// journal bound to the same program (rarely needed; export usually clones
    /// via [`journal`](Self::journal)).
    pub fn export_journal(&mut self) -> SessionJournal {
        self.refresh_checkpoint();
        let checksum = self.journal.program_checksum;
        let seed = self.journal.seed;
        std::mem::replace(&mut self.journal, SessionJournal::new(checksum, seed))
    }

    /// **Escape hatch**: the wrapped story. Reads through here never touch the
    /// journal (they can't mutate anyway).
    #[must_use]
    pub fn story(&self) -> &Story<'p, R> {
        &self.story
    }

    /// **Escape hatch**: mutable access to the wrapped story. Anything done
    /// here **bypasses the journal** — the documented journal-bypass contract.
    /// Use for foreign / shared flows (#200) whose externals never journal.
    pub fn story_mut(&mut self) -> &mut Story<'p, R> {
        &mut self.story
    }

    /// Update the journal's embedded fast-restore checkpoint from the story's
    /// current game state.
    fn refresh_checkpoint(&mut self) {
        self.journal.checkpoint = Some(self.story.save_state());
    }

    /// Record the implicit default `Start` if the session hasn't started.
    fn ensure_started(&mut self) {
        if !self.started {
            self.started = true;
            self.journal.push(JournalEvent::new(EventKind::Start {
                path: None,
                args: Vec::new(),
            }));
        }
    }

    // ── Stepping ─────────────────────────────────────────────────────

    /// Advance one step with the default (fallback) handler, journaling any
    /// inline-resolved externals. Surfaces a deferred external as
    /// [`StepOutcome::AwaitingExternal`].
    pub fn advance(&mut self) -> Result<StepOutcome, RuntimeError> {
        self.advance_with(&FallbackHandler)
    }

    /// Advance one step with a custom handler, journaling any inline-resolved
    /// externals it produces (the journaling-window gate: only this frame's
    /// externals record).
    pub fn advance_with(
        &mut self,
        handler: &dyn ExternalFnHandler,
    ) -> Result<StepOutcome, RuntimeError> {
        self.ensure_started();
        let mut sink: Vec<(String, Vec<Value>, Value)> = Vec::new();
        let outcome = {
            let jh = JournalingHandler::new(handler, &mut sink);
            self.story.advance_with(&jh)
        };
        for (name, args, result) in sink {
            self.journal.push(JournalEvent::new(EventKind::External {
                name,
                args,
                result,
            }));
        }
        outcome
    }

    /// Advance until one line of content or a yield point, journaling externals.
    pub fn continue_single(&mut self) -> Result<Line, RuntimeError> {
        self.ensure_started();
        let mut sink: Vec<(String, Vec<Value>, Value)> = Vec::new();
        let outcome = {
            let jh = JournalingHandler::new(&FallbackHandler, &mut sink);
            self.story.continue_single_with(&jh)
        };
        for (name, args, result) in sink {
            self.journal.push(JournalEvent::new(EventKind::External {
                name,
                args,
                result,
            }));
        }
        outcome
    }

    /// Advance to the next pause, journaling externals. The last line is always
    /// terminal (`Done` / `Choices` / `End`).
    pub fn continue_to_pause(&mut self) -> Result<Vec<Line>, RuntimeError> {
        self.ensure_started();
        let mut sink: Vec<(String, Vec<Value>, Value)> = Vec::new();
        let outcome = {
            let jh = JournalingHandler::new(&FallbackHandler, &mut sink);
            self.story.continue_maximally_with(&jh)
        };
        for (name, args, result) in sink {
            self.journal.push(JournalEvent::new(EventKind::External {
                name,
                args,
                result,
            }));
        }
        outcome
    }

    /// Select a choice, journaling the `Choice` event (with its advisory label).
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError> {
        self.ensure_started();
        let label = self
            .story
            .pending_choices()
            .into_iter()
            .find(|c| c.index == index)
            .map(|c| c.text);
        self.story.choose(index)?;
        #[expect(clippy::cast_possible_truncation, reason = "choice indices are small")]
        self.journal.push(JournalEvent::new(EventKind::Choice {
            index: index as u32,
            label,
        }));
        Ok(())
    }

    /// Resolve a deferred external (out-of-band, [`ExternalResult::Pending`]),
    /// journaling it as an `External` event. This is the journaling-window gate
    /// in action: the session records here because it is the session's own
    /// pause that is being resolved.
    pub fn resolve_external(&mut self, value: Value) {
        let name = self
            .story
            .pending_external_name()
            .map(str::to_owned)
            .unwrap_or_default();
        let args = self.story.pending_external_args().to_vec();
        self.journal.push(JournalEvent::new(EventKind::External {
            name,
            args,
            result: value.clone(),
        }));
        self.story.resolve_external(value);
    }

    /// Whether the session is parked on a deferred external.
    #[must_use]
    pub fn has_pending_external(&self) -> bool {
        self.story.has_pending_external()
    }

    // ── Turn-boundary mutations ──────────────────────────────────────

    /// Set a global variable. **Turn-boundary only**: rejected mid-turn.
    ///
    /// # Errors
    /// [`SessionError::MutationMidTurn`] if the session is mid-turn (status
    /// `Active`).
    pub fn set_var(&mut self, name: &str, value: Value) -> Result<bool, SessionError> {
        self.require_turn_boundary("set_var")?;
        let applied = self.story.set_variable(name, value.clone());
        if applied {
            self.journal.push(JournalEvent::new(EventKind::SetVar {
                name: name.to_owned(),
                value,
            }));
        }
        Ok(applied)
    }

    /// Move the play head to a path (ink `ChoosePathString`). **Turn-boundary
    /// only**: rejected mid-turn.
    ///
    /// # Errors
    /// [`SessionError::MutationMidTurn`] mid-turn; wrapped [`RuntimeError`]s
    /// from the jump.
    pub fn go_to_path(&mut self, path: &str, args: &[Value]) -> Result<(), SessionError> {
        self.require_turn_boundary("go_to_path")?;
        self.ensure_started();
        if args.is_empty() {
            self.story.choose_path_string(path)?;
        } else {
            self.story.choose_path_string_with_args(path, args)?;
        }
        self.journal.push(JournalEvent::new(EventKind::GoToPath {
            path: path.to_owned(),
            args: args.to_vec(),
        }));
        Ok(())
    }

    /// Load a durable [`SaveState`]. **Turn-boundary only**: rejected mid-turn.
    ///
    /// # Errors
    /// [`SessionError::MutationMidTurn`] mid-turn.
    pub fn load_state(&mut self, state: &SaveState) -> Result<(), SessionError> {
        self.require_turn_boundary("load_state")?;
        self.story.load_state(state);
        self.journal.push(JournalEvent::new(EventKind::LoadState {
            state: state.clone(),
        }));
        Ok(())
    }

    /// Capture the current durable game state (does not journal).
    #[must_use]
    pub fn save_state(&self) -> SaveState {
        self.story.save_state()
    }

    /// Evaluate an ink function from engine code, journaling a `Call` event.
    /// The function's own externals resolve through the isolated (non-journaling)
    /// [`Story::call_function`] handler path — only the top-level call journals.
    ///
    /// # Errors
    /// Wrapped [`RuntimeError`]s from evaluation.
    pub fn call_function(
        &mut self,
        name: &str,
        args: &[Value],
        handler: &dyn ExternalFnHandler,
    ) -> Result<Value, RuntimeError> {
        // Note: `story.call_function` runs isolated — its externals do NOT go
        // through the journaling handler, so they never enter the journal.
        let result = self.story.call_function(name, args, handler)?;
        self.journal.push(JournalEvent::new(EventKind::Call {
            name: name.to_owned(),
            args: args.to_vec(),
        }));
        Ok(result)
    }

    fn require_turn_boundary(&self, op: &'static str) -> Result<(), SessionError> {
        // Mid-turn == the story is Active with more content pending. A fresh,
        // not-yet-started session is at a boundary (allowed).
        if self.started && self.story.status_is_active() {
            return Err(SessionError::MutationMidTurn { op });
        }
        Ok(())
    }

    // ── Snapshot / diff ──────────────────────────────────────────────

    /// A typed snapshot of the current game state (globals with list
    /// membership, turn counts, callstack summary). See [`StateSnapshot`].
    #[must_use]
    pub fn snapshot(&self) -> StateSnapshot {
        self.story.state_snapshot()
    }

    /// Pure diff of two snapshots (convenience; see the free [`diff`] fn).
    #[must_use]
    pub fn diff(a: &StateSnapshot, b: &StateSnapshot) -> StateDiff {
        diff(a, b)
    }
}

impl<'p, R: StoryRng> StorySession<'p, R> {
    // ── Replay / restore ─────────────────────────────────────────────

    /// Fast-restore: apply a journal's embedded [`checkpoint`](SessionJournal::checkpoint)
    /// and skip replay when the program checksum matches; otherwise fall back to
    /// a full [`replay`](Self::replay).
    ///
    /// Returns the constructed session and the [`ReplayOutcome`]. On a checksum
    /// match with a present checkpoint the outcome is
    /// [`ReplayOutcome::Replayed`] with no warnings (no stepping occurred).
    ///
    /// # Errors
    /// [`SessionError::ChecksumMismatch`] only if the checksum differs *and* no
    /// checkpoint is present to restore from (nothing safe to do).
    pub fn restore(
        story: Story<'p, R>,
        journal: SessionJournal,
    ) -> Result<(Self, ReplayOutcome), SessionError> {
        let program_checksum = story.program().source_checksum();
        if program_checksum == journal.program_checksum {
            if let Some(checkpoint) = journal.checkpoint.clone() {
                let mut session = Self {
                    story,
                    journal,
                    started: true,
                    pending_replay: None,
                };
                session.story.load_state(&checkpoint);
                return Ok((
                    session,
                    ReplayOutcome::Replayed {
                        warnings: Vec::new(),
                    },
                ));
            }
        } else if journal.checkpoint.is_none() {
            return Err(SessionError::ChecksumMismatch {
                journal: journal.program_checksum,
                program: program_checksum,
            });
        }
        // Fall back to full replay (recompiled program or no checkpoint).
        Ok(Self::replay(
            story,
            &journal,
            ExternalReplayMode::Recorded,
            None,
        ))
    }

    /// Replay a journal against `story` from a fresh start. Consumes the journal
    /// prefix event-by-event; on divergence, truncates the journal at that point
    /// and parks at the reached position.
    ///
    /// The session **re-records** as it replays, rebuilding its own journal. In
    /// [`ExternalReplayMode::Recorded`], the rebuilt prefix is the **source**
    /// prefix: source `External` events are re-pushed verbatim, including any
    /// the re-run did not actually consume (a recorded-mode mismatch falls back
    /// to the ink fallback body rather than diverging) — the truncated prefix
    /// is the source's record, not a re-observed trace. In
    /// [`ExternalReplayMode::Live`] the rebuilt journal *is* a re-observed
    /// trace: live results are journaled as they resolve and source `External`
    /// events are not copied.
    ///
    /// `mode` selects recorded (journal-served) vs live externals. Live replay
    /// hitting a deferred external parks with [`FailReason::AwaitingExternal`],
    /// **retaining the un-replayed tail**: resolve the external
    /// ([`resolve_external`](Self::resolve_external)) and resume with
    /// [`continue_replay`](Self::continue_replay), which picks up the remaining
    /// recorded inputs from the park point.
    #[must_use]
    pub fn replay(
        story: Story<'p, R>,
        journal: &SessionJournal,
        mode: ExternalReplayMode,
        live_handler: Option<&dyn ExternalFnHandler>,
    ) -> (Self, ReplayOutcome) {
        let mut session = Self {
            story,
            journal: SessionJournal::new(0, None),
            started: false,
            pending_replay: None,
        };
        // Rebuild an empty journal to re-record faithfully as we replay.
        session.journal =
            SessionJournal::new(session.story.program().source_checksum(), journal.seed);
        // Queue of recorded externals for the recorded-mode handler.
        let ext_queue = journal
            .events
            .iter()
            .filter_map(|ev| match &ev.kind {
                EventKind::External { name, args, result } => {
                    Some((name.clone(), args.clone(), result.clone()))
                }
                _ => None,
            })
            .collect();
        let state = PendingReplay {
            remaining: journal.events.iter().cloned().enumerate().collect(),
            ext_queue,
            mode,
            warnings: Vec::new(),
            source_checkpoint: journal.checkpoint.clone(),
            total_events: journal.events.len(),
        };
        let outcome = session.drive_replay(state, live_handler);
        (session, outcome)
    }

    /// Whether a parked replay tail is pending (a live replay hit a deferred
    /// external). Resolve it and call [`continue_replay`](Self::continue_replay).
    #[must_use]
    pub fn has_pending_replay(&self) -> bool {
        self.pending_replay.is_some()
    }

    /// Resume a replay parked on a deferred external
    /// ([`FailReason::AwaitingExternal`]). Resolve the pending external first
    /// (via [`resolve_external`](Self::resolve_external)), then call this: the
    /// session resumes consuming the retained journal tail from the park
    /// point — it can complete ([`ReplayOutcome::Replayed`] with all warnings
    /// accumulated across parks), park again, diverge later, or fail.
    ///
    /// With **no pending replay tail** this keeps the advance-only behavior:
    /// it steps the live story to its next pause with `live_handler`
    /// (journaling externals as in normal play) and returns `Replayed` on
    /// reaching the pause, or `Failed` if it parks or errors (`at_event` is
    /// then the rebuilt journal's current length, where the next event would
    /// land).
    pub fn continue_replay(
        &mut self,
        live_handler: Option<&dyn ExternalFnHandler>,
    ) -> ReplayOutcome {
        if let Some(state) = self.pending_replay.take() {
            return self.drive_replay(state, live_handler);
        }
        // Advance-only: no recorded inputs left to consume.
        let handler = live_handler.unwrap_or(&FallbackHandler);
        let mut steps = 0usize;
        loop {
            if steps >= Self::REPLAY_STEP_BUDGET {
                self.journal.truncated = true;
                return ReplayOutcome::Failed {
                    at_event: self.journal.len(),
                    reason: FailReason::Budget,
                };
            }
            steps += 1;
            match self.advance_with(handler) {
                Ok(StepOutcome::Line(line)) if line.is_terminal() => {
                    return ReplayOutcome::Replayed {
                        warnings: Vec::new(),
                    };
                }
                Ok(StepOutcome::Line(_)) => {}
                Ok(StepOutcome::AwaitingExternal) => {
                    let name = self
                        .story
                        .pending_external_name()
                        .map(str::to_owned)
                        .unwrap_or_default();
                    return ReplayOutcome::Failed {
                        at_event: self.journal.len(),
                        reason: FailReason::AwaitingExternal { name },
                    };
                }
                Err(e) => {
                    self.journal.truncated = true;
                    return ReplayOutcome::Failed {
                        at_event: self.journal.len(),
                        reason: runtime_fail(e),
                    };
                }
            }
        }
    }

    /// Maximum `advance` calls per replay stepping burst (unbounded-growth /
    /// no-hang guard).
    const REPLAY_STEP_BUDGET: usize = 100_000;

    /// Drive (or resume) a replay from its cursor `state`. On a resumable park
    /// ([`FailReason::AwaitingExternal`]) the state is retained in
    /// [`pending_replay`](Self::pending_replay); on divergence or terminal
    /// failure it is dropped (the journal is truncated at that point).
    fn drive_replay(
        &mut self,
        mut state: PendingReplay,
        live_handler: Option<&dyn ExternalFnHandler>,
    ) -> ReplayOutcome {
        loop {
            // All recorded inputs consumed: final trailing step (a story with
            // no choices, or content after the last input, only advances
            // here), then complete.
            let Some((at, _)) = state.remaining.front().cloned() else {
                if self.started {
                    let at = state.total_events.saturating_sub(1);
                    if let Err(park) =
                        self.replay_step_to_pause(state.mode, live_handler, &mut state.ext_queue)
                    {
                        return self.park_or_fail(state, at, park);
                    }
                }
                // Carry over the terminal checkpoint if the source had one.
                self.journal.checkpoint.clone_from(&state.source_checkpoint);
                return ReplayOutcome::Replayed {
                    warnings: state.warnings,
                };
            };

            // A Choice consumes a pause: step to it first (this is also where
            // a resumed drive picks up after its external was resolved).
            let next_is_choice = matches!(
                state.remaining.front().map(|(_, ev)| &ev.kind),
                Some(EventKind::Choice { .. })
            );
            if next_is_choice
                && let Err(park) =
                    self.replay_step_to_pause(state.mode, live_handler, &mut state.ext_queue)
            {
                return self.park_or_fail(state, at, park);
            }

            let Some((i, ev)) = state.remaining.pop_front() else {
                // Unreachable: `front` was `Some` above.
                continue;
            };
            match self.replay_apply(i, &ev, state.mode, &mut state.warnings) {
                Ok(()) => {}
                Err(outcome) => return outcome,
            }
        }
    }

    /// Park (retaining `state` for [`continue_replay`](Self::continue_replay))
    /// or fail terminally (truncating the rebuilt journal).
    fn park_or_fail(&mut self, state: PendingReplay, at: usize, park: StepPark) -> ReplayOutcome {
        match park {
            StepPark::Awaiting { name } => {
                self.pending_replay = Some(state);
                ReplayOutcome::Failed {
                    at_event: at,
                    reason: FailReason::AwaitingExternal { name },
                }
            }
            StepPark::Fail(reason) => {
                self.journal.truncated = true;
                ReplayOutcome::Failed {
                    at_event: at,
                    reason,
                }
            }
        }
    }

    /// Apply one recorded input during replay. `Err` is the terminal
    /// [`ReplayOutcome`] (divergence or failure).
    fn replay_apply(
        &mut self,
        i: usize,
        ev: &JournalEvent,
        mode: ExternalReplayMode,
        warnings: &mut Vec<ReplayWarning>,
    ) -> Result<(), ReplayOutcome> {
        match &ev.kind {
            EventKind::Start { path, args } => {
                if self.started {
                    return Err(self.diverge_at(i, ev, DivergenceFound::UnexpectedEvent));
                }
                self.started = true;
                self.journal.push(JournalEvent::new(EventKind::Start {
                    path: path.clone(),
                    args: args.clone(),
                }));
                if let Some(p) = path {
                    let jump = if args.is_empty() {
                        self.story.choose_path_string(p)
                    } else {
                        self.story.choose_path_string_with_args(p, args)
                    };
                    if jump.is_err() {
                        return Err(self.diverge_at(
                            i,
                            ev,
                            DivergenceFound::UnknownPath { path: p.clone() },
                        ));
                    }
                }
            }
            EventKind::External { .. } => {
                // Recorded mode: re-push the source event verbatim — the
                // rebuilt prefix is the SOURCE prefix, not a re-observed trace
                // (a mismatched entry falls back rather than diverging, so the
                // re-run may not have consumed it). Live mode journals actual
                // results during stepping instead, so nothing to copy here.
                if mode == ExternalReplayMode::Recorded {
                    self.journal.push(ev.clone());
                }
            }
            EventKind::Choice { index, label } => {
                self.replay_choice(i, *index, label.as_ref(), ev, warnings)?;
            }
            EventKind::SetVar { name, value } => {
                self.story.set_variable(name, value.clone());
                self.journal.push(ev.clone());
            }
            EventKind::GoToPath { path, args } => {
                let jump = if args.is_empty() {
                    self.story.choose_path_string(path)
                } else {
                    self.story.choose_path_string_with_args(path, args)
                };
                if jump.is_err() {
                    return Err(self.diverge_at(
                        i,
                        ev,
                        DivergenceFound::UnknownPath { path: path.clone() },
                    ));
                }
                self.journal.push(ev.clone());
            }
            EventKind::LoadState { state } => {
                self.story.load_state(state);
                self.journal.push(ev.clone());
            }
            EventKind::Call { name, args } => {
                // Journaled but isolated: re-invoke through the fallback
                // handler (recorded externals for a Call aren't separately
                // journaled; a live handler would be host-supplied). We use
                // the fallback so replay never blocks.
                let _ = self.story.call_function(name, args, &FallbackHandler);
                self.journal.push(ev.clone());
            }
        }
        Ok(())
    }

    /// Replay one `Choice` event (the driver has already stepped to the choice
    /// pause): range-check the recorded index against what the current program
    /// presents, emit a soft label-drift warning, then select. Returns
    /// `Err(outcome)` on divergence/failure.
    fn replay_choice(
        &mut self,
        at: usize,
        index: u32,
        label: Option<&String>,
        ev: &JournalEvent,
        warnings: &mut Vec<ReplayWarning>,
    ) -> Result<(), ReplayOutcome> {
        if !self.story.status_is_waiting_for_choice() {
            return Err(self.diverge_at(at, ev, DivergenceFound::NotWaitingForChoice));
        }
        let presented = self.story.pending_choices();
        let available = presented.len();
        let Some(current) = presented.iter().find(|c| c.index == index as usize) else {
            return Err(self.diverge_at(
                at,
                ev,
                DivergenceFound::ChoiceIndexOutOfRange { index, available },
            ));
        };
        // Label-drift soft warning (matching index, different text).
        if let Some(recorded) = label
            && recorded != &current.text
        {
            warnings.push(ReplayWarning::ChoiceLabelDrift {
                at_event: at,
                index,
                recorded: recorded.clone(),
                found: current.text.clone(),
            });
        }
        if let Err(e) = self.story.choose(index as usize) {
            self.journal.truncated = true;
            return Err(ReplayOutcome::Failed {
                at_event: at,
                reason: runtime_fail(e),
            });
        }
        self.journal.push(ev.clone());
        Ok(())
    }

    /// Step to the next pause during replay, serving externals per `mode`. In
    /// Live mode, inline-resolved results are journaled as they happen (the
    /// rebuilt journal is a re-observed trace). Returns `Err(park)` when a
    /// deferred external pauses the flow (resumable) or on a terminal failure.
    fn replay_step_to_pause(
        &mut self,
        mode: ExternalReplayMode,
        live_handler: Option<&dyn ExternalFnHandler>,
        ext_queue: &mut std::collections::VecDeque<(String, Vec<Value>, Value)>,
    ) -> Result<(), StepPark> {
        let mut steps = 0usize;
        loop {
            if steps >= Self::REPLAY_STEP_BUDGET {
                return Err(StepPark::Fail(FailReason::Budget));
            }
            steps += 1;
            let outcome = match mode {
                ExternalReplayMode::Recorded => {
                    let h = RecordedReplayHandler {
                        queue: std::cell::RefCell::new(ext_queue),
                    };
                    self.story.advance_with(&h)
                }
                ExternalReplayMode::Live => {
                    // Journal live results where the VM receives them, so the
                    // rebuilt journal reflects what actually fed this run.
                    let mut sink: Vec<(String, Vec<Value>, Value)> = Vec::new();
                    let outcome = {
                        let h = live_handler.unwrap_or(&FallbackHandler);
                        let jh = JournalingHandler::new(h, &mut sink);
                        self.story.advance_with(&jh)
                    };
                    for (name, args, result) in sink {
                        self.journal.push(JournalEvent::new(EventKind::External {
                            name,
                            args,
                            result,
                        }));
                    }
                    outcome
                }
            };
            match outcome {
                Ok(StepOutcome::Line(line)) => {
                    if line.is_terminal() {
                        return Ok(());
                    }
                }
                Ok(StepOutcome::AwaitingExternal) => {
                    let name = self
                        .story
                        .pending_external_name()
                        .map(str::to_owned)
                        .unwrap_or_default();
                    return Err(StepPark::Awaiting { name });
                }
                Err(e) => return Err(StepPark::Fail(runtime_fail(e))),
            }
        }
    }

    /// Truncate the rebuilt journal at the divergence point and build a
    /// `Diverged` outcome. The rebuilt journal keeps the **source prefix** as
    /// re-pushed so far (in recorded mode this includes source `External`
    /// events verbatim, even ones the re-run fell back on instead of
    /// consuming — see [`replay`](Self::replay)); the checkpoint is cleared
    /// because it described the source's terminal state, not this park point.
    fn diverge_at(
        &mut self,
        at: usize,
        expected: &JournalEvent,
        found: DivergenceFound,
    ) -> ReplayOutcome {
        self.journal.truncated = true;
        self.journal.checkpoint = None;
        ReplayOutcome::Diverged {
            at_event: at,
            expected: Box::new(expected.clone()),
            found,
        }
    }
}

fn runtime_fail(e: RuntimeError) -> FailReason {
    match e {
        RuntimeError::StepLimitExceeded(_) | RuntimeError::LineLimitExceeded(_) => {
            FailReason::Budget
        }
        other => FailReason::RuntimeError(other.to_string()),
    }
}
