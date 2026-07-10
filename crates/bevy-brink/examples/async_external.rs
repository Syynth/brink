//! Headless example: **`bind_brink_async`** — the event+resolve primitive for
//! a multi-frame World interaction.
//!
//! Run with `cargo run --example async_external`. The story calls
//! `pick_target()`; the flow *parks* and [`BrinkExternalAwaited`] fires at the
//! flow entity. An observer "opens a targeting UI", a few frames later a
//! simulated player "click" resolves it via
//! [`resolve_brink_external`](bevy_brink::BrinkResolveExternalExt::resolve_brink_external),
//! and the flow resumes with the chosen value.
//!
//! This is the flavor for async work that needs the World over several frames
//! (UI, input, awaiting world-state). For off-thread compute that doesn't
//! touch the World, use `bind_brink_task` (see the `async_task` example).
//!
//! Correlation needs no id: a flow parks on exactly one external and is frozen
//! until resolved, so the **flow entity** is the key.

#![expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy_brink::{
    Advance, BrinkBindings, BrinkBindingsAppExt, BrinkContext, BrinkExternalAwaited, BrinkFlow,
    BrinkFlowRequest, BrinkLocale, BrinkPlugin, BrinkProgram, BrinkResolveExternalExt,
    BrinkStoryAsset, LineTablesAsset, ProgramAsset, Value,
};
use brink_runtime::FlowInstance;

const STORY: &str = "\
EXTERNAL pick_target()
The battlefield falls silent. Choose your target.
You strike at unit {pick_target()}!
-> END
";

/// A pending targeting interaction: which flow is waiting, and a countdown
/// standing in for "frames until the player clicks".
#[derive(Resource, Default)]
struct Targeting {
    flow: Option<Entity>,
    frames_until_click: u32,
}

/// Set when a flow reaches its terminal line, so `main` can stop ticking.
#[derive(Resource, Default)]
struct Finished(bool);

fn main() {
    let mut app = App::new();
    app.add_plugins((
        LogPlugin::default(),
        AssetPlugin::default(),
        BrinkPlugin::<()>::default(),
    ));
    app.init_resource::<Targeting>();
    app.init_resource::<Finished>();

    // pick_target needs a multi-frame World interaction (the targeting UI), so
    // it's an async *event* binding: the flow parks and BrinkExternalAwaited
    // fires. We do the rest with an observer + a system that simulates input.
    app.bind_brink_async::<()>("pick_target");
    app.add_observer(on_target_awaited);
    app.add_systems(Update, (simulate_player_click, drive_flows));

    let Some(story) = build_story(&mut app, STORY) else {
        warn!("failed to compile story");
        return;
    };
    app.world_mut()
        .spawn(BrinkFlowRequest::<()>::builder().story(story).build());
    app.update(); // fulfill the request → live flow components

    for frame in 0..1000 {
        app.update();
        if app.world().resource::<Finished>().0 {
            info!("flow finished by frame {frame}");
            break;
        }
    }
    info!("done.");
}

/// Observer: the flow parked on `pick_target` — "open the targeting UI".
/// Records the awaiting flow so `simulate_player_click` can resolve it later.
fn on_target_awaited(on: On<BrinkExternalAwaited<()>>, mut targeting: ResMut<Targeting>) {
    let ev = on.event();
    info!(
        "[await]    {} — opening targeting UI for flow {:?}",
        ev.name, ev.entity
    );
    targeting.flow = Some(ev.entity);
    targeting.frames_until_click = 3; // pretend the player takes a few frames
}

/// Stand-in for player input: count down, then "click" target 7 and resolve
/// the parked external with it.
fn simulate_player_click(mut targeting: ResMut<Targeting>, mut commands: Commands) {
    let Some(flow) = targeting.flow else {
        return;
    };
    if targeting.frames_until_click > 0 {
        targeting.frames_until_click -= 1;
        return;
    }
    info!("[input]    player clicked unit 7");
    commands.resolve_brink_external::<()>(flow, Value::Int(7));
    targeting.flow = None;
}

/// Step every non-parked flow once per frame, logging each line. Parked flows
/// are left to the observer/`simulate_player_click` to resolve.
#[expect(clippy::type_complexity, reason = "bevy driver query tuple")]
fn drive_flows(
    mut flows: Query<(
        Entity,
        &mut BrinkFlow<()>,
        &mut BrinkContext<()>,
        &BrinkProgram<()>,
        &BrinkLocale<()>,
    )>,
    globals: Option<ResMut<bevy_brink::BrinkGlobals<()>>>,
    programs: Res<Assets<ProgramAsset>>,
    tables: Res<Assets<LineTablesAsset>>,
    bindings: Res<BrinkBindings<()>>,
    mut commands: Commands,
    mut finished: ResMut<Finished>,
) {
    let Some(mut globals) = globals else {
        return; // no flow fulfilled yet this tick
    };
    for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
        if flow.inner.has_pending_external() || finished.0 {
            continue;
        }
        let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle)) else {
            continue;
        };
        let handler = bindings.handler();
        let mut view = bevy_brink::flow_context_view(&mut globals, &mut ctx);
        match flow.step_one(
            &p.program,
            &t.tables,
            &mut view,
            &handler,
            entity,
            &mut commands,
        ) {
            Ok(Advance::Line(line)) => {
                let text = line.text().trim_end();
                if !text.is_empty() {
                    info!("[line]     {text}");
                }
                if line.is_terminal() {
                    finished.0 = true;
                }
            }
            Ok(Advance::AwaitingQuery) => {}
            Err(err) => {
                warn!("step failed: {err}");
                finished.0 = true;
            }
        }
        handler.flush(&mut commands);
    }
}

/// Compile `src` inline, link it, and insert the story assets, returning a
/// handle. Mirrors what the asset loader does, without file IO.
fn build_story(app: &mut App, src: &str) -> Option<Handle<BrinkStoryAsset>> {
    let owned = src.to_string();
    let output = brink_compiler::compile("demo.ink", move |path| {
        if path == "demo.ink" {
            Ok(owned.clone())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no includes",
            ))
        }
    })
    .ok()?;
    let (program, tables) = brink_runtime::link(&output.data).ok()?;
    let (_, initial_context) = FlowInstance::new_at_root(&program);

    let world = app.world_mut();
    let program = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
        });
    let line_tables = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    Some(
        world
            .resource_mut::<Assets<BrinkStoryAsset>>()
            .add(BrinkStoryAsset {
                program,
                line_tables,
            }),
    )
}
