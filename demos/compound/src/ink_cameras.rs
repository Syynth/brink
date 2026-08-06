//! Phase 1c: security cameras — the pure sweep-and-detect loop, driven by
//! **ink** instead of Rust (`docs/drive-app-plan.md` §9's third entity
//! migration, README "Suggested Phase-1 migration order" #3). `src/cameras.rs`
//! stays put as the Rust baseline (only its `CAMERA_RANGE`/`CAMERA_HALF_ANGLE`
//! constants get a `pub(crate)` bump so this module can reuse them instead of
//! forking copies); this module is the side-by-side ink port selected at
//! launch with `--cameras-impl ink`, same shape as `--alarm-impl`/
//! `--doors-impl`.
//!
//! ## Shape: many instances, one program, per-flow state
//!
//! Unlike the alarm (Phase 1a, one shared flow) and doors (Phase 1b, no ink
//! state at all), a round's cameras are all `spawn_cameras_from_layout`
//! entities under one shared program. [`attach_ink_camera_flows`] attaches a
//! [`BrinkFlowRequest<CamerasStory>`] straight onto each `SecurityCamera`
//! entity — the flow entity IS the camera entity, exactly the doors'-port
//! pattern. But every flow spawned under one marker `M` advances against the
//! *same* [`bevy_brink::BrinkGlobals<M>`] `World` resource — so a plain ink
//! `VAR` would be one sweep phase shared by every camera in the compound.
//! `assets/cameras.ink` uses `#@local` for its `phase`/`facing` cells
//! precisely to avoid that (flow-private storage,
//! `docs/directive-annotations-spec.md` §3) — the reason this port needed a
//! storage-class annotation neither of the first two ports did.
//!
//! **This also means `advance_batch` is the wrong driver here.** BH-3's
//! guard (`crate::batch::homes_any_local`, #925) SKIPS any flow whose program
//! compiles `#@local` defaults, with a `warn!`, rather than silently
//! double-counting shared state — so a story built on per-flow `#@local`
//! state must stay on the serial API. This port drives every camera with
//! [`call_ink_function`] once per frame (`ink_camera_system`), the same
//! exclusive-driver shape Phase 1a's alarm uses, not Phase 1b's
//! `advance_batch`.
//!
//! ## The seams
//!
//! * **Write seam — one function call per camera per frame.**
//!   [`ink_camera_system`] calls `sweep_and_detect(dt, center_angle, range)`:
//!   `center_angle` and `range` (loadout/stealth-adjusted, computed exactly
//!   like `camera_ai_system`) are passed fresh every call rather than stored
//!   in ink — "no per-entity memory to marshal" (README #3). The function
//!   advances the camera's own `#@local` phase/facing and returns whether it
//!   currently sees the player.
//! * **Detection — a world-access binding, not ink vector math.** `sees_player`
//!   (`bind_brink_query`) does the actual cone-and-raycast test
//!   (`world::point_in_cone` + `world::raycast_clear`) against the live
//!   `Transform`s/`Collider`s, reading the calling flow entity's own
//!   `Transform` as the cone apex — ink-side vector math is icebox #827, and
//!   this is exactly the doors' `is_switch_on` shape (a query binding reading
//!   state off the calling flow entity) applied to geometry instead of a
//!   component flag.
//! * **Read seam — the alarm write happens in Rust, not ink.** The plan
//!   (`docs/drive-app-plan.md` §3) sketched cameras raising the alarm via a
//!   `#[derive(BrinkCommand)]` "the alarm was raised" command. That command
//!   shape turned out to be unreachable from this port's drive mechanism: see
//!   [`sweep_camera`]'s doc comment and `MIGRATION.md` for the discovered gap.
//!   Instead, `ink_camera_system` reads `sweep_and_detect`'s boolean return
//!   and writes [`SpottedEvent`] itself — the same seam `ink_alarm_system`
//!   already uses to read `SpottedEvent`, just one step earlier in the chain.
//! * **Disable stays Rust-only, in both modes.** `camera_interact_system` (E
//!   key) is untouched and still flips `SecurityCamera::disabled` +
//!   `Round::cameras_disabled`/`carried` for both writers; `ink_camera_system`
//!   simply skips calling ink for a disabled camera (matching
//!   `camera_ai_system`'s own `if cam.disabled { continue }`), so disabling
//!   never needs to cross the ink boundary at all.

use std::time::Instant;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy_brink::{
    BrinkFlow, BrinkFlowRequest, BrinkQueryInput, BrinkStoryAsset, Value, call_ink_function,
};

