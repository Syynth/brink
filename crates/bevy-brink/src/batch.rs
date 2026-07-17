//! Batch-mode flow advancement — the frame-start consistency semantics core
//! (`docs/effects-spec.md` §12.4; BH-2, #914). **Serial only**: no `unsafe`,
//! no task pool, no parallelism. This slice implements and *proves* the
//! frame-start guarantee in a serial driver first; BH-3 later replaces the
//! serial Step loop with a parallel one against the same semantics.
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
//!   Because each flow reads only the frame-start snapshot plus its own
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
//!   across flows. Batch mode steps each flow against a private frame-start
//!   snapshot of that shared world and buffers its writes; it does **not**
//!   route through a flow's private [`BrinkContext`](crate::BrinkContext)
//!   (`FlowLocal`) layer, which is flow-private by construction and so can
//!   never participate in a cross-flow race. Under the default all-`World`
//!   policy (every unit World-scoped — the common case, and what the property
//!   test and scenario harness exercise) this is exactly complete. A host that
//!   opts units into `Local` via a policy should keep those flows on the
//!   serial API for now; batch-mode `Local` routing is BH-3/BH-4 follow-up.
//! - **No prefetch, no parallelism.** World-access (`bind_brink_query`) and
//!   async bindings still park (`AwaitingExternal`); a parked flow is simply
//!   left for the plugin's existing resolver and re-collected next batch.
//!   §12.3 prefetch (synchronous world reads under a held borrow) is BH-3.
//! - **Per-flow snapshot clone.** Serial correctness here comes from cloning
//!   the frame-start world per flow. §12.2's "borrow, don't copy" (one
//!   `UnsafeWorldCell` scope, no clones) is the BH-3 optimization; this slice
//!   trades that throughput for a `unsafe`-free, serially-provable core.
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
use bevy_ecs::entity::Entity;
use bevy_ecs::query::Access;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_log::warn;
use brink_format::{DefinitionId, LineEntry, Value};
use brink_runtime::{
    ContextAccess, DriveOutcome, ExternalFnHandler, FallbackHandler, FastRng, FlowInstance, Line,
    Program, RuntimeError, World, WriteObserver,
};

use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::bindings::{BrinkBindings, TriggerFn};
use crate::capability::{CapabilityTable, ContainerAccessTable};
use crate::flow::{BrinkFlow, emit_event};
use crate::globals::BrinkGlobals;
use crate::line_tables::BrinkLocale;

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
    /// The flow's story's aggregate container access (union across all
    /// containers), from BH-1's [`CapabilityTable`]. `None` if no table is
    /// loaded for the story (no manifest/registry wired).
    access: Option<Access>,
}

/// Fold a story's whole [`ContainerAccessTable`] into one aggregate
/// [`Access`] — the conservative "what could any container of this flow's
/// story touch" set. BH-2 records this per flow; BH-3 narrows it to the
/// flow's currently-parked container.
fn aggregate_access(table: &ContainerAccessTable) -> Access {
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
/// The flow steps against a **private clone** of `frame_start` wrapped in an
/// [`ObservedContext`](brink_runtime::ObservedContext): reads resolve against
/// the clone (frame-start ⊕ this flow's own already-buffered writes — never a
/// peer's), writes mutate the clone (so the flow reads them back) *and* record
/// into `buf`. The clone is discarded; only `buf` (the ordered changeset)
/// survives to Apply.
fn step_flow(
    frame_start: &World,
    flow: &mut FlowInstance,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    handler: &dyn ExternalFnHandler,
    buf: &mut WriteBuffer,
) -> (Vec<Line>, bool, Option<RuntimeError>) {
    let mut scratch = frame_start.clone();
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
            writes_applied: 0,
            commands_applied: 0,
            _marker: PhantomData,
        }
    }
}

// ── The batch entry point ───────────────────────────────────────────────────

