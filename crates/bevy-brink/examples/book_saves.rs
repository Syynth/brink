//! Compile-checked source for the Bevy book's "Game-state saves" section
//! (`docs/book/src/integrations/bevy/localization.md`).
//!
//! Same mechanism as `book_flows.rs`: the book pulls the snippets shown on
//! that page out of this file via mdbook `{{#include …:anchor}}`
//! directives, so the code readers see is exactly the code that
//! `cargo build --example book_saves` compiles.
//!
//! Run with `cargo run --example book_saves` for the full round trip: two
//! flows are driven once each (diverging their private `mood`, sharing
//! `shared_count`), world + both entities are saved into a serde map
//! (round-tripped through JSON to prove it), a FRESH app loads that map
//! back into freshly re-entered flows, and each restored flow's state is
//! printed — showing it survived the round trip through its own private
//! state, not a resumed execution position.

// A docs example is exactly where panicking on impossible failures is the
// clearest behavior — a real game would surface these instead.
#![expect(
    clippy::expect_used,
    reason = "docs example: failures here are bugs in the example itself"
)]

use std::collections::HashMap;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy_brink::{
    BrinkContext, BrinkFlowRequest, BrinkGlobals, BrinkPlugin, BrinkProgram, BrinkStoryAsset,
    FlowStart, LineTablesAsset, LoadReport, ProgramAsset, SaveState, advance_flow, load_flow_state,
    save_flow_state,
};
use bevy_ecs::system::SystemState;
use brink_runtime::{FlowInstance, Scope, WorldPolicy};

const STORY: &str = "\
VAR shared_count = 0
VAR mood = 0
-> greet
=== greet ===
~ mood = mood + 1
~ shared_count = shared_count + 1
Entity greeting: mood {mood}, shared seen {shared_count}
-> DONE
";

#[expect(
    clippy::similar_names,
    reason = "the paired a/b entity naming is the point of the demo"
)]
fn main() {
    // `mood` is private per flow; `shared_count` stays World-scoped by the
    // (untouched) default. See `app_with_policy` in `book_flows.rs` for the
    // policy-installation snippet itself.
    let mut policy = WorldPolicy::default();
    policy.overrides.insert("mood".to_string(), Scope::Local);

    let mut app1 = build_app(policy.clone(), true);
    let Some(story1) = build_story(&mut app1) else {
        warn!("failed to build story");
        return;
    };
    let entity_a = spawn_and_fulfill(&mut app1, &story1, FlowStart::Root);
    let entity_b = spawn_and_fulfill(&mut app1, &story1, FlowStart::Root);
    advance_flow::<()>(app1.world_mut(), entity_a).expect("advance A");
    advance_flow::<()>(app1.world_mut(), entity_b).expect("advance B");

    // Compose one save: world once, then each entity — a plain serde map,
    // exactly the shape a host persists however it likes.
    let saves = save_all(&mut app1, [("entity_a", entity_a), ("entity_b", entity_b)]);
    let json = serde_json::to_string_pretty(&saves).expect("SaveState is Serialize");
    info!("--- save file ({} bytes) ---\n{json}", json.len());

    // Round-trip through JSON, exactly like reading a save file back in.
    let loaded: HashMap<String, SaveState> =
        serde_json::from_str(&json).expect("SaveState is Deserialize");

    // A completely FRESH app: new World, new compile, new flows re-entered
    // at `greet` — not resumed mid-line (see `save_flow_state`'s docs).
    // `with_log: false` — a global tracing subscriber can only be installed
    // once per process; app1 already installed it.
    let mut app2 = build_app(policy, false);
    let Some(story2) = build_story(&mut app2) else {
        warn!("failed to build second story");
        return;
    };
    let entity_a2 = spawn_and_fulfill(&mut app2, &story2, FlowStart::Address("greet".to_string()));
    let entity_b2 = spawn_and_fulfill(&mut app2, &story2, FlowStart::Address("greet".to_string()));
    load_all(
        &mut app2,
        &loaded,
        [("entity_a", entity_a2), ("entity_b", entity_b2)],
    );

    let resumed_a = advance_flow::<()>(app2.world_mut(), entity_a2).expect("advance A2");
    let resumed_b = advance_flow::<()>(app2.world_mut(), entity_b2).expect("advance B2");
    info!("restored A: {}", resumed_a.text());
    info!("restored B: {}", resumed_b.text());
}

/// `with_log` installs bevy's `LogPlugin` — only the first `App` built in a
/// process should pass `true` (a global tracing subscriber can only be
/// installed once).
fn build_app(policy: WorldPolicy, with_log: bool) -> App {
    let mut app = App::new();
    if with_log {
        app.add_plugins(LogPlugin::default());
    }
    app.add_plugins((
        AssetPlugin::default(),
        BrinkPlugin::<()>::default().with_policy(policy),
    ));
    // advance_flow drives flows through the binding registry; this story
    // has no bindings, but the resource must exist.
    app.init_resource::<bevy_brink::BrinkBindings<()>>();
    app
}

