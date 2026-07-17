//! Round manager — the top-level FSM.
//!
//! The game alternates `Playing → Shop → Playing …`. A round ends when the
//! player reaches the exit (escaped) or a guard makes contact (caught, unless a
//! medkit absorbs it). Ending a round awards gold and enters the shop; leaving
//! the shop bumps the difficulty and starts the next round. This is the
//! outermost statechart (plan §4) and, like the guard FSM, its reward and
//! difficulty rules are pure functions the ink port can diff against.

use bevy::prelude::*;

use crate::alarm::Alarm;
use crate::cameras::{CAMERA_BOUNTY, spawn_cameras};
use crate::doors::spawn_doors;
use crate::guards::{ReinforcementSpawner, spawn_round_guards};
use crate::stats::Loadout;
use crate::world::{Exit, PLAYER_START, Player};

/// Gold awarded for reaching the exit alive.
pub const SURVIVE_BONUS: u32 = 20;
/// How close to the exit counts as escaping.
const EXIT_RADIUS: f32 = 45.0;
/// Base guard count before per-round difficulty scaling.
const BASE_GUARDS: u32 = 10;
/// Ceiling on starting guards so difficulty stays sane.
const MAX_START_GUARDS: u32 = 24;

/// High-level game phase.
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Phase {
    #[default]
    Playing,
    Shop,
}

/// Marks an entity that belongs to the current round and should be cleared when
/// a new round starts (guards, rats, cameras, doors, switches).
#[derive(Component, Debug)]
pub struct RoundScoped;

/// Persistent run state.
#[derive(Resource, Debug)]
pub struct Round {
    pub number: u32,
    pub gold: u32,
    pub cameras_disabled: u32,
    pub reached_exit: bool,
}

impl Default for Round {
    fn default() -> Self {
        Self {
            number: 1,
            gold: 0,
            cameras_disabled: 0,
            reached_exit: false,
        }
    }
}

/// The outcome of the most recent round, shown on the shop screen.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LastOutcome {
    pub escaped: bool,
    pub reward: u32,
}

/// Emitted by a guard on contact with the player.
#[derive(Message, Debug, Clone, Copy)]
pub struct PlayerCaught;

/// Emitted to (re)build the arena for a fresh round.
#[derive(Message, Debug, Clone, Copy)]
pub struct StartRound;

/// Number of starting guards for a given round.
#[must_use]
pub fn guard_count(round_number: u32) -> u32 {
    (BASE_GUARDS + round_number).min(MAX_START_GUARDS)
}

/// Gold earned from a round's bounty and survival bonus.
#[must_use]
pub fn round_reward(cameras_disabled: u32, escaped: bool) -> u32 {
    cameras_disabled * CAMERA_BOUNTY + if escaped { SURVIVE_BONUS } else { 0 }
}

/// Rebuild the arena on a [`StartRound`] message: clear round-scoped entities,
/// reset the player and world policy, and spawn fresh doors, cameras, guards.
#[allow(clippy::too_many_arguments)]
pub fn start_round_system(
    mut commands: Commands,
    mut start: MessageReader<StartRound>,
    scoped: Query<Entity, With<RoundScoped>>,
    mut player: Query<&mut Transform, With<Player>>,
    mut alarm: ResMut<Alarm>,
    mut spawner: ResMut<ReinforcementSpawner>,
    mut round: ResMut<Round>,
) {
    // Only act once even if several messages arrived.
    if start.read().count() == 0 {
        return;
    }

    for entity in &scoped {
        commands.entity(entity).despawn();
    }

    if let Ok(mut tf) = player.single_mut() {
        tf.translation = PLAYER_START.extend(1.0);
    }

    alarm.reset();
    *spawner = ReinforcementSpawner::default();
    round.cameras_disabled = 0;
    round.reached_exit = false;

    spawn_doors(&mut commands);
    spawn_cameras(&mut commands);
    spawn_round_guards(&mut commands, guard_count(round.number));
}

/// Detect round-ending conditions while playing.
#[allow(clippy::too_many_arguments)]
pub fn round_outcome_system(
    mut caught: MessageReader<PlayerCaught>,
    mut player: Query<&mut Transform, With<Player>>,
    exit: Query<&Transform, (With<Exit>, Without<Player>)>,
    mut loadout: ResMut<Loadout>,
    mut round: ResMut<Round>,
    mut outcome: ResMut<LastOutcome>,
    mut next: ResMut<NextState<Phase>>,
) {
    // --- Escape check ---
    let player_pos = player.single().ok().map(|tf| tf.translation.truncate());
    if let (Some(pp), Ok(exit_tf)) = (player_pos, exit.single())
        && pp.distance(exit_tf.translation.truncate()) < EXIT_RADIUS
    {
        end_round(&mut round, &mut outcome, &mut next, true);
        return;
    }

    // --- Capture check ---
    let was_caught = caught.read().count() > 0;
    if was_caught {
        if loadout.medkits > 0 {
            // A medkit absorbs the hit: consume it and warp the player home.
            loadout.medkits -= 1;
            if let Ok(mut tf) = player.single_mut() {
                tf.translation = PLAYER_START.extend(1.0);
            }
        } else {
            end_round(&mut round, &mut outcome, &mut next, false);
        }
    }
}

fn end_round(
    round: &mut Round,
    outcome: &mut LastOutcome,
    next: &mut NextState<Phase>,
    escaped: bool,
) {
    let reward = round_reward(round.cameras_disabled, escaped);
    round.gold += reward;
    round.reached_exit = escaped;
    outcome.escaped = escaped;
    outcome.reward = reward;
    next.set(Phase::Shop);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_count_scales_and_caps() {
        assert_eq!(guard_count(1), BASE_GUARDS + 1);
        assert_eq!(guard_count(100), MAX_START_GUARDS);
        assert!(guard_count(3) > guard_count(1));
    }

    #[test]
    fn reward_pays_bounty_and_survival() {
        assert_eq!(round_reward(0, false), 0);
        assert_eq!(round_reward(2, false), 2 * CAMERA_BOUNTY);
        assert_eq!(round_reward(2, true), 2 * CAMERA_BOUNTY + SURVIVE_BONUS);
    }
}
