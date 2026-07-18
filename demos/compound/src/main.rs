//! The Compound — a small, complete, playable top-down stealth game built in
//! pure Bevy. It is the *control group* for the drive-app plan
//! (`docs/drive-app-plan.md`): every entity's behavior is written in plain,
//! legible Rust so that Phase 1 can port each archetype to ink one module at a
//! time and diff the result against this baseline — including the per-frame
//! behavior-system timing readout in the HUD.
//!
//! Gameplay v2 (plan §10): each round draws a fresh seeded **BSP layout**
//! (`layout_gen`) with room recipes; guards climb an **MGS-lenient suspicion
//! ladder** where breaking line of sight is the only escape (`guards`); and the
//! round is a push-your-luck of **greed vs safety** (gold banked only on exit),
//! **speed vs noise** (running is fast but loud), and consumable **coins/smoke**
//! (`loot`, `noise`).
//!
//! Module map (one entity archetype per file):
//!   `layout_gen` · `guards` · `cameras` · `doors` · `alarm` · `loot` ·
//!   `noise` · `rats` · `rounds` · `shop` · `stats` · plus infrastructure:
//!   `world` · `hud` · `timing`.

mod alarm;
mod cameras;
mod doors;
mod guards;
mod hud;
mod layout_gen;
mod loot;
mod nav;
mod noise;
mod rats;
mod rounds;
mod shop;
mod stats;
mod timing;
mod world;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

use crate::alarm::{Alarm, GlobalAlarm, SpottedEvent, alarm_system};
use crate::cameras::{camera_ai_system, camera_interact_system, draw_camera_cones};
use crate::doors::{
    Door, door_sync_system, draw_door_switch_glyphs, switch_interact_system, switch_prompt_system,
    switch_visual_system,
};
use crate::guards::{
    ReinforcementSpawner, draw_guard_cones, draw_guard_tells, guard_ai_system,
    reinforcement_system, spawn_telegraph_system,
};
use crate::hud::{setup_hud, update_hud};
use crate::loot::gold_pickup_system;
use crate::nav::NavGraph;
use crate::noise::{
    NoiseEvent, RunNoiseClock, coin_system, is_running, run_noise_system, smoke_system, spawn_coin,
    spawn_smoke,
};
use crate::rats::{RATS_PER_BATCH, Rat, RatRng, rat_system, spawn_rats};
use crate::rounds::{
    CurrentLayout, LastOutcome, Phase, PlayerCaught, Round, StartRound, round_outcome_system,
    start_round_system,
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
                title: "The Compound".into(),
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
        .init_resource::<CurrentLayout>()
        .init_resource::<LastOutcome>()
        .init_resource::<Loadout>()
        .init_resource::<BehaviorTimings>()
        .init_resource::<ReinforcementSpawner>()
        .init_resource::<NavGraph>()
        .init_resource::<RatRng>()
        .init_resource::<RunNoiseClock>()
        .insert_resource(ShowCones(true))
        .add_message::<SpottedEvent>()
        .add_message::<GlobalAlarm>()
        .add_message::<NoiseEvent>()
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
                spawn_telegraph_system,
                switch_visual_system,
                switch_prompt_system,
                draw_door_switch_glyphs,
                draw_guard_tells,
                player_movement,
                camera_interact_system,
                switch_interact_system,
                gold_pickup_system,
                run_noise_system,
                coin_system,
                smoke_system,
                throw_coin_system,
                use_smoke_system,
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

/// WASD movement with axis-separated wall/door collision. Holding Shift runs:
/// faster, but the noise it makes (see [`run_noise_system`]) draws guards.
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

    let speed = if is_running(&keys) {
        stats.run_speed()
    } else {
        stats.move_speed
    };

    let from = tf.translation.truncate();
    let desired = from + dir.normalize() * speed * time.delta_secs();

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

/// Throw a coin toward the mouse cursor (a lure): spends one coin and spawns a
/// projectile that emits noise where it lands.
fn throw_coin_system(
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut loadout: ResMut<Loadout>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    player: Query<&Transform, With<Player>>,
) {
    if !mouse.just_pressed(MouseButton::Left) || loadout.coins == 0 {
        return;
    }
    let Ok(ptf) = player.single() else {
        return;
    };
    let ppos = ptf.translation.truncate();
    let aim = cursor_world(&windows, &cameras).unwrap_or(ppos + Vec2::X * 100.0);
    let dir = aim - ppos;
    if dir.length() < 1.0 {
        return;
    }
    spawn_coin(&mut commands, ppos, dir);
    loadout.coins -= 1;
}

/// Drop a smoke bomb on the player (breaks a chase): spends one smoke charge.
fn use_smoke_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut loadout: ResMut<Loadout>,
    player: Query<&Transform, With<Player>>,
) {
    if !keys.just_pressed(KeyCode::KeyQ) || loadout.smokes == 0 {
        return;
    }
    if let Ok(ptf) = player.single() {
        spawn_smoke(&mut commands, ptf.translation.truncate());
        loadout.smokes -= 1;
    }
}

