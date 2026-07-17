//! Guards — the statechart archetype and the heart of the demo.
//!
//! v2 (plan §10.2) replaces the old snap-to-alert FSM with an **MGS-lenient
//! suspicion ladder** whose defining property is *LOS-mandatory escape*:
//!
//! ```text
//! Patrol ──see/hear──▶ Curious ──build──▶ Investigate ──close sight──▶ Chase
//!    ▲                    │                    │  (walk to LKP,           │
//!    │                    │                    │   peek, then either      │
//!    │                    ▼                    ▼   raise the alarm)        │
//!    └────── calm ◀──── decay ◀──── give up ◀────────── LOS break ────────┘
//! ```
//!
//! Rules that make it a *game*:
//!   * **Suspicion accumulates, never snaps.** A cone glimpse contributes a
//!     strength with distance + angle falloff ([`cone_strength`]); only
//!     sustained, close, unobstructed sight fills the meter to Chase.
//!   * **Cones respect walls.** Perception is gated by a line-of-sight raycast
//!     ([`crate::world::raycast_clear`]) against the wall rects.
//!   * **Breaking LOS is the only escape.** While chasing, a guard sees the
//!     player through *any* open sightline regardless of range, so pure running
//!     never breaks contact — you must put a wall (or smoke) between you. On an
//!     LOS break the guard converges on the last-known position, searches, and
//!     decays.
//!   * **No telepathy.** An alerted guard *shouts* (radius recruitment) and, to
//!     wake the whole compound, must physically *reach an alarm panel* — which
//!     the player can outlast by staying hidden until it gives up.
//!
//! The suspicion integrator and transition rule stay **pure functions** so the
//! Phase-1 ink port diffs against them line-for-line.

use bevy::prelude::*;
use std::time::Instant;

use crate::alarm::{Alarm, AlarmPanel, GlobalAlarm, SpottedEvent, tier_speed_bonus};
use crate::layout_gen::LayoutData;
use crate::noise::{NoiseEvent, SmokeCloud};
use crate::rounds::{PlayerCaught, RoundScoped};
use crate::stats::{Loadout, PlayerStats};
use crate::timing::BehaviorTimings;
use crate::world::{Blocker, Collider, Player, Wall, raycast_clear};

// --- Tunables ---------------------------------------------------------------

pub const GUARD_HALF: Vec2 = Vec2::new(13.0, 13.0);
const VISION_RANGE: f32 = 210.0;
const VISION_HALF_ANGLE: f32 = 0.52; // radians (~30 deg)
/// While chasing, a guard tracks the player through any open sightline out to
/// this range (effectively the whole arena) — only a wall or smoke breaks it.
const CHASE_LOS_RANGE: f32 = 1000.0;
const CATCH_RADIUS: f32 = 24.0;
/// Sustained-contact time (seconds, while chasing) required to catch.
const CATCH_TIME: f32 = 0.30;
const WAYPOINT_REACHED: f32 = 12.0;
/// How close an alerted guard must get to a panel to raise the global alarm.
const PANEL_REACH: f32 = 34.0;

const SUSPICION_MAX: f32 = 100.0;
/// Ladder thresholds. Lenient: a glimpse barely moves the meter.
const CURIOUS_THRESHOLD: f32 = 14.0;
const INVESTIGATE_THRESHOLD: f32 = 45.0;
const CHASE_THRESHOLD: f32 = 74.0;
/// Hysteresis: Curious falls back to Patrol only once nearly calm.
const CURIOUS_CALM: f32 = 4.0;
/// Suspicion gained per second at full perception strength.
const SUSPICION_RISE: f32 = 95.0;
/// Suspicion lost per second while unperceived (the grace/decay).
const SUSPICION_DECAY: f32 = 26.0;

/// How far an alerted guard's shout recruits calmer guards.
const SHOUT_RADIUS: f32 = 190.0;
/// Seconds a guard dwells at each peek spot while investigating.
const PEEK_DWELL: f32 = 0.7;

const MAX_REINFORCEMENTS: u32 = 8;
const REINFORCE_INTERVAL: f32 = 3.0;
/// Fallback reinforcement entry if the layout has no barracks.
const DEFAULT_ENTRY: Vec2 = Vec2::new(540.0, -270.0);

