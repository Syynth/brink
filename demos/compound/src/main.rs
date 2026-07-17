//! The Compound — Phase 0.
//!
//! A small, complete, playable top-down stealth game built in pure Bevy. It is
//! the *control group* for the drive-app plan (`docs/drive-app-plan.md`): every
//! entity's behavior is written in plain, legible Rust so that Phase 1 can port
//! each archetype to ink one module at a time and diff the result against this
//! baseline — including the per-frame behavior-system timing readout in the HUD.
//!
//! Module map (one entity archetype per file, migration order-of-battle):
//!   guards · cameras · doors · alarm · rats · rounds · shop · stats
//! plus infrastructure: world (arena + geometry), hud, timing.

mod alarm;
mod cameras;
mod doors;
mod guards;
mod hud;
mod rats;
mod rounds;
mod shop;
mod stats;
mod timing;
mod world;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

use crate::alarm::{Alarm, SpottedEvent, alarm_system};
use crate::cameras::{camera_ai_system, camera_interact_system, draw_camera_cones};
use crate::doors::{Door, door_sync_system, switch_interact_system, switch_visual_system};
use crate::guards::{
    ReinforcementSpawner, draw_guard_cones, guard_ai_system, reinforcement_system,
};
use crate::hud::{setup_hud, update_hud};
use crate::rats::{RATS_PER_BATCH, Rat, RatRng, rat_system, spawn_rats};
use crate::rounds::{
    LastOutcome, Phase, PlayerCaught, Round, StartRound, round_outcome_system, start_round_system,
};
use crate::shop::{setup_shop, shop_button_system, shop_refresh_system, teardown_shop};
use crate::stats::{Loadout, PlayerStats};
use crate::timing::BehaviorTimings;
use crate::world::{
    Blocker, Collider, PLAYER_HALF, Player, Wall, resolve_collision, setup_static_world,
};

/// Whether vision-cone debug gizmos are drawn (toggled with F1).
#[derive(Resource, Debug)]
pub struct ShowCones(pub bool);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "The Compound — Phase 0".into(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .init_state::<Phase>()
        .insert_resource(ClearColor(Color::srgb(0.05, 0.05, 0.06)))
        .init_resource::<Alarm>()
        .init_resource::<Round>()
        .init_resource::<LastOutcome>()
        .init_resource::<Loadout>()
        .init_resource::<BehaviorTimings>()
        .init_resource::<ReinforcementSpawner>()
        .init_resource::<RatRng>()
        .insert_resource(ShowCones(true))
        .add_message::<SpottedEvent>()
        .add_message::<PlayerCaught>()
        .add_message::<StartRound>()
        .add_systems(
            Startup,
            (setup_static_world, setup_hud, kick_off_first_round),
        )
        // Always-on systems.
        .add_systems(
            Update,
            (start_round_system, update_hud, toggle_cones, spawn_rats_key),
        )
        // Debug cone gizmos, only when enabled.
        .add_systems(
            Update,
            (draw_guard_cones, draw_camera_cones).run_if(cones_enabled),
        )
        // Core gameplay, only while playing.
        .add_systems(
            Update,
            (
                // The five timed behavior systems, chained so their order (and
                // thus the alarm's same-frame consistency) is deterministic.
                (
                    guard_ai_system,
                    camera_ai_system,
                    door_sync_system,
                    alarm_system,
                    rat_system,
                )
                    .chain(),
                reinforcement_system,
                switch_visual_system,
                player_movement,
                camera_interact_system,
                switch_interact_system,
                round_outcome_system,
                reset_round_key,
            )
                .run_if(in_state(Phase::Playing)),
        )
        // Shop intermission.
        .add_systems(OnEnter(Phase::Shop), setup_shop)
        .add_systems(
            Update,
            (shop_button_system, shop_refresh_system).run_if(in_state(Phase::Shop)),
        )
        .add_systems(OnExit(Phase::Shop), teardown_shop)
        .run();
}

/// Kick off round 1 once the static world (and the player) exist.
fn kick_off_first_round(mut start: MessageWriter<StartRound>) {
    start.write(StartRound);
}