/// Convert the cursor's screen position to a world position via the 2D camera.
fn cursor_world(
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform)>,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let cursor = window.cursor_position()?;
    let (camera, cam_tf) = cameras.iter().next()?;
    camera.viewport_to_world_2d(cam_tf, cursor).ok()
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
    use crate::alarm::spawn_alarm_panels;
    use crate::cameras::spawn_cameras_from_layout;
    use crate::doors::spawn_doors_from_layout;
    use crate::guards::spawn_round_guards_from_layout;
    use crate::layout_gen::generate;
    use crate::nav::RoomGraph;
    use crate::world::spawn_layout_walls;

    fn seed(mut commands: Commands, mut rng: ResMut<RatRng>, mut nav: ResMut<NavGraph>) {
        commands.spawn((Transform::default(), PlayerStats::default(), Player));
        let layout = generate(42);
        spawn_layout_walls(&mut commands, &layout);
        spawn_doors_from_layout(&mut commands, &layout);
        spawn_cameras_from_layout(&mut commands, &layout);
        spawn_alarm_panels(&mut commands, &layout);
        spawn_round_guards_from_layout(&mut commands, &layout, 12);
        spawn_rats(&mut commands, &mut rng, 0, 1000);
        nav.0 = Some(RoomGraph::from_layout(&layout));
    }

    #[test]
    fn behavior_stack_runs_headless_and_is_measured() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Alarm>()
            .init_resource::<Loadout>()
            .init_resource::<BehaviorTimings>()
            .init_resource::<RatRng>()
            .init_resource::<ReinforcementSpawner>()
            .init_resource::<NavGraph>()
            .init_resource::<Round>()
            .add_message::<SpottedEvent>()
            .add_message::<GlobalAlarm>()
            .add_message::<NoiseEvent>()
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

        eprintln!(
            "behavior cost (12 guards, seeded layout, 1000 rats):\n  \
             guards {:?}  cameras {:?}  doors {:?}  alarm {:?}  rats {:?}  TOTAL {:?}",
            t.guards,
            t.cameras,
            t.doors,
            t.alarm,
            t.rats,
            t.total()
        );

        assert!(
            t.total() < core::time::Duration::from_millis(16),
            "behavior stack should fit a frame: {:?}",
            t.total()
        );
    }
}

/// The **never-inside-a-wall invariant** (#1044): whatever a guard is doing —
/// patrolling across the compound, converging on a last-known position, walking
/// to an alarm panel, peeking a search point, or chasing a moving player — its
/// collision box must never overlap a wall. This drives the real `guard_ai_system`
/// headless (the same `MinimalPlugins` harness the timing test uses), cycles guards
/// through every FSM state with cross-map targets, roams the player to provoke
/// curious/chase transitions, and asserts the invariant every frame across
/// several seeds. It is the defense-in-depth backstop for the room-graph pathing:
/// even if a path were wrong, `resolve_collision` keeps guards out of walls.
#[cfg(test)]
mod guard_wall_invariant {
    use super::*;
    use crate::alarm::spawn_alarm_panels;
    use crate::guards::{GUARD_HALF, Guard, GuardState, spawn_round_guards_from_layout};
    use crate::layout_gen::{Recipe, Room, generate};
    use crate::nav::RoomGraph;
    use crate::world::spawn_layout_walls;

    /// Signed overlap of two AABBs on the worst axis: >0 means interpenetration.
    fn penetration(a: Vec2, ahalf: Vec2, b: Vec2, bhalf: Vec2) -> f32 {
        let px = (ahalf.x + bhalf.x) - (a.x - b.x).abs();
        let py = (ahalf.y + bhalf.y) - (a.y - b.y).abs();
        px.min(py)
    }

