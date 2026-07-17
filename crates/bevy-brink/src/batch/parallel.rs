//! BH-3: the **parallel Step phase** (`docs/effects-spec.md` §12.2–§12.4;
//! decision-log 2026-07-16 "sanctioned-unsafe exemption"; issues #927/#926).
//!
//! This is the project's **one** `unsafe` exemption. The workspace-wide
//! `unsafe_code` deny stands in every other module; here the parallel Step
//! phase drives access-disjoint flows on [`ComputeTaskPool`] through an
//! [`UnsafeWorldCell`] — bevy's own multi-threaded-executor primitive, reused
//! "one storey down" (§12.2). Every `unsafe` block below carries a written
//! safety argument grounded in per-flow entity/[`Access`](bevy_ecs::query::Access)-set
//! disjointness (BH-1's proof obligation).
//!
//! ## The determinism law — the standing behavioral witness
//!
//! The exemption is licensed on one non-negotiable law
//! ([`super::tests::parallel_equals_serial_*`]):
//!
//! > **parallel ≡ serial-in-flow-id-order, byte-identical.**
//!
//! It holds here **by construction**, not by luck: [`advance_batch_parallel`]
//! shares Collect, per-flow Step ([`super::step_one`]), and Apply
//! ([`super::apply_batch_writes`]) with the serial [`advance_batch`] verbatim.
//! The only difference is *where* the Step loop runs — the task pool instead
//! of the main thread. Because each flow steps against a **private clone** of
//! the frame-start world and buffers its writes (BH-2's proven core), the
//! Step phase touches no shared mutable state, so thread interleaving cannot
//! affect any flow's outcome; Apply then flushes every buffer in flow-id
//! order, so the converged world is a pure function of the flow-id order — the
//! same order the serial driver applies in.
//!
//! Per the ruling, **parallelism is a perf feature, never a correctness
//! dependency**: if the law ever fails to hold, quarantine this module and
//! route hosts back to the serial [`advance_batch`] (the ruled fallback).
//!
//! ## Scope fence (single-`()`-marker; #912 deferred)
//!
//! Like BH-1/BH-2, this slice is per-marker `M`: one batch turn drives every
//! pending flow under `M` against `M`'s shared world. Multi-marker scheduling
//! semantics await the #912 maintainer ruling and are **not** decided here.
//!
//! [`advance_batch`]: super::advance_batch

#![expect(
    unsafe_code,
    reason = "BH-3 sanctioned-unsafe module (decision-log 2026-07-16): the parallel Step phase drives access-disjoint flows on ComputeTaskPool through an UnsafeWorldCell, bevy's own multi-threaded-executor primitive. The workspace-wide unsafe_code deny stands everywhere else; every unsafe block carries a SAFETY argument grounded in per-flow entity/Access-set disjointness."
)]

use bevy_asset::{AssetId, Assets};
use bevy_ecs::entity::Entity;
use bevy_ecs::query::Access;
use bevy_ecs::system::Commands;
use bevy_ecs::world::{CommandQueue, World};
use bevy_log::warn;
use bevy_tasks::{ComputeTaskPool, TaskPool};
use brink_format::LineEntry;
use brink_runtime::{Program, World as BrinkWorld};

use super::{
    BrinkBatchReport, FlowBatchOutcome, aggregate_access, apply_batch_writes, flush_deferred,
    homes_any_local, step_one,
};
use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::bindings::BrinkBindings;
use crate::capability::CapabilityTable;
use crate::flow::BrinkFlow;
use crate::globals::{BrinkGlobals, BrinkWorldPolicy};
use crate::line_tables::BrinkLocale;

/// One collected flow's fully-resolved batch inputs — owned so it survives the
/// drop of the query/resource borrows that produced it (Collect runs on the
/// main thread; Step then re-resolves the shared asset refs under the
/// [`UnsafeWorldCell`]).
struct Prep {
    entity: Entity,
    story: AssetId<ProgramAsset>,
    line_tables: AssetId<LineTablesAsset>,
    /// `true` if this flow's policy homes state to `Local` (host-installed or
    /// compiled `#@local`) — the #925 guard skips it rather than stepping.
    is_local: bool,
    access: Option<Access>,
}

/// A non-skipped flow's shared, immutable Step inputs, resolved from the
/// [`Assets`] held under the [`UnsafeWorldCell`] for the Step scope.
struct Job<'w> {
    entity: Entity,
    story: AssetId<ProgramAsset>,
    program: &'w Program,
    tables: &'w [Vec<LineEntry>],
    access: Option<Access>,
}

