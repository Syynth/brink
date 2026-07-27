//! Batch-mode flow advancement — the frame-start consistency semantics core
//! (`docs/effects-spec.md` §12.4; BH-2, #914).
//!
//! The Collect / Step / Apply phase model, the per-flow buffered writes, and
//! the flow-id-ordered Apply live here and are `unsafe`-free. Two drivers use
//! them: the serial [`advance_batch`] (this module) and the **parallel**
//! [`advance_batch_parallel`](parallel::advance_batch_parallel) (BH-3, #927 —
//! the sanctioned-unsafe [`parallel`] submodule). Per-flow Step and the
//! flow-id-ordered Apply are literally the same functions called from both
//! drivers; each driver walks its own Collect query (one as a system param,
//! one against a raw `&mut World`) and the two must be kept filter-identical
//! by hand (#1633 is the standing example of what happens when that drifts).
//! Together they make the drivers **byte-identical** by construction (the
//! determinism law): the parallel driver only moves the Step *loop* onto
//! [`ComputeTaskPool`](bevy_tasks::ComputeTaskPool); every flow still steps
//! against its own read-only view of the frame-start world and writes only
//! into its own buffer, so thread interleaving cannot affect any outcome, and
//! Apply flushes in flow-id order either way.
//!
//! ## What batch mode changes
//!
//! The serial API ([`BrinkFlow::advance_until_terminal`](crate::BrinkFlow),
//! [`advance_flow`](crate::advance_flow), etc.) keeps **immediate
//! visibility**: a flow's write to a shared [`BrinkWorld`](crate::BrinkWorld)
//! cell is visible to the very next flow stepped in the same frame, and a
//! command binding fires as soon as the handler is flushed. That is the
//! documented *serial mode* and is untouched here.
//!
//! **Batch mode** ([`advance_batch`]) drives N pending flows as one batch
//! turn with three phases (§12.1):
//!
//! - **Collect** — gather the pending flows (loaded program + line tables,
//!   not paused on a deferred external), in a deterministic **flow-id order**
//!   (bevy [`Entity`] order — a fixed total order within a batch).
//! - **Step** — advance each flow against the **frame-start world**: reads
//!   pin to the shared world's state as it was at the start of the batch
//!   turn (a peer's same-frame write is *not* visible — it lands next frame,
//!   double-buffered / simulation-tick semantics); World-scoped writes and
//!   command triggers **buffer** per flow instead of applying immediately.
//!   Because each flow reads only the frame-start state plus its own
//!   buffered writes, its produced lines and its buffer are a pure function
//!   of (frame-start, that flow's own parked state) — **independent of the
//!   order flows are stepped in**. That is the order-invariance property BH-2
//!   is gated on (see [`tests`]).
//! - **Apply** — flush every flow's buffered writes, then its buffered
//!   command triggers, then its line events, in **flow-id order**. Write-write
//!   conflicts resolve deterministically by apply order (§12.4: "even
//!   write-write is deterministic by apply order"), so no conflict
//!   partitioning is needed.
//!
//! ## Scope of this slice (honest)
//!
//! - **World-scoped state only.** Frame-start consistency is a property of the
//!   *shared* [`BrinkWorld`](crate::BrinkWorld) — the only state visible
//!   across flows. Batch mode steps each flow through a borrowed,
//!   read-only view pinned to that shared world's frame-start state (a
//!   private per-flow write overlay on top — see "Borrowed frame-start reads"
//!   below) and buffers its writes; it does **not** route through a flow's
//!   private [`BrinkContext`](crate::BrinkContext)
//!   (`FlowLocal`) layer, which is flow-private by construction and so can
//!   never participate in a cross-flow race. Under the default all-`World`
//!   policy (every unit World-scoped — the common case, and what the property
//!   test and scenario harness exercise) this is exactly complete. A host that
//!   opts units into `Local` via a policy should keep those flows on the
//!   serial API. **BH-3 (#925) now *guards* this** rather than leaving it
//!   silent: a Local-policy flow (host-installed or compiled `#@local`) is
//!   skipped with a `warn!` and counted in
//!   [`BrinkBatchReport::skipped_local`], never stepped — the full `Local`
//!   routing itself remains a BH-4 follow-up.
//! - **No prefetch.** World-access (`bind_brink_query`) and async bindings
//!   still park (`AwaitingExternal`); a parked flow is simply left for the
//!   plugin's existing resolver and re-collected next batch. §12.3 prefetch
//!   (synchronous world reads under a held borrow) is a later slice.
//! - **Borrowed frame-start reads (§12.2, #937).** Both drivers step each
//!   flow through a [`FrameStartView`] — a shared-immutable *borrow* of the
//!   frame-start world plus a private write overlay — not a per-flow clone.
//!   That is what makes the concurrent Step trivially race-free (no task
//!   writes shared state) at `O(1)` per flow instead of `O(world size)`.
//!   §12.3 **prefetch** (synchronous world reads served under the same held
//!   borrow) remains a later slice; see "No prefetch" above.
//!
//! ## Capability bookkeeping (BH-1 wiring)
//!
//! Each batch turn consults the [`CapabilityTable`](crate::CapabilityTable)
//! (BH-1, #906) for every stepped flow's story and records the flow's
//! aggregate container [`Access`] into [`BrinkBatchReport`]. BH-2 only
//! *records* it (the serial Step loop needs no disjointness proof); BH-3's
//! parallel Step consumes exactly this bookkeeping to prove access-disjoint
//! flows may advance concurrently.

