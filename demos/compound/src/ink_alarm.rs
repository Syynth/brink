//! Phase 1a: the global alarm, driven by **ink** instead of Rust.
//!
//! This is the first drive-app-plan (`docs/drive-app-plan.md` §9) entity
//! migration. `src/alarm.rs` stays put as the Rust baseline; this module is
//! the side-by-side ink port selected at launch with `--alarm-impl ink`. Only
//! *one* of the two writers runs per round (see the run-conditions in
//! `main.rs`); everything downstream (`guards`, `cameras`, `hud`) still reads
//! the shared [`Alarm`] resource and never learns which writer filled it.
//!
//! The escalation STATE and LOGIC live in `assets/alarm.ink`. This module is
//! only the two seams the port exists to exercise:
//!
//! * **World-policy WRITE seam.** Guards and cameras emit [`SpottedEvent`] /
//!   [`GlobalAlarm`] messages exactly as before. [`ink_alarm_system`] folds
//!   each frame's messages into ink by *calling the ink functions*
//!   ([`call_ink_function`]) — `decay(dt)`, then `escalate_spotting(amount)`
//!   per sighting, then `trigger_global()` if a guard reached a panel. Because
//!   the soft-cap / decay math must live in ink, calling into ink functions is
//!   the natural grain here: a `bind_brink_*` binding is ink→engine (the wrong
//!   direction), and a raw `set_global` would bypass the very logic the port
//!   is meant to move into ink. See `MIGRATION.md` for the rationale.
//!
//! * **World-policy READ seam.** After driving ink, the system mirrors ink's
//!   `alarm_level` / `alarm_global` globals back into the shared [`Alarm`]
//!   resource. Every reader stays a cheap ECS `Res<Alarm>` read — mirroring
//!   ink state into an ECS resource once per frame is far cheaper than each
//!   reader re-entering the VM, and re-entry needs `&mut World` anyway. That
//!   ergonomic finding is the point of the seam (`MIGRATION.md`).

use std::time::Instant;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy_brink::runtime::ContextAccess as _;
use bevy_brink::{
    BrinkFlow, BrinkFlowRequest, BrinkGlobals, BrinkProgram, BrinkStoryAsset, ProgramAsset, Value,
    call_ink_function,
};

use crate::alarm::{Alarm, GlobalAlarm, SpottedEvent};
use crate::rounds::StartRound;
use crate::timing::BehaviorTimings;

/// Story marker for the alarm ink instance. Gives it its own
/// `BrinkGlobals<AlarmStory>` and `BrinkFlow<AlarmStory>` so a later entity
/// port (guards, cameras) can run a *different* story under its own marker in
/// the same app with no runtime overhead.
pub struct AlarmStory;

/// Which implementation drives the alarm this run, chosen at launch.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlarmImpl {
    /// The Rust baseline (`src/alarm.rs`, `alarm_system`).
    #[default]
    Rust,
    /// The ink port (`assets/alarm.ink`, [`ink_alarm_system`]).
    Ink,
}

impl AlarmImpl {
    /// Parse `--alarm-impl rust|ink` from the process args (default: Rust).
    #[must_use]
    pub fn from_args() -> Self {
        let mut args = std::env::args();
        while let Some(arg) = args.next() {
            let value = if arg == "--alarm-impl" {
                args.next()
            } else {
                arg.strip_prefix("--alarm-impl=").map(str::to_owned)
            };
            if let Some(value) = value {
                return match value.as_str() {
                    "ink" => Self::Ink,
                    _ => Self::Rust,
                };
            }
        }
        Self::Rust
    }

    /// Human-readable label for the HUD.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Ink => "ink",
        }
    }
}

/// Run condition: the Rust alarm writer is active.
pub fn alarm_is_rust(mode: Res<AlarmImpl>) -> bool {
    *mode == AlarmImpl::Rust
}

/// Run condition: the ink alarm writer is active.
pub fn alarm_is_ink(mode: Res<AlarmImpl>) -> bool {
    *mode == AlarmImpl::Ink
}