/// **Parallel** batch-mode flow driver (§12.2–§12.4; BH-3): identical
/// semantics to [`advance_batch`](super::advance_batch), but the Step phase
/// runs on [`ComputeTaskPool`] with each flow's `FlowInstance` accessed through
/// an [`UnsafeWorldCell`] (bevy's own executor pattern). Collect, per-flow
/// Step, and the flow-id-ordered Apply are shared verbatim with the serial
/// driver, so the two are **byte-identical** (the determinism law).
///
/// This is an **exclusive system** (§12.5 "Level 2 v1": the exclusive-system
/// driver with internal per-flow parallelism). Like
/// [`advance_batch`](super::advance_batch) it is not auto-registered — a host
/// opts in when it wants parallel stepping:
///
/// ```ignore
/// app.add_systems(Update, advance_batch_parallel::<MyStory>);
/// ```
pub fn advance_batch_parallel<M: Send + Sync + 'static>(world: &mut World) {
    // No shared world for `M` yet → nothing to drive.
    if world.get_resource::<BrinkGlobals<M>>().is_none() {
        return;
    }

    // ── Collect (main thread) ── resolve every pending flow's batch inputs
    // into owned `Prep`s, so the query/resource borrows can be dropped before
    // the parallel Step takes the world cell. Flow-id (Entity) order is all the
    // frame-start guarantee needs.
    let mut query = world.query::<(Entity, &BrinkFlow<M>, &BrinkProgram<M>, &BrinkLocale<M>)>();
    let policy_local = world
        .get_resource::<BrinkWorldPolicy<M>>()
        .is_some_and(|p| homes_any_local(&p.policy));
    let programs = world.resource::<Assets<ProgramAsset>>();
    let line_tables_assets = world.resource::<Assets<LineTablesAsset>>();
    let cap_table = world.resource::<CapabilityTable<M>>();

    let mut preps: Vec<Prep> = query
        .iter(world)
        .filter(|(_, flow, _, _)| !flow.inner.has_pending_external())
        .filter_map(|(entity, _, program_ref, locale)| {
            // Match the serial driver's order: require both assets loaded
            // before deciding anything (an unloaded flow is neither stepped
            // nor skip-counted — it is simply left for a later turn).
            let story = program_ref.handle.id();
            let program_asset = programs.get(story)?;
            let line_tables = locale.handle.id();
            line_tables_assets.get(line_tables)?;
            let access = cap_table.access_for(story).map(aggregate_access);
            let is_local = policy_local || program_asset.program.has_local_defaults();
            Some(Prep {
                entity,
                story,
                line_tables,
                is_local,
                access,
            })
        })
        .collect();
    preps.sort_unstable_by_key(|p| p.entity);

    // Frame-start snapshot ── the pinned world every flow reads this turn.
    // Cloned before the world cell is taken (immutable resource read).
    let frame_start: BrinkWorld = world.resource::<BrinkGlobals<M>>().inner.clone();

    // ── Step (parallel) ── advance each non-skipped flow against
    // `frame_start` on the task pool. Skipped (Local-policy) flows produce a
    // skipped outcome without stepping (#925).
    let outcomes = parallel_step::<M>(world, &preps, &frame_start);

    // ── Apply (main thread, flow-id order) ── writes first (§12.4), then the
    // deferred command triggers + line events. Two passes so the `&mut World`
    // write borrow and the `Commands` flush borrow don't fight; they are
    // order-equivalent because commands/events defer regardless.
    let (result, deferred) = {
        let mut globals = world.resource_mut::<BrinkGlobals<M>>();
        apply_batch_writes(outcomes, &mut globals.inner)
    };
    let mut queue = CommandQueue::default();
    {
        let mut commands = Commands::new(&mut queue, world);
        flush_deferred::<M>(deferred, &mut commands);
    }
    queue.apply(world);

    if let Some(mut report) = world.get_resource_mut::<BrinkBatchReport<M>>() {
        report.record(result);
    }
}