use std::marker::PhantomData;

use bevy_asset::{AssetId, Assets};
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::change_detection::Tick;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::Access;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_log::warn;
use brink_format::{DefinitionId, LineEntry, Value};
use brink_runtime::{
    ContextAccess, DriveOutcome, ExternalFnHandler, FallbackHandler, FastRng, FlowInstance,
    FrameStartView, Line, Program, RuntimeError, Scope, World, WorldPolicy, WriteObserver,
};

use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::bindings::{BrinkBindings, TriggerFn};
use crate::capability::{CapabilityTable, ContainerAccessTable};
use crate::flow::{BrinkFlow, emit_event};
use crate::globals::{BrinkGlobals, BrinkWorldPolicy};
use crate::line_tables::BrinkLocale;
use crate::sleep::FlowSleep;
use crate::wake_delta::{BrinkWorldDelta, WorldDelta};

/// BH-3's sanctioned-unsafe parallel Step phase (`ComputeTaskPool` +
/// `UnsafeWorldCell`). The workspace-wide `unsafe_code` deny stands
/// everywhere else; this is the one exempt module.
pub mod parallel;

/// Does a host-installed [`WorldPolicy`] home **any** unit of story-state to
/// [`Scope::Local`]? Batch mode ([`advance_batch`], [`advance_batch_parallel`])
/// routes only the shared [`World`]; a flow whose policy homes anything to
/// `Local` reads/writes a per-flow `FlowLocal` layer batch mode never wraps,
/// so batching it would silently drop those reads/writes. This is the cheap
/// interim guard's host-side half (#925): the whole batch shares one
/// `BrinkWorldPolicy<M>`, so a single check gates every flow under `M`; the
/// per-story compiled-`#@local` half rides [`Program::has_local_defaults`].
pub(crate) fn homes_any_local(policy: &WorldPolicy) -> bool {
    policy.default == Scope::Local
        || policy.turn_index == Scope::Local
        || policy.rng == Scope::Local
        || policy.overrides.values().any(|s| *s == Scope::Local)
}

// ── Buffered world writes ───────────────────────────────────────────────────

/// One buffered mutation to the shared world, captured during a flow's batch
/// Step and replayed at Apply. Every variant carries an **absolute** value
/// (increments are captured as the resulting count in the flow's own
/// frame-start snapshot), so replaying the ordered list onto the shared world
/// is a deterministic, idempotent-per-field sequence of sets.
#[derive(Debug, Clone)]
pub(crate) enum WorldWrite {
    Global(u32, Value),
    VisitCount(DefinitionId, u32),
    TurnCount(DefinitionId, u32),
    TurnIndex(u32),
    RngSeed(i32),
    PreviousRandom(i32),
}

impl WorldWrite {
    /// Replay this buffered write onto `target` (the shared world, at Apply).
    fn apply(&self, target: &mut World) {
        match *self {
            WorldWrite::Global(idx, ref value) => target.set_global(idx, value.clone()),
            WorldWrite::VisitCount(id, count) => target.set_visit_count(id, count),
            WorldWrite::TurnCount(id, turn) => target.set_turn_count(id, turn),
            WorldWrite::TurnIndex(index) => target.set_turn_index(index),
            WorldWrite::RngSeed(seed) => target.set_rng_seed(seed),
            WorldWrite::PreviousRandom(val) => target.set_previous_random(val),
        }
    }
}

