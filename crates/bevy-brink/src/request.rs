//! Request-component pattern for spawning flows.
//!
//! Consumers spawn an entity carrying a [`BrinkFlowRequest<M>`] and a
//! handle to a [`BrinkStoryAsset`](crate::BrinkStoryAsset). A
//! plugin-managed system ([`fulfill_flow_requests`]) waits for the
//! story's sub-assets to load, builds a `FlowInstance`, replaces the
//! request component with [`BrinkFlow<M>`](crate::BrinkFlow) +
//! [`BrinkProgram<M>`](crate::BrinkProgram), and bootstraps
//! [`BrinkGlobals<M>`](crate::BrinkGlobals) the first time.
//!
//! No polling, no readiness latches: the user just spawns the request
//! and lets the plugin fulfill it whenever assets become available.

use std::marker::PhantomData;

use bevy_asset::{Assets, Handle};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::query::Without;
use bevy_log::{error, warn};
use brink_runtime::FlowInstance;

use crate::asset::{
    BrinkProgram, BrinkStoryAsset, InitialGlobalsAsset, LineTablesAsset, ProgramAsset,
};
use crate::flow::BrinkFlow;
use crate::globals::BrinkGlobals;
use crate::line_tables::BrinkLineTables;

/// Where a freshly-spawned flow should begin executing.
#[derive(Default, Clone, Debug)]
pub enum FlowStart {
    /// File root — the program's first container. Suitable for trivial
    /// demos and tests; most games spawn at named knots instead.
    #[default]
    Root,
    /// Resolve a knot/stitch name to a starting position. Errors at
    /// fulfillment if the name is unknown.
    Address(String),
}

/// Marker component requesting that this entity become a flow once its
/// story assets are available.
///
/// Spawn it with [`BrinkFlowRequest::builder`] (a `bon`-generated
/// builder) and let the fulfillment system handle the rest:
///
/// ```ignore
/// commands.spawn(
///     BrinkFlowRequest::<()>::builder()
///         .story(asset_server.load("dialogue.ink"))
///         .start(FlowStart::Address("intro_scene".into()))
///         .build(),
/// );
/// ```
///
/// The fulfillment system removes this component and inserts
/// [`BrinkFlow<M>`](crate::BrinkFlow) + [`BrinkProgram<M>`](crate::BrinkProgram)
/// once the program, line tables, and initial-globals subassets are all
/// loaded. Mutating the request after fulfillment is a no-op (in debug
/// builds, a warning is emitted via [`warn_post_fulfillment_mutations`]).
#[derive(Component, bon::Builder)]
pub struct BrinkFlowRequest<M: Send + Sync + 'static = ()> {
    /// The story to spawn this flow against.
    pub story: Handle<BrinkStoryAsset>,
    /// Where to start. Defaults to `FlowStart::Root`.
    #[builder(default)]
    pub start: FlowStart,
    #[builder(skip)]
    _marker: PhantomData<fn() -> M>,
}

/// Plugin-managed system: walk pending [`BrinkFlowRequest<M>`] entities,
/// fulfill each whose assets are ready, and bootstrap shared resources.
///
/// Behavior:
///
/// - Skips requests whose `BrinkStoryAsset` (or any of its sub-assets)
///   isn't loaded yet — the request just waits.
/// - On first fulfillment for marker `M`, inserts [`BrinkGlobals<M>`]
///   seeded from either the program's fresh defaults (for `FlowStart::Root`)
///   or from [`InitialGlobalsAsset`] (for `FlowStart::Address`). This
///   matters because root-start flows naturally run init code when they
///   advance, while named-knot flows skip init and need post-init globals.
/// - Refreshes [`BrinkLineTables<M>`] from the loaded asset every time
///   (so hot-reload of the source file's line tables propagates).
/// - Errors and removes the request if `FlowStart::Address` references
///   a name that isn't in the program.
#[expect(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    reason = "bevy systems take Res/Query by value and naturally have many arguments"
)]
pub fn fulfill_flow_requests<M: Send + Sync + 'static>(
    requests: Query<(Entity, &BrinkFlowRequest<M>), Without<BrinkFlow<M>>>,
    stories: Res<Assets<BrinkStoryAsset>>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables: Res<Assets<LineTablesAsset>>,
    initial_globals: Res<Assets<InitialGlobalsAsset>>,
    globals: Option<Res<BrinkGlobals<M>>>,
    mut line_tables_res: ResMut<BrinkLineTables<M>>,
    mut commands: Commands,
) {
    let mut globals_seeded = globals.is_some();

    for (entity, req) in &requests {
        let Some(bundle) = stories.get(&req.story) else {
            continue;
        };
        let Some(program_asset) = programs.get(&bundle.program) else {
            continue;
        };
        let Some(lt_asset) = line_tables.get(&bundle.line_tables) else {
            continue;
        };
        let Some(ig_asset) = initial_globals.get(&bundle.initial_globals) else {
            continue;
        };

        // Resolve start position.
        let (flow, fresh_ctx) = match &req.start {
            FlowStart::Root => FlowInstance::new_at_root(&program_asset.program),
            FlowStart::Address(name) => {
                let Some((idx, _)) = program_asset.program.find_address(name) else {
                    error!("BrinkFlowRequest: knot '{name}' not found; removing request");
                    commands.entity(entity).remove::<BrinkFlowRequest<M>>();
                    continue;
                };
                FlowInstance::new_at(&program_asset.program, idx)
            }
        };

        // Bootstrap BrinkGlobals<M> on first fulfillment.
        if !globals_seeded {
            // For root-start flows, init runs naturally during play —
            // seed with fresh defaults to avoid double-applying init.
            // For named-knot flows, the captured post-init context is
            // the right starting point.
            let starting_context = match &req.start {
                FlowStart::Root => fresh_ctx,
                FlowStart::Address(_) => ig_asset.context.clone(),
            };
            commands.insert_resource(BrinkGlobals::<M>::new(starting_context));
            globals_seeded = true;
        }

        // Always refresh line tables — hot-reload of the source file
        // (or future locale swap) propagates through this resource.
        line_tables_res.tables.clone_from(&lt_asset.tables);

        // Materialize real components, drop the request.
        commands
            .entity(entity)
            .remove::<BrinkFlowRequest<M>>()
            .insert((
                BrinkFlow::<M>::new(flow),
                BrinkProgram::<M>::new(bundle.program.clone()),
            ));
    }
}

/// Debug-build warning system: detects entities that have *both*
/// `BrinkFlowRequest<M>` and `BrinkFlow<M>` (which only happens if the
/// user re-inserts the request after fulfillment). Mutating the request
/// post-fulfillment has no effect — the system warns so the bug is
/// visible during development.
#[cfg(debug_assertions)]
#[expect(clippy::type_complexity, reason = "bevy query filter type")]
pub fn warn_post_fulfillment_mutations<M: Send + Sync + 'static>(
    misuse: Query<
        Entity,
        (
            bevy_ecs::query::With<BrinkFlowRequest<M>>,
            bevy_ecs::query::With<BrinkFlow<M>>,
        ),
    >,
) {
    for entity in &misuse {
        warn!(
            "entity {entity:?} has both BrinkFlowRequest<M> and BrinkFlow<M> — \
             mutating the request after fulfillment is a no-op. To re-spawn, \
             despawn the entity and spawn a fresh request."
        );
    }
}

#[cfg(not(debug_assertions))]
pub fn warn_post_fulfillment_mutations<M: Send + Sync + 'static>() {}
