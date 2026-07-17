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
use crate::capability::{CapabilityManifest, CapabilityRegistry, check_load_capability_gate};
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
#[expect(
    clippy::too_many_arguments,
    reason = "bevy system: flow/program/story/line-table state plus the #997 capability manifest+registry gate"
)]
#[expect(
    clippy::too_many_lines,
    reason = "bevy system: hot-reload reconstruction plus the #997 capability load-boundary gate"
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
    capability_manifest: Res<CapabilityManifest>,
    capability_registry: Res<CapabilityRegistry<M>>,
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

        // Issue #997 (the #912 load-boundary gate's sibling-path gap): the
        // reloaded program's capabilities must re-clear this marker's
        // registry before we reconstruct anything against it — a hot
        // reload that drops (or never had) a manifest-required capability
        // must fail exactly as loudly as the initial `fulfill_flow_requests`
        // load, not silently rebuild a flow that can no longer resolve its
        // externals. Checked before the `BrinkFlowReset` trigger fires, so
        // a rejected reload leaves the entity's existing (pre-reload) flow
        // untouched rather than resetting it and then aborting partway.
        let story_ident = log
            .story
            .path()
            .map_or_else(|| format!("{:?}", log.story.id()), ToString::to_string);
        if let Err(err) = check_load_capability_gate(
            &program_asset.program,
            &program_asset.effect_rows,
            &capability_manifest,
            &capability_registry,
            story_ident,
        ) {
            warn!("replay: {err}; leaving entity {entity:?} on its pre-reload flow");
            continue;
        }

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

// ── Issue #997: the #912 load-boundary capability gate must also cover ────
// this dev-only hot-reload reconstruction path, not just the initial
// `fulfill_flow_requests` load.
#[cfg(test)]
mod tests {
    use bevy_app::App;
    use bevy_asset::Assets;
    use bevy_ecs::component::Component;
    use bevy_ecs::prelude::*;

    use crate::capability::{
        BrinkCapabilityAppExt as _, CapabilityEffects, CapabilityManifest,
        CapabilityManifestExternal,
    };
    use crate::request::BrinkFlowRequest;
    use crate::test_support::{add_story_assets, compile_test_story};

    /// Counts `BrinkFlowReset<()>` triggers. `replay_on_reload` fires this
    /// *after* the capability gate clears (see the gate's placement ahead of
    /// the trigger in the function body) — so a count of 0 after a simulated
    /// reload means the gate refused the rebuild before touching the entity
    /// at all; a count of 1 means the reload proceeded normally.
    #[derive(Resource, Default)]
    struct ResetCount(u32);

    fn install_reset_counter(app: &mut App) {
        app.insert_resource(ResetCount::default());
        app.add_observer(
            |_: On<crate::event::BrinkFlowReset<()>>, mut count: ResMut<ResetCount>| {
                count.0 += 1;
            },
        );
    }