/// A [`WriteObserver`] that records every world mutation a flow makes during
/// its batch Step into an ordered buffer. Wrapped around the flow's private
/// frame-start snapshot via
/// [`ObservedContext`](brink_runtime::ObservedContext), so the snapshot stays
/// self-consistent (the flow reads back its own writes) while the buffer
/// captures the changeset for flow-id-ordered Apply.
#[derive(Default)]
pub(crate) struct WriteBuffer {
    writes: Vec<WorldWrite>,
}

impl WriteBuffer {
    /// Replay the buffered writes onto `target` in capture order. Called at
    /// Apply, once per flow, with flows visited in flow-id order.
    fn apply_to(&self, target: &mut World) {
        for w in &self.writes {
            w.apply(target);
        }
    }

    /// Fold this flow's changeset into the turn's [`WorldDelta`] — the
    /// row-directed wake-dirtying ledger (issue #1146). A global write is
    /// recorded per **slot index** (so a condition reading another cell stays
    /// inert); every other variant is bookkeeping, which effect rows model no
    /// read of, so they collapse into the one coarse bit.
    fn record_into(&self, delta: &mut WorldDelta) {
        for w in &self.writes {
            match *w {
                WorldWrite::Global(idx, _) => delta.note_global(idx),
                WorldWrite::VisitCount(..)
                | WorldWrite::TurnCount(..)
                | WorldWrite::TurnIndex(_)
                | WorldWrite::RngSeed(_)
                | WorldWrite::PreviousRandom(_) => delta.note_bookkeeping(),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.writes.len()
    }
}

impl WriteObserver for WriteBuffer {
    fn on_set_global(&mut self, idx: u32, value: &Value) {
        self.writes.push(WorldWrite::Global(idx, value.clone()));
    }
    fn on_increment_visit(&mut self, id: DefinitionId, new_count: u32) {
        self.writes.push(WorldWrite::VisitCount(id, new_count));
    }
    fn on_set_visit_count(&mut self, id: DefinitionId, count: u32) {
        self.writes.push(WorldWrite::VisitCount(id, count));
    }
    fn on_set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.writes.push(WorldWrite::TurnCount(id, turn));
    }
    fn on_increment_turn_index(&mut self, new_value: u32) {
        self.writes.push(WorldWrite::TurnIndex(new_value));
    }
    fn on_set_turn_index(&mut self, index: u32) {
        self.writes.push(WorldWrite::TurnIndex(index));
    }
    fn on_set_rng_seed(&mut self, new_seed: i32) {
        self.writes.push(WorldWrite::RngSeed(new_seed));
    }
    fn on_set_previous_random(&mut self, new_val: i32) {
        self.writes.push(WorldWrite::PreviousRandom(new_val));
    }
}

// ── Per-flow batch outcome ──────────────────────────────────────────────────

/// The result of stepping one flow in a batch turn — everything Apply needs
/// to flush it, plus the capability bookkeeping BH-3 will consume. Ordered by
/// flow-id at Apply.
pub(crate) struct FlowBatchOutcome {
    entity: Entity,
    story: AssetId<ProgramAsset>,
    writes: WriteBuffer,
    triggers: Vec<TriggerFn>,
    lines: Vec<Line>,
    /// `true` if the flow paused on a deferred external during Step (parked,
    /// not advanced to terminal) — left for the plugin's existing resolver.
    awaiting: bool,
    /// `true` if `step_flow` returned a [`RuntimeError`] (e.g. the
    /// [`FlowInstance::LINE_LIMIT`] budget tripping `LineLimitExceeded`).
    /// Mutually exclusive with `awaiting`: an errored flow reached neither a
    /// terminal line nor a deferred-external park this turn, and must not be
    /// counted as either at Apply.
    errored: bool,
    /// `true` if the flow was **skipped**, not stepped, because its policy
    /// homes state to [`Scope::Local`] (the #925 guard). A skipped flow
    /// produced no lines, buffered no writes, and is not parked — it is left
    /// untouched for the serial API and counted distinctly, never folded into
    /// `stepped`/`awaiting`/`errored`.
    skipped_local: bool,
    /// The flow's story's aggregate container access (union across all
    /// containers), from BH-1's [`CapabilityTable`]. `None` if no table is
    /// loaded for the story (no manifest/registry wired).
    access: Option<Access>,
}

impl FlowBatchOutcome {
    /// Build the outcome for a flow **skipped** by the Local-policy guard
    /// (#925): no Step ran, so no lines/writes/triggers, `skipped_local` set.
    /// Its BH-1 `access` is still recorded for the batch report.
    pub(crate) fn skipped_local(
        entity: Entity,
        story: AssetId<ProgramAsset>,
        access: Option<Access>,
    ) -> Self {
        Self {
            entity,
            story,
            writes: WriteBuffer::default(),
            triggers: Vec::new(),
            lines: Vec::new(),
            awaiting: false,
            errored: false,
            skipped_local: true,
            access,
        }
    }

