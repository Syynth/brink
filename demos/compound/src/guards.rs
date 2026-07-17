//! Guards — the statechart archetype.
//!
//! This is the module the whole demo exists to stress-test (plan §3): a
//! per-entity FSM `Patrol → Suspicious → Alert → Search → ReturnToPost` driven
//! by a suspicion meter and a vision cone. In Phase 1 this is the first thing
//! ported to ink knots-as-states, so the transition rule and the suspicion
//! integrator are kept as **pure functions** ([`guard_transition`],
//! [`update_suspicion`]) that the ink port can diff against line-for-line. The
//! Bevy system is a thin shell that gathers inputs, calls the pure logic, and
//! applies movement.

use bevy::prelude::*;
use std::time::Instant;

use crate::alarm::{Alarm, SpottedEvent, tier_speed_bonus};
use crate::rounds::{PlayerCaught, RoundScoped};
use crate::stats::{Loadout, PlayerStats};
use crate::timing::BehaviorTimings;
use crate::world::{Player, point_in_cone};

// --- Tunables ---------------------------------------------------------------

pub const GUARD_HALF: Vec2 = Vec2::new(13.0, 13.0);
const VISION_RANGE: f32 = 220.0;
const VISION_HALF_ANGLE: f32 = 0.5; // radians (~28.6 deg)
const CATCH_RADIUS: f32 = 22.0;
const WAYPOINT_REACHED: f32 = 12.0;

const SUSPICION_MAX: f32 = 100.0;
const ALERT_THRESHOLD: f32 = 80.0;
const SUSPICION_RISE: f32 = 120.0; // per second while seen
const SUSPICION_DECAY: f32 = 40.0; // per second while unseen

const MAX_REINFORCEMENTS: u32 = 8;
const REINFORCE_INTERVAL: f32 = 3.0;
const REINFORCE_ENTRY: Vec2 = Vec2::new(540.0, -270.0);

/// Patrol anchor points scattered around the arena.
const POSTS: [Vec2; 6] = [
    Vec2::new(-400.0, -180.0),
    Vec2::new(-350.0, 180.0),
    Vec2::new(60.0, -200.0),
    Vec2::new(120.0, 150.0),
    Vec2::new(430.0, -150.0),
    Vec2::new(450.0, 120.0),
];

// --- Components -------------------------------------------------------------

/// The guard finite-state-machine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardState {
    Patrol,
    Suspicious,
    Alert,
    Search,
    ReturnToPost,
}

impl GuardState {
    fn color(self) -> Color {
        match self {
            GuardState::Patrol => Color::srgb(0.55, 0.75, 0.55),
            GuardState::Suspicious => Color::srgb(0.9, 0.85, 0.3),
            GuardState::Alert => Color::srgb(0.95, 0.25, 0.25),
            GuardState::Search => Color::srgb(0.95, 0.55, 0.2),
            GuardState::ReturnToPost => Color::srgb(0.45, 0.6, 0.9),
        }
    }
}

/// A guard's mutable behavior state.
#[derive(Component, Debug)]
pub struct Guard {
    pub state: GuardState,
    pub suspicion: f32,
    pub home_post: Vec2,
    pub patrol: Vec<Vec2>,
    pub patrol_index: usize,
    pub last_seen: Vec2,
    pub facing: f32,
}

impl Guard {
    fn at_post(&self, pos: Vec2) -> bool {
        pos.distance(self.home_post) < WAYPOINT_REACHED
    }
}

/// Reinforcement wave bookkeeping. Reset each round.
#[derive(Resource, Debug)]
pub struct ReinforcementSpawner {
    pub timer: Timer,
    pub spawned: u32,
}

impl Default for ReinforcementSpawner {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(REINFORCE_INTERVAL, TimerMode::Repeating),
            spawned: 0,
        }
    }
}

// --- Pure logic (unit-tested) ----------------------------------------------

/// Integrate the suspicion meter for one tick.
#[must_use]
pub fn update_suspicion(suspicion: f32, sees_player: bool, dt: f32) -> f32 {
    let next = if sees_player {
        suspicion + SUSPICION_RISE * dt
    } else {
        suspicion - SUSPICION_DECAY * dt
    };
    next.clamp(0.0, SUSPICION_MAX)
}

