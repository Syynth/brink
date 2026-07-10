//! Request-component pattern for spawning flows.
//!
//! Consumers spawn an entity carrying a [`BrinkFlowRequest<M>`] and a
//! handle to a [`BrinkStoryAsset`](crate::BrinkStoryAsset). A
//! plugin-managed system ([`fulfill_flow_requests`]) waits for the
//! story's sub-assets to load, builds a `FlowInstance`, replaces the
//! request component with [`BrinkFlow<M>`](crate::BrinkFlow), the
//! [`BrinkStory<M>`](crate::BrinkStory) bundle (program + locale
//! handles), and a fresh per-flow [`BrinkContext<M>`](crate::BrinkContext).
//!
//! No polling, no readiness latches: the user just spawns the request
//! and lets the plugin fulfill it whenever assets become available.

use std::marker::PhantomData;

use bevy_asset::{Assets, Handle};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::Without;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_log::{error, warn};
use brink_runtime::{FlowInstance, FlowLocal, World};

use crate::asset::{BrinkStory, BrinkStoryAsset, ProgramAsset};
use crate::flow::BrinkFlow;
use crate::globals::{BrinkContext, BrinkGlobals, BrinkWorldPolicy};

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
/// bundle, and a fresh per-flow [`BrinkContext<M>`](crate::BrinkContext)
/// once the program and line-tables subassets are loaded. Spawning a flow
/// takes no seed/policy parameter — its `FlowLocal` starts empty and its
/// story-state routes World-vs-Local per the policy installed once at
/// [`BrinkPlugin::with_policy`](crate::BrinkPlugin::with_policy) (see the F6
/// AMENDMENT in `docs/scoped-flow-state-spec.md`). Mutating the request
/// after fulfillment is a no-op (in debug builds, a warning is emitted via
/// [`warn_post_fulfillment_mutations`]).
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
/// - On first fulfillment for marker `M`, creates the single shared
///   [`BrinkGlobals<M>`] `World` via [`World::new`], resolving the policy
///   installed at [`BrinkPlugin::with_policy`](crate::BrinkPlugin::with_policy)
///   against this program's symbol table. If the policy names an unknown
///   variable or knot/stitch ([`PolicyError`](brink_runtime::PolicyError)),
///   this is logged as a clear setup error (not a panic) and the request is
///   removed — every later request for this marker will hit the same error
///   until the host fixes its policy.
/// - Inserts a fresh, empty per-flow [`BrinkContext<M>`] component — no
///   seeding, no policy parameter (see the F6 AMENDMENT in
///   `docs/scoped-flow-state-spec.md`).
/// - Inserts the [`BrinkStory<M>`] bundle (program + locale handles).
/// - Errors and removes the request if `FlowStart::Address` references
///   a name that isn't in the program.
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "bevy system: flow + globals + locale assets/resources for spawn-time locale reconcile"
)]
pub fn fulfill_flow_requests<M: Send + Sync + 'static>(
    requests: Query<(Entity, &BrinkFlowRequest<M>), Without<BrinkFlow<M>>>,
    stories: Res<Assets<BrinkStoryAsset>>,
    programs: Res<Assets<ProgramAsset>>,
    globals: Option<Res<BrinkGlobals<M>>>,
    policy: Res<BrinkWorldPolicy<M>>,
    current_locale: Option<Res<crate::locale::BrinkCurrentLocale<M>>>,
    locales: Res<Assets<crate::locale::LocaleAsset>>,
    mut line_tables: ResMut<Assets<crate::asset::LineTablesAsset>>,
    mut cache: ResMut<crate::locale::LocalizedTablesCache<M>>,
    mut commands: Commands,
) {
    // Whether BrinkGlobals<M> exists yet. Tracked separately from `globals`
    // (an `Option<Res<_>>` snapshot from system start) so that once this
    // batch creates it on the first request, later requests in the same
    // batch don't try to create it again.
    let mut globals_ready = globals.is_some();

    for (entity, req) in &requests {
        let Some(bundle) = stories.get(&req.story) else {
            continue;
        };
        let Some(program_asset) = programs.get(&bundle.program) else {
            continue;
        };

        if !globals_ready {
            match World::new(&program_asset.program, &policy.policy) {
                Ok(world) => {
                    commands.insert_resource(BrinkGlobals::<M>::new(world));
                    globals_ready = true;
                }
                Err(err) => {
                    error!(
                        "BrinkFlowRequest: world policy error creating BrinkGlobals: {err}; \
                         removing request (fix the policy passed to BrinkPlugin::with_policy)"
                    );
                    commands.entity(entity).remove::<BrinkFlowRequest<M>>();
                    continue;
                }
            }
        }

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

        // Resolve the flow's starting locale: base unless a global locale is
        // active and its overlay is loaded (otherwise base now, caught up by
        // `catch_up_loaded_locales` when the `.inkl` loads). `BrinkBaseLocale`
        // retains the canonical base so future switches always overlay it.
        let base_handle = bundle.line_tables.clone();
        let active_handle = crate::locale::initial_locale_handle::<M>(
            &base_handle,
            program_asset,
            current_locale.as_deref(),
            &locales,
            &mut cache,
            &mut line_tables,
        );

        // Materialize real components, drop the request.
        let mut entity_cmds = commands.entity(entity);
        entity_cmds.remove::<BrinkFlowRequest<M>>();
        entity_cmds.insert((
            BrinkFlow::<M>::new(flow),
            BrinkContext::<M>::new(FlowLocal::new()),
            BrinkStory::<M>::new(bundle.program.clone(), active_handle),
            crate::locale::BrinkBaseLocale::<M>::new(base_handle),
        ));

        // In dev builds, attach a replay log so hot-reload can rebuild
        // the flow and replay choices.
        #[cfg(feature = "dev")]
        entity_cmds.insert(crate::replay::BrinkReplayLog::<M>::new(
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

    /// `FlowStart::Address` resolves at fulfillment time. If the address
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

    /// `FlowStart::Address` resolves when the knot exists.
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

    /// Multiple flow requests share the same `BrinkGlobals` — the first
    /// fulfillment seeds it, subsequent ones reuse.
    #[test]
    fn multiple_requests_share_globals() {
        let mut app = make_test_app();
        let (program, tables, ctx) =
            compile_test_story("VAR shared_counter = 0\n=== start ===\nhi\n* [Continue] -> END\n");
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

    // ── F6.2: scoped-flow-state semantics (shared-by-default World) ──────
    //
    // The pre-F6.2 model gave every flow its OWN full `World` clone —
    // isolation was the default and had to be explicitly committed back.
    // F6.2 flips that: one shared `World` per marker, private state is the
    // opt-in case via `WorldPolicy` overrides. These three tests are the
    // semantic anchor for that flip (see the F6 AMENDMENT in
    // `docs/scoped-flow-state-spec.md`): (a) default policy shares live,
    // (b) a `Local` override isolates just the named unit while the rest
    // stays shared, (c) a bad override name fails cleanly, not a panic.

    use crate::globals::flow_context_view;
    use crate::{Advance, BrinkGlobals};
    use bevy_app::{App, Update};
    use brink_runtime::{Scope, StoryStatus, WorldPolicy};

    #[derive(bevy_ecs::resource::Resource, Default)]
    struct Texts(Vec<String>);

    /// Drive every `Active` flow (skips ones already at a terminal status,
    /// e.g. `Done` from an earlier pass in the same test) to its first
    /// terminal line, recording the produced text. Shared by the three
    /// tests below.
    #[expect(
        clippy::type_complexity,
        clippy::needless_pass_by_value,
        reason = "bevy systems take Res/Query by value and have complex query tuples"
    )]
    fn drive_all_active(
        mut flows: Query<(
            Entity,
            &mut BrinkFlow<()>,
            &mut BrinkContext<()>,
            &crate::BrinkProgram<()>,
            &crate::BrinkLocale<()>,
        )>,
        globals: Option<ResMut<BrinkGlobals<()>>>,
        programs: Res<Assets<ProgramAsset>>,
        tables: Res<Assets<crate::asset::LineTablesAsset>>,
        mut texts: ResMut<Texts>,
        mut commands: Commands,
    ) {
        let Some(mut globals) = globals else {
            return; // nothing fulfilled yet this tick
        };
        for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
            if flow.inner.status() != StoryStatus::Active {
                continue;
            }
            let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle)) else {
                continue;
            };
            let mut view = flow_context_view(&mut globals, &mut ctx);
            if let Ok(Advance::Line(line)) = flow.advance_until_terminal(
                &p.program,
                &t.tables,
                &mut view,
                &brink_runtime::FallbackHandler,
                entity,
                &mut commands,
            ) {
                texts.0.push(line.text().to_string());
            }
        }
    }

    /// (a) Default policy (`WorldPolicy::default()`, installed automatically
    /// by `BrinkPlugin::default()`): two flows spawned over the same marker
    /// share the one `BrinkGlobals<M>` `World` live. Each flow's `~ counter
    /// = counter + 1` lands in the SAME shared slot, so the two flows'
    /// outputs are "1" and "2" — not both "1", which is what independent
    /// per-flow copies (the pre-F6.2 model) would have produced.
    #[test]
    fn default_policy_two_flows_share_one_world_global() {
        let mut app = App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(crate::BrinkPlugin::<()>::default());
        app.insert_resource(Texts::default());
        app.add_systems(Update, drive_all_active);

        let (program, tables, ctx) = compile_test_story(
            "VAR counter = 0\n~ counter = counter + 1\nCounter is {counter}.\n-> DONE\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);

        app.world_mut().spawn(
            BrinkFlowRequest::<()>::builder()
                .story(story.clone())
                .build(),
        );
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());
        app.update(); // fulfill both
        app.update(); // drive both to Done in one pass

        let mut texts = app.world().resource::<Texts>().0.clone();
        texts.sort();
        assert_eq!(
            texts,
            vec!["Counter is 1.\n".to_string(), "Counter is 2.\n".to_string()],
            "two flows sharing the default (all-World) policy should observe \
             cumulative shared state, not independent per-flow copies; got {texts:?}"
        );
    }

    /// (b) A policy with a `Local`-scoped knot (`overrides: {"start":
    /// Local}`) alongside the `World`-scoped default: each flow's visit
    /// count for the `start` knot is its own, so both flows entering it for
    /// their first time see the sequence's first branch ("Hello") — but the
    /// plain `VAR shared_visits` stays `World`-scoped by the (untouched)
    /// default, so it keeps counting across both flows.
    #[test]
    fn local_knot_override_isolates_visit_state_while_world_var_stays_shared() {
        let mut app = App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        let mut policy = WorldPolicy::default();
        policy.overrides.insert("start".to_string(), Scope::Local);
        app.add_plugins(crate::BrinkPlugin::<()>::default().with_policy(policy));
        app.insert_resource(Texts::default());
        app.add_systems(Update, drive_all_active);

        // Root diverts straight into `start` — a real `-> start` divert, so
        // entering it goes through the normal goto machinery that bumps
        // its visit count (starting a flow directly AT a knot via
        // `FlowStart::Address` does not: only diverting *into* a knot
        // counts as a visit).
        let (program, tables, ctx) = compile_test_story(
            "VAR shared_visits = 0\n-> start\n=== start ===\n\
             ~ shared_visits = shared_visits + 1\n\
             {start: Hello|Welcome back} (shared {shared_visits}).\n-> DONE\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);

        // Flow 1: spawn, fulfill, drive to its first Done in isolation so
        // the shared-VAR assertion below has an unambiguous "after flow 1"
        // checkpoint.
        app.world_mut().spawn(
            BrinkFlowRequest::<()>::builder()
                .story(story.clone())
                .build(),
        );
        app.update(); // fulfill flow 1
        app.update(); // drive flow 1 to Done
        let flow1_text = app.world_mut().resource_mut::<Texts>().0.remove(0);
        assert!(
            flow1_text.contains("Hello"),
            "flow 1's first-ever visit to a Local-scoped knot should take \
             the sequence's first branch; got {flow1_text:?}"
        );
        assert!(
            flow1_text.contains("shared 1"),
            "the World-scoped VAR should count flow 1's visit; got {flow1_text:?}"
        );

        // Flow 2: a fresh flow entering the SAME Local-scoped knot. If the
        // knot's visit count were (incorrectly) World-scoped, flow 2 would
        // see "Welcome back" (visit count already 1); because it's Local,
        // flow 2's own count starts at 0 and it also sees "Hello".
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());
        app.update(); // fulfill flow 2 (flow 1 no longer matches `Without<BrinkFlow<M>>`)
        app.update(); // drive flow 2 to Done
        let flow2_text = app.world_mut().resource_mut::<Texts>().0.remove(0);
        assert!(
            flow2_text.contains("Hello"),
            "flow 2's own Local visit count should also start fresh, \
             independent of flow 1's; got {flow2_text:?}"
        );
        assert!(
            flow2_text.contains("shared 2"),
            "the World-scoped VAR should keep counting across flows \
             (flow 1's 1, then flow 2's 2); got {flow2_text:?}"
        );
    }

    /// (c) A policy override naming a variable/knot the program doesn't
    /// declare is a [`brink_runtime::PolicyError`] at `BrinkGlobals`
    /// creation — `fulfill_flow_requests` must surface it as a logged
    /// fulfillment error on the offending request (removed, not left
    /// dangling), and — the actual point of this test — must not panic.
    #[test]
    fn unknown_policy_override_surfaces_as_fulfillment_error_not_panic() {
        let mut app = App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        let mut policy = WorldPolicy::default();
        policy
            .overrides
            .insert("does_not_exist".to_string(), Scope::Local);
        app.add_plugins(crate::BrinkPlugin::<()>::default().with_policy(policy));

        let (program, tables, ctx) = compile_test_story("Hello.\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        // The point of the test: this must not panic.
        app.update();

        assert!(
            !app.world().entity(entity).contains::<BrinkFlow<()>>(),
            "flow should not materialize when the policy fails to resolve"
        );
        assert!(
            !app.world()
                .entity(entity)
                .contains::<BrinkFlowRequest<()>>(),
            "the invalid request should be removed, not left pending forever"
        );
        assert!(
            !app.world().contains_resource::<BrinkGlobals<()>>(),
            "BrinkGlobals must never be created from a policy that fails to resolve"
        );
    }
}