    /// The flow-id (Entity) this outcome belongs to — Apply orders by it.
    pub(crate) fn entity(&self) -> Entity {
        self.entity
    }
}

/// Step exactly one flow for a batch turn and package its
/// [`FlowBatchOutcome`] — the per-flow work shared by the serial
/// ([`advance_batch`]) and parallel
/// ([`parallel::advance_batch_parallel`]) drivers, so both produce
/// **byte-identical** per-flow outcomes (the determinism law). Creates the
/// flow's own command-trigger-buffering handler, runs [`step_flow`] against
/// `frame_start`, drains the buffered triggers, and (on a [`RuntimeError`])
/// `warn!`s + flags `errored` so the fault is surfaced, never laundered into
/// a normal terminal step.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a distinct, already-resolved Step input (frame-start, flow, program, tables, bindings, story, entity, access); bundling them into a struct would just relocate the same fields with no clarity gain and force both call sites to build it"
)]
pub(crate) fn step_one<M: Send + Sync + 'static>(
    frame_start: &World,
    flow_inner: &mut FlowInstance,
    program: &Program,
    tables: &[Vec<LineEntry>],
    bindings: Option<&BrinkBindings<M>>,
    story: AssetId<ProgramAsset>,
    entity: Entity,
    access: Option<Access>,
) -> FlowBatchOutcome {
    let mut buf = WriteBuffer::default();
    // A `BrinkBindings` handler buffers command triggers per flow (drained
    // after Step, flushed at Apply in flow-id order); with no bindings
    // registered, the fallback handler runs the in-story fallback bodies and
    // buffers nothing. The handler is created here, used only by this one
    // flow, and dropped before the outcome is returned — never shared across
    // flows (so the parallel driver's per-task handler is race-free).
    let handler = bindings.map(BrinkBindings::handler);
    let handler_ref: &dyn ExternalFnHandler = match &handler {
        Some(h) => h,
        None => &FallbackHandler,
    };
    let (lines, awaiting, error) = step_flow(
        frame_start,
        flow_inner,
        program,
        tables,
        handler_ref,
        &mut buf,
    );
    let errored = if let Some(err) = &error {
        warn!("batch step faulted for flow {entity:?} (story {story:?}): {err}");
        true
    } else {
        false
    };
    let triggers = handler.map(|h| h.take_queued()).unwrap_or_default();

    FlowBatchOutcome {
        entity,
        story,
        writes: buf,
        triggers,
        lines,
        awaiting,
        errored,
        skipped_local: false,
        access,
    }
}

/// Fold a story's whole [`ContainerAccessTable`] into one aggregate
/// [`Access`] — the conservative "what could any container of this flow's
/// story touch" set. BH-2 records this per flow; BH-3 narrows it to the
/// flow's currently-parked container. `pub(crate)` so the host-side
/// ground-truth check (#938, `crate::ground_truth`) can compare a real
/// dispatch's observed access against the same declared aggregate BH-2/BH-3
/// already consume, rather than reimplementing the fold.
pub(crate) fn aggregate_access(table: &ContainerAccessTable) -> Access {
    let mut acc = Access::default();
    for container in table.values() {
        acc.extend(&container.access);
    }
    acc
}

// ── Step: advance one flow against the frame-start snapshot ─────────────────

