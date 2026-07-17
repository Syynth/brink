//! Noise, coins, and smoke — the speed-vs-noise axis and the two consumables
//! (plan §10.3).
//!
//! * **Running** (hold Shift) is faster but emits [`NoiseEvent`]s at the
//!   player's position; walking is silent. Guards within a noise's radius are
//!   drawn toward its **origin** (handled in [`crate::guards`]).
//! * **Coins** are thrown ammo that fly, land, and emit a noise where they
//!   settle — a lure to pull guards off your route.
//! * **Smoke** is the one panic button: a cloud that instantly breaks line of
//!   sight, so a chase drops to a search.
//!
//! Emitting and reacting are separated: this module produces stimuli, guards
//! consume them, keeping the perception logic in one place.

use bevy::prelude::*;

use crate::rounds::RoundScoped;
use crate::stats::PlayerStats;
use crate::world::Player;

/// Radius a running player's footstep noise reaches.
const RUN_NOISE_RADIUS: f32 = 210.0;
/// Radius a landed coin's clink reaches.
const COIN_NOISE_RADIUS: f32 = 170.0;
/// Minimum seconds between run-noise pulses (so it is not per-frame spam).
const RUN_NOISE_INTERVAL: f32 = 0.3;
/// Coin flight speed and lifetime.
const COIN_SPEED: f32 = 520.0;
const COIN_LIFE: f32 = 0.5;
/// Smoke cloud radius + duration.
pub const SMOKE_RADIUS: f32 = 90.0;
const SMOKE_DURATION: f32 = 3.0;

/// A stimulus the guards can hear: something happened at `origin` audible
/// within `radius`.
#[derive(Message, Debug, Clone, Copy)]
pub struct NoiseEvent {
    pub origin: Vec2,
    pub radius: f32,
}

/// A thrown coin in flight. Emits a [`NoiseEvent`] where it lands.
#[derive(Component, Debug)]
pub struct Coin {
    vel: Vec2,
    life: f32,
}

/// An active smoke cloud. While the player is inside one, no guard can see them.
#[derive(Component, Debug)]
pub struct SmokeCloud {
    pub radius: f32,
    timer: Timer,
}

/// Throttle for run-noise pulses.
#[derive(Resource, Debug)]
pub struct RunNoiseClock(Timer);

impl Default for RunNoiseClock {
    fn default() -> Self {
        Self(Timer::from_seconds(
            RUN_NOISE_INTERVAL,
            TimerMode::Repeating,
        ))
    }
}

/// Whether the run key is held this frame. Shared with player movement so the
/// speed bonus and the noise agree on "running".
#[must_use]
pub fn is_running(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

/// Emit periodic footstep noise while the player runs and actually moves.
pub fn run_noise_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clock: ResMut<RunNoiseClock>,
    player: Query<(&Transform, &PlayerStats), With<Player>>,
    mut noise: MessageWriter<NoiseEvent>,
) {
    clock.0.tick(time.delta());
    if !is_running(&keys) {
        return;
    }
    let moving = keys.pressed(KeyCode::KeyW)
        || keys.pressed(KeyCode::KeyA)
        || keys.pressed(KeyCode::KeyS)
        || keys.pressed(KeyCode::KeyD)
        || keys.pressed(KeyCode::ArrowUp)
        || keys.pressed(KeyCode::ArrowDown)
        || keys.pressed(KeyCode::ArrowLeft)
        || keys.pressed(KeyCode::ArrowRight);
    if !moving || !clock.0.just_finished() {
        return;
    }
    if let Ok((tf, stats)) = player.single() {
        noise.write(NoiseEvent {
            origin: tf.translation.truncate(),
            radius: RUN_NOISE_RADIUS + stats.noise_bonus,
        });
    }
}

/// Spawn a coin flying from `from` toward `dir` (a lure).
pub fn spawn_coin(commands: &mut Commands, from: Vec2, dir: Vec2) {
    let vel = dir.normalize_or_zero() * COIN_SPEED;
    commands.spawn((
        Sprite::from_color(Color::srgb(0.95, 0.85, 0.35), Vec2::new(7.0, 7.0)),
        Transform::from_translation(from.extend(0.8)),
        Coin {
            vel,
            life: COIN_LIFE,
        },
        RoundScoped,
    ));
}

/// Fly coins and, when one lands (life expires), emit a noise where it settled.
pub fn coin_system(
    time: Res<Time>,
    mut commands: Commands,
    mut coins: Query<(Entity, &mut Transform, &mut Coin)>,
    mut noise: MessageWriter<NoiseEvent>,
) {
    let dt = time.delta_secs();
    for (entity, mut tf, mut coin) in &mut coins {
        coin.life -= dt;
        let step = coin.vel * dt;
        tf.translation += step.extend(0.0);
        if coin.life <= 0.0 {
            noise.write(NoiseEvent {
                origin: tf.translation.truncate(),
                radius: COIN_NOISE_RADIUS,
            });
            commands.entity(entity).despawn();
        }
    }
}

/// Drop a smoke cloud at `pos`.
pub fn spawn_smoke(commands: &mut Commands, pos: Vec2) {
    commands.spawn((
        Sprite::from_color(
            Color::srgba(0.8, 0.82, 0.85, 0.55),
            Vec2::splat(SMOKE_RADIUS * 2.0),
        ),
        Transform::from_translation(pos.extend(0.9)),
        SmokeCloud {
            radius: SMOKE_RADIUS,
            timer: Timer::from_seconds(SMOKE_DURATION, TimerMode::Once),
        },
        RoundScoped,
    ));
}

/// Expire smoke clouds and fade them out over their lifetime.
pub fn smoke_system(
    time: Res<Time>,
    mut commands: Commands,
    mut clouds: Query<(Entity, &mut SmokeCloud, &mut Sprite)>,
) {
    for (entity, mut cloud, mut sprite) in &mut clouds {
        cloud.timer.tick(time.delta());
        let remaining = cloud.timer.fraction_remaining();
        sprite.color.set_alpha(0.55 * remaining);
        if cloud.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}
