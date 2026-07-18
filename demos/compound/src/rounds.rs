//! Round manager — the top-level FSM.
//!
//! The game alternates `Playing → Shop → Playing …`. Each round draws a fresh
//! **seeded BSP layout** (plan §10.1, [`crate::layout_gen`]) and instantiates
//! it. A round ends when the player reaches the exit (**escaped** — the carried
//! gold is banked) or a guard makes sustained contact during a chase
//! (**caught** — the unbanked haul is lost). Leaving the shop bumps the round
//! number and starts the next, richer round.
//!
//! Two pure/edge-triggered properties are unit-tested here:
//!   * banking ([`banked_on_exit`]) — the push-your-luck payout (§10.3);
//!   * **edge-triggered round end** (#1024) — the escape check runs every
//!     frame, so `end_round` is latched behind `round.ended` to grant exactly
//!     one reward per round no matter how many frames the exit condition holds.

use bevy::prelude::*;

use crate::alarm::{Alarm, spawn_alarm_panels};
use crate::cameras::spawn_cameras_from_layout;
use crate::doors::spawn_doors_from_layout;
use crate::guards::{ReinforcementSpawner, spawn_round_guards_from_layout};
use crate::layout_gen::{LayoutData, generate};
use crate::loot::spawn_gold_from_layout;
use crate::nav::{NavGraph, RoomGraph};
use crate::world::{Exit, Player, spawn_layout_walls};

/// How close to the exit counts as escaping.
const EXIT_RADIUS: f32 = 45.0;
/// Base guard count before per-round difficulty scaling.
const BASE_GUARDS: u32 = 6;
/// Ceiling on starting guards so difficulty stays sane.
const MAX_START_GUARDS: u32 = 20;

/// High-level game phase.
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Phase {
    #[default]
    Playing,
    Shop,
}

/// Marks an entity that belongs to the current round and should be cleared when
/// a new round starts (guards, rats, cameras, doors, switches, walls, gold, …).
#[derive(Component, Debug)]
pub struct RoundScoped;

/// Persistent run state.
#[derive(Resource, Debug)]
pub struct Round {
    pub number: u32,
    /// Gold safely banked across rounds — the only spendable balance.
    pub banked: u32,
    /// Gold picked up this round; banked on exit, lost if caught (§10.3).
    pub carried: u32,
    pub cameras_disabled: u32,
    /// This round's layout seed (deterministic per round number).
    pub seed: u64,
    /// Edge-trigger latch: true once this round has ended, so the reward path
    /// runs exactly once (#1024). Reset by [`start_round_system`].
    pub ended: bool,
}

impl Default for Round {
    fn default() -> Self {
        Self {
            number: 1,
            banked: 0,
            carried: 0,
            cameras_disabled: 0,
            seed: round_seed(1),
            ended: false,
        }
    }
}

/// The current round's generated layout, kept for systems that need geometry
/// after spawn (guards raycasting against walls, reinforcement entry points).
#[derive(Resource, Debug, Default)]
pub struct CurrentLayout(pub Option<LayoutData>);

/// The outcome of the most recent round, shown on the shop screen.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LastOutcome {
    pub escaped: bool,
    pub reward: u32,
}

/// Emitted by a guard on sustained contact with the player during a chase.
#[derive(Message, Debug, Clone, Copy)]
pub struct PlayerCaught;

/// Emitted to (re)build the arena for a fresh round.
#[derive(Message, Debug, Clone, Copy)]
pub struct StartRound;

