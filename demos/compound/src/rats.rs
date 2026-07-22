//! Rats — the throughput spectacle.
//!
//! Rats do nothing interesting individually: they wander, occasionally pick a
//! new heading, and bounce off the arena bounds. The point is scale — press
//! `+` to spawn 500 at a time. In Phase 1 they become the batch/parallel
//! benchmark (plan §3), so the per-rat tick is kept as cheap as possible: no
//! collision, no queries beyond the rat's own transform, one branch and one
//! vector add.

use bevy::prelude::*;
use std::time::Instant;

use crate::rounds::RoundScoped;
use crate::timing::BehaviorTimings;
use crate::world::ARENA_HALF;

/// Hard cap on live rats, so `+` cannot exhaust memory.
pub const MAX_RATS: usize = 8000;
/// How many rats each `+` press spawns.
pub const RATS_PER_BATCH: usize = 500;

const RAT_SPEED: f32 = 45.0;
const RAT_HALF: Vec2 = Vec2::new(2.5, 2.5);

/// A wandering rat.
#[derive(Component, Debug)]
pub struct Rat {
    /// Current heading, radians.
    pub heading: f32,
    /// Seconds until the next heading change.
    pub retarget: f32,
}

/// Tiny deterministic PRNG (xorshift32) shared across rat spawning + wander, so
/// the demo needs no `rand` dependency and stays reproducible.
#[derive(Resource, Debug)]
pub struct RatRng {
    state: u32,
}

impl Default for RatRng {
    fn default() -> Self {
        Self { state: 0x1234_5678 }
    }
}

impl RatRng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform float in `[0, 1)`.
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform angle in `[0, 2π)`.
    fn next_angle(&mut self) -> f32 {
        self.next_f32() * std::f32::consts::TAU
    }
}

/// Move every rat and bounce it off the arena bounds.
pub fn rat_system(
    time: Res<Time>,
    mut rng: ResMut<RatRng>,
    mut rats: Query<(&mut Transform, &mut Rat)>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();
    let dt = time.delta_secs();
    let limit = ARENA_HALF - RAT_HALF;

    for (mut tf, mut rat) in &mut rats {
        rat.retarget -= dt;
        if rat.retarget <= 0.0 {
            rat.heading = rng.next_angle();
            rat.retarget = 1.0 + rng.next_f32() * 2.0;
        }

        let dir = Vec2::new(rat.heading.cos(), rat.heading.sin());
        let mut p = tf.translation.truncate() + dir * RAT_SPEED * dt;

        // Reflect off the walls.
        if p.x < -limit.x || p.x > limit.x {
            rat.heading = std::f32::consts::PI - rat.heading;
            p.x = p.x.clamp(-limit.x, limit.x);
        }
        if p.y < -limit.y || p.y > limit.y {
            rat.heading = -rat.heading;
            p.y = p.y.clamp(-limit.y, limit.y);
        }

        tf.translation.x = p.x;
        tf.translation.y = p.y;
    }

    timings.rats = start.elapsed();
}

/// Spawn a batch of rats at random positions, respecting [`MAX_RATS`].
pub fn spawn_rats(commands: &mut Commands, rng: &mut RatRng, current: usize, batch: usize) {
    let room = MAX_RATS.saturating_sub(current);
    let n = batch.min(room);
    for _ in 0..n {
        let x = (rng.next_f32() * 2.0 - 1.0) * (ARENA_HALF.x - 20.0);
        let y = (rng.next_f32() * 2.0 - 1.0) * (ARENA_HALF.y - 20.0);
        commands.spawn((
            Sprite::from_color(Color::srgb(0.6, 0.45, 0.4), RAT_HALF * 2.0),
            Transform::from_xyz(x, y, 0.7),
            Rat {
                heading: rng.next_angle(),
                retarget: rng.next_f32() * 2.0,
            },
            RoundScoped,
        ));
    }
}
