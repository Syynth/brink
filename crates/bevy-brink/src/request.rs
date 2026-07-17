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
use bevy_log::error;
// Only the `#[cfg(debug_assertions)]` variant of `warn_post_fulfillment_mutations`
// below calls `warn!`; in non-debug builds (e.g. the `bench` profile used by
// `benches/scenario_bench.rs` under `--features bench-counters`) that variant
// isn't compiled, so an unconditional import would be unused there.
#[cfg(debug_assertions)]
use bevy_log::warn;
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

// Never called: `BrinkPlugin::build` only wires the debug variant above
// (its `app.add_systems` call is itself `#[cfg(debug_assertions)]`-gated),
// so this stub exists purely to give the generic a body in non-debug
// builds. Confirmed via `RUSTFLAGS="-C debug-assertions=off" cargo clippy
// -p bevy-brink --features bench-counters` (#923) — the profile that
// actually flips `debug_assertions` off is `bench` (built by
// `benches/scenario_bench.rs`, gated on this same feature), which no
// default CI job exercised until #923 wired one up.
#[cfg(not(debug_assertions))]
#[expect(
    dead_code,
    reason = "generic stub kept for API parity with the debug_assertions variant; never called in release/bench profiles"
)]
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

    // ── F6.3: per-entity SaveState durability ─────────────────────────────
    //
    // A save is one SaveState for the shared World + one per entity,
    // composed host-side (see the F6 AMENDMENT ruling 4 and the
    // "Save/load" section of globals.rs's module docs). These tests drive
    // two flows to diverge their private state under a policy with a
    // Local-marked VAR and a Local-marked knot, save world + both entities,
    // then load into a completely FRESH app (fresh World, fresh
    // FlowInstances re-entered at a knot) and check each flow recovers
    // exactly its own private state while the shared VAR converges once.

    use crate::globals::{load_flow_state, save_flow_state};
    use bevy_ecs::system::SystemState;
    use brink_format::Value;
    use brink_runtime::{ContextAccess, Line};

    /// The ink source shared by the F6.3 tests: `mood` (a private counter)
    /// and the `greet` knot's own visit count (read via `READ_COUNT`, so
    /// assertions don't depend on interpreting sequence-cycling text) are
    /// marked `Local` by the test policy; `shared_count` stays `World` by
    /// the (untouched) default. Printing numeric values rather than relying
    /// on `{greet: A|B}`-style sequence text keeps the assertions exact and
    /// independent of sequence-indexing semantics.
    const SAVE_TEST_SRC: &str = "VAR shared_count = 0\nVAR mood = 0\n-> greet\n\
         === greet ===\n\
         ~ mood = mood + 1\n\
         ~ shared_count = shared_count + 1\n\
         Greeting mood={mood} visits={READ_COUNT(-> greet)} shared={shared_count}\n\
         * [Again] -> greet\n\
         * [Done] -> END\n";

    /// `mood` + `greet`'s own visit count are private per flow; everything
    /// else (including `shared_count`) stays World-scoped by default.
    fn save_test_policy() -> WorldPolicy {
        let mut policy = WorldPolicy::default();
        policy.overrides.insert("mood".to_string(), Scope::Local);
        policy.overrides.insert("greet".to_string(), Scope::Local);
        policy
    }

    /// System-state shape shared by the driving/save/load helpers below:
    /// every flow component plus the assets and shared globals needed to
    /// build a [`flow_context_view`] and advance/save/load through it.
    /// `'static` lifetimes here follow the same pattern as `flow.rs`'s
    /// `ChooseState` — a type alias for a one-off `SystemState`, not a
    /// generic query type.
    type FlowQuery = SystemState<(
        Query<
            'static,
            'static,
            (
                &'static mut BrinkFlow<()>,
                &'static mut BrinkContext<()>,
                &'static crate::BrinkProgram<()>,
                &'static crate::BrinkLocale<()>,
            ),
        >,
        ResMut<'static, BrinkGlobals<()>>,
        Res<'static, Assets<ProgramAsset>>,
        Res<'static, Assets<crate::LineTablesAsset>>,
        Commands<'static, 'static>,
    )>;

    /// Drive one entity's flow to its next terminal line, via
    /// `flow_context_view` exactly like a real advance system would build
    /// it. Panics if the flow parks on a world-access external (none of
    /// these tests use externals) or if any required asset/component is
    /// missing.
    fn drive_entity(app: &mut App, entity: Entity) -> Line {
        let mut state: FlowQuery = SystemState::new(app.world_mut());
        let (mut flows, mut globals, programs, tables, mut commands) =
            state.get_mut(app.world_mut()).expect("system params");
        let (mut flow, mut ctx, prog, loc) = flows.get_mut(entity).expect("flow components");
        let program = &programs.get(&prog.handle).expect("program asset").program;
        let line_tables = &tables.get(&loc.handle).expect("line tables asset").tables;
        let mut view = flow_context_view(&mut globals, &mut ctx);
        let advance = flow
            .advance_until_terminal(
                program,
                line_tables,
                &mut view,
                &brink_runtime::FallbackHandler,
                entity,
                &mut commands,
            )
            .expect("advance");
        state.apply(app.world_mut());
        match advance {
            Advance::Line(line) => line,
            // None of the F6.3 tests use externals, so a pause here can
            // only be a bug in the test setup.
            Advance::AwaitingQuery => unreachable!("unexpected pending external in F6.3 tests"),
        }
    }

    /// Pick choice `index` on one entity's flow.
    fn choose_entity(app: &mut App, entity: Entity, index: usize) {
        let mut state: FlowQuery = SystemState::new(app.world_mut());
        let (mut flows, mut globals, _programs, _tables, _commands) =
            state.get_mut(app.world_mut()).expect("system params");
        let (mut flow, mut ctx, _prog, _loc) = flows.get_mut(entity).expect("flow components");
        let mut view = flow_context_view(&mut globals, &mut ctx);
        flow.choose(&mut view, index).expect("choose");
        state.apply(app.world_mut());
    }

    /// Spawn a `BrinkFlowRequest` for `story` starting at `start` and run
    /// one tick to fulfill it. Returns the entity.
    fn spawn_fulfilled(app: &mut App, story: &Handle<BrinkStoryAsset>, start: FlowStart) -> Entity {
        let entity = app
            .world_mut()
            .spawn(
                BrinkFlowRequest::<()>::builder()
                    .story(story.clone())
                    .start(start)
                    .build(),
            )
            .id();
        app.update();
        entity
    }

    /// A fresh app wired with `BrinkPlugin` under the F6.3 test policy.
    fn app_with_save_policy() -> App {
        let mut app = App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(crate::BrinkPlugin::<()>::default().with_policy(save_test_policy()));
        app
    }

    /// (a) Full roundtrip: two flows diverge their private state (flow A
    /// visits `greet` once, flow B twice — different `mood` values and
    /// different `greet` visit counts) while sharing `shared_count`. Save
    /// world + both entities, build a completely FRESH app (fresh
    /// `Program` compile, fresh `World`, fresh `FlowInstance`s re-entered
    /// at `greet` via `FlowStart::Address`), load world then each entity,
    /// and check each flow's re-entry sees ITS OWN restored private state
    /// — not the other flow's, not a fresh start — while the shared VAR is
    /// the single converged value from both saves.
    #[test]
    #[expect(
        clippy::similar_names,
        reason = "the paired a/b entity naming is the point of the test"
    )]
    fn full_roundtrip_world_plus_two_entities() {
        // ── App 1: drive two flows to diverge, then save. ──
        let mut app1 = app_with_save_policy();
        let (program1, tables1, ctx1) = compile_test_story(SAVE_TEST_SRC);
        let story1 = add_story_assets(&mut app1, program1, tables1, ctx1);

        let entity_a = spawn_fulfilled(&mut app1, &story1, FlowStart::Root);
        let entity_b = spawn_fulfilled(&mut app1, &story1, FlowStart::Root);

        // Flow A: one pass through `greet` (mood=1, greet visits=1).
        let line_a = drive_entity(&mut app1, entity_a);
        assert!(
            line_a.text().contains("mood=1") && line_a.text().contains("visits=1"),
            "flow A's first pass; got {:?}",
            line_a.text()
        );

        // Flow B: two passes through `greet` (mood=2, greet visits=2).
        let line_b1 = drive_entity(&mut app1, entity_b);
        assert!(
            line_b1.text().contains("mood=1") && line_b1.text().contains("visits=1"),
            "flow B's first pass; got {:?}",
            line_b1.text()
        );
        choose_entity(&mut app1, entity_b, 0); // "Again" -> greet
        let line_b2 = drive_entity(&mut app1, entity_b);
        assert!(
            line_b2.text().contains("mood=2") && line_b2.text().contains("visits=2"),
            "flow B's second pass; got {:?}",
            line_b2.text()
        );

        // Capture saves — world once, then each entity — all reading the
        // same settled state (no mutation happens between these calls).
        let (world_save, save_a, save_b) = {
            let mut state: FlowQuery = SystemState::new(app1.world_mut());
            let (mut flows, mut globals, programs, _tables, _commands) =
                state.get_mut(app1.world_mut()).expect("system params");

            let handle = flows.get(entity_a).expect("flow a").2.handle.clone();
            let program = &programs.get(&handle).expect("program asset").program;

            let world_save = globals.save_state(program);

            let (_flow_a, mut ctx_a, _p, _l) = flows.get_mut(entity_a).expect("flow a");
            let save_a = save_flow_state(&mut globals, &mut ctx_a, program);

            let (_flow_b, mut ctx_b, _p, _l) = flows.get_mut(entity_b).expect("flow b");
            let save_b = save_flow_state(&mut globals, &mut ctx_b, program);

            (world_save, save_a, save_b)
        };

        // Sanity on what got captured before crossing into the fresh app.
        assert_eq!(save_a.globals.get("mood"), Some(&Value::Int(1)));
        assert_eq!(save_b.globals.get("mood"), Some(&Value::Int(2)));
        // shared_count must be identical across world + both entity saves —
        // the "same save moment" property `load_flow_state`'s idempotent
        // World rewrite depends on.
        assert_eq!(save_a.globals.get("shared_count"), Some(&Value::Int(3)));
        assert_eq!(save_b.globals.get("shared_count"), Some(&Value::Int(3)));
        assert_eq!(world_save.globals.get("shared_count"), Some(&Value::Int(3)));

        // ── App 2: a completely fresh app — new compile, new World, new
        // FlowInstances re-entered at `greet` (not resumed mid-line). ──
        let mut app2 = app_with_save_policy();
        let (program2, tables2, ctx2) = compile_test_story(SAVE_TEST_SRC);
        let story2 = add_story_assets(&mut app2, program2, tables2, ctx2);

        let entity_a2 =
            spawn_fulfilled(&mut app2, &story2, FlowStart::Address("greet".to_string()));
        let entity_b2 =
            spawn_fulfilled(&mut app2, &story2, FlowStart::Address("greet".to_string()));

        // Load world first, then each entity through its own view.
        {
            let mut state: FlowQuery = SystemState::new(app2.world_mut());
            let (mut flows, mut globals, programs, _tables, _commands) =
                state.get_mut(app2.world_mut()).expect("system params");

            let handle = flows.get(entity_a2).expect("flow a2").2.handle.clone();
            let program = &programs.get(&handle).expect("program asset").program;

            let world_report = globals.load_state(program, &world_save);
            assert!(
                world_report.is_clean(),
                "world load should be clean: {world_report:?}"
            );

            let (_flow, mut ctx_a2, _p, _l) = flows.get_mut(entity_a2).expect("flow a2");
            let report_a = load_flow_state(&mut globals, &mut ctx_a2, program, &save_a);
            assert!(
                report_a.is_clean(),
                "entity A load should be clean: {report_a:?}"
            );

            let (_flow, mut ctx_b2, _p, _l) = flows.get_mut(entity_b2).expect("flow b2");
            let report_b = load_flow_state(&mut globals, &mut ctx_b2, program, &save_b);
            assert!(
                report_b.is_clean(),
                "entity B load should be clean: {report_b:?}"
            );
        }

        // Re-enter each flow at `greet` (FlowStart::Address does NOT bump
        // greet's own visit count — see `fulfillment_resolves_named_address`
        // and its sibling comments above — so READ_COUNT still reflects the
        // RESTORED count, not a fresh 0, proving state (not position) is
        // what carries the resume forward).
        let resumed_a = drive_entity(&mut app2, entity_a2);
        assert!(
            resumed_a.text().contains("mood=2") && resumed_a.text().contains("visits=1"),
            "flow A2 should resume from its own restored state (mood 1->2, \
             greet visits still 1, unbumped by address-entry); got {:?}",
            resumed_a.text()
        );
        let resumed_b = drive_entity(&mut app2, entity_b2);
        assert!(
            resumed_b.text().contains("mood=3") && resumed_b.text().contains("visits=2"),
            "flow B2 should resume from ITS OWN restored state (mood 2->3, \
             greet visits still 2) — distinct from flow A2's; got {:?}",
            resumed_b.text()
        );
    }

    /// (b) Entity load routes by scope: loading a `SaveState` carrying both
    /// a `Local`-marked `VAR` (`mood`) and a `World`-marked `VAR`
    /// (`shared_count`) into one flow lands `mood` in that entity's own
    /// `FlowLocal` — invisible through the shared `World` directly, and
    /// invisible to any *other* flow's view — while `shared_count` lands in
    /// the shared `World` exactly as saved (idempotent rewrite), visible
    /// both through the raw `World` and through any flow's view.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one scope-routing scenario checked from three vantage points"
    )]
    fn entity_load_routes_local_to_flow_local_and_world_stays_shared() {
        let mut app = app_with_save_policy();
        let (program, tables, ctx) = compile_test_story(SAVE_TEST_SRC);
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = spawn_fulfilled(&mut app, &story, FlowStart::Address("greet".to_string()));
        // A second, untouched flow sharing the same BrinkGlobals — used to
        // prove the loaded Local value is NOT visible globally.
        let other = spawn_fulfilled(&mut app, &story, FlowStart::Address("greet".to_string()));

        // Hand-built SaveState: mood (Local) = 42, shared_count (World) = 7.
        let mut save = brink_runtime::SaveState {
            version: brink_runtime::SAVE_FORMAT_VERSION,
            globals: std::collections::BTreeMap::new(),
            global_ids: std::collections::BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
            suspended: None,
        };
        save.globals.insert("mood".to_string(), Value::Int(42));
        save.globals
            .insert("shared_count".to_string(), Value::Int(7));

        {
            let mut state: FlowQuery = SystemState::new(app.world_mut());
            let (mut flows, mut globals, programs, _tables, _commands) =
                state.get_mut(app.world_mut()).expect("system params");
            let handle = flows.get(entity).expect("flow").2.handle.clone();
            let program = &programs.get(&handle).expect("program asset").program;
            let (_flow, mut ctx, _p, _l) = flows.get_mut(entity).expect("flow");
            let report = load_flow_state(&mut globals, &mut ctx, program, &save);
            assert!(report.is_clean(), "load should be clean: {report:?}");
        }

        let mood_idx = {
            let programs = app.world().resource::<Assets<ProgramAsset>>();
            let handle = app
                .world()
                .entity(entity)
                .get::<crate::BrinkProgram<()>>()
                .expect("BrinkProgram")
                .handle
                .clone();
            programs
                .get(&handle)
                .expect("program asset")
                .program
                .global_index("mood")
                .expect("mood global")
        };
        let shared_idx = {
            let programs = app.world().resource::<Assets<ProgramAsset>>();
            let handle = app
                .world()
                .entity(entity)
                .get::<crate::BrinkProgram<()>>()
                .expect("BrinkProgram")
                .handle
                .clone();
            programs
                .get(&handle)
                .expect("program asset")
                .program
                .global_index("shared_count")
                .expect("shared_count global")
        };

        // The loaded entity's own EFFECTIVE view sees the restored mood.
        {
            let mut state: FlowQuery = SystemState::new(app.world_mut());
            let (mut flows, mut globals, _programs, _tables, _commands) =
                state.get_mut(app.world_mut()).expect("system params");
            let (_flow, mut ctx, _p, _l) = flows.get_mut(entity).expect("flow");
            let view = flow_context_view(&mut globals, &mut ctx);
            assert_eq!(
                view.global(mood_idx),
                &Value::Int(42),
                "the loaded entity's own view should see the restored Local mood"
            );
            assert_eq!(
                view.global(shared_idx),
                &Value::Int(7),
                "the loaded entity's own view should see the restored World shared_count"
            );
        }

        // Raw World storage never received the Local write.
        {
            let globals = app.world().resource::<BrinkGlobals<()>>();
            assert_ne!(
                globals.inner.global(mood_idx),
                &Value::Int(42),
                "Local-scoped mood must NOT have been written into the shared World"
            );
            assert_eq!(
                globals.inner.global(shared_idx),
                &Value::Int(7),
                "World-scoped shared_count should have rewritten the shared World directly"
            );
        }

        // A completely different flow sharing the same BrinkGlobals does
        // NOT see the loaded entity's private mood (proves it landed in
        // THAT entity's own FlowLocal, not anywhere globally visible) but
        // DOES see the shared shared_count (World-scoped, visible to all).
        {
            let mut state: FlowQuery = SystemState::new(app.world_mut());
            let (mut flows, mut globals, _programs, _tables, _commands) =
                state.get_mut(app.world_mut()).expect("system params");
            let (_flow, mut ctx, _p, _l) = flows.get_mut(other).expect("other flow");
            let view = flow_context_view(&mut globals, &mut ctx);
            assert_ne!(
                view.global(mood_idx),
                &Value::Int(42),
                "a different flow must not see another entity's private mood"
            );
            assert_eq!(
                view.global(shared_idx),
                &Value::Int(7),
                "a different flow should see the same shared shared_count"
            );
        }
    }

    /// (c) `LoadReport` surfaces unknown globals without erroring: a
    /// `SaveState` naming a `VAR` the program doesn't declare loads
    /// cleanly for every other entry, with the unknown name reported.
    #[test]
    fn load_report_surfaces_unknown_globals() {
        let mut app = app_with_save_policy();
        let (program, tables, ctx) = compile_test_story(SAVE_TEST_SRC);
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = spawn_fulfilled(&mut app, &story, FlowStart::Address("greet".to_string()));

        let mut save = brink_runtime::SaveState {
            version: brink_runtime::SAVE_FORMAT_VERSION,
            globals: std::collections::BTreeMap::new(),
            global_ids: std::collections::BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
            suspended: None,
        };
        save.globals.insert("mood".to_string(), Value::Int(5));
        save.globals
            .insert("does_not_exist".to_string(), Value::Int(99));

        let (report, mood_idx) = {
            let mut state: FlowQuery = SystemState::new(app.world_mut());
            let (mut flows, mut globals, programs, _tables, _commands) =
                state.get_mut(app.world_mut()).expect("system params");
            let handle = flows.get(entity).expect("flow").2.handle.clone();
            let program = &programs.get(&handle).expect("program asset").program;
            let mood_idx = program.global_index("mood").expect("mood global");
            let (_flow, mut ctx, _p, _l) = flows.get_mut(entity).expect("flow");
            let report = load_flow_state(&mut globals, &mut ctx, program, &save);
            (report, mood_idx)
        };

        assert!(!report.is_clean(), "report should not be clean: {report:?}");
        assert_eq!(report.unknown_globals, vec!["does_not_exist".to_string()]);

        // The known entry still applied despite the unknown one.
        let mut state: FlowQuery = SystemState::new(app.world_mut());
        let (mut flows, mut globals, _programs, _tables, _commands) =
            state.get_mut(app.world_mut()).expect("system params");
        let (_flow, mut ctx, _p, _l) = flows.get_mut(entity).expect("flow");
        let view = flow_context_view(&mut globals, &mut ctx);
        assert_eq!(
            view.global(mood_idx),
            &Value::Int(5),
            "the known global should still apply even though another was unknown"
        );
    }
}
