//! Security cameras — a pure sweeping-cone detector.
//!
//! Cameras are the simplest perception entity: no memory, no FSM, just a cone
//! that oscillates and raises the alarm when the player crosses it. Disabling
//! one (E while close) feeds the round bounty. This is the Rust baseline
//! (`camera_ai_system`); as of Phase 1c the sweep-and-detect loop also has an
//! ink port side-by-side (`assets/cameras.ink` + `src/ink_cameras.rs`,
//! `--cameras-impl ink`) — see that module's docs for why it stayed a pure
//! return-value loop instead of the `#[derive(BrinkCommand)]` shape the plan
//! originally sketched, and disabling stays Rust-only in both modes.

use bevy::prelude::*;
use std::time::Instant;

use crate::alarm::SpottedEvent;
use crate::layout_gen::LayoutData;
use crate::rounds::{Round, RoundScoped};
use crate::stats::{Loadout, PlayerStats};
use crate::timing::BehaviorTimings;
use crate::world::{Collider, Player, Wall, point_in_cone, raycast_clear};

/// Base vision range before the loadout/stealth adjustment. `pub(crate)` so
/// the Phase 1c ink port (`ink_cameras.rs`) can compute the identical
/// `effective_range` formula for its `sweep_and_detect` calls.
pub(crate) const CAMERA_RANGE: f32 = 210.0;
/// Cone half-angle, radians. `pub(crate)` so the ink port's `sees_player`
/// world-access binding can reuse `world::point_in_cone` with the same
/// geometry the Rust baseline uses.
pub(crate) const CAMERA_HALF_ANGLE: f32 = 0.45;
const DISABLE_RADIUS: f32 = 46.0;
const CAMERA_HALF: Vec2 = Vec2::new(11.0, 11.0);

/// Bounty (gold) awarded per camera disabled, paid out at round end.
pub const CAMERA_BOUNTY: u32 = 15;

/// A sweeping security camera. Named `SecurityCamera` to avoid colliding with
/// Bevy's rendering `Camera`.
#[derive(Component, Debug)]
pub struct SecurityCamera {
    /// Center of the sweep, radians.
    pub center_angle: f32,
    /// Sweep amplitude, radians.
    pub sweep_half: f32,
    /// Sweep angular speed.
    pub speed: f32,
    /// Accumulated sweep phase.
    pub phase: f32,
    /// Current facing, derived from the sweep each tick.
    pub facing: f32,
    pub disabled: bool,
}

impl SecurityCamera {
    fn new(center_angle: f32) -> Self {
        Self {
            center_angle,
            sweep_half: 0.7,
            speed: 1.1,
            phase: 0.0,
            facing: center_angle,
            disabled: false,
        }
    }
}

/// Sweep every camera and raise the alarm on line-of-sight.
#[allow(clippy::too_many_arguments)]
pub fn camera_ai_system(
    time: Res<Time>,
    loadout: Res<Loadout>,
    player: Query<(&Transform, &PlayerStats), With<Player>>,
    mut cameras: Query<(&Transform, &mut SecurityCamera, &mut Sprite)>,
    walls: Query<(&Transform, &Collider), With<Wall>>,
    mut spotted: MessageWriter<SpottedEvent>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();
    let dt = time.delta_secs();

    let Ok((player_tf, stats)) = player.single() else {
        timings.cameras = start.elapsed();
        return;
    };
    let player_pos = player_tf.translation.truncate();
    let effective_range =
        (CAMERA_RANGE * loadout.enemy_vision_scale - stats.stealth_radius).max(24.0);

    // Wall rectangles the camera's line of sight must clear (plan §10.2).
    let blockers: Vec<(Vec2, Vec2)> = walls
        .iter()
        .map(|(t, c)| (t.translation.truncate(), c.half_extents))
        .collect();

    for (tf, mut cam, mut sprite) in &mut cameras {
        if cam.disabled {
            sprite.color = Color::srgb(0.3, 0.3, 0.3);
            continue;
        }
        cam.phase += dt * cam.speed;
        cam.facing = cam.center_angle + cam.sweep_half * cam.phase.sin();

        let apex = tf.translation.truncate();
        let sees = point_in_cone(
            apex,
            cam.facing,
            CAMERA_HALF_ANGLE,
            effective_range,
            player_pos,
        ) && raycast_clear(apex, player_pos, &blockers);
        if sees {
            spotted.write(SpottedEvent {
                intensity: 1.0 * dt,
            });
            sprite.color = Color::srgb(1.0, 0.3, 0.3);
        } else {
            sprite.color = Color::srgb(0.8, 0.7, 0.2);
        }
    }

    timings.cameras = start.elapsed();
}

/// Disable the nearest camera when the player presses E next to it. Feeds the
/// round bounty.
pub fn camera_interact_system(
    keys: Res<ButtonInput<KeyCode>>,
    player: Query<&Transform, With<Player>>,
    mut cameras: Query<(&Transform, &mut SecurityCamera)>,
    mut round: ResMut<Round>,
) {
    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Ok(player_tf) = player.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    // Disable the single closest live camera in range.
    let mut best: Option<(f32, Mut<SecurityCamera>)> = None;
    for (tf, cam) in &mut cameras {
        if cam.disabled {
            continue;
        }
        let d = tf.translation.truncate().distance(player_pos);
        if d < DISABLE_RADIUS && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, cam));
        }
    }
    if let Some((_, mut cam)) = best {
        cam.disabled = true;
        round.cameras_disabled += 1;
        // Bounty is carried gold — banked on exit, lost if caught (§10.3).
        round.carried += CAMERA_BOUNTY;
    }
}

/// Draw active camera cones (debug gizmos, F1).
pub fn draw_camera_cones(cameras: Query<(&Transform, &SecurityCamera)>, mut gizmos: Gizmos) {
    for (tf, cam) in &cameras {
        if cam.disabled {
            continue;
        }
        let apex = tf.translation.truncate();
        crate::world::draw_cone(
            &mut gizmos,
            apex,
            cam.facing,
            CAMERA_HALF_ANGLE,
            CAMERA_RANGE,
            Color::srgba(1.0, 0.4, 0.4, 0.5),
        );
    }
}

/// Spawn the cameras the layout's `CameraNest` recipes placed. Each round
/// begins with every camera live.
pub fn spawn_cameras_from_layout(commands: &mut Commands, layout: &LayoutData) {
    for c in &layout.cameras {
        commands.spawn((
            Sprite::from_color(Color::srgb(0.8, 0.7, 0.2), CAMERA_HALF * 2.0),
            Transform::from_translation(c.pos.extend(1.0)),
            SecurityCamera::new(c.angle),
            RoundScoped,
        ));
    }
}