/// How long a freshly spawned reinforcement guard sits still with a pulsing
/// marker before joining the sim (telegraphed arrival, #1010).
const SPAWN_TELEGRAPH_SECS: f32 = 0.6;

/// Upper bound on the per-frame `dt` used for guard motion/suspicion (#1010),
/// so a hitch can never move a guard far enough to read as a teleport.
const MAX_STEP_DT: f32 = 1.0 / 20.0;

// --- Components -------------------------------------------------------------

/// The guard suspicion-ladder state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardState {
    Patrol,
    Curious,
    Investigate,
    Chase,
}

impl GuardState {
    fn color(self) -> Color {
        match self {
            GuardState::Patrol => Color::srgb(0.55, 0.75, 0.55),
            GuardState::Curious => Color::srgb(0.9, 0.85, 0.3),
            GuardState::Investigate => Color::srgb(0.95, 0.55, 0.2),
            GuardState::Chase => Color::srgb(0.95, 0.25, 0.25),
        }
    }
}

/// A guard's mutable behavior state.
#[derive(Component, Debug)]
pub struct Guard {
    pub state: GuardState,
    pub suspicion: f32,
    pub patrol: Vec<Vec2>,
    pub patrol_index: usize,
    /// Last-known player position (LKP) or the origin of a heard noise.
    pub last_seen: Vec2,
    pub facing: f32,
    /// Once true the guard is committed to raising the alarm (heads for a
    /// panel when it cannot see the player). Cleared on standing down.
    pub alerted: bool,
    /// Sustained-contact timer; a catch requires it to exceed [`CATCH_TIME`]
    /// while chasing.
    pub contact: f32,
    /// Peek spots visited while investigating a spot.
    peek_points: Vec<Vec2>,
    peek_index: usize,
    peek_timer: f32,
}

impl Guard {
    /// A few spots to peek around a last-known position.
    fn build_peeks(&mut self, lkp: Vec2) {
        self.peek_points = vec![
            lkp,
            lkp + Vec2::new(64.0, 0.0),
            lkp + Vec2::new(-56.0, 40.0),
            lkp + Vec2::new(0.0, -60.0),
        ];
        self.peek_index = 0;
        self.peek_timer = PEEK_DWELL;
    }

    fn peeks_done(&self) -> bool {
        !self.alerted && self.peek_index >= self.peek_points.len()
    }
}

/// Marks a guard mid-telegraph: spawned but frozen and excluded from the AI
/// until the timer finishes (#1010).
#[derive(Component, Debug)]
pub struct SpawnTelegraph {
    timer: Timer,
}

impl SpawnTelegraph {
    fn new() -> Self {
        Self {
            timer: Timer::from_seconds(SPAWN_TELEGRAPH_SECS, TimerMode::Once),
        }
    }
}

/// Reinforcement wave bookkeeping. Reset each round.
#[derive(Resource, Debug)]
pub struct ReinforcementSpawner {
    pub timer: Timer,
    pub spawned: u32,
    /// The barracks entry point for this round's layout.
    pub entry: Option<Vec2>,
}

impl Default for ReinforcementSpawner {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(REINFORCE_INTERVAL, TimerMode::Repeating),
            spawned: 0,
            entry: None,
        }
    }
}

// --- Pure logic (unit-tested) ----------------------------------------------

/// Cap a frame's delta-time (#1010).
#[must_use]
pub fn clamp_step_dt(dt: f32) -> f32 {
    dt.min(MAX_STEP_DT)
}

/// Perception strength in `0..=1` for a target in a vision cone: 0 outside the
/// cone/range, rising toward 1 as the target is centered and close. This is the
/// leniency knob — cone-edge or distant glimpses contribute little.
#[must_use]
pub fn cone_strength(apex: Vec2, facing: f32, half_angle: f32, range: f32, target: Vec2) -> f32 {
    let to = target - apex;
    let dist = to.length();
    if dist > range {
        return 0.0;
    }
    if dist < f32::EPSILON {
        return 1.0;
    }
    let facing_dir = Vec2::new(facing.cos(), facing.sin());
    let cos_theta = to.normalize().dot(facing_dir);
    let cos_half = half_angle.cos();
    if cos_theta < cos_half {
        return 0.0;
    }
    let dist_falloff = 1.0 - dist / range;
    // 1 at the cone's center line, 0 at its edge.
    let angle_falloff = if (1.0 - cos_half) > f32::EPSILON {
        (cos_theta - cos_half) / (1.0 - cos_half)
    } else {
        1.0
    };
    (dist_falloff * angle_falloff).clamp(0.0, 1.0)
}

