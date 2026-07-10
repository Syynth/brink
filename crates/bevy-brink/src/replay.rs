//! Dev-mode replay log: capture per-flow start state + choices so the
//! plugin can rebuild flows on hot-reload without losing player progress.
//!
//! Available only when the `dev` feature is enabled. The plugin
//! automatically attaches a [`BrinkReplayLog<M>`] to every fulfilled
//! [`BrinkFlow<M>`](crate::BrinkFlow), and a reload-handler system
//! reacts to `AssetEvent::Modified<ProgramAsset>` by rebuilding each
//! tracked flow against the new bytecode + replaying recorded choices.
//!
//! To make choices replayable, call [`BrinkFlow::choose_recording`]
//! (rather than `flow.inner.choose`) — that path appends to the log.

use std::marker::PhantomData;

use bevy_asset::{AssetEvent, Assets, Handle};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::World;
use bevy_log::{info, warn};
use brink_format::LineEntry;
use brink_runtime::{
    ContextAccess, ExternalFnHandler, FastRng, FlowInstance, FlowLocal, ReplayHandler, ReplayMode,
    ReplayRecorder, RuntimeError,
};

use crate::asset::{BrinkProgram, BrinkStoryAsset, LineTablesAsset, ProgramAsset};
use crate::event::BrinkFlowReset;
use crate::flow::BrinkFlow;
use crate::globals::{BrinkContext, BrinkGlobals, flow_context_view};
use crate::request::FlowStart;

/// Global default [`ReplayMode`] for hot-reload replay (the shared
/// [`brink_runtime`] primitive). Override on a specific flow with
/// [`ReplayQueryModeOverride`].
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct BrinkReplayConfig {
    /// Replay mode used when a flow has no [`ReplayQueryModeOverride`].
    pub query_mode: ReplayMode,
}

/// Per-flow override of the global [`BrinkReplayConfig`] replay mode. Insert on
/// a flow entity to make that flow replay with a specific [`ReplayMode`].
#[derive(Component, Clone, Copy, Debug)]
pub struct ReplayQueryModeOverride(pub ReplayMode);

/// Per-flow log used to reconstruct the flow on hot-reload.
///
/// Inserted alongside [`BrinkFlow<M>`] by the fulfillment system when
/// the `dev` feature is enabled. Contains the start address, the story
/// handle (so we can find the new program after reload), and the running
/// list of choice selections + recorded external results. There is no
/// per-flow `World` snapshot to restore — every flow's `FlowLocal` starts
/// fresh at spawn (see the F6 AMENDMENT in
/// `docs/scoped-flow-state-spec.md`), so reconstruction resets the entity's
/// [`BrinkContext`](crate::BrinkContext) to a fresh [`FlowLocal`] and
/// re-walks against the (unreset, still-live) shared
/// [`BrinkGlobals`](crate::BrinkGlobals) `World`.
///
/// **Known limitation (dev-only):** because the shared `World` is never
/// reset, a `World`-scoped unit this flow already wrote during its original
/// play (e.g. a visit count bumped by entering a knot) is *not* undone
/// before the re-walk — the re-walk bumps it again. Under the all-`World`
/// default this means repeated hot-reloads can drift `World`-scoped
/// visit/turn counts upward each time. This is a real consequence of the
/// shared-by-default model (the pre-F6.2 per-flow-private-`World` replay
/// could safely reset-then-rewalk because nothing was shared); fully
/// solving it (e.g. a per-flow world-write journal to undo) is out of scope
/// for F6.2 — flag it if it becomes a practical problem.
#[derive(Component)]
pub struct BrinkReplayLog<M: Send + Sync + 'static = ()> {
    /// Where this flow began executing.
    pub start: FlowStart,
    /// The story this flow is bound to (for re-resolving the program
    /// asset after reload).
    pub story: Handle<BrinkStoryAsset>,
    /// Choices made so far, in order. Populated by
    /// [`BrinkFlow::choose_recording`].
    pub choices_made: Vec<usize>,
    /// External-call results recorded during live play (the shared
    /// [`ReplayRecorder`]), replayed back during reload reconstruction so
    /// query-gated branches resolve faithfully instead of via fallback.
    pub recorder: ReplayRecorder,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkReplayLog<M> {
    pub(crate) fn new(start: FlowStart, story: Handle<BrinkStoryAsset>) -> Self {
        Self {
            start,
            story,
            choices_made: Vec::new(),
            recorder: ReplayRecorder::new(),
            _marker: PhantomData,
        }
    }
}