/// Startup (ink mode only): load `alarm.ink` and spawn its flow request. The
/// `dev`-feature `InkLoader` compiles the source at asset-load time, so a live
/// edit to `assets/alarm.ink` hot-reloads the alarm logic while the game runs.
pub fn spawn_ink_alarm(asset_server: Res<AssetServer>, mut commands: Commands) {
    let story: Handle<BrinkStoryAsset> = asset_server.load("alarm.ink");
    commands.spawn(
        BrinkFlowRequest::<AlarmStory>::builder()
            .story(story)
            .build(),
    );
    info!("[alarm] ink mode: loading alarm.ink");
}

/// Reader bundle for the exclusive driver: `Time`, the two alarm messages, and
/// round-start (so ink state is reset in ink too, not just the mirror).
type DriverReaders<'w, 's> = SystemState<(
    Res<'w, Time>,
    MessageReader<'w, 's, SpottedEvent>,
    MessageReader<'w, 's, GlobalAlarm>,
    MessageReader<'w, 's, StartRound>,
)>;

/// The ink counterpart of `alarm::alarm_system`. Exclusive because
/// [`call_ink_function`] re-enters the VM and needs `&mut World`.
///
/// Records the *ink* alarm's per-frame cost into [`BehaviorTimings::alarm`] —
/// the same field the Rust `alarm_system` fills — so the HUD's alarm µs line
/// reports whichever writer is live, directly against the ~42 ns Rust number.
pub fn ink_alarm_system(
    world: &mut World,
    mut readers: Local<Option<DriverReaders<'static, 'static>>>,
) {
    // Bail until the async asset has loaded and the flow is fulfilled.
    let Some(flow) = world
        .query_filtered::<Entity, With<BrinkFlow<AlarmStory>>>()
        .iter(world)
        .next()
    else {
        return;
    };

    let readers = readers.get_or_insert_with(|| SystemState::new(world));
    let (dt, spots, has_global, round_started) = {
        let Ok((time, mut spotted, mut global, mut start)) = readers.get_mut(world) else {
            return;
        };
        let dt = time.delta_secs();
        let spots: Vec<f32> = spotted.read().map(|e| e.intensity).collect();
        let has_global = global.read().count() > 0;
        let round_started = start.read().count() > 0;
        (dt, spots, has_global, round_started)
    };

    // ── WRITE seam: fold this frame's events into ink. The wall-clock cost of
    //    this block is the ink alarm's per-frame number.
    let start = Instant::now();
    if round_started {
        call_alarm_fn(world, flow, "alarm_reset", &[]);
    }
    call_alarm_fn(world, flow, "decay", &[Value::Float(dt)]);
    for amount in spots {
        call_alarm_fn(world, flow, "escalate_spotting", &[Value::Float(amount)]);
    }
    if has_global {
        call_alarm_fn(world, flow, "trigger_global", &[]);
    }
    let elapsed = start.elapsed();

    // ── READ seam: mirror ink globals → the shared Alarm resource.
    let (level, global_latch) = read_alarm_state(world, flow);
    {
        let mut alarm = world.resource_mut::<Alarm>();
        alarm.level = level;
        alarm.global = global_latch;
    }
    world.resource_mut::<BehaviorTimings>().alarm = elapsed;
}

/// Call one alarm ink function, logging (not swallowing) any error so a broken
/// binding surfaces instead of silently freezing the alarm.
fn call_alarm_fn(world: &mut World, flow: Entity, name: &str, args: &[Value]) {
    if let Err(err) = call_ink_function::<AlarmStory>(world, flow, name, args) {
        warn!("[alarm] ink call {name} failed: {err}");
    }
}

/// Read the ink-owned `alarm_level` / `alarm_global` globals out of the story
/// `World`. This is the literal world-policy read: resolve each name to its
/// global slot on the [`Program`](bevy_brink::runtime), then read the slot from
/// [`BrinkGlobals`]. Tier is derived Rust-side by [`Alarm::tier`] exactly as in
/// the baseline (`level.floor()`), so the tier every reader sees is a floor of
/// ink-owned state.
fn read_alarm_state(world: &World, flow: Entity) -> (f32, bool) {
    let Some(prog) = world.get::<BrinkProgram<AlarmStory>>(flow) else {
        return (0.0, false);
    };
    let Some(assets) = world.get_resource::<Assets<ProgramAsset>>() else {
        return (0.0, false);
    };
    let Some(program) = assets.get(&prog.handle).map(|a| &a.program) else {
        return (0.0, false);
    };
    let Some(globals) = world.get_resource::<BrinkGlobals<AlarmStory>>() else {
        return (0.0, false);
    };
    let level = program
        .global_index("alarm_level")
        .and_then(|i| globals.inner.global(i).as_float())
        .unwrap_or(0.0);
    let global = program
        .global_index("alarm_global")
        .and_then(|i| globals.inner.global(i).as_bool())
        .unwrap_or(false);
    (level, global)
}