/// Run the Step phase across `preps` in parallel and return every flow's
/// outcome in **flow-id order**. Skipped (Local-policy) flows are handled on
/// the main thread; the rest step concurrently on [`ComputeTaskPool`].
///
/// This is the sole `unsafe`-bearing function: it takes an [`UnsafeWorldCell`]
/// over `world` and, per task, obtains a non-aliasing `&mut FlowInstance` for a
/// distinct entity. See the per-block SAFETY arguments.
fn parallel_step<M: Send + Sync + 'static>(
    world: &mut World,
    preps: &[Prep],
    frame_start: &BrinkWorld,
) -> Vec<FlowBatchOutcome> {
    let mut outcomes: Vec<FlowBatchOutcome> = Vec::with_capacity(preps.len());

    // Local-policy flows (#925): skip (never step) on the main thread, warn,
    // record distinctly. Their BH-1 access is still reported.
    for prep in preps.iter().filter(|p| p.is_local) {
        warn!(
            "parallel batch skipping Local-policy flow {:?} (story {:?}): \
             batch mode routes only shared World state — keep it on the serial API",
            prep.entity, prep.story
        );
        outcomes.push(FlowBatchOutcome::skipped_local(
            prep.entity,
            prep.story,
            prep.access.clone(),
        ));
    }

    let cell = world.as_unsafe_world_cell();

    // SAFETY: these are immutable resource reads, held only for the Step scope
    // below. Nothing in this function mutates `Assets<ProgramAsset>`,
    // `Assets<LineTablesAsset>`, or `BrinkBindings<M>`, and the parallel tasks
    // touch them shared-only — so the `&` references never alias a `&mut`.
    let programs = unsafe { cell.get_resource::<Assets<ProgramAsset>>() };
    // SAFETY: as above — immutable resource read for the Step scope.
    let line_tables_assets = unsafe { cell.get_resource::<Assets<LineTablesAsset>>() };
    // SAFETY: as above — immutable resource read for the Step scope.
    let bindings = unsafe { cell.get_resource::<BrinkBindings<M>>() };

    // Both asset stores must be present for any flow to have survived Collect;
    // if either vanished, there is nothing to step (the Local-skip outcomes
    // already collected still apply).
    let (Some(programs), Some(line_tables_assets)) = (programs, line_tables_assets) else {
        return finish(outcomes);
    };

    // Resolve the shared, immutable Step inputs for every non-skipped flow.
    let jobs: Vec<Job> = preps
        .iter()
        .filter(|p| !p.is_local)
        .filter_map(|prep| {
            let program = &programs.get(prep.story)?.program;
            let tables = &line_tables_assets.get(prep.line_tables)?.tables;
            Some(Job {
                entity: prep.entity,
                story: prep.story,
                program,
                tables,
                access: prep.access.clone(),
            })
        })
        .collect();

    let pool = ComputeTaskPool::get_or_init(TaskPool::default);
    let stepped: Vec<FlowBatchOutcome> = pool
        .scope(|scope| {
            for job in &jobs {
                scope.spawn(async move {
                    // SAFETY: `job.entity` is unique across `jobs` (Collect
                    // deduplicates by flow-id via the entity-keyed query and a
                    // stable sort), so `get_mut::<BrinkFlow<M>>` yields a
                    // non-aliasing `&mut` to *this* flow's component — no two
                    // tasks touch the same entity. Every other datum a task
                    // reads is shared-immutable (`frame_start`, `job.program`,
                    // `job.tables`, `bindings`). Crucially, no task writes the
                    // shared world during Step — `step_one` runs the flow
                    // against a private clone of `frame_start` and buffers its
                    // writes (BH-2's core) — so the concurrent component
                    // accesses are provably disjoint (BH-1's Access-set
                    // disjointness, made unconditional here by the private
                    // clone). The cell outlives the scope (`frame_start`,
                    // `programs`, `bindings` are owned/borrowed above it).
                    let entity_cell = cell.get_entity(job.entity).ok()?;
                    // SAFETY: see the block comment above — a unique-entity,
                    // otherwise-immutable, no-shared-write access. `BrinkFlow`
                    // is a mutable component.
                    let mut flow = unsafe { entity_cell.get_mut::<BrinkFlow<M>>() }?;
                    Some(step_one::<M>(
                        frame_start,
                        &mut flow.inner,
                        job.program,
                        job.tables,
                        bindings,
                        job.story,
                        job.entity,
                        job.access.clone(),
                    ))
                });
            }
        })
        .into_iter()
        .flatten()
        .collect();

    outcomes.extend(stepped);
    finish(outcomes)
}

/// Sort the combined (skipped + stepped) outcomes into flow-id order — the
/// order Apply must flush in, and the order the serial driver produces.
fn finish(mut outcomes: Vec<FlowBatchOutcome>) -> Vec<FlowBatchOutcome> {
    outcomes.sort_unstable_by_key(FlowBatchOutcome::entity);
    outcomes
}
