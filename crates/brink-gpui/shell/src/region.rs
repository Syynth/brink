//! The region model — `docs/gpui-studio-spec.md` §4.1 (ruled 2026-09-04,
//! "The native studio's region model drops the bottom rail").
//!
//! There is no bottom rail. The left and right rails are each split into an
//! upper and a lower group, and **the lower groups address the bottom
//! dock**. With the editor center and the status bar that is five surfaces
//! and one placement rule.
//!
//! The point of the rule is that a tool window's home is declared exactly
//! once. With a bottom rail it was declared twice — which rail, and which
//! dock — and the two were free to disagree. Here the rail slot *is* the
//! home, and re-homing a tool window is one operation.
//!
//! This module is deliberately pure: the mapping is the ruled part, so it is
//! a function over plain enums that can be tested without a window.

use gpui_component::dock::DockPlacement;

/// Which rail a tool window's button lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RailEdge {
    Left,
    Right,
}

/// Which half of that rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RailGroup {
    /// Opens in the rail's own side dock.
    Upper,
    /// Opens in the bottom dock — this is what replaces the bottom rail.
    Lower,
}

/// A tool window's home: the one place it is declared.
///
/// Ordered so that iterating a rail's slots yields upper before lower, which
/// is the order they are drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RailSlot {
    pub edge: RailEdge,
    pub group: RailGroup,
}

impl RailSlot {
    pub const LEFT_UPPER: Self = Self::new(RailEdge::Left, RailGroup::Upper);
    pub const LEFT_LOWER: Self = Self::new(RailEdge::Left, RailGroup::Lower);
    pub const RIGHT_UPPER: Self = Self::new(RailEdge::Right, RailGroup::Upper);
    pub const RIGHT_LOWER: Self = Self::new(RailEdge::Right, RailGroup::Lower);

    /// Every slot, in draw order: each rail top-to-bottom, left rail first.
    pub const ALL: [Self; 4] = [
        Self::LEFT_UPPER,
        Self::LEFT_LOWER,
        Self::RIGHT_UPPER,
        Self::RIGHT_LOWER,
    ];

    #[must_use]
    pub const fn new(edge: RailEdge, group: RailGroup) -> Self {
        Self { edge, group }
    }

    /// The dock this slot opens into — **the ruled mapping**.
    ///
    /// Both lower groups address the same bottom dock; which side of it they
    /// take is [`Self::bottom_side`].
    #[must_use]
    pub const fn dock(self) -> DockPlacement {
        match (self.edge, self.group) {
            (_, RailGroup::Lower) => DockPlacement::Bottom,
            (RailEdge::Left, RailGroup::Upper) => DockPlacement::Left,
            (RailEdge::Right, RailGroup::Upper) => DockPlacement::Right,
        }
    }

    /// Which side of the bottom dock a lower-group tool window takes, or
    /// `None` for an upper-group one, which does not share its dock.
    ///
    /// The bottom dock is a horizontal split of two tab groups. When only
    /// one side has anything open the split collapses and that side takes
    /// the full width — `normalize`'s rule 2 replaces a one-child `Split`
    /// with that child, which keeps its own `NodeId`, so the panel entity is
    /// not torn down and rebuilt across the collapse.
    #[must_use]
    pub const fn bottom_side(self) -> Option<RailEdge> {
        match self.group {
            RailGroup::Lower => Some(self.edge),
            RailGroup::Upper => None,
        }
    }

    /// A stable key for layout persistence. Keyed on the slot rather than
    /// the dock, because the slot is what survives a re-home.
    #[must_use]
    pub const fn persistence_key(self) -> &'static str {
        match (self.edge, self.group) {
            (RailEdge::Left, RailGroup::Upper) => "left.upper",
            (RailEdge::Left, RailGroup::Lower) => "left.lower",
            (RailEdge::Right, RailGroup::Upper) => "right.upper",
            (RailEdge::Right, RailGroup::Lower) => "right.lower",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lower_groups_address_the_bottom_dock() {
        // This is the ruling. If it ever reads Left/Right here, the bottom
        // rail has effectively come back.
        assert_eq!(RailSlot::LEFT_LOWER.dock(), DockPlacement::Bottom);
        assert_eq!(RailSlot::RIGHT_LOWER.dock(), DockPlacement::Bottom);
    }

    #[test]
    fn the_upper_groups_address_their_own_side() {
        assert_eq!(RailSlot::LEFT_UPPER.dock(), DockPlacement::Left);
        assert_eq!(RailSlot::RIGHT_UPPER.dock(), DockPlacement::Right);
    }

    #[test]
    fn only_lower_slots_take_a_side_of_the_bottom_dock() {
        assert_eq!(RailSlot::LEFT_LOWER.bottom_side(), Some(RailEdge::Left));
        assert_eq!(RailSlot::RIGHT_LOWER.bottom_side(), Some(RailEdge::Right));
        assert_eq!(RailSlot::LEFT_UPPER.bottom_side(), None);
        assert_eq!(RailSlot::RIGHT_UPPER.bottom_side(), None);
    }

    #[test]
    fn there_are_four_slots_and_three_docks() {
        assert_eq!(RailSlot::ALL.len(), 4);
        let mut docks: Vec<DockPlacement> = RailSlot::ALL.iter().map(|s| s.dock()).collect();
        docks.sort_by_key(|d| format!("{d:?}"));
        docks.dedup();
        assert_eq!(
            docks.len(),
            3,
            "four slots must address exactly three docks"
        );
        assert!(
            !docks.contains(&DockPlacement::Center),
            "no rail slot addresses the editor center"
        );
    }

    #[test]
    fn slots_iterate_in_draw_order() {
        // Each rail top-to-bottom, left rail first — the order the buttons
        // appear, so a caller can lay a rail out by filtering ALL.
        assert_eq!(
            RailSlot::ALL,
            [
                RailSlot::LEFT_UPPER,
                RailSlot::LEFT_LOWER,
                RailSlot::RIGHT_UPPER,
                RailSlot::RIGHT_LOWER,
            ]
        );
        let left: Vec<_> = RailSlot::ALL
            .iter()
            .filter(|s| s.edge == RailEdge::Left)
            .collect();
        assert_eq!(left, [&RailSlot::LEFT_UPPER, &RailSlot::LEFT_LOWER]);
    }

    #[test]
    fn persistence_keys_are_distinct_and_slot_shaped() {
        let mut keys: Vec<&str> = RailSlot::ALL.iter().map(|s| s.persistence_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 4, "a collision would merge two homes on reload");
        // Keyed on the slot, not the dock: both lower slots share a dock but
        // must persist separately.
        assert_ne!(
            RailSlot::LEFT_LOWER.persistence_key(),
            RailSlot::RIGHT_LOWER.persistence_key()
        );
    }
}