/// Batch-mode flow driver (§12.4; BH-2): advance every pending flow under
/// marker `M` as one batch turn with frame-start read pinning, per-flow
/// buffered writes/commands, and a deterministic flow-id-ordered Apply.
///
/// **Not auto-registered.** Like [`advance_flows`](crate::advance_flows), a
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
    reason = "bevy systems take Res/Query by value; the flow query tuple is inherently wide, and the phase inputs (globals, assets, bindings, capability table, report) are each a distinct system param"
)]
pub fn advance_batch<M: Send + Sync + 'static>(
    mut flows: Query<(Entity, &mut BrinkFlow<M>, &BrinkProgram<M>, &BrinkLocale<M>)>,
    globals: Option<ResMut<BrinkGlobals<M>>>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables_assets: Res<Assets<LineTablesAsset>>,
    bindings: Option<Res<BrinkBindings<M>>>,
    cap_table: Res<CapabilityTable<M>>,
    report: Option<ResMut<BrinkBatchReport<M>>>,
    mut commands: Commands,
) {
    let Some(mut globals) = globals else {
        return;
    };

    // ── Collect ── pending flows in flow-id (Entity) order. A stable total
    // order within the batch is all the frame-start guarantee needs: Step is
    // order-invariant, and Apply is deterministic in exactly this order.
    let mut collected: Vec<Entity> = flows
        .iter()
        .filter(|(_, flow, _, _)| !flow.inner.has_pending_external())
        .map(|(e, _, _, _)| e)
        .collect();
    collected.sort_unstable();

    // ── Frame-start snapshot ── the pinned world every flow reads this turn.
    let frame_start = globals.inner.clone();

    // ── Step ── advance each flow against the frame-start snapshot, buffering
    // writes + command triggers. Serial here; order-invariant by construction
    // (each flow reads only `frame_start` ⊕ its own buffer).
    let mut outcomes: Vec<FlowBatchOutcome> = Vec::with_capacity(collected.len());
    for &entity in &collected {
        let Ok((_, mut flow, program_ref, locale)) = flows.get_mut(entity) else {
            continue;
        };
        let Some(program_asset) = programs.get(&program_ref.handle) else {
            continue;
        };
        let Some(lt_asset) = line_tables_assets.get(&locale.handle) else {
            continue;
        };

        let access = cap_table
            .access_for(program_ref.handle.id())
            .map(aggregate_access);

        let mut buf = WriteBuffer::default();
        // A `BrinkBindings` handler buffers command triggers per flow (drained
        // after Step, flushed at Apply in flow-id order); with no bindings
        // registered, the fallback handler runs the in-story fallback bodies
        // and buffers nothing.
        let handler = bindings.as_deref().map(BrinkBindings::handler);
        let handler_ref: &dyn ExternalFnHandler = match &handler {
            Some(h) => h,
            None => &FallbackHandler,
        };
        let (lines, awaiting, error) = step_flow(
            &frame_start,
            &mut flow.inner,
            &program_asset.program,
            &lt_asset.tables,
            handler_ref,
            &mut buf,
        );
        let errored = if let Some(err) = &error {
            warn!(
                "batch step faulted for flow {entity:?} (story {:?}): {err}",
                program_ref.handle.id()
            );
            true
        } else {
            false
        };
        let triggers = handler.map(|h| h.take_queued()).unwrap_or_default();

        outcomes.push(FlowBatchOutcome {
            entity,
            story: program_ref.handle.id(),
            writes: buf,
            triggers,
            lines,
            awaiting,
            errored,
            access,
        });
    }

    // ── Apply ── flush buffered writes, then command triggers, then line
    // events, in flow-id order (`outcomes` is already flow-id-sorted, matching
    // `collected`). Write-write conflicts resolve by this order (§12.4).
    let mut report_flows = Vec::with_capacity(outcomes.len());
    let mut stepped = 0usize;
    let mut awaiting_count = 0usize;
    let mut errored_count = 0usize;
    let mut writes_applied = 0usize;
    let mut commands_applied = 0usize;
    for outcome in outcomes {
        outcome.writes.apply_to(&mut globals.inner);
        writes_applied += outcome.writes.writes.len();
        for trigger in outcome.triggers {
            commands.queue(trigger);
            commands_applied += 1;
        }
        for line in &outcome.lines {
            emit_event::<M>(line, outcome.entity, &mut commands);
        }
        if outcome.errored {
            errored_count += 1;
        } else if outcome.awaiting {
            awaiting_count += 1;
        } else {
            stepped += 1;
        }
        report_flows.push(FlowAccessRecord {
            entity: outcome.entity,
            story: outcome.story,
            access: outcome.access,
            awaiting: outcome.awaiting,
            errored: outcome.errored,
        });
    }

    if let Some(mut report) = report {
        report.flows = report_flows;
        report.stepped = stepped;
        report.awaiting = awaiting_count;
        report.errored = errored_count;
        report.writes_applied = writes_applied;
        report.commands_applied = commands_applied;
    }
}

#[cfg(test)]
#[expect(clippy::panic, reason = "tests assert via panic on the error arm")]
mod tests;
