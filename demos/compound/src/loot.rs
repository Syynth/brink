//! Gold — the greed side of the greed-vs-safety axis (plan §10.3).
//!
//! Recipes place gold pickups in dangerous rooms (Storage stashes, the Vault's
//! high-value hoard). The player collects them into a *carried* haul that is
//! only banked on exit — get caught and the unbanked gold is lost. This is the
//! push-your-luck tension the whole layout is built to create; the picking-up
//! is a trivial proximity system, the banking lives in [`crate::rounds`].

use bevy::prelude::*;

use crate::layout_gen::LayoutData;
use crate::rounds::{Round, RoundScoped};
use crate::world::Player;

const PICKUP_RADIUS: f32 = 26.0;
const GOLD_COLOR: Color = Color::srgb(0.95, 0.82, 0.25);
const VAULT_GOLD_COLOR: Color = Color::srgb(1.0, 0.55, 0.85);
const GOLD_HALF: Vec2 = Vec2::new(9.0, 9.0);
const VAULT_GOLD_HALF: Vec2 = Vec2::new(14.0, 14.0);

/// A collectible gold stash. Worth `value` (vault stashes are simply worth far
/// more, encoded in the value the recipe placed).
#[derive(Component, Debug)]
pub struct GoldPickup {
    pub value: u32,
}

/// Collect any gold the player is standing on, adding it to the carried haul.
pub fn gold_pickup_system(
    mut commands: Commands,
    mut round: ResMut<Round>,
    player: Query<&Transform, With<Player>>,
    pickups: Query<(Entity, &Transform, &GoldPickup)>,
) {
    let Ok(player_tf) = player.single() else {
        return;
    };
    let pp = player_tf.translation.truncate();
    for (entity, tf, gold) in &pickups {
        if tf.translation.truncate().distance(pp) < PICKUP_RADIUS {
            round.carried += gold.value;
            commands.entity(entity).despawn();
        }
    }
}

/// Spawn every gold pickup the layout placed.
pub fn spawn_gold_from_layout(commands: &mut Commands, layout: &LayoutData) {
    for g in &layout.gold {
        let (color, half) = if g.vault {
            (VAULT_GOLD_COLOR, VAULT_GOLD_HALF)
        } else {
            (GOLD_COLOR, GOLD_HALF)
        };
        commands.spawn((
            Sprite::from_color(color, half * 2.0),
            Transform::from_translation(g.pos.extend(0.6)),
            GoldPickup { value: g.value },
            RoundScoped,
        ));
    }
}
