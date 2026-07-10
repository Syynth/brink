//! Headless example: **`bind_brink_task`** — an async external whose value is
//! computed by a detached task and resolved across frames.
//!
//! Run with `cargo run --example async_task`. The story calls
//! `expensive_roll(20)`; the flow *parks* while bevy-brink runs the future on
//! [`AsyncComputeTaskPool`](bevy_tasks::AsyncComputeTaskPool) off the main
//! thread, then `poll_brink_tasks` resolves it and the flow resumes — all
//! driven by ordinary `app.update()` frames.
//!
//! Use task bindings for work that doesn't touch the World (heavy compute, IO,
//! network). For World-dependent multi-frame work (UI, input), use
//! `bind_brink_async` + `BrinkExternalAwaited` (see the `async_external`
//! example).

#![expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]

use std::time::Duration;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy_brink::{
    Advance, BrinkBindings, BrinkBindingsAppExt, BrinkContext, BrinkFlow, BrinkFlowRequest,
    BrinkLocale, BrinkPlugin, BrinkProgram, BrinkStoryAsset, LineTablesAsset, ProgramAsset, Value,
};
use brink_runtime::FlowInstance;

const STORY: &str = "\
EXTERNAL expensive_roll(sides)
The dice tumble across the table...
You rolled a {expensive_roll(20)}.
-> END
";

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
    app.init_resource::<Finished>();

    // expensive_roll resolves on the async task pool — the future is
    // `Send + 'static`, runs off the main thread, and computes purely from the
    // ink arguments (no World access).
    app.bind_brink_task::<(), _, _>("expensive_roll", |args: Vec<Value>| async move {
        let sides = args.first().and_then(Value::as_int).unwrap_or(6);
        // Simulate real off-thread work that takes time: the flow stays parked
        // across many app frames until this future completes. (A real binding
        // might `.await` a network round-trip or run a heavy computation.)
        std::thread::sleep(Duration::from_millis(40));
        Value::Int(sides / 2 + 4)
    });
    app.add_systems(Update, drive_flows);

    let Some(story) = build_story(&mut app, STORY) else {
        warn!("failed to compile story");
        return;
    };
    app.world_mut()
        .spawn(BrinkFlowRequest::<()>::builder().story(story).build());
    app.update(); // fulfill the request → live flow components

    // Tick until the flow ends (capped so the example can't spin forever).
    for frame in 0..1000 {
        app.update();
        if app.world().resource::<Finished>().0 {
            info!("flow finished by frame {frame}");
            break;
        }
    }
    info!("done.");
}

/// Step every non-parked flow once per frame, logging each line. Flows parked
/// on a pending external are left alone — `resolve_pending_externals` spawns
/// the task and `poll_brink_tasks` resolves it; we resume on a later frame.
#[expect(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "bevy driver query tuple; the shared BrinkGlobals param joins the driver params"
)]
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
    // Announce the parked state once (the loop runs every frame while parked).
    mut announced: Local<bool>,
) {
    let Some(mut globals) = globals else {
        return; // no flow fulfilled yet this tick
    };
    for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
        if flow.inner.has_pending_external() {
            if !*announced {
                info!("[await]    flow parked — expensive_roll running off-thread…");
                *announced = true;
            }
            continue;
        }
        *announced = false;
        if finished.0 {
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
            // Parked on the task this step; the plugin resolver/poller handles
            // it and we resume next frame.
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