use crate::alarm::SpottedEvent;
use crate::cameras::{CAMERA_HALF_ANGLE, CAMERA_RANGE, SecurityCamera};
use crate::stats::{Loadout, PlayerStats};
use crate::timing::BehaviorTimings;
use crate::world::{Collider, Player, Wall, point_in_cone, raycast_clear};

/// Story marker for the cameras ink instance. Every camera in a round is its
/// own flow under this one marker (see the module docs for why that's safe —
/// no shared per-camera state rides `BrinkGlobals<CamerasStory>`;
/// `cameras.ink`'s sweep state is entirely `#@local`).
pub struct CamerasStory;

/// Which implementation drives cameras this run, chosen at launch.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CamerasImpl {
    /// The Rust baseline (`src/cameras.rs`, `camera_ai_system`).
    #[default]
    Rust,
    /// The ink port (`assets/cameras.ink`, this module).
    Ink,
}

impl CamerasImpl {
    /// Parse `--cameras-impl rust|ink` from the process args (default: Rust).
    #[must_use]
    pub fn from_args() -> Self {
        let mut args = std::env::args();
        while let Some(arg) = args.next() {
            let value = if arg == "--cameras-impl" {
                args.next()
            } else {
                arg.strip_prefix("--cameras-impl=").map(str::to_owned)
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

/// Run condition: the Rust camera writer (`camera_ai_system`) is active.
pub fn cameras_is_rust(mode: Res<CamerasImpl>) -> bool {
    *mode == CamerasImpl::Rust
}

/// Run condition: the ink camera writer ([`ink_camera_system`]) is active.
pub fn cameras_is_ink(mode: Res<CamerasImpl>) -> bool {
    *mode == CamerasImpl::Ink
}

/// Ink mode only (`--cameras-impl ink`): attach a fresh flow to every
/// freshly spawned camera entity. `cameras::spawn_cameras_from_layout` is
/// unmodified and still runs for BOTH implementations; this system only
/// decorates the `SecurityCamera` entities it produces.
///
/// Runs every `Update` frame (a fresh layout spawns fresh cameras on every
/// `StartRound`, not just the game's first round), matching
/// `attach_ink_door_flows`'s shape.
pub fn attach_ink_camera_flows(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    cameras: Query<Entity, Added<SecurityCamera>>,
) {
    for entity in &cameras {
        let story: Handle<BrinkStoryAsset> = asset_server.load("cameras.ink");
        commands.entity(entity).insert(
            BrinkFlowRequest::<CamerasStory>::builder()
                .story(story)
                .build(),
        );
    }
}

/// World-access binding (`bind_brink_query`): does the camera facing
/// `facing` radians see the player within `range`, respecting walls? Reads
/// the calling flow entity's own `Transform` as the cone apex — the flow
/// entity IS the camera entity (see [`attach_ink_camera_flows`]) — so no
/// camera id needs to cross the ink/engine boundary. The actual geometry is
/// `world::point_in_cone` + `world::raycast_clear`, unchanged from
/// `camera_ai_system` — ink never does vector math (icebox #827).
pub fn sees_player(
    In((flow, args)): In<BrinkQueryInput>,
    cameras: Query<&Transform>,
    player: Query<&Transform, With<Player>>,
    walls: Query<(&Transform, &Collider), With<Wall>>,
) -> Value {
    let facing = args.first().and_then(Value::as_float).unwrap_or(0.0);
    let range = args.get(1).and_then(Value::as_float).unwrap_or(0.0);

    let Ok(apex_tf) = cameras.get(flow) else {
        return Value::Bool(false);
    };
    let Ok(player_tf) = player.single() else {
        return Value::Bool(false);
    };
    let apex = apex_tf.translation.truncate();
    let player_pos = player_tf.translation.truncate();
    let blockers: Vec<(Vec2, Vec2)> = walls
        .iter()
        .map(|(t, c)| (t.translation.truncate(), c.half_extents))
        .collect();

    let sees = point_in_cone(apex, facing, CAMERA_HALF_ANGLE, range, player_pos)
        && raycast_clear(apex, player_pos, &blockers);
    Value::Bool(sees)
}

/// Reader bundle for the exclusive driver: `Time`, `Loadout`, and the
/// player's stats (for the same `effective_range` formula
/// `camera_ai_system` uses).
type CameraReaders<'w, 's> = SystemState<(
    Res<'w, Time>,
    Res<'w, Loadout>,
    Query<'w, 's, &'static PlayerStats, With<Player>>,
)>;

/// The ink counterpart of `cameras::camera_ai_system`. Exclusive because
/// [`call_ink_function`] re-enters the VM and needs `&mut World`; one call
/// (well, two — see [`sweep_camera`]) per LIVE camera per frame, since there
/// is no batch engine→ink call surface yet (#1058) — the same friction
/// Phase 1a hit, now multiplied by the camera count instead of a fixed
/// handful of alarm calls.
///
/// Records the *ink* cameras' per-frame cost into [`BehaviorTimings::cameras`]
/// — the same field `camera_ai_system` fills — so the HUD's cameras µs line
/// reports whichever writer is live.
pub fn ink_camera_system(
    world: &mut World,
    mut readers: Local<Option<CameraReaders<'static, 'static>>>,
) {
    let readers = readers.get_or_insert_with(|| SystemState::new(world));
    let (dt, effective_range) = {
        let Ok((time, loadout, player)) = readers.get_mut(world) else {
            return;
        };
        let dt = time.delta_secs();
        let stealth_radius = player.single().map_or(0.0, |s| s.stealth_radius);
        let effective_range =
            (CAMERA_RANGE * loadout.enemy_vision_scale - stealth_radius).max(24.0);
        (dt, effective_range)
    };

    let start = Instant::now();

    // Snapshot the fields we need before mutating (avoid holding a live
    // query across `call_ink_function`'s exclusive `&mut World` access).
    let cams: Vec<(Entity, f32, bool)> = world
        .query_filtered::<(Entity, &SecurityCamera), With<BrinkFlow<CamerasStory>>>()
        .iter(world)
        .map(|(e, cam)| (e, cam.center_angle, cam.disabled))
        .collect();

    for (entity, center_angle, disabled) in cams {
        if disabled {
            if let Some(mut sprite) = world.get_mut::<Sprite>(entity) {
                sprite.color = Color::srgb(0.3, 0.3, 0.3);
            }
            continue;
        }

        let (sees, facing) = sweep_camera(world, entity, dt, center_angle, effective_range);

        if sees {
            world
                .resource_mut::<Messages<SpottedEvent>>()
                .write(SpottedEvent { intensity: dt });
        }
        if let Some(mut cam) = world.get_mut::<SecurityCamera>(entity) {
            cam.facing = facing;
        }
        if let Some(mut sprite) = world.get_mut::<Sprite>(entity) {
            sprite.color = if sees {
                Color::srgb(1.0, 0.3, 0.3)
            } else {
                Color::srgb(0.8, 0.7, 0.2)
            };
        }
    }

    world.resource_mut::<BehaviorTimings>().cameras = start.elapsed();
}

/// Drive one camera's per-frame sweep: `sweep_and_detect` (advance + detect,
/// returns whether it sees the player) then `camera_facing` (the read seam
/// for the debug gizmo / parity test).
///
/// **Why two calls instead of one `#[derive(BrinkCommand)]`-fired alarm**:
/// the plan (`docs/drive-app-plan.md` §3) sketched cameras raising the alarm
/// via an ink→engine command. At the time this port was written,
/// `call_ink_function`'s evaluation handler only resolved `bind_brink_fn`
/// and `bind_brink_query` bindings inline — a `bind_brink_command`-bound
/// `EXTERNAL` reached this way would silently fall back instead of firing
/// the event, so this port reads the boolean return instead and writes
/// `SpottedEvent` itself — filed as a new drive-it issue (`MIGRATION.md`'s
/// Phase 1c entry). **#1096 has since closed that gap**: a command binding
/// reached via `call_ink_function` now buffers and fires correctly, mirroring
/// the serial `step_one`/`advance_flow` path. This port is left on the
/// boolean-return shape rather than switched to the originally-planned
/// command — that would be an architecture change to this demo, not a bug
/// fix, and belongs in its own pass.
///
/// Logs (never silently swallows) a call failure and falls back to "doesn't
/// see the player" / "facing unchanged" so a broken binding surfaces instead
/// of silently freezing a camera.
fn sweep_camera(
    world: &mut World,
    entity: Entity,
    dt: f32,
    center_angle: f32,
    range: f32,
) -> (bool, f32) {
    let sees = call_camera_fn(
        world,
        entity,
        "sweep_and_detect",
        &[
            Value::Float(dt),
            Value::Float(center_angle),
            Value::Float(range),
        ],
    )
    .and_then(|v| v.as_bool())
    .unwrap_or(false);

    let facing = call_camera_fn(world, entity, "camera_facing", &[])
        .and_then(|v| v.as_float())
        .unwrap_or(center_angle);

    (sees, facing)
}

/// Call one cameras ink function, logging (not swallowing) any error.
fn call_camera_fn(world: &mut World, entity: Entity, name: &str, args: &[Value]) -> Option<Value> {
    match call_ink_function::<CamerasStory>(world, entity, name, args) {
        Ok(value) => Some(value),
        Err(err) => {
            warn!("[cameras] ink call {name} failed: {err}");
            None
        }
    }
}

/// Frame-by-frame semantics parity: the ink cameras and a from-scratch Rust
/// reference computation (the same formulas `camera_ai_system` uses) must
/// agree on sweep facing and detection outcome over a scripted player path.
/// This is the port's correctness bar (drive-app-plan Phase 1c).
#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::AssetPlugin;
    use bevy_brink::{BrinkBindingsAppExt, BrinkPlugin};

    /// Compile `assets/cameras.ink` inline (deterministic, no async
    /// `AssetServer`) via [`bevy_brink::compile_story_inline`] (#1060) —
    /// matching `ink_alarm`/`ink_doors`'s post-#1060 shape.
    fn build_cameras_story(app: &mut App) -> Handle<BrinkStoryAsset> {
        let src = include_str!("../assets/cameras.ink");
        bevy_brink::compile_story_inline(app, "cameras.ink", src).expect("cameras.ink compiles")
    }

    fn build_app() -> App {
        let mut app = App::new();
        // `MinimalPlugins` supplies `Time` (`ink_camera_system`'s
        // `CameraReaders` reads `Res<Time>` — without it, `SystemState::get_mut`
        // fails validation and the system silently no-ops every frame, the
        // same shape as a missing resource anywhere else in bevy 0.19's
        // fallible-param world).
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            BrinkPlugin::<CamerasStory>::default(),
        ));
        app.bind_brink_fn::<CamerasStory, _, _>("sin", |args| {
            args.first().and_then(Value::as_float).unwrap_or(0.0).sin()
        });
        app.bind_brink_query::<CamerasStory, _, _>("sees_player", sees_player);
        app
    }

    /// Spawn one camera flow instance: a `Transform` (cone apex) + the flow
    /// request. `SecurityCamera`/`Sprite` aren't needed by the driver
    /// functions this test calls directly (it drives `call_ink_function`
    /// itself, not `ink_camera_system`), only a `Transform` for the
    /// `sees_player` binding to read.
    fn spawn_camera(app: &mut App, story: &Handle<BrinkStoryAsset>, pos: Vec2) -> Entity {
        app.world_mut()
            .spawn((
                Transform::from_translation(pos.extend(1.0)),
                BrinkFlowRequest::<CamerasStory>::builder()
                    .story(story.clone())
                    .build(),
            ))
            .id()
    }

    fn spawn_player(app: &mut App, pos: Vec2) -> Entity {
        app.world_mut()
            .spawn((Transform::from_translation(pos.extend(0.0)), Player))
            .id()
    }

    fn spawn_wall(app: &mut App, pos: Vec2, half: Vec2) {
        app.world_mut().spawn((
            Transform::from_translation(pos.extend(0.0)),
            Collider { half_extents: half },
            Wall,
        ));
    }

    /// One frame of the from-scratch Rust reference: the exact formulas
    /// `camera_ai_system` uses, kept independent of `ink_camera_system` so
    /// the test doesn't just check ink against itself.
    struct RustCamera {
        center_angle: f32,
        phase: f32,
    }

    impl RustCamera {
        fn step(&mut self, dt: f32) -> f32 {
            self.phase += dt * 1.1;
            self.center_angle + 0.7 * self.phase.sin()
        }
    }

    #[test]
    fn ink_camera_matches_rust_sweep_and_detection_over_a_scripted_path() {
        let mut app = build_app();
        let story = build_cameras_story(&mut app);

        let apex = Vec2::new(0.0, 0.0);
        let camera = spawn_camera(&mut app, &story, apex);
        let player = spawn_player(&mut app, Vec2::new(500.0, 500.0));
        // A wall that blocks line of sight when the player stands directly
        // behind it from the camera's position, exercising the raycast
        // branch (not just the cone-angle branch).
        spawn_wall(&mut app, Vec2::new(60.0, 0.0), Vec2::new(10.0, 40.0));

        // One tick fulfills the flow request.
        app.update();
        let range = 210.0;

        let mut rust = RustCamera {
            center_angle: 0.0,
            phase: 0.0,
        };

        // A scripted player path: approach through the cone (mostly
        // wall-blocked — the wall at x∈[50,70] sits on the y=0 line between
        // the apex and every target with x > 50), then hold at a close,
        // unblocked spot (x=45 < 50, clear of the wall) for long enough to
        // span a full sweep period (2π/1.1 ≈ 343 frames at speed 1.1
        // rad/s) — guaranteeing the cone crosses the target at least once
        // regardless of the phase the approach leg left it at, since
        // `facing` oscillates through the whole ±0.7 rad band every period
        // and only needs to land within ±`CAMERA_HALF_ANGLE` (0.45 rad) of
        // the target's angle to count as seen.
        let path: Vec<Vec2> = {
            let mut p = Vec::new();
            for i in 0..30 {
                let t = i as f32 / 29.0;
                p.push(Vec2::new(500.0, 500.0).lerp(Vec2::new(180.0, 0.0), t));
            }
            for i in 0..20 {
                let t = i as f32 / 19.0;
                p.push(Vec2::new(180.0, 0.0).lerp(Vec2::new(40.0, 0.0), t));
            }
            for _ in 0..360 {
                p.push(Vec2::new(45.0, 0.0));
            }
            p
        };

        let dt = 1.0 / 60.0;
        let mut any_saw = false;
        let mut any_missed = false;
        for (i, &pos) in path.iter().enumerate() {
            app.world_mut()
                .get_mut::<Transform>(player)
                .unwrap()
                .translation = pos.extend(0.0);

            let rust_facing = rust.step(dt);
            let rust_sees = point_in_cone(apex, rust_facing, CAMERA_HALF_ANGLE, range, pos)
                && raycast_clear(apex, pos, &[(Vec2::new(60.0, 0.0), Vec2::new(10.0, 40.0))]);

            let (ink_sees, ink_facing) = sweep_camera(app.world_mut(), camera, dt, 0.0, range);

            assert!(
                (ink_facing - rust_facing).abs() <= 1e-4,
                "facing mismatch at frame {i}: rust={rust_facing} ink={ink_facing}"
            );
            assert_eq!(
                ink_sees, rust_sees,
                "detection mismatch at frame {i}: rust={rust_sees} ink={ink_sees} pos={pos:?}"
            );

            any_saw |= ink_sees;
            any_missed |= !ink_sees;
        }

        assert!(
            any_saw,
            "fixture sanity: the scripted path must be seen at least once"
        );
        assert!(
            any_missed,
            "fixture sanity: the scripted path must also be missed at least once (behind the wall)"
        );
    }

    /// Reachability: `ink_camera_system` itself (not just the test-only
    /// `sweep_camera` helper) writes `SpottedEvent` when a live, non-disabled
    /// camera sees the player, and skips a disabled one entirely.
    #[test]
    fn ink_camera_system_writes_spotted_event_and_skips_disabled() {
        let mut app = build_app();
        app.init_resource::<BehaviorTimings>();
        app.init_resource::<Loadout>();
        app.add_message::<SpottedEvent>();
        app.add_systems(Update, ink_camera_system);
        let story = build_cameras_story(&mut app);

        // A camera facing +X, close enough that the player standing right
        // in front of it is always seen at phase 0 regardless of sweep.
        let camera = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO),
                Sprite::from_color(Color::srgb(0.8, 0.7, 0.2), Vec2::splat(20.0)),
                SecurityCamera {
                    center_angle: 0.0,
                    sweep_half: 0.7,
                    speed: 1.1,
                    phase: 0.0,
                    facing: 0.0,
                    disabled: false,
                },
                BrinkFlowRequest::<CamerasStory>::builder()
                    .story(story.clone())
                    .build(),
            ))
            .id();
        let _player = spawn_player(&mut app, Vec2::new(20.0, 0.0));

        // Frame 1 fulfills the flow request (`ink_camera_system` no-ops —
        // no `BrinkFlow` yet this frame); frame 2 actually sweeps.
        app.update();
        app.update();

        {
            let mut spotted = app.world_mut().resource_mut::<Messages<SpottedEvent>>();
            assert!(
                spotted.drain().next().is_some(),
                "a live camera facing the player must write SpottedEvent"
            );
        }

        // Disable the camera; it must stop being stepped (and stop spotting).
        app.world_mut()
            .get_mut::<SecurityCamera>(camera)
            .unwrap()
            .disabled = true;
        for _ in 0..5 {
            app.update();
        }
        let mut spotted = app.world_mut().resource_mut::<Messages<SpottedEvent>>();
        assert!(
            spotted.drain().next().is_none(),
            "a disabled camera must not spot the player"
        );
    }
}