/// Step one flow to a terminal line against `frame_start` (the pinned
/// frame-start world), buffering its world writes into `buf` and returning
/// the produced lines, whether it parked on a deferred external, and (on
/// fault) the [`RuntimeError`] that ended its turn early — e.g. the
/// [`FlowInstance::LINE_LIMIT`] budget tripping `LineLimitExceeded`. A fault
/// is never silently folded into a normal terminal outcome: the caller must
/// surface it (log + distinct bookkeeping), not count it as a stepped flow.
///
/// The flow steps against a **borrowed** view of `frame_start` — a
/// [`FrameStartView`], wrapped in an
/// [`ObservedContext`](brink_runtime::ObservedContext): reads resolve against
/// frame-start ⊕ this flow's own already-buffered writes (never a peer's),
/// writes land in the view's private overlay (so the flow reads them back)
/// *and* record into `buf`. The overlay is discarded; only `buf` (the ordered
/// changeset) survives to Apply.
///
/// The view **borrows** `frame_start` shared-immutably rather than cloning it
/// (§12.2 "borrow, don't copy"; issue #937). Both properties the phase model
/// rests on survive that swap unchanged, because the view is observationally
/// identical to the clone it replaces (`brink_runtime`'s
/// `equivalent_to_stepping_against_a_private_clone`): the flow still cannot
/// see a peer's same-turn write, and it still cannot mutate the shared world
/// during Step. What changes is only the cost — `O(1)` to open plus `O(cells
/// this flow wrote)`, instead of `O(world size)` per flow per turn.
fn step_flow(
    frame_start: &World,
    flow: &mut FlowInstance,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    handler: &dyn ExternalFnHandler,
    buf: &mut WriteBuffer,
) -> (Vec<Line>, bool, Option<RuntimeError>) {
    let mut scratch = FrameStartView::new(frame_start);
    let mut observed = brink_runtime::ObservedContext::new(&mut scratch, buf);
    let mut budget = FlowInstance::LINE_LIMIT;
    match flow.drive::<FastRng>(
        program,
        line_tables,
        &mut observed,
        handler,
        None,
        &mut budget,
    ) {
        Ok(DriveOutcome::Terminal(lines)) => (lines, false, None),
        Ok(DriveOutcome::AwaitingExternal(lines)) => (lines, true, None),
        Err(err) => (Vec::new(), false, Some(err)),
    }
}

// ── Batch report (BH-1 bookkeeping surface) ─────────────────────────────────

/// Per-flow record of a batch turn: which flow, its story, and the aggregate
/// container [`Access`] BH-1 computed for that story. Recorded by
/// [`advance_batch`] into [`BrinkBatchReport`].
#[derive(Debug, Clone)]
pub struct FlowAccessRecord {
    /// The flow entity (flow-id).
    pub entity: Entity,
    /// The flow's story program asset.
    pub story: AssetId<ProgramAsset>,
    /// Aggregate container access for the story, or `None` if BH-1 has no
    /// table loaded for it (no capability manifest/registry wired).
    pub access: Option<Access>,
    /// `true` if the flow parked on a deferred external this turn.
    pub awaiting: bool,
    /// `true` if the flow's Step faulted with a [`RuntimeError`] this turn
    /// (e.g. `LineLimitExceeded`) — logged via `bevy_log::warn!` and counted
    /// in [`BrinkBatchReport::errored`], never folded into `stepped`.
    pub errored: bool,
    /// `true` if the flow was skipped by the Local-policy guard (#925) — its
    /// policy homes state to [`Scope::Local`], which batch mode does not
    /// route, so it was left for the serial API rather than stepped.
    pub skipped_local: bool,
}