    /// Compile `source` and return `(Program, line tables, EffectRowEntry
    /// rows)` — unlike `test_support::compile_test_story`, this keeps the
    /// real compiled effect rows (rather than discarding them) so a test can
    /// exercise `missing_capabilities`/`check_load_capability_gate` against a
    /// program that actually calls a manifest-declared external.
    fn compile_with_effect_rows(
        source: &str,
    ) -> (
        brink_runtime::Program,
        Vec<Vec<brink_format::LineEntry>>,
        Vec<brink_format::EffectRowEntry>,
    ) {
        let source = source.to_string();
        let out = brink_compiler::compile("t.ink", move |p| {
            if p == "t.ink" {
                Ok(source.clone())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
        })
        .expect("test fixture should compile");
        let mut inkb = Vec::new();
        brink_format::write_inkb(&out.data, &mut inkb);
        let loaded = brink_format::read_inkb(&inkb).expect("read_inkb");
        let (program, tables) = brink_runtime::link(&loaded).expect("link");
        (program, tables, loaded.effect_rows)
    }

    /// A manifest declaring that the `get_position` external reads the
    /// `Transform` capability — shared by both tests below. Whether the
    /// reload is admitted or refused depends solely on whether the
    /// marker's `CapabilityRegistry` has `Transform` registered.
    fn install_transform_manifest(app: &mut App) {
        let mut manifest = CapabilityManifest::default();
        manifest.externals.push(CapabilityManifestExternal {
            name: "get_position".to_string(),
            effects: CapabilityEffects {
                reads: vec!["Transform".to_string()],
                writes: vec![],
                detect: std::collections::BTreeMap::new(),
            },
        });
        app.insert_resource(manifest);
    }

    #[derive(Component)]
    struct Transform;

    const V1_SOURCE: &str = "=== start ===\nhello\n-> END\n";
    const V2_SOURCE_CALLS_GET_POSITION: &str = "EXTERNAL get_position(id)\n=== start ===\n~ temp x = get_position(0)\nBRAND NEW WORDS\n-> END\n";

    /// Hot-reload with a missing capability fails loudly: reloading to a
    /// program version that calls an external requiring a capability this
    /// marker's registry never registered must refuse to rebuild the flow
    /// (no `BrinkFlowReset` fires), exactly as the initial load boundary
    /// (#912) already refuses to admit such a story in the first place.
    #[test]
    fn hot_reload_missing_capability_refuses_rebuild() {
        let mut app = App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(crate::BrinkPlugin::<()>::default());
        install_transform_manifest(&mut app);
        install_reset_counter(&mut app);
        // Deliberately never call `register_capability::<(), Transform>` —
        // this marker's registry has no `Transform` entry.

        let (program_v1, tables_v1, ctx_v1) = compile_test_story(V1_SOURCE);
        let story = add_story_assets(&mut app, program_v1, tables_v1, ctx_v1);
        app.world_mut().spawn(
            BrinkFlowRequest::<()>::builder()
                .story(story.clone())
                .build(),
        );
        app.update(); // fulfill

        let (program_v2, tables_v2, effect_rows_v2) =
            compile_with_effect_rows(V2_SOURCE_CALLS_GET_POSITION);

        let program_handle = {
            let stories = app.world().resource::<Assets<crate::BrinkStoryAsset>>();
            stories.get(&story).expect("story bundle").program.clone()
        };
        let line_tables_handle = {
            let stories = app.world().resource::<Assets<crate::BrinkStoryAsset>>();
            stories
                .get(&story)
                .expect("story bundle")
                .line_tables
                .clone()
        };
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<Assets<crate::asset::ProgramAsset>>();
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                slot.program = program_v2;
                slot.effect_rows = effect_rows_v2;
            }
        }
        {
            let mut tables = app
                .world_mut()
                .resource_mut::<Assets<crate::asset::LineTablesAsset>>();
            if let Some(mut slot) = tables.get_mut(&line_tables_handle) {
                slot.tables = tables_v2;
            }
        }

        // Two ticks: propagate the asset event, then flush any deferred
        // triggers — mirrors the pattern the existing hot-reload tests use.
        app.update();
        app.update();

        assert_eq!(
            app.world().resource::<ResetCount>().0,
            0,
            "a reload that would drop below the manifest-required Transform \
             capability must be refused before BrinkFlowReset fires — the \
             #912 hard-error boundary must hold on the replay path too"
        );
    }

    /// Normal reload is unaffected: the exact same reload as above, but with
    /// `Transform` registered on this marker's registry, must proceed and
    /// rebuild the flow exactly as before this gate was added to the replay
    /// path — proving the fix doesn't regress the ordinary hot-reload case.
    #[test]
    fn hot_reload_with_satisfied_capability_rebuilds_normally() {
        let mut app = App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(crate::BrinkPlugin::<()>::default());
        install_transform_manifest(&mut app);
        install_reset_counter(&mut app);
        app.register_capability::<(), Transform>("Transform");

        let (program_v1, tables_v1, ctx_v1) = compile_test_story(V1_SOURCE);
        let story = add_story_assets(&mut app, program_v1, tables_v1, ctx_v1);
        app.world_mut().spawn(
            BrinkFlowRequest::<()>::builder()
                .story(story.clone())
                .build(),
        );
        app.update(); // fulfill

        let (program_v2, tables_v2, effect_rows_v2) =
            compile_with_effect_rows(V2_SOURCE_CALLS_GET_POSITION);

        let program_handle = {
            let stories = app.world().resource::<Assets<crate::BrinkStoryAsset>>();
            stories.get(&story).expect("story bundle").program.clone()
        };
        let line_tables_handle = {
            let stories = app.world().resource::<Assets<crate::BrinkStoryAsset>>();
            stories
                .get(&story)
                .expect("story bundle")
                .line_tables
                .clone()
        };
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<Assets<crate::asset::ProgramAsset>>();
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                slot.program = program_v2;
                slot.effect_rows = effect_rows_v2;
            }
        }
        {
            let mut tables = app
                .world_mut()
                .resource_mut::<Assets<crate::asset::LineTablesAsset>>();
            if let Some(mut slot) = tables.get_mut(&line_tables_handle) {
                slot.tables = tables_v2;
            }
        }

        app.update();
        app.update();

        assert_eq!(
            app.world().resource::<ResetCount>().0,
            1,
            "a reload whose required capabilities are all registered must \
             proceed exactly as before — the gate must not block a \
             legitimate reload"
        );
    }
}
