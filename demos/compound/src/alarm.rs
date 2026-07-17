//! Global alarm escalation.
//!
//! The alarm is a single world-policy value in `0.0..=3.0`. Guards and cameras
//! never mutate it directly — they emit [`SpottedEvent`] messages, and the one
//! [`alarm_system`] folds those into the level and applies slow decay. That
//! single-writer shape is exactly the "World-policy writes, frame-start
//! consistency" seam the plan calls out (§4).
//!
//! v2 (plan §10.2/§10.3) splits the alarm into two regimes:
//!   * **Spotting** raises it only up to a soft cap ([`SOFT_CAP`], tier 1) — a
//!     guard seeing you makes them *personally* alert but does not wake the
//!     whole compound.
//!   * **Global alarm** (tier 3) fires only when an *alerted guard reaches an
//!     Alarm-panel room* ([`GlobalAlarm`]). That is the intercept-or-flee
//!     counterplay: stop the runner and the compound stays quiet. The
//!     compound-wide sweep (reinforcements, faster sweeps) exists only at
//!     alarm ≥ 2, and decays away if the player stays hidden.

use bevy::prelude::*;
use std::time::Instant;

use crate::layout_gen::LayoutData;
use crate::rounds::RoundScoped;
use crate::timing::BehaviorTimings;

/// Maximum escalation level.
pub const MAX_LEVEL: f32 = 3.0;
/// Level at/above which the compound-wide sweep (reinforcements) runs.
pub const REINFORCE_LEVEL: u8 = 2;
/// Ceiling that plain spotting can raise the alarm to. Passing it — the global
/// alarm — requires a guard physically reaching an alarm panel.
pub const SOFT_CAP: f32 = 1.9;

/// How fast the alarm bleeds off per second when nothing is spotting.
const DECAY_PER_SEC: f32 = 0.25;

/// Marks an alarm-panel objective. An alerted guard that reaches one raises the
/// global alarm; the player can intercept the guard first.
#[derive(Component, Debug)]
pub struct AlarmPanel;

/// The global alarm state.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Alarm {
    /// Continuous escalation, `0.0..=MAX_LEVEL`.
    pub level: f32,
    /// Whether the global alarm has been raised this round (a guard reached a
    /// panel). Latches the HUD tell; decay still lowers `level`.
    pub global: bool,
}

impl Alarm {
    /// Raise the alarm from *spotting*, capped at [`SOFT_CAP`]. Never lowers an
    /// already-higher level (e.g. a global alarm mid-decay).
    pub fn escalate_spotting(&mut self, amount: f32) {
        if self.level < SOFT_CAP {
            self.level = (self.level + amount).min(SOFT_CAP);
        }
    }

    /// Raise the *global* alarm — a guard reached an alarm panel.
    pub fn trigger_global(&mut self) {
        self.level = MAX_LEVEL;
        self.global = true;
    }

    /// Bleed the alarm down over `dt`.
    pub fn decay(&mut self, dt: f32) {
        self.level = (self.level - DECAY_PER_SEC * dt).max(0.0);
        if self.level < f32::EPSILON {
            self.global = false;
        }
    }

    /// The discrete alarm level, `0..=3`, that guards read.
    pub fn tier(&self) -> u8 {
        self.level.floor() as u8
    }

    /// Reset to calm (used when a new round starts).
    pub fn reset(&mut self) {
        self.level = 0.0;
        self.global = false;
    }
}

/// Emitted whenever a guard or camera sees the player. `intensity` is how much
/// it should raise the (spotting-capped) alarm.
#[derive(Message, Debug, Clone, Copy)]
pub struct SpottedEvent {
    pub intensity: f32,
}

/// Emitted when an alerted guard reaches an alarm panel: raise the global alarm.
#[derive(Message, Debug, Clone, Copy)]
pub struct GlobalAlarm;

/// Fold spotting + global-alarm messages into the alarm and apply decay. Single
/// writer.
pub fn alarm_system(
    time: Res<Time>,
    mut alarm: ResMut<Alarm>,
    mut spotted: MessageReader<SpottedEvent>,
    mut global: MessageReader<GlobalAlarm>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();

    alarm.decay(time.delta_secs());
    for ev in spotted.read() {
        alarm.escalate_spotting(ev.intensity);
    }
    if global.read().count() > 0 {
        alarm.trigger_global();
    }

    timings.alarm = start.elapsed();
}

/// Speed multiplier a given alarm tier grants to everyone who reads it.
pub fn tier_speed_bonus(tier: u8) -> f32 {
    1.0 + 0.2 * f32::from(tier)
}

/// Spawn the alarm-panel objectives the layout placed.
pub fn spawn_alarm_panels(commands: &mut Commands, layout: &LayoutData) {
    for &pos in &layout.alarm_panels {
        commands.spawn((
            Sprite::from_color(Color::srgb(0.95, 0.2, 0.2), Vec2::new(26.0, 26.0)),
            Transform::from_translation(pos.extend(0.4)),
            AlarmPanel,
            RoundScoped,
        ));
    }
}

/// Bevy `Time` isn't available in unit tests, so the tests below exercise the
/// pure `Alarm` methods directly.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotting_is_capped_below_global() {
        let mut a = Alarm::default();
        a.escalate_spotting(10.0);
        assert!((a.level - SOFT_CAP).abs() < f32::EPSILON);
        assert!(a.tier() < REINFORCE_LEVEL, "spotting alone never sweeps");
        assert!(!a.global);
    }

    #[test]
    fn only_a_panel_triggers_the_global_sweep() {
        let mut a = Alarm::default();
        a.escalate_spotting(10.0);
        assert!(a.tier() < REINFORCE_LEVEL);
        a.trigger_global();
        assert!(a.tier() >= REINFORCE_LEVEL, "panel wakes the compound");
        assert!(a.global);
    }

    #[test]
    fn spotting_never_lowers_a_global_alarm() {
        let mut a = Alarm::default();
        a.trigger_global();
        a.escalate_spotting(0.5);
        assert!((a.level - MAX_LEVEL).abs() < f32::EPSILON);
    }

    #[test]
    fn decay_bleeds_down_and_clears_global() {
        let mut a = Alarm::default();
        a.trigger_global();
        a.decay(100.0); // way more than enough
        assert!(a.level.abs() < f32::EPSILON);
        assert!(!a.global, "global clears once the alarm bleeds out");
    }

    #[test]
    fn tier_floors_the_level() {
        let mut a = Alarm::default();
        a.escalate_spotting(1.5);
        assert_eq!(a.tier(), 1);
    }

    #[test]
    fn higher_tier_moves_faster() {
        assert!(tier_speed_bonus(3) > tier_speed_bonus(0));
    }
}