/// Deterministic per-round seed (splitmix64 of the round number + a fixed base)
/// so round N always draws the same layout, but each round differs.
#[must_use]
pub fn round_seed(round_number: u32) -> u64 {
    let mut z = (u64::from(round_number)).wrapping_add(0x00C0_FFEE_1234_5678);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Number of starting guards for a given round (scales, then caps).
#[must_use]
pub fn guard_count(round_number: u32) -> u32 {
    (BASE_GUARDS + round_number).min(MAX_START_GUARDS)
}

/// The gold banked when a round ends: the whole carried haul on escape, nothing
/// if caught (§10.3 push-your-luck). Pure — the ink port diffs against this.
#[must_use]
pub fn banked_on_exit(carried: u32, escaped: bool) -> u32 {
    if escaped { carried } else { 0 }
}

/// Rebuild the arena on a [`StartRound`] message: clear round-scoped entities,
/// generate + instantiate a fresh seeded layout, reset the player and world
/// policy.
#[allow(clippy::too_many_arguments)]
pub fn start_round_system(
    mut commands: Commands,
    mut start: MessageReader<StartRound>,
    scoped: Query<Entity, With<RoundScoped>>,
    mut player: Query<&mut Transform, With<Player>>,
    mut alarm: ResMut<Alarm>,
    mut spawner: ResMut<ReinforcementSpawner>,
    mut round: ResMut<Round>,
    mut layout_res: ResMut<CurrentLayout>,
    mut nav: ResMut<NavGraph>,
) {
    // Only act once even if several messages arrived.
    if start.read().count() == 0 {
        return;
    }

    for entity in &scoped {
        commands.entity(entity).despawn();
    }

    round.seed = round_seed(round.number);
    let layout = generate(round.seed);

    if let Ok(mut tf) = player.single_mut() {
        tf.translation = layout.player_start.extend(1.0);
    }

    alarm.reset();
    *spawner = ReinforcementSpawner::default();
    spawner.entry = layout.barracks.first().copied();
    round.cameras_disabled = 0;
    round.carried = 0;
    round.ended = false;

    spawn_layout_walls(&mut commands, &layout);
    spawn_doors_from_layout(&mut commands, &layout);
    spawn_cameras_from_layout(&mut commands, &layout);
    spawn_gold_from_layout(&mut commands, &layout);
    spawn_alarm_panels(&mut commands, &layout);
    spawn_round_guards_from_layout(&mut commands, &layout, guard_count(round.number));

    // Build the guard navigation graph for this layout (#1044).
    nav.0 = Some(RoomGraph::from_layout(&layout));

    layout_res.0 = Some(layout);
}

/// Detect round-ending conditions while playing. Edge-triggered via
/// `round.ended` so the reward is granted exactly once (#1024).
#[allow(clippy::too_many_arguments)]
pub fn round_outcome_system(
    mut caught: MessageReader<PlayerCaught>,
    player: Query<&Transform, With<Player>>,
    exit: Query<&Transform, (With<Exit>, Without<Player>)>,
    mut round: ResMut<Round>,
    mut outcome: ResMut<LastOutcome>,
    mut next: ResMut<NextState<Phase>>,
) {
    // Already ended this round — drain the caught queue and do nothing else, so
    // the escape/capture path cannot fire twice before the state transition
    // takes effect (#1024).
    if round.ended {
        caught.clear();
        return;
    }

    // --- Escape check ---
    let player_pos = player.single().ok().map(|tf| tf.translation.truncate());
    if let (Some(pp), Ok(exit_tf)) = (player_pos, exit.single())
        && pp.distance(exit_tf.translation.truncate()) < EXIT_RADIUS
    {
        end_round(&mut round, &mut outcome, &mut next, true);
        return;
    }

    // --- Capture check ---
    if caught.read().count() > 0 {
        end_round(&mut round, &mut outcome, &mut next, false);
    }
}

fn end_round(
    round: &mut Round,
    outcome: &mut LastOutcome,
    next: &mut NextState<Phase>,
    escaped: bool,
) {
    let banked = banked_on_exit(round.carried, escaped);
    round.banked += banked;
    round.carried = 0;
    round.ended = true;
    outcome.escaped = escaped;
    outcome.reward = banked;
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
    fn banking_pays_only_on_escape() {
        assert_eq!(banked_on_exit(50, true), 50);
        assert_eq!(banked_on_exit(50, false), 0);
        assert_eq!(banked_on_exit(0, true), 0);
    }

    #[test]
    fn round_seed_is_deterministic_but_varies() {
        assert_eq!(round_seed(1), round_seed(1));
        assert_ne!(round_seed(1), round_seed(2));
    }

    /// #1024: the escape check runs every frame, so reaching the exit must
    /// grant exactly one reward even if the outcome system ticks several times
    /// before the Playing→Shop transition applies. This drives the real system
    /// wiring (not just the pure `banked_on_exit`).
    #[test]
    fn reaching_exit_grants_reward_exactly_once() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<Phase>()
            .init_resource::<LastOutcome>()
            .add_message::<PlayerCaught>()
            .add_systems(Update, round_outcome_system);

        // Player sitting on the exit, carrying 40 gold.
        app.world_mut().insert_resource(Round {
            number: 1,
            banked: 0,
            carried: 40,
            cameras_disabled: 0,
            seed: 0,
            ended: false,
        });
        app.world_mut()
            .spawn((Transform::from_xyz(0.0, 0.0, 0.0), Player));
        app.world_mut()
            .spawn((Transform::from_xyz(0.0, 0.0, 0.0), Exit));

        // Tick the outcome system many times while the player stays on the exit.
        for _ in 0..10 {
            app.update();
        }

        let round = app.world().resource::<Round>();
        assert_eq!(round.banked, 40, "exactly one banking of the carried haul");
        assert_eq!(round.carried, 0, "carried haul consumed once");
        assert!(round.ended, "round latched as ended");
    }

    #[test]
    fn being_caught_banks_nothing() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<Phase>()
            .init_resource::<LastOutcome>()
            .add_message::<PlayerCaught>()
            .add_systems(Update, round_outcome_system);

        app.world_mut().insert_resource(Round {
            number: 1,
            banked: 100,
            carried: 55,
            cameras_disabled: 0,
            seed: 0,
            ended: false,
        });
        // Player far from any exit.
        app.world_mut()
            .spawn((Transform::from_xyz(9999.0, 9999.0, 0.0), Player));
        app.world_mut()
            .spawn((Transform::from_xyz(0.0, 0.0, 0.0), Exit));

        // Fire two caught messages across frames; only one round-end may apply.
        app.world_mut()
            .resource_mut::<Messages<PlayerCaught>>()
            .write(PlayerCaught);
        app.update();
        app.world_mut()
            .resource_mut::<Messages<PlayerCaught>>()
            .write(PlayerCaught);
        app.update();

        let round = app.world().resource::<Round>();
        assert_eq!(round.banked, 100, "no gold banked when caught");
        assert_eq!(round.carried, 0, "carried haul lost");
        assert!(round.ended);
    }
}
