//! Headless example: the **engine↔ink dynamic binding facility**.
//!
//! Run with `cargo run --example engine_bindings`. It prints a short
//! trace (via `info!`) and exits — no window.
//!
//! Where `play_story` covers interactive playback (pure `bind_brink_fn`
//! and fire-and-forget `bind_brink_command`, the transcript, choices,
//! hot-reload), this one covers the *dynamic world-access* pieces:
//!
//! 1. **`bind_brink_query`** — an ink function reads live World state. The
//!    binding is a Bevy system with arbitrary params; it runs against the
//!    World, so the designer can query anything with no upfront
//!    declaration.
//! 2. **`call_ink_function`** — the engine evaluates an ink function
//!    synchronously from an exclusive (`&mut World`) context. The ink
//!    function may itself call world-access bindings; they resolve inline.
//! 3. **`commands.brink_call(...).observe(...)`** — the same, but deferred,
//!    for a normal (non-exclusive) system. The result is delivered to a
//!    per-call-scoped observer, so it can never be mis-correlated.
//! 4. **`commands.brink_call_batch(...).observe(...)`** (#1076) — the
//!    deferred *batch* counterpart: a normal system queues a whole ordered
//!    call list at once, resolved through `call_ink_functions` in one
//!    VM-eval setup, delivering one result `Vec` (call order preserved) to
//!    the observer.
//! 5. **`advance_flow`** — drive normal playback from an exclusive context,
//!    resolving inline `{query()}` calls in the narration transparently.
//!
//! The story is compiled inline and its assets inserted directly (no file
//! loading), so the example is fully deterministic.

#![expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res/Query by value"
)]

use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy_brink::{
    BrinkBindingsAppExt, BrinkCallBatchResolved, BrinkCallCommandsExt, BrinkCallResolved,
    BrinkCommand, BrinkFlow, BrinkFlowRequest, BrinkPlugin, BrinkQueryInput, BrinkStoryAsset,
    LineTablesAsset, ProgramAsset, Value, advance_flow, call_ink_function,
};
use brink_runtime::FlowInstance;

