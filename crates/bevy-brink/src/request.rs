//! Request-component pattern for spawning flows.
//!
//! Consumers spawn an entity carrying a [`BrinkFlowRequest<M>`] and a
//! handle to a [`BrinkStoryAsset`](crate::BrinkStoryAsset). A
//! plugin-managed system ([`fulfill_flow_requests`]) waits for the
//! story's sub-assets to load, builds a `FlowInstance`, replaces the
//! request component with [`BrinkFlow<M>`](crate::BrinkFlow), the
//! [`BrinkStory<M>`](crate::BrinkStory) bundle (program + locale
//! handles), and a per-flow [`BrinkContext<M>`](crate::BrinkContext)
//! seeded from [`BrinkGlobals<M>`](crate::BrinkGlobals).
//!
//! No polling, no readiness latches: the user just spawns the request
//! and lets the plugin fulfill it whenever assets become available.

use std::marker::PhantomData;

use bevy_asset::{Assets, Handle};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Commands, Query, Res};
use bevy_ecs::query::Without;
use bevy_log::{error, warn};
use brink_runtime::FlowInstance;

use crate::asset::{BrinkStory, BrinkStoryAsset, ProgramAsset};
use crate::flow::BrinkFlow;
use crate::globals::{BrinkContext, BrinkGlobals};

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
/// [`BrinkFlow<M>`](crate::BrinkFlow), the [`BrinkStory<M>`](crate::BrinkStory)
/// bundle, and a per-flow [`BrinkContext<M>`](crate::BrinkContext)
/// (seeded from [`BrinkGlobals<M>`](crate::BrinkGlobals), or from
/// [`ProgramAsset::initial_context`](crate::ProgramAsset) on first
/// fulfillment) once the program and line-tables subassets are loaded.
/// Mutating the request after fulfillment is a no-op (in debug builds,
/// a warning is emitted via [`warn_post_fulfillment_mutations`]).
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
/// fulfill each whose assets are ready, and bootstrap the entity's
/// per-flow components.
///
/// Behavior:
///
/// - Skips requests whose `BrinkStoryAsset` (or any of its sub-assets)
///   isn't loaded yet — the request just waits.
/// - On first fulfillment for marker `M`, inserts [`BrinkGlobals<M>`]
///   seeded from [`ProgramAsset::initial_context`](crate::ProgramAsset)
///   (the fresh starting `Context` — globals from `VAR`/`CONST`/`LIST`
///   defaults, zero visit/turn counts). Acts as the "save data" the
///   flow's per-entity [`BrinkContext`] is cloned from.
/// - Inserts the per-flow [`BrinkContext<M>`] component, seeded by
///   cloning the current `BrinkGlobals<M>` resource. Each flow has its
///   own `Context`; globals are not auto-shared.
/// - Inserts the [`BrinkStory<M>`] bundle (program + locale handles).
/// - Errors and removes the request if `FlowStart::Address` references
///   a name that isn't in the program.
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]
pub fn fulfill_flow_requests<M: Send + Sync + 'static>(
    requests: Query<(Entity, &BrinkFlowRequest<M>), Without<BrinkFlow<M>>>,
    stories: Res<Assets<BrinkStoryAsset>>,
    programs: Res<Assets<ProgramAsset>>,
    globals: Option<Res<BrinkGlobals<M>>>,
    mut commands: Commands,
) {
    // Snapshot of what to seed each flow's BrinkContext from. If
    // BrinkGlobals already exists (the "save data"), we clone it. If
    // not, the first fulfilled flow this call creates it from the
    // program's initial_context, and remaining flows in this batch
    // seed from the same snapshot.
    let mut shared_seed: Option<brink_runtime::Context> =
        globals.as_ref().map(|g| g.inner.clone());

    for (entity, req) in &requests {
        let Some(bundle) = stories.get(&req.story) else {
            continue;
        };
        let Some(program_asset) = programs.get(&bundle.program) else {
            continue;
        };

        // Resolve start position.
        let flow = match &req.start {
            FlowStart::Root => {
                let (flow, _ctx) = FlowInstance::new_at_root(&program_asset.program);
                flow
            }
            FlowStart::Address(name) => {
                let Some((idx, _)) = program_asset.program.find_address(name) else {
                    error!("BrinkFlowRequest: knot '{name}' not found; removing request");
                    commands.entity(entity).remove::<BrinkFlowRequest<M>>();
                    continue;
                };
                let (flow, _ctx) = FlowInstance::new_at(&program_asset.program, idx);
                flow
            }
        };

        let starting_context = if let Some(ctx) = &shared_seed {
            ctx.clone()
        } else {
            let ctx = program_asset.initial_context.clone();
            commands.insert_resource(BrinkGlobals::<M>::new(ctx.clone()));
            shared_seed = Some(ctx.clone());
            ctx
        };

        // Materialize real components, drop the request.
        let mut entity_cmds = commands.entity(entity);
        entity_cmds.remove::<BrinkFlowRequest<M>>();
        entity_cmds.insert((
            BrinkFlow::<M>::new(flow),
            BrinkContext::<M>::new(starting_context.clone()),
            BrinkStory::<M>::new(bundle.program.clone(), bundle.line_tables.clone()),
        ));

        // In dev builds, attach a replay log so hot-reload can rebuild
        // the flow and replay choices.
        #[cfg(feature = "dev")]
        entity_cmds.insert(crate::replay::BrinkReplayLog::<M>::new(
            starting_context,
            req.start.clone(),
            req.story.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_story_assets, compile_test_story, make_test_app};

    /// One tick is enough to fulfill a request once its assets are
    /// already present.
    #[test]
    fn fulfillment_replaces_request_with_flow_components() {
        let mut app = make_test_app();
        let (program, tables, ctx) =
            compile_test_story("=== start ===\nhello\n* [Continue] -> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);

        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        app.update();

        let world = app.world();
        let entity_ref = world.entity(entity);
        assert!(
            entity_ref.contains::<BrinkFlow<()>>(),
            "fulfilled entity should have BrinkFlow"
        );
        assert!(
            entity_ref.contains::<crate::BrinkProgram<()>>(),
            "fulfilled entity should have BrinkProgram"
        );
        assert!(
            entity_ref.contains::<crate::BrinkLocale<()>>(),
            "fulfilled entity should have BrinkLocale"
        );
        assert!(
            entity_ref.contains::<BrinkContext<()>>(),
            "fulfilled entity should have BrinkContext"
        );
        assert!(
            !entity_ref.contains::<BrinkFlowRequest<()>>(),
            "request component should be removed after fulfillment"
        );
        assert!(
            world.contains_resource::<BrinkGlobals<()>>(),
            "globals should be inserted on first fulfillment"
        );
    }

    /// In dev builds, the replay log gets attached automatically so
    /// hot-reload works.
    #[test]
    #[cfg(feature = "dev")]
    fn fulfillment_attaches_replay_log_in_dev() {
        let mut app = make_test_app();
        let (program, tables, ctx) =
            compile_test_story("=== start ===\nhello\n* [Continue] -> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);

        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        app.update();

        assert!(
            app.world()
                .entity(entity)
                .contains::<crate::replay::BrinkReplayLog<()>>(),
            "BrinkReplayLog should be attached when dev feature is enabled"
        );
    }

    /// FlowStart::Address resolves at fulfillment time. If the address
    /// is unknown, the request is removed and no flow is materialized.
    #[test]
    fn fulfillment_removes_request_for_unknown_address() {
        let mut app = make_test_app();
        let (program, tables, ctx) = compile_test_story(
            "=== start ===\nhello\n* [Continue] -> END\n=== outro ===\nbye\n-> END\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);

        let entity = app
            .world_mut()
            .spawn(
                BrinkFlowRequest::<()>::builder()
                    .story(story)
                    .start(FlowStart::Address("nonexistent_knot".to_string()))
                    .build(),
            )
            .id();

        app.update();

        let entity_ref = app.world().entity(entity);
        assert!(
            !entity_ref.contains::<BrinkFlowRequest<()>>(),
            "request should be removed when address can't be resolved"
        );
        assert!(
            !entity_ref.contains::<BrinkFlow<()>>(),
            "no flow should materialize for unresolvable address"
        );
    }

    /// FlowStart::Address resolves when the knot exists.
    #[test]
    fn fulfillment_resolves_named_address() {
        let mut app = make_test_app();
        let (program, tables, ctx) = compile_test_story(
            "=== start ===\nhello\n* [Continue] -> END\n=== outro ===\nbye\n-> END\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);

        let entity = app
            .world_mut()
            .spawn(
                BrinkFlowRequest::<()>::builder()
                    .story(story)
                    .start(FlowStart::Address("outro".to_string()))
                    .build(),
            )
            .id();

        app.update();

        assert!(
            app.world().entity(entity).contains::<BrinkFlow<()>>(),
            "flow should materialize when address resolves"
        );
    }

    /// Multiple flow requests share the same BrinkGlobals — the first
    /// fulfillment seeds it, subsequent ones reuse.
    #[test]
    fn multiple_requests_share_globals() {
        let mut app = make_test_app();
        let (program, tables, ctx) = compile_test_story(
            "VAR shared_counter = 0\n=== start ===\nhi\n* [Continue] -> END\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);

        let e1 = app
            .world_mut()
            .spawn(
                BrinkFlowRequest::<()>::builder()
                    .story(story.clone())
                    .build(),
            )
            .id();
        let e2 = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        app.update();

        let world = app.world();
        assert!(world.entity(e1).contains::<BrinkFlow<()>>());
        assert!(world.entity(e2).contains::<BrinkFlow<()>>());
        // Single resource for the marker — both flows reference it via
        // the system's ResMut<BrinkGlobals<M>>.
        assert!(world.contains_resource::<BrinkGlobals<()>>());
    }
}