    fn all_states() -> [GuardState; 4] {
        [
            GuardState::Patrol,
            GuardState::Curious,
            GuardState::Investigate,
            GuardState::Chase,
        ]
    }

    #[test]
    fn guards_never_enter_a_wall_in_any_state_across_seeds() {
        for &seed in &[0u64, 1, 7, 42, 100, 777] {
            let layout = generate(seed);
            // The wall rects to test against (guards must never overlap these).
            let walls: Vec<(Vec2, Vec2)> =
                layout.walls.iter().map(|w| (w.center, w.half)).collect();

            // Cross-map anchors the guards will be sent to in every state.
            let entry = layout
                .rooms
                .iter()
                .find(|r| r.recipe == Recipe::Entry)
                .map_or(layout.player_start, Room::center);
            let mut anchors: Vec<Vec2> = vec![entry, layout.exit];
            anchors.extend(layout.guard_posts.iter().copied());
            anchors.extend(layout.alarm_panels.iter().copied());
            if let Some(v) = layout.vault {
                anchors.push(v);
            }

            let layout_for_seed = layout.clone();
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .init_resource::<Alarm>()
                .init_resource::<Loadout>()
                .init_resource::<BehaviorTimings>()
                .init_resource::<NavGraph>()
                .add_message::<SpottedEvent>()
                .add_message::<GlobalAlarm>()
                .add_message::<NoiseEvent>()
                .add_message::<PlayerCaught>()
                .add_systems(
                    Startup,
                    move |mut commands: Commands, mut nav: ResMut<NavGraph>| {
                        commands.spawn((
                            Transform::from_translation(layout_for_seed.player_start.extend(1.0)),
                            PlayerStats::default(),
                            Player,
                        ));
                        spawn_layout_walls(&mut commands, &layout_for_seed);
                        spawn_alarm_panels(&mut commands, &layout_for_seed);
                        spawn_round_guards_from_layout(&mut commands, &layout_for_seed, 12);
                        nav.0 = Some(RoomGraph::from_layout(&layout_for_seed));
                    },
                )
                .add_systems(Update, guard_ai_system);

            // First tick spawns everything.
            app.update();

            let states = all_states();
            for frame in 0..400u32 {
                // Every 40 frames, redirect each guard: assign a state and a
                // cross-map last-known-position / patrol so guards continuously
                // path across the whole compound in every FSM state.
                if frame % 40 == 0 && !anchors.is_empty() {
                    let base = (frame / 40) as usize;
                    let mut q = app.world_mut().query_filtered::<&mut Guard, ()>();
                    let world = app.world_mut();
                    for (gi, mut guard) in q.iter_mut(world).enumerate() {
                        let st = states[(base + gi) % states.len()];
                        guard.state = st;
                        guard.last_seen = anchors[(base + gi) % anchors.len()];
                        guard.suspicion = 60.0;
                        guard.alerted = matches!(st, GuardState::Chase | GuardState::Investigate);
                        guard.patrol = vec![
                            anchors[(base + gi) % anchors.len()],
                            anchors[(base + gi + 1) % anchors.len()],
                        ];
                        guard.patrol_index = 0;
                    }
                }

                // Roam the player along the anchors to provoke sight/curious/chase.
                if !anchors.is_empty() {
                    let target = anchors[(frame as usize / 7) % anchors.len()];
                    let mut pq = app
                        .world_mut()
                        .query_filtered::<&mut Transform, With<Player>>();
                    let world = app.world_mut();
                    if let Some(mut tf) = pq.iter_mut(world).next() {
                        let cur = tf.translation.truncate();
                        let step = (target - cur).clamp_length_max(6.0);
                        tf.translation.x = cur.x + step.x;
                        tf.translation.y = cur.y + step.y;
                    }
                }

                app.update();

                // Assert the invariant for every guard this frame.
                let mut gq = app.world_mut().query_filtered::<&Transform, With<Guard>>();
                let world = app.world();
                for tf in gq.iter(world) {
                    let p = tf.translation.truncate();
                    for &(wc, wh) in &walls {
                        let pen = penetration(p, GUARD_HALF, wc, wh);
                        assert!(
                            pen < 0.5,
                            "seed {seed} frame {frame}: guard at {p:?} penetrates wall \
                             (center {wc:?}, half {wh:?}) by {pen}"
                        );
                    }
                }
            }
        }
    }
}