/// The story. `can_advance()` and the inline `{enemy_count()}` both call
/// the world-access `enemy_count` query binding; `play_sound` is a
/// fire-and-forget command; `shout` is a pure function. `bump_checks()`
/// mutates `checks` and returns the running total — used to demonstrate
/// the batch call's front-to-back ordering guarantee.
const STORY: &str = "\
EXTERNAL enemy_count()
EXTERNAL play_sound(name)
EXTERNAL shout(text)
VAR checks = 0
~ play_sound(\"ambient_hum\")
You scan the clearing. Enemies near: {enemy_count()}.
{shout(\"stay sharp\")}
-> END

=== function can_advance() ===
~ return enemy_count() < 3

=== function bump_checks() ===
~ checks = checks + 1
~ return checks
";

/// A unit of live World state the ink story can query.
#[derive(Component)]
struct Enemy;

/// Fire-and-forget command event (see `bind_brink_command`).
#[derive(Event, BrinkCommand)]
struct PlaySound {
    name: String,
}

/// World-access query binding: count `Enemy` entities. Runs against the
/// World via `run_system_with`; gets the calling flow entity + ink args.
fn enemy_count(In((_flow, _args)): In<BrinkQueryInput>, enemies: Query<&Enemy>) -> Value {
    #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    Value::Int(enemies.iter().count() as i32)
}

/// React to the story's `~ play_sound(...)` — a real game plays audio here.
fn on_play_sound(trigger: On<PlaySound>) {
    info!("[command]  play_sound(\"{}\")", trigger.event().name);
}

fn main() {
    let mut app = App::new();
    app.add_plugins((
        LogPlugin::default(),
        AssetPlugin::default(),
        BrinkPlugin::<()>::default(),
    ));

    // ── Register the bindings ──────────────────────────────────────────
    app.bind_brink_query::<(), _, _>("enemy_count", enemy_count)
        .bind_brink_fn::<(), _, _>("shout", |args| {
            args.first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_uppercase()
        })
        .bind_brink_command::<(), PlaySound>("play_sound")
        .add_observer(on_play_sound)
        .add_systems(Update, (request_deferred_check, request_deferred_batch));

    // ── Build the story + the world it queries ─────────────────────────
    let Some(story) = build_story(&mut app, STORY) else {
        warn!("failed to compile story");
        return;
    };
    app.world_mut().spawn(Enemy);
    app.world_mut().spawn(Enemy);
    let flow = app
        .world_mut()
        .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
        .id();

    // One tick fulfills the request (request → live flow components).
    app.update();

    // ── (2) Engine→ink, synchronous — call_ink_function ────────────────
    // can_advance() calls the world-access enemy_count() binding; it
    // resolves inline because we're in an exclusive (&mut World) context.
    info!("--- engine→ink (sync): can_advance() ---");
    report_can_advance(&mut app, flow); // 2 enemies → true
    app.world_mut().spawn(Enemy);
    app.world_mut().spawn(Enemy);
    report_can_advance(&mut app, flow); // 4 enemies → false
    // Restore to 2 enemies for the rest of the demo.
    despawn_two_enemies(&mut app);

    // ── (3) Engine→ink, deferred — commands.brink_call(_batch) ─────────
    // request_deferred_check and request_deferred_batch (normal systems)
    // each issued their request; a few ticks let the plugin's resolvers
    // evaluate them and fire their observers.
    info!("--- engine→ink (deferred): commands.brink_call / brink_call_batch ---");
    for _ in 0..3 {
        app.update();
    }

    // ── (5) Playback with inline world queries — advance_flow ──────────
    info!("--- playback (advance_flow): inline {{enemy_count()}} ---");
    drive_to_end(&mut app, flow);

    info!("done.");
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
    // The fresh starting Context (VAR defaults, zero counts) — the runtime
    // produces it alongside a root flow; we only want the Context.
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
    Some(
        world
            .resource_mut::<Assets<BrinkStoryAsset>>()
            .add(BrinkStoryAsset {
                program,
                line_tables,
            }),
    )
}

/// Evaluate `can_advance()` synchronously and log the result.
fn report_can_advance(app: &mut App, flow: Entity) {
    match call_ink_function::<()>(app.world_mut(), flow, "can_advance", &[]) {
        Ok(value) => info!("can_advance() = {:?}", value.as_bool()),
        Err(err) => warn!("can_advance() failed: {err}"),
    }
}

/// A normal (non-exclusive) system: issue one deferred `brink_call` once a
/// flow exists, reacting to the result with a per-call-scoped observer.
fn request_deferred_check(
    mut issued: Local<bool>,
    mut commands: Commands,
    flows: Query<Entity, With<BrinkFlow<()>>>,
) {
    if *issued {
        return;
    }
    if let Ok(flow) = flows.single() {
        *issued = true;
        commands.brink_call::<()>(flow, "can_advance", ()).observe(
            |on: On<BrinkCallResolved<()>>| {
                info!(
                    "can_advance() = {:?} (delivered to observer)",
                    on.event().value.as_bool()
                );
            },
        );
    }
}

/// A normal (non-exclusive) system: issue one deferred *batch*
/// (`brink_call_batch`, #1076) once a flow exists — two `bump_checks()`
/// calls then a `can_advance()`, run front-to-back in a single VM-eval
/// setup, reacting to the whole ordered result `Vec` with one
/// per-batch-scoped observer.
fn request_deferred_batch(
    mut issued: Local<bool>,
    mut commands: Commands,
    flows: Query<Entity, With<BrinkFlow<()>>>,
) {
    if *issued {
        return;
    }
    if let Ok(flow) = flows.single() {
        *issued = true;
        commands
            .brink_call_batch::<()>(
                flow,
                [
                    ("bump_checks", Vec::<Value>::new()),
                    ("bump_checks", Vec::<Value>::new()),
                    ("can_advance", Vec::<Value>::new()),
                ],
            )
            .observe(|on: On<BrinkCallBatchResolved<()>>| {
                info!(
                    "brink_call_batch results (delivered to observer, in order): {:?}",
                    on.event().results
                );
            });
    }
}

/// Drive the flow to its end with `advance_flow`, resolving inline world
/// queries, logging each line.
fn drive_to_end(app: &mut App, flow: Entity) {
    loop {
        match advance_flow::<()>(app.world_mut(), flow) {
            Ok(line) => {
                let text = line.text().trim_end();
                if !text.is_empty() {
                    info!("[line]     {text}");
                }
                if line.is_terminal() {
                    break;
                }
            }
            Err(err) => {
                warn!("advance_flow failed: {err}");
                break;
            }
        }
    }
}

/// Remove two `Enemy` entities (restore the count after the sync demo).
fn despawn_two_enemies(app: &mut App) {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<Enemy>>();
    let to_remove: Vec<Entity> = query.iter(world).take(2).collect();
    for entity in to_remove {
        world.despawn(entity);
    }
}