/// Diagnostic record of the most recent [`advance_batch`] turn under marker
/// `M` — the access bookkeeping (BH-1 wiring) plus phase counts the scenario
/// harness (BH-B) and tests read. Overwritten each batch turn.
#[derive(Resource)]
pub struct BrinkBatchReport<M: Send + Sync + 'static = ()> {
    /// One record per flow stepped this turn, in flow-id order.
    pub flows: Vec<FlowAccessRecord>,
    /// Flows advanced to a terminal line this turn.
    pub stepped: usize,
    /// Flows that parked on a deferred external this turn.
    pub awaiting: usize,
    /// Flows whose Step faulted with a [`RuntimeError`] this turn (e.g. the
    /// [`FlowInstance::LINE_LIMIT`] budget tripping `LineLimitExceeded`).
    /// Disjoint from both `stepped` and `awaiting` — a faulted flow's turn
    /// produced no lines and is not parked, so it must not be silently
    /// counted as a normal terminal step.
    pub errored: usize,
    /// Flows skipped by the Local-policy guard this turn (#925): their policy
    /// homes state to [`Scope::Local`], which batch mode does not route.
    /// Disjoint from `stepped`/`awaiting`/`errored` — a skipped flow was never
    /// stepped, so it must not be counted as any of those.
    pub skipped_local: usize,
    /// Total buffered world writes applied this turn (across all flows).
    pub writes_applied: usize,
    /// Total buffered command triggers applied this turn (across all flows).
    pub commands_applied: usize,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkBatchReport<M> {
    fn default() -> Self {
        Self {
            flows: Vec::new(),
            stepped: 0,
            awaiting: 0,
            errored: 0,
            skipped_local: 0,
            writes_applied: 0,
            commands_applied: 0,
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkBatchReport<M> {
    /// Overwrite this report with the outcome of a batch turn's Apply phase.
    fn record(&mut self, result: BatchApplyResult) {
        self.flows = result.flows;
        self.stepped = result.stepped;
        self.awaiting = result.awaiting;
        self.errored = result.errored;
        self.skipped_local = result.skipped_local;
        self.writes_applied = result.writes_applied;
        self.commands_applied = result.commands_applied;
    }
}

// ── Apply: shared between the serial and parallel drivers ────────────────────

/// The counts + per-flow records an Apply pass produces — folded into a
/// [`BrinkBatchReport`] by whichever driver ran. Shared so the serial
/// ([`advance_batch`]) and parallel ([`parallel::advance_batch_parallel`])
/// drivers report identically.
///
/// `Default` is the all-zero/empty "nothing happened" result — used when a
/// turn collects zero flows, so the caller can skip ever touching
/// [`BrinkGlobals`](crate::globals::BrinkGlobals) (see the call sites: taking
/// `&mut` on an empty batch would still trip Bevy change detection, sending a
/// **spurious** "the World changed" signal to [`crate::sleep::mark_wake_dirty`]
/// on a turn where nothing did — issue #1082).
#[derive(Default)]
pub(crate) struct BatchApplyResult {
    pub flows: Vec<FlowAccessRecord>,
    pub stepped: usize,
    pub awaiting: usize,
    pub errored: usize,
    pub skipped_local: usize,
    pub writes_applied: usize,
    pub commands_applied: usize,
    /// Which shared-world cells this turn's writes actually touched — the
    /// row-directed wake-dirtying changeset (issue #1146), folded into the
    /// [`BrinkWorldDelta`] ledger by whichever driver ran. Empty for a turn
    /// that collected nothing (the `Default` "nothing happened" result).
    pub changed: WorldDelta,
}

/// One flow's deferred Apply work — the command triggers and line events that
/// flush through a [`Commands`] *after* all buffered writes have landed. Held
/// so the write pass (which needs `&mut World`) and the flush pass (which needs
/// `&mut Commands`) don't fight over the world borrow in the exclusive-system
/// parallel driver; the two passes are order-equivalent because commands and
/// events are deferred regardless of when they're queued within the turn.
pub(crate) struct DeferredFlush {
    entity: Entity,
    triggers: Vec<TriggerFn>,
    lines: Vec<Line>,
}

/// Apply pass 1 — flush every flow's buffered world writes onto `world` (the
/// shared [`BrinkGlobals`] world) in flow-id order (`outcomes` must already be
/// flow-id-sorted). Write-write conflicts resolve by this order (§12.4).
/// Builds the [`BatchApplyResult`] counts + per-flow records and hands back the
/// deferred command/event work for [`flush_deferred`] to queue.
pub(crate) fn apply_batch_writes(
    outcomes: Vec<FlowBatchOutcome>,
    world: &mut World,
) -> (BatchApplyResult, Vec<DeferredFlush>) {
    let mut flows = Vec::with_capacity(outcomes.len());
    let mut deferred = Vec::with_capacity(outcomes.len());
    let mut stepped = 0usize;
    let mut awaiting = 0usize;
    let mut errored = 0usize;
    let mut skipped_local = 0usize;
    let mut writes_applied = 0usize;
    let mut commands_applied = 0usize;
    let mut changed = WorldDelta::default();

    for outcome in outcomes {
        outcome.writes.apply_to(world);
        outcome.writes.record_into(&mut changed);
        writes_applied += outcome.writes.writes.len();
        commands_applied += outcome.triggers.len();
        if outcome.skipped_local {
            skipped_local += 1;
        } else if outcome.errored {
            errored += 1;
        } else if outcome.awaiting {
            awaiting += 1;
        } else {
            stepped += 1;
        }
        flows.push(FlowAccessRecord {
            entity: outcome.entity,
            story: outcome.story,
            access: outcome.access,
            awaiting: outcome.awaiting,
            errored: outcome.errored,
            skipped_local: outcome.skipped_local,
        });
        deferred.push(DeferredFlush {
            entity: outcome.entity,
            triggers: outcome.triggers,
            lines: outcome.lines,
        });
    }

    (
        BatchApplyResult {
            flows,
            stepped,
            awaiting,
            errored,
            skipped_local,
            writes_applied,
            commands_applied,
            changed,
        },
        deferred,
    )
}

/// Fold one batch turn's outcome into the marker's row-directed wake-dirtying
/// ledger (issue #1146) — the tail both drivers share.
///
/// `globals_changed_on_entry` is [`BrinkGlobals`]'s change bit **as read
/// before** this turn applied anything: `true` means somebody the ledger
/// cannot see (a host system, the serial driver, a direct
/// `BrinkGlobals::inner` write) touched the shared world since this driver
/// last ran, so the window stops being a complete account and the wake pass
/// must stay conservative. See `crate::wake_delta`'s attribution contract.
pub(crate) fn record_wake_delta<M: Send + Sync + 'static>(
    ledger: &mut BrinkWorldDelta<M>,
    result: &BatchApplyResult,
    globals_changed_on_entry: bool,
    globals_tick: Option<Tick>,
) {
    if globals_changed_on_entry {
        ledger.note_foreign();
    }
    ledger.record(&result.changed, globals_tick);
}

/// Apply pass 2 — queue every flow's buffered command triggers then its line
/// events, in flow-id order (`deferred` preserves the sort). Both are deferred
/// through `commands`, so this runs after all writes have already landed.
pub(crate) fn flush_deferred<M: Send + Sync + 'static>(
    deferred: Vec<DeferredFlush>,
    commands: &mut Commands,
) {
    for flush in deferred {
        for trigger in flush.triggers {
            commands.queue(trigger);
        }
        for line in &flush.lines {
            emit_event::<M>(line, flush.entity, commands);
        }
    }
}

// ── The batch entry point ───────────────────────────────────────────────────

/// Batch-mode flow driver (§12.4; BH-2): advance every pending flow under
/// marker `M` as one batch turn with frame-start read pinning, per-flow
/// buffered writes/commands, and a deterministic flow-id-ordered Apply.
///
/// **Not auto-registered.** Like [`advance_flow`](crate::advance_flow), a
/// host opts in explicitly when it wants the batched, frame-start-consistent
/// stepping semantics for its flows:
///
/// ```ignore
/// app.add_systems(Update, advance_batch::<MyStory>);
/// ```
///
/// See the module docs for the phase model and the scope of this slice.
#[expect(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "bevy systems take Res/Query by value; the flow query tuple is inherently wide, and the phase inputs (globals, policy, assets, bindings, capability table, report) are each a distinct system param"
)]
pub fn advance_batch<M: Send + Sync + 'static>(
    mut flows: Query<(
        Entity,
        &mut BrinkFlow<M>,
        &BrinkProgram<M>,
        &BrinkLocale<M>,
        Option<&FlowSleep<M>>,
    )>,
    globals: Option<ResMut<BrinkGlobals<M>>>,
    policy: Option<Res<BrinkWorldPolicy<M>>>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables_assets: Res<Assets<LineTablesAsset>>,
    bindings: Option<Res<BrinkBindings<M>>>,
    cap_table: Res<CapabilityTable<M>>,
    report: Option<ResMut<BrinkBatchReport<M>>>,
    wake_delta: Option<ResMut<BrinkWorldDelta<M>>>,
    mut commands: Commands,
) {
    let Some(mut globals) = globals else {
        return;
    };

    // Read *before* Apply takes `&mut globals.inner` (which sets the change
    // bit itself): did anything this driver cannot account for write the
    // shared world since this system last ran? That is what decides whether
    // this turn's changeset is usable as a complete account by the wake pass
    // (issue #1146 — see `crate::wake_delta`).
    let globals_changed_on_entry = globals.is_changed();

    // Host-installed policy: if it homes any unit to `Local`, batch mode can't
    // route those flows (#925) — every flow under `M` shares this one policy.
    let policy_local = policy.is_some_and(|p| homes_any_local(&p.policy));

    // ── Collect ── pending flows in flow-id (Entity) order. A stable total
    // order within the batch is all the frame-start guarantee needs: Step is
    // order-invariant, and Apply is deterministic in exactly this order.
    //
    // BH-4 (§13.1; #973): a flow under a `FlowSleep` policy that isn't woken
    // (parked / cancelled / faulted) is **skipped by Collect** — a parked
    // reactive-sleep flow costs zero per turn. A flow with no policy, or a
    // woken one, collects normally.
    let mut collected: Vec<Entity> = flows
        .iter()
        .filter(|(_, flow, _, _, sleep)| {
            !flow.inner.has_pending_external() && sleep.is_none_or(FlowSleep::wants_collect)
        })
        .map(|(e, _, _, _, _)| e)
        .collect();
    collected.sort_unstable();

    // ── Frame-start snapshot ── the pinned world every flow reads this turn.
    let frame_start = globals.inner.clone();

    // ── Step ── advance each flow against the frame-start snapshot, buffering
    // writes + command triggers. Serial here; order-invariant by construction
    // (each flow reads only `frame_start` ⊕ its own buffer).
    let mut outcomes: Vec<FlowBatchOutcome> = Vec::with_capacity(collected.len());
    for &entity in &collected {
        let Ok((_, mut flow, program_ref, locale, _)) = flows.get_mut(entity) else {
            continue;
        };
        let Some(program_asset) = programs.get(&program_ref.handle) else {
            continue;
        };
        let Some(lt_asset) = line_tables_assets.get(&locale.handle) else {
            continue;
        };

        let story = program_ref.handle.id();
        let access = cap_table.access_for(story).map(aggregate_access);

        // Local-policy guard (#925): skip (don't step) a flow whose policy —
        // host-installed or compiled `#@local` — homes state to `Local`, so
        // batch mode never silently drops its `FlowLocal` reads/writes.
        if policy_local || program_asset.program.has_local_defaults() {
            warn!(
                "batch skipping Local-policy flow {entity:?} (story {story:?}): \
                 batch mode routes only shared World state — keep it on the serial API"
            );
            outcomes.push(FlowBatchOutcome::skipped_local(entity, story, access));
            continue;
        }

        outcomes.push(step_one::<M>(
            &frame_start,
            &mut flow.inner,
            &program_asset.program,
            &lt_asset.tables,
            bindings.as_deref(),
            story,
            entity,
            access,
        ));
    }

    // ── Apply ── flush buffered writes, then command triggers, then line
    // events, in flow-id order (`outcomes` is already flow-id-sorted, matching
    // `collected`). Write-write conflicts resolve by this order (§12.4).
    //
    // A turn that collects nothing must not touch `globals` at all (issue
    // #1082): `&mut globals.inner` trips Bevy change detection the instant
    // it's taken, regardless of whether anything is actually written, and
    // `mark_wake_dirty` treats *any* `BrinkGlobals` change as "re-check every
    // Parked all-detect-capable policy" — so an empty turn would otherwise
    // manufacture a spurious wake-up signal on every single frame this system
    // runs, self-sustaining a persistent condition's re-evaluation long after
    // its one real dependency change was already consumed.
    let (result, deferred) = if outcomes.is_empty() {
        (BatchApplyResult::default(), Vec::new())
    } else {
        apply_batch_writes(outcomes, &mut globals.inner)
    };
    flush_deferred::<M>(deferred, &mut commands);

    if let Some(mut ledger) = wake_delta {
        record_wake_delta(
            &mut ledger,
            &result,
            globals_changed_on_entry,
            Some(globals.last_changed()),
        );
    }
    if let Some(mut report) = report {
        report.record(result);
    }
}

#[cfg(test)]
#[expect(clippy::panic, reason = "tests assert via panic on the error arm")]
mod tests;
