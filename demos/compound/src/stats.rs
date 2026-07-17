//! Player stats and the shop loadout.
//!
//! Migration note: in Phase 1 the shop economy and item effects are a prime
//! candidate for ink `STRUCT` value tables (`STRUCT Item = #{name, price, ...}`).
//! Everything here is deliberately data-shaped — a flat list of items and pure
//! `apply` logic — so the ink port is a near-mechanical translation.

use bevy::prelude::*;

/// Per-player tunable stats. Lives on the player entity so a future ink script
/// can own it as `#@local` state.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerStats {
    /// Movement speed in world units per second.
    pub move_speed: f32,
    /// How much the player's sneaking shrinks every enemy vision cone's
    /// effective range, in world units. Bigger = harder to see.
    pub stealth_radius: f32,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            move_speed: 260.0,
            stealth_radius: 0.0,
        }
    }
}

/// Persistent, cross-round purchases. Resets only on a full game reset.
#[derive(Resource, Debug, Clone)]
pub struct Loadout {
    /// Multiplier applied to every enemy vision cone's range. The sneak cloak
    /// drives this below 1.0.
    pub enemy_vision_scale: f32,
    /// Number of medkits held. Each one absorbs one capture instead of ending
    /// the round.
    pub medkits: u32,
    /// Which one-shot items have already been bought (so they can't be bought
    /// twice).
    pub owned: Vec<ItemId>,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            enemy_vision_scale: 1.0,
            medkits: 0,
            owned: Vec::new(),
        }
    }
}

/// Stable identity for a shop item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemId {
    SpeedBoots,
    SneakCloak,
    Medkit,
}

/// A purchasable upgrade. STRUCT-shaped on purpose (see module docs).
#[derive(Debug, Clone, Copy)]
pub struct ShopItem {
    pub id: ItemId,
    pub name: &'static str,
    pub price: u32,
    pub blurb: &'static str,
    /// One-shot items can be owned at most once; sticky items restock forever.
    pub once_only: bool,
}

/// The full catalogue. ~3 items per the v1 content budget.
pub const CATALOGUE: [ShopItem; 3] = [
    ShopItem {
        id: ItemId::SpeedBoots,
        name: "Speed Boots",
        price: 40,
        blurb: "+60 move speed",
        once_only: true,
    },
    ShopItem {
        id: ItemId::SneakCloak,
        name: "Sneak Cloak",
        price: 60,
        blurb: "enemy vision cones -25%",
        once_only: true,
    },
    ShopItem {
        id: ItemId::Medkit,
        name: "Medkit",
        price: 30,
        blurb: "absorbs one capture",
        once_only: false,
    },
];

/// Whether `item` can currently be purchased with `gold` and `loadout`.
pub fn can_buy(item: &ShopItem, gold: u32, loadout: &Loadout) -> bool {
    if gold < item.price {
        return false;
    }
    if item.once_only && loadout.owned.contains(&item.id) {
        return false;
    }
    true
}

/// Apply a purchased item's effect. Returns the gold cost that should be
/// deducted, or `None` if the purchase was not valid.
pub fn apply_purchase(
    item: &ShopItem,
    gold: u32,
    stats: &mut PlayerStats,
    loadout: &mut Loadout,
) -> Option<u32> {
    if !can_buy(item, gold, loadout) {
        return None;
    }
    match item.id {
        ItemId::SpeedBoots => stats.move_speed += 60.0,
        ItemId::SneakCloak => loadout.enemy_vision_scale *= 0.75,
        ItemId::Medkit => loadout.medkits += 1,
    }
    if item.once_only {
        loadout.owned.push(item.id);
    }
    Some(item.price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cannot_buy_without_gold() {
        let loadout = Loadout::default();
        let boots = CATALOGUE[0];
        assert!(!can_buy(&boots, boots.price - 1, &loadout));
        assert!(can_buy(&boots, boots.price, &loadout));
    }

    #[test]
    fn once_only_items_cannot_be_rebought() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let boots = CATALOGUE[0];
        assert_eq!(
            apply_purchase(&boots, 1000, &mut stats, &mut loadout),
            Some(boots.price)
        );
        assert!(!can_buy(&boots, 1000, &loadout));
        assert_eq!(apply_purchase(&boots, 1000, &mut stats, &mut loadout), None);
    }

    #[test]
    fn sticky_medkit_restocks() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let medkit = CATALOGUE[2];
        apply_purchase(&medkit, 1000, &mut stats, &mut loadout);
        apply_purchase(&medkit, 1000, &mut stats, &mut loadout);
        assert_eq!(loadout.medkits, 2);
    }

    #[test]
    fn effects_change_stats() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let base_speed = stats.move_speed;
        apply_purchase(&CATALOGUE[0], 1000, &mut stats, &mut loadout);
        assert!(stats.move_speed > base_speed);
        apply_purchase(&CATALOGUE[1], 1000, &mut stats, &mut loadout);
        assert!(loadout.enemy_vision_scale < 1.0);
    }
}