/// WASD movement with axis-separated wall/door collision.
fn player_movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player: Query<(&mut Transform, &PlayerStats), With<Player>>,
    walls: Query<(&Transform, &Collider), (With<Wall>, Without<Player>)>,
    doors: Query<(&Transform, &Collider, &Door), Without<Player>>,
) {
    let Ok((mut tf, stats)) = player.single_mut() else {
        return;
    };

    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    if dir == Vec2::ZERO {
        return;
    }

    let from = tf.translation.truncate();
    let desired = from + dir.normalize() * stats.move_speed * time.delta_secs();

    let mut blockers: Vec<Blocker> = walls
        .iter()
        .map(|(t, c)| (t.translation.truncate(), c.half_extents))
        .collect();
    blockers.extend(
        doors
            .iter()
            .filter(|(_, _, d)| !d.open)
            .map(|(t, c, _)| (t.translation.truncate(), c.half_extents)),
    );

    let out = resolve_collision(from, desired, PLAYER_HALF, &blockers);
    tf.translation.x = out.x;
    tf.translation.y = out.y;
}

/// F1 toggles the vision-cone overlay.
fn toggle_cones(keys: Res<ButtonInput<KeyCode>>, mut show: ResMut<ShowCones>) {
    if keys.just_pressed(KeyCode::F1) {
        show.0 = !show.0;
    }
}

/// Run condition: cones are enabled.
fn cones_enabled(show: Res<ShowCones>) -> bool {
    show.0
}

/// `+` spawns a batch of rats.
fn spawn_rats_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut rng: ResMut<RatRng>,
    rats: Query<(), With<Rat>>,
) {
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        let current = rats.iter().count();
        spawn_rats(&mut commands, &mut rng, current, RATS_PER_BATCH);
    }
}

/// `R` restarts the current round.
fn reset_round_key(keys: Res<ButtonInput<KeyCode>>, mut start: MessageWriter<StartRound>) {
    if keys.just_pressed(KeyCode::KeyR) {
        start.write(StartRound);
    }
}

/// Headless cost harness: runs the full behavior stack (no renderer, no window)
/// so the per-system timing can be measured in CI and reported. This is the
/// same `BehaviorTimings` the HUD shows; keeping it exercised without a display
/// means the Phase 1 ink port has a reproducible Rust baseline to diff against.
#[cfg(test)]
mod behavior_cost {
    use super::*;
    use crate::cameras::{camera_ai_system, spawn_cameras};
    use crate::doors::door_sync_system;
    use crate::guards::spawn_round_guards;

    fn seed(mut commands: Commands, mut rng: ResMut<RatRng>) {
        commands.spawn((Transform::default(), PlayerStats::default(), Player));
        spawn_round_guards(&mut commands, 12);
        spawn_cameras(&mut commands);
        crate::doors::spawn_doors(&mut commands);
        spawn_rats(&mut commands, &mut rng, 0, 1000);
    }

    #[test]
    fn behavior_stack_runs_headless_and_is_measured() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Alarm>()
            .init_resource::<Loadout>()
            .init_resource::<BehaviorTimings>()
            .init_resource::<RatRng>()
            .init_resource::<crate::guards::ReinforcementSpawner>()
            .init_resource::<Round>()
            .add_message::<SpottedEvent>()
            .add_message::<PlayerCaught>()
            .add_systems(Startup, seed)
            .add_systems(
                Update,
                (
                    guard_ai_system,
                    camera_ai_system,
                    door_sync_system,
                    alarm_system,
                    rat_system,
                )
                    .chain(),
            );

        // Warm up, then sample.
        for _ in 0..200 {
            app.update();
        }
        let t = *app.world().resource::<BehaviorTimings>();

        // Report the numbers (visible with `cargo test -- --nocapture`).
        eprintln!(
            "behavior cost (12 guards, 4 cameras, 2 doors, 1000 rats):\n  \
             guards {:?}  cameras {:?}  doors {:?}  alarm {:?}  rats {:?}  TOTAL {:?}",
            t.guards,
            t.cameras,
            t.doors,
            t.alarm,
            t.rats,
            t.total()
        );

        // Sanity: the whole stack stays well under a 60 FPS frame budget.
        assert!(
            t.total() < core::time::Duration::from_millis(16),
            "behavior stack should fit a frame: {:?}",
            t.total()
        );
    }
}
