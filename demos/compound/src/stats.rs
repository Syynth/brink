//! Player stats and the shop loadout.
//!
//! v2 makes every shop item a **sidegrade, not an upgrade** (plan §10.3): each
//! trades one axis for another, so buying is a decision about *how* you want to
//! play rather than raw power creep. Rounds escalate the compound, never the
//! player. Two consumables (thrown coins, one smoke) round out the kit.
//!
//! Migration note: the item table is deliberately data-shaped — a flat list of
//! structs and pure `apply` logic — so the Phase-1 ink `STRUCT` port is a
//! near-mechanical translation.

use bevy::prelude::*;

/// Per-player tunable stats. Lives on the player entity so a future ink script
/// can own it as `#@local` state.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerStats {
    /// Walking speed in world units per second (silent).
    pub move_speed: f32,
    /// Multiplier applied to `move_speed` while running (Shift). Running is
    /// faster but noisy.
    pub run_mult: f32,
    /// Extra footstep-noise radius added when running. Boots raise it; muffled
    /// soles push it negative (quieter than default).
    pub noise_bonus: f32,
    /// How much sneaking shrinks every enemy vision cone's effective range, in
    /// world units. Bigger = harder to see.
    pub stealth_radius: f32,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            move_speed: 240.0,
            run_mult: 1.45,
            noise_bonus: 0.0,
            stealth_radius: 0.0,
        }
    }
}

impl PlayerStats {
    /// The player's current running speed.
    #[must_use]
    pub fn run_speed(&self) -> f32 {
        self.move_speed * self.run_mult
    }
}

/// Persistent, cross-round purchases. Resets only on a full game reset.
#[derive(Resource, Debug, Clone)]
pub struct Loadout {
    /// Multiplier applied to every enemy vision cone's range. The sneak cloak
    /// drives this below 1.0.
    pub enemy_vision_scale: f32,
    /// Thrown-coin ammo (lures).
    pub coins: u32,
    /// Smoke charges (each breaks one chase).
    pub smokes: u32,
    /// Which one-shot sidegrades have already been bought.
    pub owned: Vec<ItemId>,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            enemy_vision_scale: 1.0,
            coins: 3,
            smokes: 0,
            owned: Vec::new(),
        }
    }
}

/// Stable identity for a shop item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemId {
    SpeedBoots,
    SneakCloak,
    MuffledSoles,
    Coins,
    Smoke,
}

/// A purchasable sidegrade or consumable. STRUCT-shaped on purpose.
#[derive(Debug, Clone, Copy)]
pub struct ShopItem {
    pub id: ItemId,
    pub name: &'static str,
    pub price: u32,
    pub blurb: &'static str,
    /// One-shot sidegrades can be owned at most once; consumables restock.
    pub once_only: bool,
}

/// The full catalogue: three sidegrades + two consumables.
pub const CATALOGUE: [ShopItem; 5] = [
    ShopItem {
        id: ItemId::SpeedBoots,
        name: "Speed Boots",
        price: 45,
        blurb: "+move speed, but louder",
        once_only: true,
    },
    ShopItem {
        id: ItemId::SneakCloak,
        name: "Sneak Cloak",
        price: 55,
        blurb: "-enemy vision, -run speed",
        once_only: true,
    },
    ShopItem {
        id: ItemId::MuffledSoles,
        name: "Muffled Soles",
        price: 45,
        blurb: "-noise, -top speed",
        once_only: true,
    },
    ShopItem {
        id: ItemId::Coins,
        name: "Coins x3",
        price: 20,
        blurb: "throwable lures (+3)",
        once_only: false,
    },
    ShopItem {
        id: ItemId::Smoke,
        name: "Smoke Bomb",
        price: 40,
        blurb: "breaks a chase (+1)",
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

/// Apply a purchased item's effect. Returns the gold cost to deduct, or `None`
/// if the purchase was not valid.
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
        ItemId::SpeedBoots => {
            stats.move_speed += 45.0;
            stats.noise_bonus += 70.0;
        }
        ItemId::SneakCloak => {
            loadout.enemy_vision_scale *= 0.78;
            stats.run_mult = (stats.run_mult - 0.25).max(1.0);
        }
        ItemId::MuffledSoles => {
            stats.noise_bonus -= 80.0;
            stats.move_speed -= 35.0;
        }
        ItemId::Coins => loadout.coins += 3,
        ItemId::Smoke => loadout.smokes += 1,
    }
    if item.once_only {
        loadout.owned.push(item.id);
    }
    Some(item.price)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: ItemId) -> ShopItem {
        *CATALOGUE.iter().find(|i| i.id == id).expect("item exists")
    }

    #[test]
    fn cannot_buy_without_gold() {
        let loadout = Loadout::default();
        let boots = item(ItemId::SpeedBoots);
        assert!(!can_buy(&boots, boots.price - 1, &loadout));
        assert!(can_buy(&boots, boots.price, &loadout));
    }

    #[test]
    fn once_only_items_cannot_be_rebought() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let boots = item(ItemId::SpeedBoots);
        assert_eq!(
            apply_purchase(&boots, 1000, &mut stats, &mut loadout),
            Some(boots.price)
        );
        assert!(!can_buy(&boots, 1000, &loadout));
        assert_eq!(apply_purchase(&boots, 1000, &mut stats, &mut loadout), None);
    }

    #[test]
    fn consumables_restock() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let base = loadout.coins;
        apply_purchase(&item(ItemId::Coins), 1000, &mut stats, &mut loadout);
        apply_purchase(&item(ItemId::Coins), 1000, &mut stats, &mut loadout);
        assert_eq!(loadout.coins, base + 6);
    }

    #[test]
    fn boots_are_a_sidegrade_faster_but_louder() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let (s0, n0) = (stats.move_speed, stats.noise_bonus);
        apply_purchase(&item(ItemId::SpeedBoots), 1000, &mut stats, &mut loadout);
        assert!(stats.move_speed > s0, "faster");
        assert!(stats.noise_bonus > n0, "but louder");
    }

    #[test]
    fn cloak_trades_stealth_for_run_speed() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let r0 = stats.run_mult;
        apply_purchase(&item(ItemId::SneakCloak), 1000, &mut stats, &mut loadout);
        assert!(loadout.enemy_vision_scale < 1.0, "harder to see");
        assert!(stats.run_mult < r0, "but slower running");
    }

    #[test]
    fn muffled_soles_trade_noise_for_top_speed() {
        let mut stats = PlayerStats::default();
        let mut loadout = Loadout::default();
        let (s0, n0) = (stats.move_speed, stats.noise_bonus);
        apply_purchase(&item(ItemId::MuffledSoles), 1000, &mut stats, &mut loadout);
        assert!(stats.noise_bonus < n0, "quieter");
        assert!(stats.move_speed < s0, "but slower");
    }
}
