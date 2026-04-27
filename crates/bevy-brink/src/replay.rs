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
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_log::{info, warn};
use brink_runtime::{Context, FallbackHandler, FastRng, FlowInstance, Line, RuntimeError};

use crate::asset::{BrinkProgram, BrinkStoryAsset, ProgramAsset};
use crate::event::BrinkFlowReset;
use crate::flow::{BrinkFlow, emit_event};
use crate::globals::BrinkGlobals;
use crate::line_tables::BrinkLineTables;
use crate::request::FlowStart;

/// Per-flow snapshot used to reconstruct the flow on hot-reload.
///
/// Inserted alongside [`BrinkFlow<M>`] by the fulfillment system when
/// the `dev` feature is enabled. Contains the globals snapshot at the
/// moment the flow was spawned, the start address, the story handle
/// (so we can find the new program after reload), and the running list
/// of choice selections.
#[derive(Component)]
pub struct BrinkReplayLog<M: Send + Sync + 'static = ()> {
    /// Snapshot of [`BrinkGlobals`](crate::BrinkGlobals)'s context taken at fulfillment.
    pub start_globals: Context,
    /// Where this flow began executing.
    pub start: FlowStart,
    /// The story this flow is bound to (for re-resolving the program
    /// asset after reload).
    pub story: Handle<BrinkStoryAsset>,
    /// Choices made so far, in order. Populated by
    /// [`BrinkFlow::choose_recording`].
    pub choices_made: Vec<usize>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkReplayLog<M> {
    pub(crate) fn new(
        start_globals: Context,
        start: FlowStart,
        story: Handle<BrinkStoryAsset>,
    ) -> Self {
        Self {
            start_globals,
            start,
            story,
            choices_made: Vec::new(),
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
///   1. Reset [`BrinkGlobals<M>`] from `log.start_globals`.
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
        &mut BrinkReplayLog<M>,
    )>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables: Res<BrinkLineTables<M>>,
    mut globals: Option<ResMut<BrinkGlobals<M>>>,
    mut commands: Commands,
) {
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

    let Some(globals) = globals.as_mut() else {
        return;
    };

    for (entity, mut flow, brink_program, log) in &mut flows {
        let Some(program_asset) = programs.get(&brink_program.handle) else {
            continue;
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
            commands.entity(entity).insert(BrinkFlow::<M>::new(root_flow));
            continue;
        };

        // Reset globals to the captured snapshot.
        globals.inner = log.start_globals.clone();

        // Replace the in-place flow with the freshly-built one.
        flow.inner = new_flow;

        // Replay each recorded choice. We step until a Choices line
        // (firing observer events for the new Text along the way), then
        // call choose. If we ever can't find a choice point for the
        // next recorded index, stop replay and leave the flow at the
        // last successful position.
        let mut replay_failed = false;
        for (i, &choice_idx) in log.choices_made.iter().enumerate() {
            if let Err(err) = step_to_next_choices::<M>(
                &mut flow.inner,
                &program_asset.program,
                &line_tables,
                &mut globals.inner,
                entity,
                &mut commands,
            ) {
                warn!(
                    "replay: failed to reach choice point {i} for entity {entity:?}: {err}; \
                     stopping replay"
                );
                replay_failed = true;
                break;
            }
            if let Err(err) = flow.inner.choose(&mut globals.inner, choice_idx) {
                warn!(
                    "replay: choose({choice_idx}) at step {i} for entity {entity:?}: {err}; \
                     stopping replay"
                );
                replay_failed = true;
                break;
            }
        }

        if !replay_failed {
            info!(
                "replay: rebuilt flow on entity {entity:?} from start={:?} +{} choice(s)",
                log.start,
                log.choices_made.len()
            );
        }
    }
}

/// Step the flow forward until we land on a `Choices` line, firing
/// observer events for every line produced along the way (so consumer
/// UIs see the replayed text exactly as if the player were re-playing
/// the choices).
fn step_to_next_choices<M: Send + Sync + 'static>(
    flow: &mut FlowInstance,
    program: &brink_runtime::Program,
    line_tables: &BrinkLineTables<M>,
    context: &mut Context,
    entity: bevy_ecs::entity::Entity,
    commands: &mut Commands,
) -> Result<(), RuntimeError> {
    const STEP_LIMIT: usize = 10_000;
    for _ in 0..STEP_LIMIT {
        let line = flow.step_single_line::<FastRng>(
            program,
            &line_tables.tables,
            context,
            &FallbackHandler,
            None,
        )?;
        emit_event::<M>(&line, entity, commands);
        match line {
            // Choices: ready for the next replayed pick.
            // Done / End: nothing to choose against; replay caller will
            // notice and stop iterating.
            Line::Choices { .. } | Line::Done { .. } | Line::End { .. } => return Ok(()),
            Line::Text { .. } => {}
        }
    }
    Err(RuntimeError::StepLimitExceeded(STEP_LIMIT as u64))
}
