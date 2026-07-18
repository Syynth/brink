//! Engine→ink batch entry point benchmark (#1058, BH-5).
//!
//! The alarm write-seam (`demos/compound/src/ink_alarm.rs`) folds a frame's
//! events into ink by calling ink functions — `decay(dt)` then one
//! `escalate_spotting(amount)` **per sighting**. Each is a separate
//! [`call_ink_function`](bevy_brink::call_ink_function), and each pays the full
//! VM-eval setup (building the `SystemState` over the flow components, assets,
//! and bindings). This bench measures the amortization
//! [`call_ink_functions`](bevy_brink::call_ink_functions) buys: one setup for
//! the whole frame's batch instead of one per call.
//!
//! Two `divan` cases at a fixed batch size `N` (a frame with N-1 sightings +
//! one `decay`):
//!
//! - `single_calls` — the status-quo loop: N separate `call_ink_function`s,
//!   N setups.
//! - `batch` — one `call_ink_functions` of the same N calls, one setup.
//!
//! Both report **per whole batch**; divide by `N` for the µs/call figure the
//! PR reports. The story is a tiny arithmetic accumulator compiled in memory —
//! the oracle corpus (`tests/tier{1,2,3}`) is never touched.
//!
//! ```sh
//! cargo bench -p bevy-brink --bench batch_call
//! ```
#![expect(
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "benchmark harness: a panic on a broken fixture is the right failure, and the tiny usize→f32 index cast is cosmetic (same stance as the scenario bench)"
)]

use bevy_app::App;
use bevy_asset::{AssetPlugin, Assets, Handle};
use bevy_brink::{
    BrinkFlow, BrinkFlowRequest, BrinkPlugin, BrinkStoryAsset, LineTablesAsset, ProgramAsset,
    Value, call_ink_function, call_ink_functions,
};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::With;
use brink_runtime::FlowInstance;

/// Batch sizes to sweep: a calm frame (1 = just `decay`) up to a busy frame.
const BATCH_SIZES: &[usize] = &[1, 2, 4, 8, 16];

/// A minimal accumulator story mirroring the alarm's shape: one global, one
/// function that mutates it and returns the running value. `bump(x)` stands in
/// for both `decay` and `escalate_spotting` — the point is the per-call VM
/// re-entry cost, not the arithmetic.
const STORY: &str = "\
VAR level = 0.0
-> END

=== function bump(x) ===
~ level = level * 0.99 + x
~ return level
";

/// Compile `STORY` inline and insert the story assets, returning the handle.
/// Mirrors the asset loader without file IO (same recipe as the
/// `engine_bindings` example).
fn build_story(app: &mut App) -> Handle<BrinkStoryAsset> {
    let output = brink_compiler::compile("bench.ink", move |path| {
        if path == "bench.ink" {
            Ok(STORY.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no includes",
            ))
        }
    })
    .expect("bench story compiles");
    let (program, tables) = brink_runtime::link(&output.data).expect("bench story links");
    let (_, initial_context) = FlowInstance::new_at_root(&program);

    let world = app.world_mut();
    let program = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
            effect_rows: Vec::new(),
        });
    let line_tables = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    world
        .resource_mut::<Assets<BrinkStoryAsset>>()
        .add(BrinkStoryAsset {
            program,
            line_tables,
        })
}

/// Build a headless app, load the story, and fulfil its flow — returning the
/// app and the live flow entity ready to be driven.
fn setup() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((AssetPlugin::default(), BrinkPlugin::<()>::default()));
    let story = build_story(&mut app);
    app.world_mut()
        .spawn(BrinkFlowRequest::<()>::builder().story(story).build());
    app.update(); // fulfil the request
    let flow = app
        .world_mut()
        .query_filtered::<Entity, With<BrinkFlow<()>>>()
        .iter(app.world())
        .next()
        .expect("flow fulfilled");
    (app, flow)
}

/// The N `(name, args)` calls a frame of size `n` folds into ink.
fn frame_calls(n: usize) -> Vec<(&'static str, Vec<Value>)> {
    (0..n)
        .map(|i| ("bump", vec![Value::Float(0.5 + i as f32 * 0.01)]))
        .collect()
}

/// Status quo: N separate `call_ink_function`s — N VM-eval setups.
#[divan::bench(args = BATCH_SIZES)]
fn single_calls(bencher: divan::Bencher, n: usize) {
    let (mut app, flow) = setup();
    let calls = frame_calls(n);
    bencher.bench_local(|| {
        for (name, args) in &calls {
            let v =
                call_ink_function::<()>(app.world_mut(), flow, name, args).expect("bump resolves");
            divan::black_box(v);
        }
    });
}

/// The batch entry point: one `call_ink_functions` of the same N calls — one
/// VM-eval setup amortized across the frame.
#[divan::bench(args = BATCH_SIZES)]
fn batch(bencher: divan::Bencher, n: usize) {
    let (mut app, flow) = setup();
    let calls = frame_calls(n);
    bencher.bench_local(|| {
        // Borrow the prebuilt calls (no per-iteration clone) so the only
        // difference measured against `single_calls` is the amortized setup.
        let results = call_ink_functions::<(), _, _>(
            app.world_mut(),
            flow,
            calls.iter().map(|(name, args)| (*name, args.as_slice())),
        );
        for r in results {
            divan::black_box(r.expect("bump resolves"));
        }
    });
}

fn main() {
    divan::main();
}