/// The guard transition rule. Pure: state in, state out.
#[must_use]
pub fn guard_transition(
    state: GuardState,
    suspicion: f32,
    sees_player: bool,
    reached_target: bool,
    at_post: bool,
) -> GuardState {
    match state {
        GuardState::Patrol => {
            if sees_player {
                GuardState::Suspicious
            } else {
                GuardState::Patrol
            }
        }
        GuardState::Suspicious => {
            if suspicion >= ALERT_THRESHOLD {
                GuardState::Alert
            } else if suspicion <= 0.0 {
                GuardState::ReturnToPost
            } else {
                GuardState::Suspicious
            }
        }
        GuardState::Alert => {
            if sees_player {
                GuardState::Alert
            } else {
                GuardState::Search
            }
        }
        GuardState::Search => {
            if sees_player {
                GuardState::Alert
            } else if reached_target || suspicion <= 0.0 {
                GuardState::ReturnToPost
            } else {
                GuardState::Search
            }
        }
        GuardState::ReturnToPost => {
            if sees_player {
                GuardState::Suspicious
            } else if at_post {
                GuardState::Patrol
            } else {
                GuardState::ReturnToPost
            }
        }
    }
}

/// Base movement speed for a state, before the alarm bonus.
fn state_speed(state: GuardState) -> f32 {
    match state {
        GuardState::Patrol => 70.0,
        GuardState::Suspicious => 60.0,
        GuardState::Alert => 150.0,
        GuardState::Search => 95.0,
        GuardState::ReturnToPost => 110.0,
    }
}

// --- Systems ----------------------------------------------------------------

/// The core guard behavior tick: perception → suspicion → transition → move.
#[allow(clippy::too_many_arguments)]
pub fn guard_ai_system(
    time: Res<Time>,
    alarm: Res<Alarm>,
    loadout: Res<Loadout>,
    player: Query<(&Transform, &PlayerStats), With<Player>>,
    mut guards: Query<(&mut Transform, &mut Guard, &mut Sprite), Without<Player>>,
    mut spotted: MessageWriter<SpottedEvent>,
    mut caught: MessageWriter<PlayerCaught>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();
    let dt = time.delta_secs();

    let Ok((player_tf, stats)) = player.single() else {
        timings.guards = start.elapsed();
        return;
    };
    let player_pos = player_tf.translation.truncate();

    let effective_range =
        (VISION_RANGE * loadout.enemy_vision_scale - stats.stealth_radius).max(24.0);
    let speed_bonus = tier_speed_bonus(alarm.tier());

    for (mut tf, mut guard, mut sprite) in &mut guards {
        let pos = tf.translation.truncate();

        // --- Perception ---
        let sees_player = point_in_cone(
            pos,
            guard.facing,
            VISION_HALF_ANGLE,
            effective_range,
            player_pos,
        );
        guard.suspicion = update_suspicion(guard.suspicion, sees_player, dt);
        if sees_player {
            guard.last_seen = player_pos;
            // Continuous escalation; Alert guards shout louder.
            let intensity = if guard.state == GuardState::Alert {
                1.4
            } else {
                0.7
            };
            spotted.write(SpottedEvent {
                intensity: intensity * dt,
            });
        }

        // --- Movement target for the current state ---
        let target = match guard.state {
            GuardState::Patrol => guard.patrol[guard.patrol_index],
            GuardState::Suspicious | GuardState::Alert | GuardState::Search => {
                if sees_player {
                    player_pos
                } else {
                    guard.last_seen
                }
            }
            GuardState::ReturnToPost => guard.home_post,
        };
        let reached_target = pos.distance(target) < WAYPOINT_REACHED;

        // Advance the patrol loop when a waypoint is reached.
        if guard.state == GuardState::Patrol && reached_target && !guard.patrol.is_empty() {
            guard.patrol_index = (guard.patrol_index + 1) % guard.patrol.len();
        }

        // --- Transition ---
        let at_post = guard.at_post(pos);
        guard.state = guard_transition(
            guard.state,
            guard.suspicion,
            sees_player,
            reached_target,
            at_post,
        );
        sprite.color = guard.state.color();

        // --- Move + face ---
        let to_target = target - pos;
        if to_target.length() > 1.0 {
            let dir = to_target.normalize();
            let step = dir * state_speed(guard.state) * speed_bonus * dt;
            tf.translation += step.extend(0.0);
            // Face the direction of travel (or the player when seen).
            let face_dir = if sees_player {
                player_pos - pos
            } else {
                to_target
            };
            guard.facing = face_dir.y.atan2(face_dir.x);
        }

        // --- Contact catch ---
        if tf.translation.truncate().distance(player_pos) < CATCH_RADIUS {
            caught.write(PlayerCaught);
        }
    }

    timings.guards = start.elapsed();
}

/// Spawn reinforcement waves while the alarm is high enough. Capped per round.
pub fn reinforcement_system(
    time: Res<Time>,
    alarm: Res<Alarm>,
    mut spawner: ResMut<ReinforcementSpawner>,
    mut commands: Commands,
) {
    if alarm.tier() < crate::alarm::REINFORCE_LEVEL || spawner.spawned >= MAX_REINFORCEMENTS {
        return;
    }
    spawner.timer.tick(time.delta());
    if !spawner.timer.just_finished() {
        return;
    }
    // Two guards per wave, marching in from the entry toward the arena center.
    for i in 0..2 {
        if spawner.spawned >= MAX_REINFORCEMENTS {
            break;
        }
        let jitter = Vec2::new(0.0, 30.0 * i as f32);
        spawn_guard(
            &mut commands,
            REINFORCE_ENTRY + jitter,
            vec![REINFORCE_ENTRY + jitter, Vec2::ZERO],
            GuardState::Search,
        );
        spawner.spawned += 1;
    }
}