/// Plugin-managed system: when `ProgramAsset` reloads (file watcher saw
/// a change), rebuild each tracked flow against the new bytecode and
/// replay any recorded choices to restore approximate position.
///
/// Behavior:
/// - For each entity with both `BrinkFlow<M>` and `BrinkReplayLog<M>`:
///   1. Reset the entity's [`BrinkContext<M>`] to a fresh, empty
///      [`FlowLocal`] (the shared [`BrinkGlobals<M>`] `World` is left as-is
///      — see [`BrinkReplayLog`]'s "known limitation" doc).
///   2. Resolve `log.start` against the new program; build fresh `FlowInstance`.
///   3. For each choice in `log.choices_made`: step until a `Choices` line
///      appears, then call `choose(idx)`. If anything fails (choice index
///      out of range, runtime error), warn and stop replaying.
///   4. Replace the entity's `BrinkFlow<M>` component.
///
/// If the new program no longer has the start address (e.g. user
/// renamed the knot), warn and leave the flow in a fresh-start state.
#[expect(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    reason = "bevy systems take Res/Query by value and have complex query tuples"
)]
pub fn replay_on_reload<M: Send + Sync + 'static>(
    mut events: MessageReader<AssetEvent<ProgramAsset>>,
    mut flows: Query<(
        Entity,
        &mut BrinkFlow<M>,
        &BrinkProgram<M>,
        &mut BrinkContext<M>,
        &mut BrinkReplayLog<M>,
    )>,
    globals: Option<ResMut<BrinkGlobals<M>>>,
    programs: Res<Assets<ProgramAsset>>,
    stories: Res<Assets<BrinkStoryAsset>>,
    line_tables_assets: Res<Assets<LineTablesAsset>>,
    mut commands: Commands,
) {
    let Some(mut globals) = globals else {
        return; // no flow has ever been fulfilled for this marker
    };
    // Drain events; we only care that *some* program changed. Per-flow
    // routing is handled by the BrinkProgram handle below.
    let mut any_modified = false;
    for event in events.read() {
        if matches!(event, AssetEvent::Modified { .. }) {
            any_modified = true;
        }
    }
    if !any_modified {
        return;
    }

    for (entity, mut flow, brink_program, mut context, log) in &mut flows {
        let Some(program_asset) = programs.get(&brink_program.handle) else {
            continue;
        };

        // Look up the new line tables via the story bundle. Without
        // this, the post-reload walk would read NEW program against
        // OLD tables and either render stale text or fail to resolve
        // new string IDs.
        let line_tables: &[Vec<LineEntry>] = match stories
            .get(&log.story)
            .and_then(|bundle| line_tables_assets.get(&bundle.line_tables))
        {
            Some(lt_asset) => &lt_asset.tables,
            None => continue,
        };

        // Tell consumers a rebuild is starting *before* we fire any
        // line-delivery events from replay. Triggers process in order,
        // so observers for BrinkFlowReset run first (typically clearing
        // UI state), then the per-line events repopulate.
        commands.trigger(BrinkFlowReset::<M>::new(entity));

        // Resolve start position against the (possibly changed) program.
        let new_flow_result = match &log.start {
            FlowStart::Root => Some(FlowInstance::new_at_root(&program_asset.program)),
            FlowStart::Address(name) => program_asset
                .program
                .find_address(name)
                .map(|(idx, _)| FlowInstance::new_at(&program_asset.program, idx)),
        };

        let Some((new_flow, _fresh_ctx)) = new_flow_result else {
            warn!(
                "replay: knot '{:?}' missing in reloaded program; entity {entity:?} will start at root",
                log.start
            );
            let (root_flow, _) = FlowInstance::new_at_root(&program_asset.program);
            commands
                .entity(entity)
                .insert(BrinkFlow::<M>::new(root_flow));
            continue;
        };

        // Reset the per-flow FlowLocal to fresh/empty — every flow's local
        // layer starts empty at spawn, so a rebuild starts the same way.
        // The shared World is deliberately left untouched (see
        // `BrinkReplayLog`'s "known limitation" doc).
        context.inner = FlowLocal::new();

        // Replace the in-place flow with the freshly-built one.
        flow.inner = new_flow;

        // Split the log so the recorded externals can drive replay (via a
        // single `ReplayHandler` over the whole re-walk) while we read the
        // recorded choices from a disjoint field. Recorded during live play
        // by `advance_flow`; fed back here so query-gated branches resolve
        // faithfully instead of via fallback (and recorded effects don't
        // re-fire — replay re-executes nothing). Uncovered / divergent calls
        // fall through to the ink fallback body, exactly as before.
        let log = log.into_inner();
        let replay = ReplayHandler::new(&mut log.recorder);

        // Replay each recorded choice *silently* — we step the VM
        // through to each choice point and consume the choice without
        // firing observer events. The events would mislead consumers
        // into thinking those intermediate choice points are the
        // current state, but they're bookkeeping; the actual current
        // state is whatever comes after the last choose.
        let mut replay_failed = false;
        for (i, &choice_idx) in log.choices_made.iter().enumerate() {
            let mut view = flow_context_view(&mut globals, &mut context);
            if let Err(err) = step_to_next_choices(
                &mut flow.inner,
                &program_asset.program,
                line_tables,
                &mut view,
                &replay,
            ) {
                warn!(
                    "replay: failed to reach choice point {i} for entity {entity:?}: {err}; \
                     stopping replay"
                );
                replay_failed = true;
                break;
            }
            let mut view = flow_context_view(&mut globals, &mut context);
            if let Err(err) = flow.inner.choose(&mut view, choice_idx) {
                warn!(
                    "replay: choose({choice_idx}) at step {i} for entity {entity:?}: {err}; \
                     stopping replay"
                );
                replay_failed = true;
                break;
            }
        }

        if replay_failed {
            continue;
        }

        // Now advance until the next terminal *with* events firing, so
        // the UI sees the user's current page in the new program.
        let mut view = flow_context_view(&mut globals, &mut context);
        match flow.advance_until_terminal(
            &program_asset.program,
            line_tables,
            &mut view,
            &replay,
            entity,
            &mut commands,
        ) {
            Ok(_) => {
                info!(
                    "replay: rebuilt flow on entity {entity:?} from start={:?} +{} choice(s)",
                    log.start,
                    log.choices_made.len()
                );
            }
            Err(err) => {
                warn!("replay: advance after replay failed on entity {entity:?}: {err}");
            }
        }
    }
}

