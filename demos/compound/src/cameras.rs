//! Security cameras — a pure sweeping-cone detector.
//!
//! Cameras are the simplest perception entity: no memory, no FSM, just a cone
//! that oscillates and raises the alarm when the player crosses it. Disabling
//! one (E while close) feeds the round bounty. In Phase 1 this becomes a pure
//! ink logic loop plus a `#[derive(BrinkCommand)]` "camera disabled" command
//! (plan §3), so the sweep math is kept trivially portable.

use bevy::prelude::*;
use std::time::Instant;

use crate::alarm::SpottedEvent;
use crate::rounds::{Round, RoundScoped};
use crate::stats::{Loadout, PlayerStats};
use crate::timing::BehaviorTimings;
use crate::world::{Player, point_in_cone};

const CAMERA_RANGE: f32 = 210.0;
const CAMERA_HALF_ANGLE: f32 = 0.45;
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
        );
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

/// Spawn the round's cameras. Called from the round-start handler so each
/// round begins with every camera live.
pub fn spawn_cameras(commands: &mut Commands) {
    // (position, center facing angle)
    let placements = [
        (Vec2::new(-200.0, 250.0), -std::f32::consts::FRAC_PI_2),
        (Vec2::new(200.0, -250.0), std::f32::consts::FRAC_PI_2),
        (Vec2::new(0.0, 0.0), 0.0),
        (Vec2::new(420.0, 260.0), std::f32::consts::PI),
    ];
    for (pos, angle) in placements {
        commands.spawn((
            Sprite::from_color(Color::srgb(0.8, 0.7, 0.2), CAMERA_HALF * 2.0),
            Transform::from_translation(pos.extend(1.0)),
            SecurityCamera::new(angle),
            RoundScoped,
        ));
    }
}