/// Draw every guard's vision cone (debug gizmos, toggled by F1).
pub fn draw_guard_cones(guards: Query<(&Transform, &Guard)>, mut gizmos: Gizmos) {
    for (tf, guard) in &guards {
        let apex = tf.translation.truncate();
        let mut color = guard.state.color();
        color.set_alpha(0.5);
        crate::world::draw_cone(
            &mut gizmos,
            apex,
            guard.facing,
            VISION_HALF_ANGLE,
            VISION_RANGE,
            color,
        );
    }
}

/// Spawn the round's starting guards. `count` scales with round difficulty.
pub fn spawn_round_guards(commands: &mut Commands, count: u32) {
    for i in 0..count as usize {
        let post_a = POSTS[i % POSTS.len()];
        let post_b = POSTS[(i + 1) % POSTS.len()];
        spawn_guard(commands, post_a, vec![post_a, post_b], GuardState::Patrol);
    }
}

fn spawn_guard(commands: &mut Commands, pos: Vec2, patrol: Vec<Vec2>, state: GuardState) {
    commands.spawn((
        Sprite::from_color(state.color(), GUARD_HALF * 2.0),
        Transform::from_translation(pos.extend(1.0)),
        Guard {
            state,
            suspicion: 0.0,
            home_post: pos,
            patrol,
            patrol_index: 0,
            last_seen: pos,
            facing: 0.0,
        },
        RoundScoped,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspicion_rises_when_seen_and_caps() {
        let s = update_suspicion(0.0, true, 1.0);
        assert!(s > 0.0);
        assert!(update_suspicion(99.0, true, 10.0) <= SUSPICION_MAX);
    }

    #[test]
    fn suspicion_decays_when_unseen_and_floors() {
        assert!(update_suspicion(10.0, false, 0.1) < 10.0);
        assert!(update_suspicion(1.0, false, 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn patrol_becomes_suspicious_on_sight() {
        assert_eq!(
            guard_transition(GuardState::Patrol, 5.0, true, false, false),
            GuardState::Suspicious
        );
    }

    #[test]
    fn suspicious_escalates_to_alert_at_threshold() {
        assert_eq!(
            guard_transition(GuardState::Suspicious, ALERT_THRESHOLD, true, false, false),
            GuardState::Alert
        );
    }

    #[test]
    fn suspicious_returns_when_calmed() {
        assert_eq!(
            guard_transition(GuardState::Suspicious, 0.0, false, false, false),
            GuardState::ReturnToPost
        );
    }

    #[test]
    fn alert_drops_to_search_when_sight_lost() {
        assert_eq!(
            guard_transition(GuardState::Alert, 90.0, false, false, false),
            GuardState::Search
        );
    }

    #[test]
    fn search_returns_after_reaching_last_seen() {
        assert_eq!(
            guard_transition(GuardState::Search, 50.0, false, true, false),
            GuardState::ReturnToPost
        );
    }

    #[test]
    fn search_re_alerts_on_sight() {
        assert_eq!(
            guard_transition(GuardState::Search, 10.0, true, false, false),
            GuardState::Alert
        );
    }

    #[test]
    fn return_to_post_resumes_patrol_at_post() {
        assert_eq!(
            guard_transition(GuardState::ReturnToPost, 0.0, false, false, true),
            GuardState::Patrol
        );
        assert_eq!(
            guard_transition(GuardState::ReturnToPost, 0.0, false, false, false),
            GuardState::ReturnToPost
        );
    }

    #[test]
    fn full_arc_patrol_to_alert_to_return() {
        // Sight the player from patrol.
        let mut s = guard_transition(GuardState::Patrol, 0.0, true, false, false);
        assert_eq!(s, GuardState::Suspicious);
        // Meter fills, escalate.
        s = guard_transition(s, ALERT_THRESHOLD, true, false, false);
        assert_eq!(s, GuardState::Alert);
        // Lose sight, search.
        s = guard_transition(s, 70.0, false, false, false);
        assert_eq!(s, GuardState::Search);
        // Reach last-known, give up.
        s = guard_transition(s, 0.0, false, true, false);
        assert_eq!(s, GuardState::ReturnToPost);
        // Arrive home, patrol again.
        s = guard_transition(s, 0.0, false, false, true);
        assert_eq!(s, GuardState::Patrol);
    }
}