/// Frame-by-frame semantics parity: the ink alarm and the Rust [`Alarm`] must
/// reach identical tier / global / level when driven through the same event
/// script. This is the port's correctness bar (drive-app-plan Phase 1a).
#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::AssetPlugin;
    use bevy_brink::runtime::FlowInstance;
    use bevy_brink::{BrinkPlugin, LineTablesAsset};

    /// One scripted frame applied identically to both implementations.
    struct Frame {
        dt: f32,
        spots: &'static [f32],
        global: bool,
        reset: bool,
    }

    /// Compile `assets/alarm.ink` inline (deterministic, no async `AssetServer`)
    /// and insert the story assets, mirroring what `InkLoader` does at runtime.
    fn build_alarm_story(app: &mut App) -> Handle<BrinkStoryAsset> {
        let src = include_str!("../assets/alarm.ink").to_string();
        let output = brink_compiler::compile("alarm.ink", move |path| {
            if path == "alarm.ink" {
                Ok(src.clone())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no includes",
                ))
            }
        })
        .expect("alarm.ink compiles");
        let (program, tables) = bevy_brink::runtime::link(&output.data).expect("alarm.ink links");
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

    /// Drive one frame of the ink alarm through the same call sequence
    /// `ink_alarm_system` uses (reset? → decay → escalate* → global?).
    fn step_ink(app: &mut App, flow: Entity, frame: &Frame) {
        let world = app.world_mut();
        if frame.reset {
            call_ink_function::<AlarmStory>(world, flow, "alarm_reset", &[]).expect("reset");
        }
        call_ink_function::<AlarmStory>(world, flow, "decay", &[Value::Float(frame.dt)])
            .expect("decay");
        for &amount in frame.spots {
            call_ink_function::<AlarmStory>(
                world,
                flow,
                "escalate_spotting",
                &[Value::Float(amount)],
            )
            .expect("escalate");
        }
        if frame.global {
            call_ink_function::<AlarmStory>(world, flow, "trigger_global", &[]).expect("global");
        }
    }

    /// Drive one frame of the Rust alarm through the identical sequence.
    fn step_rust(alarm: &mut Alarm, frame: &Frame) {
        if frame.reset {
            alarm.reset();
        }
        alarm.decay(frame.dt);
        for &amount in frame.spots {
            alarm.escalate_spotting(amount);
        }
        if frame.global {
            alarm.trigger_global();
        }
    }

    /// Measure the ink alarm's per-frame drive cost (a calm frame = one
    /// `decay(dt)` call, and a hot frame = decay + one spotting) against the
    /// Rust baseline. Prints the numbers for the friction journal; run with
    /// `cargo test measure_ink_alarm_cost -- --nocapture`.
    #[test]
    fn measure_ink_alarm_cost() {
        use std::hint::black_box;

        const N: u32 = 10_000;

        let mut app = App::new();
        app.add_plugins((AssetPlugin::default(), BrinkPlugin::<AlarmStory>::default()));
        let story = build_alarm_story(&mut app);
        app.world_mut().spawn(
            BrinkFlowRequest::<AlarmStory>::builder()
                .story(story)
                .build(),
        );
        app.update();
        let flow = app
            .world_mut()
            .query_filtered::<Entity, With<BrinkFlow<AlarmStory>>>()
            .iter(app.world())
            .next()
            .expect("alarm flow fulfilled");

        let dt = 1.0 / 60.0;

        // Calm frame: one decay call.
        let start = std::time::Instant::now();
        for _ in 0..N {
            call_ink_function::<AlarmStory>(app.world_mut(), flow, "decay", &[Value::Float(dt)])
                .expect("decay");
        }
        let calm = start.elapsed() / N;

        // Hot frame: decay + one spotting escalation.
        let start = std::time::Instant::now();
        for _ in 0..N {
            call_ink_function::<AlarmStory>(app.world_mut(), flow, "decay", &[Value::Float(dt)])
                .expect("decay");
            call_ink_function::<AlarmStory>(
                app.world_mut(),
                flow,
                "escalate_spotting",
                &[Value::Float(0.5)],
            )
            .expect("escalate");
        }
        let hot = start.elapsed() / N;

        // The Rust baseline, same shapes (black_box keeps the optimizer from
        // collapsing the pure-arithmetic loop).
        let mut alarm = Alarm::default();
        let start = std::time::Instant::now();
        for _ in 0..N {
            black_box(&mut alarm).decay(black_box(dt));
        }
        let rust_calm = start.elapsed() / N;
        let start = std::time::Instant::now();
        for _ in 0..N {
            black_box(&mut alarm).decay(black_box(dt));
            black_box(&mut alarm).escalate_spotting(black_box(0.5));
        }
        let rust_hot = start.elapsed() / N;

        println!(
            "ink alarm per-frame: calm {calm:?}  hot {hot:?}   | rust baseline: calm {rust_calm:?}  hot {rust_hot:?}"
        );
    }

    #[test]
    fn ink_alarm_matches_rust_frame_by_frame() {
        let mut app = App::new();
        app.add_plugins((AssetPlugin::default(), BrinkPlugin::<AlarmStory>::default()));
        let story = build_alarm_story(&mut app);
        app.world_mut().spawn(
            BrinkFlowRequest::<AlarmStory>::builder()
                .story(story)
                .build(),
        );
        // One tick fulfills the request (inserted assets are immediately ready).
        app.update();
        let flow = app
            .world_mut()
            .query_filtered::<Entity, With<BrinkFlow<AlarmStory>>>()
            .iter(app.world())
            .next()
            .expect("alarm flow fulfilled");

        // A script that exercises every alarm regime the Rust module tests,
        // but *over time*: ramp to the soft cap, decay, panel jump, spotting
        // that must not lower a global, full bleed-out (clears the latch), and
        // a round-start reset back to calm.
        let dt = 1.0 / 60.0;
        let mut script: Vec<Frame> = Vec::new();
        // Ramp: repeated spotting soft-caps at 1.9 (tier 1), never sweeps.
        for _ in 0..20 {
            script.push(Frame {
                dt,
                spots: &[0.5],
                global: false,
                reset: false,
            });
        }
        // Decay only for a stretch (dt = 0.1s steps).
        for _ in 0..10 {
            script.push(Frame {
                dt: 0.1,
                spots: &[],
                global: false,
                reset: false,
            });
        }
        // A guard reaches a panel: jump to 3.0 (tier 3), latch global.
        script.push(Frame {
            dt,
            spots: &[],
            global: true,
            reset: false,
        });
        // Spotting must not lower the decaying global.
        for _ in 0..5 {
            script.push(Frame {
                dt: 0.1,
                spots: &[0.5],
                global: false,
                reset: false,
            });
        }
        // Full bleed-out over many frames clears the latch at zero.
        for _ in 0..200 {
            script.push(Frame {
                dt: 0.1,
                spots: &[],
                global: false,
                reset: false,
            });
        }
        // Round start resets to calm.
        script.push(Frame {
            dt,
            spots: &[],
            global: false,
            reset: true,
        });
        // And escalates again afterward.
        for _ in 0..5 {
            script.push(Frame {
                dt,
                spots: &[0.5],
                global: false,
                reset: false,
            });
        }

        let mut rust = Alarm::default();
        for (i, frame) in script.iter().enumerate() {
            step_rust(&mut rust, frame);
            step_ink(&mut app, flow, frame);
            let (level, global) = read_alarm_state(app.world(), flow);

            assert_eq!(
                rust.tier(),
                (level.floor() as u8),
                "tier mismatch at frame {i}: rust={} ink_level={level}",
                rust.tier()
            );
            assert_eq!(
                rust.global, global,
                "global-latch mismatch at frame {i}: rust={} ink={global}",
                rust.global
            );
            assert!(
                (rust.level - level).abs() <= 1e-4,
                "level mismatch at frame {i}: rust={} ink={level}",
                rust.level
            );
        }
    }
}