/// Integrate the suspicion meter for one tick. Rises with perception strength,
/// decays otherwise (the grace period).
#[must_use]
pub fn update_suspicion(suspicion: f32, strength: f32, dt: f32) -> f32 {
    let next = if strength > 0.0 {
        suspicion + SUSPICION_RISE * strength * dt
    } else {
        suspicion - SUSPICION_DECAY * dt
    };
    next.clamp(0.0, SUSPICION_MAX)
}

/// The guard transition rule. Pure: state in, state out. `has_los` is whether
/// the guard currently has an unobstructed line to the player; `peeks_done`
/// means a non-alerted investigation has finished its search.
#[must_use]
pub fn guard_transition(
    state: GuardState,
    suspicion: f32,
    has_los: bool,
    peeks_done: bool,
) -> GuardState {
    match state {
        GuardState::Patrol => {
            if has_los || suspicion >= CURIOUS_THRESHOLD {
                GuardState::Curious
            } else {
                GuardState::Patrol
            }
        }
        GuardState::Curious => {
            if has_los && suspicion >= CHASE_THRESHOLD {
                GuardState::Chase
            } else if suspicion >= INVESTIGATE_THRESHOLD {
                GuardState::Investigate
            } else if suspicion <= CURIOUS_CALM {
                GuardState::Patrol
            } else {
                GuardState::Curious
            }
        }
        GuardState::Investigate => {
            if has_los && suspicion >= CHASE_THRESHOLD {
                GuardState::Chase
            } else if peeks_done || suspicion <= 0.0 {
                GuardState::Patrol
            } else {
                GuardState::Investigate
            }
        }
        // Chase holds *only* while LOS is held — losing it drops to a search,
        // which is what makes breaking sight the mandatory escape (§10.2).
        GuardState::Chase => {
            if has_los {
                GuardState::Chase
            } else {
                GuardState::Investigate
            }
        }
    }
}

/// Base movement speed for a state, before the alarm bonus.
fn state_speed(state: GuardState) -> f32 {
    match state {
        GuardState::Patrol => 78.0,
        GuardState::Curious => 48.0,
        GuardState::Investigate => 128.0,
        GuardState::Chase => 208.0,
    }
}

// --- Systems ----------------------------------------------------------------