fn build_story(app: &mut App) -> Option<Handle<BrinkStoryAsset>> {
    let out = brink_compiler::compile("demo.ink", |p| {
        if p == "demo.ink" {
            Ok(STORY.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no includes",
            ))
        }
    })
    .ok()?;
    let (program, tables) = brink_runtime::link(&out.data).ok()?;
    let (_, initial_context) = FlowInstance::new_at_root(&program);

    let world = app.world_mut();
    let program_h = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
        });
    let tables_h = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    Some(
        world
            .resource_mut::<Assets<BrinkStoryAsset>>()
            .add(BrinkStoryAsset {
                program: program_h,
                line_tables: tables_h,
            }),
    )
}

fn spawn_and_fulfill(app: &mut App, story: &Handle<BrinkStoryAsset>, start: FlowStart) -> Entity {
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

/// `SystemState` shape for the save/load helpers below: every flow's
/// components plus the assets and shared globals needed to build a
/// routing view (`flow_context_view`) and save/load through it.
type FlowState = SystemState<(
    Query<'static, 'static, (&'static BrinkProgram<()>, &'static mut BrinkContext<()>)>,
    ResMut<'static, BrinkGlobals<()>>,
    Res<'static, Assets<ProgramAsset>>,
)>;

/// Compose one save: the shared `World`'s [`SaveState`] plus one per named
/// entity, into a plain serde map — `bevy-brink` hands back `SaveState`s,
/// the host decides how to store the collection (here: a `HashMap` you
/// serialize directly; a real game might key by save-slot + NPC id).
fn save_all(app: &mut App, entities: [(&str, Entity); 2]) -> HashMap<String, SaveState> {
    let mut state: FlowState = SystemState::new(app.world_mut());
    let (mut flows, mut globals, programs) = state.get_mut(app.world_mut());
    let (prog, _) = flows.get(entities[0].1).expect("flow");
    let program = &programs.get(&prog.handle).expect("program asset").program;

    let mut saves = HashMap::new();
    // ANCHOR: save_world
    let world_save = globals.save_state(program);
    // ANCHOR_END: save_world
    saves.insert("world".to_string(), world_save);
    for (name, entity) in entities {
        let (_, mut ctx) = flows.get_mut(entity).expect("flow");
        // ANCHOR: save_entity
        let entity_save = save_flow_state(&mut globals, &mut ctx, program);
        // ANCHOR_END: save_entity
        saves.insert(name.to_string(), entity_save);
    }
    state.apply(app.world_mut());
    saves
}

/// Load a save map back: world first, then each named entity through its
/// own [`flow_context_view`] (`Local`-scoped entries land in that entity's
/// own `BrinkContext`; `World`-scoped entries idempotently rewrite the
/// shared `World` — see `save_flow_state`'s docs). Panics on a dirty
/// [`LoadReport`] here only because this demo has nothing stale to report;
/// a real host would surface `report.unknown_globals` to the player/log
/// instead.
fn load_all(app: &mut App, saves: &HashMap<String, SaveState>, entities: [(&str, Entity); 2]) {
    let mut state: FlowState = SystemState::new(app.world_mut());
    let (mut flows, mut globals, programs) = state.get_mut(app.world_mut());
    let (prog, _) = flows.get(entities[0].1).expect("flow");
    let program = &programs.get(&prog.handle).expect("program asset").program;

    let world_save = &saves["world"];
    // ANCHOR: load_world
    let report: LoadReport = globals.load_state(program, world_save);
    // ANCHOR_END: load_world
    assert!(report.is_clean(), "unexpected unknown globals: {report:?}");

    for (name, entity) in entities {
        let entity_save = &saves[name];
        let (_, mut ctx) = flows.get_mut(entity).expect("flow");
        // ANCHOR: load_entity
        let report = load_flow_state(&mut globals, &mut ctx, program, entity_save);
        // ANCHOR_END: load_entity
        assert!(report.is_clean(), "unexpected unknown globals: {report:?}");
    }
    state.apply(app.world_mut());
}

/// Spawn a fresh flow re-entered at a knot after a load — the host's choice
/// of where a restored entity resumes, since execution position itself is
/// never captured. Shown for the book; not called by `main` (which builds
/// `entity_a2`/`entity_b2` via `spawn_and_fulfill` directly above).
#[expect(
    dead_code,
    reason = "included verbatim into the Bevy book; not exercised standalone here"
)]
fn reenter_at_knot(commands: &mut Commands, story: Handle<BrinkStoryAsset>) {
    // ANCHOR: reenter
    commands.spawn(
        BrinkFlowRequest::<()>::builder()
            .story(story)
            .start(FlowStart::Address("greet".to_string()))
            .build(),
    );
    // ANCHOR_END: reenter
}
