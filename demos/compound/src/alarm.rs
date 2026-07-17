//! Global alarm escalation.
//!
//! The alarm is a single world-policy value in `0.0..=3.0`. Guards and cameras
//! never mutate it directly — they emit [`SpottedEvent`] messages, and the one
//! [`alarm_system`] folds those into the level and applies slow decay. That
//! single-writer shape is exactly the "World-policy writes, frame-start
//! consistency" seam the plan calls out (§4): in Phase 1 the alarm becomes an
//! ink global, and having one writer keeps the port honest.

use bevy::prelude::*;
use std::time::Instant;

use crate::timing::BehaviorTimings;

/// Maximum escalation level.
pub const MAX_LEVEL: f32 = 3.0;
/// Level at/above which reinforcements are called.
pub const REINFORCE_LEVEL: u8 = 2;

/// How fast the alarm bleeds off per second when nothing is spotting.
const DECAY_PER_SEC: f32 = 0.25;

/// The global alarm state.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Alarm {
    /// Continuous escalation, `0.0..=MAX_LEVEL`.
    pub level: f32,
}

impl Alarm {
    /// Bump the alarm, clamped to the ceiling.
    pub fn escalate(&mut self, amount: f32) {
        self.level = (self.level + amount).clamp(0.0, MAX_LEVEL);
    }

    /// Bleed the alarm down over `dt`.
    pub fn decay(&mut self, dt: f32) {
        self.level = (self.level - DECAY_PER_SEC * dt).max(0.0);
    }

    /// The discrete alarm level, `0..=3`, that guards read.
    pub fn tier(&self) -> u8 {
        // floor, so the level must fully reach 1.0 before it "counts".
        self.level.floor() as u8
    }

    /// Reset to calm (used when a new round starts).
    pub fn reset(&mut self) {
        self.level = 0.0;
    }
}

/// Emitted whenever a guard or camera sees the player. `intensity` is how much
/// it should raise the alarm.
#[derive(Message, Debug, Clone, Copy)]
pub struct SpottedEvent {
    pub intensity: f32,
}

/// Fold spotting messages into the alarm and apply decay. Single writer.
pub fn alarm_system(
    time: Res<Time>,
    mut alarm: ResMut<Alarm>,
    mut spotted: MessageReader<SpottedEvent>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();

    alarm.decay(time.delta_secs());
    for ev in spotted.read() {
        alarm.escalate(ev.intensity);
    }

    timings.alarm = start.elapsed();
}

/// Speed multiplier a given alarm tier grants to everyone who reads it.
pub fn tier_speed_bonus(tier: u8) -> f32 {
    1.0 + 0.2 * f32::from(tier)
}

/// Bevy `Time` isn't available in unit tests, so the tests below exercise the
/// pure `Alarm` methods directly.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalate_clamps_to_ceiling() {
        let mut a = Alarm::default();
        a.escalate(10.0);
        assert!((a.level - MAX_LEVEL).abs() < f32::EPSILON);
    }

    #[test]
    fn tier_floors_the_level() {
        let mut a = Alarm::default();
        a.escalate(1.9);
        assert_eq!(a.tier(), 1);
        a.escalate(0.2);
        assert_eq!(a.tier(), 2);
    }

    #[test]
    fn decay_bleeds_down_and_floors_at_zero() {
        let mut a = Alarm { level: 0.1 };
        a.decay(10.0); // 0.25*10 = 2.5 >> 0.1
        assert!((a.level).abs() < f32::EPSILON);
    }

    #[test]
    fn reinforce_reads_tier() {
        let mut a = Alarm::default();
        a.escalate(2.0);
        assert!(a.tier() >= REINFORCE_LEVEL);
    }

    #[test]
    fn higher_tier_moves_faster() {
        assert!(tier_speed_bonus(3) > tier_speed_bonus(0));
    }
}