/// The core guard behavior tick: perception → suspicion → transition → move,
/// plus shout recruitment and alarm-panel seeking.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn guard_ai_system(
    time: Res<Time>,
    alarm: Res<Alarm>,
    loadout: Res<Loadout>,
    player: Query<(&Transform, &PlayerStats), With<Player>>,
    walls: Query<(&Transform, &Collider), (With<Wall>, Without<Guard>)>,
    panels: Query<&Transform, (With<AlarmPanel>, Without<Guard>)>,
    smokes: Query<(&Transform, &SmokeCloud), Without<Guard>>,
    mut noise: MessageReader<NoiseEvent>,
    mut guards: Query<
        (&mut Transform, &mut Guard, &mut Sprite),
        (Without<Player>, Without<SpawnTelegraph>),
    >,
    mut spotted: MessageWriter<SpottedEvent>,
    mut global: MessageWriter<GlobalAlarm>,
    mut caught: MessageWriter<PlayerCaught>,
    mut timings: ResMut<BehaviorTimings>,
) {
    let start = Instant::now();
    let dt = clamp_step_dt(time.delta_secs());

    let Ok((player_tf, stats)) = player.single() else {
        timings.guards = start.elapsed();
        return;
    };
    let player_pos = player_tf.translation.truncate();

    let blockers: Vec<Blocker> = walls
        .iter()
        .map(|(t, c)| (t.translation.truncate(), c.half_extents))
        .collect();
    let panel_positions: Vec<Vec2> = panels.iter().map(|t| t.translation.truncate()).collect();
    // The player is invisible while inside a smoke cloud (§10.3).
    let player_hidden = smokes
        .iter()
        .any(|(t, s)| t.translation.truncate().distance(player_pos) < s.radius);
    let noises: Vec<NoiseEvent> = noise.read().copied().collect();

    let effective_range =
        (VISION_RANGE * loadout.enemy_vision_scale - stats.stealth_radius).max(24.0);
    let speed_bonus = tier_speed_bonus(alarm.tier());

    // Sources of shouts this frame (position of every alerting guard) to spread
    // alert without telepathy.
    let mut shouts: Vec<(Vec2, Vec2)> = Vec::new(); // (guard pos, its LKP)

    for (mut tf, mut guard, mut sprite) in &mut guards {
        let pos = tf.translation.truncate();

        // --- Perception (cone + wall LOS; chase widens the sightline) ---
        let (has_los, strength) = if player_hidden {
            (false, 0.0)
        } else if guard.state == GuardState::Chase {
            let clear = pos.distance(player_pos) < CHASE_LOS_RANGE
                && raycast_clear(pos, player_pos, &blockers);
            (clear, if clear { 1.0 } else { 0.0 })
        } else {
            let s = cone_strength(
                pos,
                guard.facing,
                VISION_HALF_ANGLE,
                effective_range,
                player_pos,
            );
            let clear = s > 0.0 && raycast_clear(pos, player_pos, &blockers);
            if clear { (true, s) } else { (false, 0.0) }
        };

        guard.suspicion = update_suspicion(guard.suspicion, strength, dt);

        if has_los {
            guard.last_seen = player_pos;
            let intensity = if guard.state == GuardState::Chase {
                1.4
            } else {
                0.7
            };
            spotted.write(SpottedEvent {
                intensity: intensity * dt,
            });
        } else {
            // Heard, not seen: the loudest noise in range pulls the guard toward
            // the SOUND ORIGIN (§10.3).
            if let Some(ev) = noises
                .iter()
                .filter(|e| pos.distance(e.origin) < e.radius)
                .min_by(|a, b| {
                    pos.distance(a.origin)
                        .partial_cmp(&pos.distance(b.origin))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                guard.last_seen = ev.origin;
                guard.suspicion = guard.suspicion.max(INVESTIGATE_THRESHOLD + 3.0);
            }
        }

        // --- Movement target for the current state ---
        let mut target = match guard.state {
            GuardState::Patrol => guard.patrol[guard.patrol_index],
            GuardState::Curious => guard.last_seen,
            GuardState::Investigate => {
                if guard.alerted && !panel_positions.is_empty() {
                    nearest(&panel_positions, pos)
                } else if guard.peek_index < guard.peek_points.len() {
                    guard.peek_points[guard.peek_index]
                } else {
                    guard.last_seen
                }
            }
            GuardState::Chase => {
                if has_los {
                    player_pos
                } else {
                    guard.last_seen
                }
            }
        };

        let reached_target = pos.distance(target) < WAYPOINT_REACHED;

        // Per-state arrival bookkeeping.
        match guard.state {
            GuardState::Patrol => {
                if reached_target && !guard.patrol.is_empty() {
                    guard.patrol_index = (guard.patrol_index + 1) % guard.patrol.len();
                }
            }
            GuardState::Investigate => {
                if guard.alerted && !panel_positions.is_empty() {
                    // Reached a panel: raise the global alarm and stand down.
                    if pos.distance(target) < PANEL_REACH {
                        global.write(GlobalAlarm);
                        guard.alerted = false;
                        guard.suspicion = 0.0;
                    }
                } else if guard.peek_index < guard.peek_points.len() && reached_target {
                    guard.peek_timer -= dt;
                    if guard.peek_timer <= 0.0 {
                        guard.peek_index += 1;
                        guard.peek_timer = PEEK_DWELL;
                    }
                }
            }
            _ => {}
        }

        // --- Transition ---
        let old_state = guard.state;
        let peeks_done = guard.peeks_done();
        let new_state = guard_transition(guard.state, guard.suspicion, has_los, peeks_done);
        guard.state = new_state;

        // Entering Chase commits the guard to the alarm.
        if new_state == GuardState::Chase {
            guard.alerted = true;
        }
        // Standing down clears the commitment and resets peeks.
        if new_state == GuardState::Patrol {
            guard.alerted = false;
            guard.peek_points.clear();
            guard.peek_index = 0;
        }
        // Freshly entering an investigation: plan the peek sweep around the LKP.
        if new_state == GuardState::Investigate
            && old_state != GuardState::Investigate
            && !guard.alerted
        {
            let lkp = guard.last_seen;
            guard.build_peeks(lkp);
        }

        if new_state == GuardState::Chase || guard.alerted {
            shouts.push((pos, guard.last_seen));
        }

        sprite.color = new_state.color();

        // Re-derive the target if the state just changed, so movement is
        // responsive rather than lagging a frame.
        if new_state != old_state {
            target = match new_state {
                GuardState::Patrol => guard.patrol[guard.patrol_index],
                GuardState::Curious => guard.last_seen,
                GuardState::Investigate => {
                    if guard.alerted && !panel_positions.is_empty() {
                        nearest(&panel_positions, pos)
                    } else if !guard.peek_points.is_empty() {
                        guard.peek_points[0]
                    } else {
                        guard.last_seen
                    }
                }
                GuardState::Chase => {
                    if has_los {
                        player_pos
                    } else {
                        guard.last_seen
                    }
                }
            };
        }

        // --- Move + face ---
        let to_target = target - pos;
        if to_target.length() > 1.0 {
            let dir = to_target.normalize();
            let step = dir * state_speed(new_state) * speed_bonus * dt;
            tf.translation += step.extend(0.0);
            let face_dir = if has_los { player_pos - pos } else { to_target };
            if face_dir.length() > f32::EPSILON {
                guard.facing = face_dir.y.atan2(face_dir.x);
            }
        } else if guard.state == GuardState::Curious {
            // Curious guards that have arrived still turn toward the stimulus.
            let face_dir = guard.last_seen - pos;
            if face_dir.length() > f32::EPSILON {
                guard.facing = face_dir.y.atan2(face_dir.x);
            }
        }

        // --- Catch (sustained contact, chase only) ---
        let now = tf.translation.truncate();
        if new_state == GuardState::Chase && now.distance(player_pos) < CATCH_RADIUS {
            guard.contact += dt;
            if guard.contact >= CATCH_TIME {
                caught.write(PlayerCaught);
            }
        } else {
            guard.contact = 0.0;
        }
    }

    // --- Shout recruitment: alerting guards pull calmer nearby guards up to an
    // investigation of the same spot (no telepathy — bounded by radius). ---
    if !shouts.is_empty() {
        for (tf, mut guard, _sprite) in &mut guards {
            if matches!(guard.state, GuardState::Chase) || guard.alerted {
                continue;
            }
            let pos = tf.translation.truncate();
            if guard.suspicion < INVESTIGATE_THRESHOLD
                && let Some((_, lkp)) = shouts
                    .iter()
                    .find(|(sp, _)| sp.distance(pos) < SHOUT_RADIUS && sp.distance(pos) > 1.0)
            {
                guard.suspicion = INVESTIGATE_THRESHOLD + 2.0;
                guard.last_seen = *lkp;
            }
        }
    }

    timings.guards = start.elapsed();
}

fn nearest(points: &[Vec2], from: Vec2) -> Vec2 {
    points
        .iter()
        .copied()
        .min_by(|a, b| {
            from.distance_squared(*a)
                .partial_cmp(&from.distance_squared(*b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(from)
}

/// Spawn reinforcement waves while the alarm is high enough (the compound-wide
/// sweep, gated at tier ≥ 2). Capped per round. Guards enter from the barracks.
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
    let entry = spawner.entry.unwrap_or(DEFAULT_ENTRY);
    for i in 0..2 {
        if spawner.spawned >= MAX_REINFORCEMENTS {
            break;
        }
        let jitter = Vec2::new(0.0, 30.0 * i as f32);
        spawn_guard(
            &mut commands,
            entry + jitter,
            vec![entry + jitter, Vec2::ZERO],
            GuardState::Investigate,
            true,
        );
        spawner.spawned += 1;
    }
}

/// Draw every active guard's vision cone (debug gizmos, toggled by F1).
pub fn draw_guard_cones(
    guards: Query<(&Transform, &Guard), Without<SpawnTelegraph>>,
    mut gizmos: Gizmos,
) {
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

/// Draw the always-on suspicion tell above each active guard (?, ?, !) so the
/// ladder is readable at a glance (plan §10.2).
pub fn draw_guard_tells(
    guards: Query<(&Transform, &Guard), Without<SpawnTelegraph>>,
    mut gizmos: Gizmos,
) {
    for (tf, guard) in &guards {
        let base = tf.translation.truncate() + Vec2::new(0.0, 26.0);
        match guard.state {
            GuardState::Patrol => {}
            GuardState::Curious => draw_question(&mut gizmos, base, Color::srgb(0.95, 0.9, 0.35)),
            GuardState::Investigate => {
                draw_question(&mut gizmos, base, Color::srgb(0.98, 0.6, 0.2));
            }
            GuardState::Chase => draw_bang(&mut gizmos, base, Color::srgb(1.0, 0.25, 0.25)),
        }
    }
}

fn draw_question(gizmos: &mut Gizmos, at: Vec2, color: Color) {
    // A small hook + dot approximating "?".
    gizmos.circle_2d(at + Vec2::new(0.0, 4.0), 4.0, color);
    gizmos.line_2d(at + Vec2::new(0.0, -1.0), at + Vec2::new(0.0, -4.0), color);
    gizmos.circle_2d(at + Vec2::new(0.0, -8.0), 0.9, color);
}

fn draw_bang(gizmos: &mut Gizmos, at: Vec2, color: Color) {
    gizmos.line_2d(at + Vec2::new(0.0, 8.0), at + Vec2::new(0.0, -2.0), color);
    gizmos.circle_2d(at + Vec2::new(0.0, -6.0), 1.1, color);
}

/// Draw a pulsing warning ring over every telegraphed guard, fading its sprite
/// in as the timer closes (#1010).
pub fn spawn_telegraph_system(
    time: Res<Time>,
    mut commands: Commands,
    mut guards: Query<(Entity, &Transform, &mut SpawnTelegraph, &mut Sprite)>,
    mut gizmos: Gizmos,
) {
    for (entity, tf, mut telegraph, mut sprite) in &mut guards {
        telegraph.timer.tick(time.delta());
        let t = telegraph.timer.fraction();

        let pos = tf.translation.truncate();
        let radius = 12.0 + 20.0 * t;
        let mut ring_color = Color::srgb(0.95, 0.3, 0.3);
        ring_color.set_alpha(1.0 - 0.6 * t);
        gizmos.circle_2d(pos, radius, ring_color);

        let mut fade = sprite.color;
        fade.set_alpha(0.25 + 0.75 * t);
        sprite.color = fade;

        if telegraph.timer.is_finished() {
            commands.entity(entity).remove::<SpawnTelegraph>();
        }
    }
}

/// Spawn the round's starting guards, patrolling between the layout's guard
/// posts. `count` scales with round difficulty (the compound escalates, not the
/// player — §10.3).
pub fn spawn_round_guards_from_layout(commands: &mut Commands, layout: &LayoutData, count: u32) {
    let posts = &layout.guard_posts;
    if posts.is_empty() {
        return;
    }
    for i in 0..count as usize {
        let post_a = posts[i % posts.len()];
        let post_b = posts[(i + 1) % posts.len()];
        spawn_guard(
            commands,
            post_a,
            vec![post_a, post_b],
            GuardState::Patrol,
            false,
        );
    }
}

fn spawn_guard(
    commands: &mut Commands,
    pos: Vec2,
    patrol: Vec<Vec2>,
    state: GuardState,
    telegraphed: bool,
) {
    let mut sprite = Sprite::from_color(state.color(), GUARD_HALF * 2.0);
    if telegraphed {
        sprite.color.set_alpha(0.25);
    }

    let mut entity = commands.spawn((
        sprite,
        Transform::from_translation(pos.extend(1.0)),
        Guard {
            state,
            suspicion: 0.0,
            patrol,
            patrol_index: 0,
            last_seen: pos,
            facing: 0.0,
            alerted: false,
            contact: 0.0,
            peek_points: Vec::new(),
            peek_index: 0,
            peek_timer: PEEK_DWELL,
        },
        RoundScoped,
    ));
    if telegraphed {
        entity.insert(SpawnTelegraph::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_step_dt_caps_a_hitch() {
        assert!((clamp_step_dt(1.0) - MAX_STEP_DT).abs() < f32::EPSILON);
        let dt = 1.0 / 60.0;
        assert!((clamp_step_dt(dt) - dt).abs() < f32::EPSILON);
    }

    #[test]
    fn cone_strength_falls_off_with_distance_and_angle() {
        // Directly ahead, close: strong.
        let near = cone_strength(Vec2::ZERO, 0.0, 0.5, 200.0, Vec2::new(20.0, 0.0));
        // Directly ahead, far: weaker.
        let far = cone_strength(Vec2::ZERO, 0.0, 0.5, 200.0, Vec2::new(190.0, 0.0));
        assert!(near > far, "closer glimpse is stronger");
        // Near the cone edge: weaker than centered at the same distance.
        let centered = cone_strength(Vec2::ZERO, 0.0, 0.5, 200.0, Vec2::new(100.0, 0.0));
        let edge = cone_strength(Vec2::ZERO, 0.0, 0.5, 200.0, Vec2::new(100.0, 47.0));
        assert!(centered > edge, "edge glimpse is weaker");
        // Outside the cone / range: zero.
        assert!(cone_strength(Vec2::ZERO, 0.0, 0.5, 200.0, Vec2::new(-50.0, 0.0)) < f32::EPSILON);
        assert!(cone_strength(Vec2::ZERO, 0.0, 0.5, 200.0, Vec2::new(300.0, 0.0)) < f32::EPSILON);
    }

    #[test]
    fn suspicion_is_lenient_a_glimpse_barely_moves_it() {
        // A weak, brief glimpse adds only a little.
        let s = update_suspicion(0.0, 0.1, 1.0 / 60.0);
        assert!(
            s > 0.0 && s < CURIOUS_THRESHOLD,
            "one weak frame stays sub-curious"
        );
    }

    #[test]
    fn suspicion_decays_when_unperceived() {
        assert!(update_suspicion(50.0, 0.0, 0.1) < 50.0);
        assert!(update_suspicion(1.0, 0.0, 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn patrol_wakes_to_curious_then_investigate() {
        assert_eq!(
            guard_transition(GuardState::Patrol, 0.0, true, false),
            GuardState::Curious
        );
        assert_eq!(
            guard_transition(GuardState::Curious, INVESTIGATE_THRESHOLD, false, false),
            GuardState::Investigate
        );
    }

    #[test]
    fn chase_requires_held_los_and_high_suspicion() {
        // High suspicion but no LOS: never jumps straight to chase.
        assert_ne!(
            guard_transition(GuardState::Curious, CHASE_THRESHOLD, false, false),
            GuardState::Chase
        );
        assert_eq!(
            guard_transition(GuardState::Curious, CHASE_THRESHOLD, true, false),
            GuardState::Chase
        );
    }

    #[test]
    fn breaking_los_is_the_only_escape_from_chase() {
        // With LOS held, chase persists no matter the suspicion.
        assert_eq!(
            guard_transition(GuardState::Chase, SUSPICION_MAX, true, false),
            GuardState::Chase
        );
        assert_eq!(
            guard_transition(GuardState::Chase, 10.0, true, false),
            GuardState::Chase,
            "pure running never escapes while LOS is held"
        );
        // Only losing LOS drops to a search.
        assert_eq!(
            guard_transition(GuardState::Chase, SUSPICION_MAX, false, false),
            GuardState::Investigate,
            "breaking LOS always drops the chase"
        );
    }

    #[test]
    fn investigation_ends_when_search_completes_or_calms() {
        assert_eq!(
            guard_transition(GuardState::Investigate, 30.0, false, true),
            GuardState::Patrol
        );
        assert_eq!(
            guard_transition(GuardState::Investigate, 0.0, false, false),
            GuardState::Patrol
        );
        // Re-sighting the player during a search re-escalates to chase.
        assert_eq!(
            guard_transition(GuardState::Investigate, CHASE_THRESHOLD, true, false),
            GuardState::Chase
        );
    }

    #[test]
    fn full_ladder_patrol_to_chase_to_search_to_calm() {
        let mut s = guard_transition(GuardState::Patrol, CURIOUS_THRESHOLD, false, false);
        assert_eq!(s, GuardState::Curious);
        s = guard_transition(s, INVESTIGATE_THRESHOLD, false, false);
        assert_eq!(s, GuardState::Investigate);
        s = guard_transition(s, CHASE_THRESHOLD, true, false);
        assert_eq!(s, GuardState::Chase);
        s = guard_transition(s, CHASE_THRESHOLD, false, false); // lost sight
        assert_eq!(s, GuardState::Investigate);
        s = guard_transition(s, 0.0, false, false); // calmed
        assert_eq!(s, GuardState::Patrol);
    }
}