/// Step the flow forward silently until we land on a terminal line
/// (`Choices`, `Done`, or `End`).
///
/// Used during replay reconstruction: we walk the new bytecode to each
/// choice point so we can re-apply the recorded selection. No observer
/// events are fired — these intermediate steps are bookkeeping, not
/// the user's current state. The post-replay `advance_until_terminal`
/// in [`replay_on_reload`] is what fires events for the actual current
/// page.
///
/// Delegates to the shared Layer-2 [`FlowInstance::drive_to_terminal`] op
/// (F6.2) — the produced `Line`s are discarded (this walk is silent by
/// design), but the shared loop is what gives it the bounded
/// [`FlowInstance::LINE_LIMIT`] safety cap instead of a hand-rolled one.
/// `ReplayHandler` (the only handler this is ever called with) never
/// defers, so `drive_to_terminal`'s "errors instead of pausing on a
/// deferred external" behavior never actually triggers here.
fn step_to_next_choices(
    flow: &mut FlowInstance,
    program: &brink_runtime::Program,
    line_tables: &[Vec<LineEntry>],
    context: &mut (impl ContextAccess + ?Sized),
    handler: &dyn ExternalFnHandler,
) -> Result<(), RuntimeError> {
    flow.drive_to_terminal::<FastRng>(program, line_tables, context, handler, None)?;
    Ok(())
}

/// Take the flow's [`ReplayRecorder`] out of its [`BrinkReplayLog<M>`], leaving
/// an empty one behind. Returns `None` for an entity with no replay log (not a
/// dev-tracked flow).
///
/// Paired with [`put_recorder`]: an exclusive `&mut World` driver
/// ([`advance_flow`](crate::advance_flow)) takes the recorder, wraps its handler
/// with a [`RecordingHandler`](brink_runtime::RecordingHandler) (and records
/// out-of-band query results) for the duration of the pass, then puts it back —
/// avoiding holding the component borrowed across the `run_system_with`
/// re-borrows of the World.
pub(crate) fn take_recorder<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
) -> Option<ReplayRecorder> {
    world
        .get_mut::<BrinkReplayLog<M>>(entity)
        .map(|mut log| std::mem::take(&mut log.recorder))
}

/// Restore a recorder taken by [`take_recorder`] into the flow's
/// [`BrinkReplayLog<M>`]. A no-op if the log is gone (entity despawned or the
/// component removed mid-pass).
pub(crate) fn put_recorder<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    recorder: ReplayRecorder,
) {
    if let Some(mut log) = world.get_mut::<BrinkReplayLog<M>>(entity) {
        log.recorder = recorder;
    }
}

/// Record one out-of-band external result into the flow's [`BrinkReplayLog<M>`]
/// recorder, if the entity has one. Used by the world-access / async / task
/// resolve sites — which resolve *after* the VM parks (`ExternalResult::Pending`)
/// and so supply their value here rather than through the inline
/// [`RecordingHandler`](brink_runtime::RecordingHandler).
///
/// Always recording (for any dev-tracked flow) is safe even when the flow is
/// driven by a non-recording `step_one`: a partial recording simply diverges to
/// the ink fallback body earlier during replay (never feeding a misaligned
/// value), so it is never worse than recording nothing.
pub(crate) fn record_external<M: Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    name: &str,
    args: &[brink_format::Value],
    result: &brink_format::Value,
) {
    if let Some(mut log) = world.get_mut::<BrinkReplayLog<M>>(entity) {
        log.recorder.record(name, args, result);
    }
}
