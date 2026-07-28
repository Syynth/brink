//! Alias-aware scope rebinding for the translation workflow (#1442).
//!
//! `#@was(old_name)` compiles to an old→new [`AliasEntry`] table
//! (`docs/modules-spec.md` §5) that the *save* path already consults on its
//! miss path. The translation workflow historically did not: `compile-locale`
//! matched scope ids by string equality and `regenerate_lines` only ever saw
//! two `LinesJson`, so a **declared** rename still orphaned every translation
//! beneath it. This module is the shared index that makes both surfaces
//! alias-aware.
//!
//! Two lookup directions are needed because the two surfaces sit on opposite
//! sides of the rename:
//!
//! - `compile-locale` holds a *stale* translation (pre-rename ids) and a
//!   *fresh* base `.inkb` (post-rename ids), so it resolves **forward**
//!   ([`ScopeAliasIndex::current`]).
//! - `regenerate` walks the *fresh* export and looks translations up in the
//!   *stale* file, so it resolves **backward** ([`ScopeAliasIndex::previous`]).
//!
//! Alias chains are never followed, mirroring `Program::resolve_alias`: the
//! compiler always emits `old -> new` against the definition's *current* id,
//! never `old -> old2`.

use std::collections::BTreeMap;

use brink_format::{AliasEntry, DefinitionId};

/// A bidirectional view over a compiled `#@was` alias table, used to rebind
/// translation scopes whose `DefinitionId` moved because a definition (or an
/// ancestor of one) was renamed.
///
/// Built from `StoryData::alias_table` (regeneration) or from the base
/// `.inkb`'s `AliasTable` section (locale compilation). Empty — and therefore
/// inert — for every story that uses no `#@was`, which keeps the whole
/// pre-M-3 corpus byte-identical.
#[derive(Debug, Default, Clone)]
pub struct ScopeAliasIndex {
    /// `old -> new`, the direction the alias table is written in.
    current: BTreeMap<DefinitionId, DefinitionId>,
    /// `new -> [old, …]`, sorted ascending so a definition that absorbed two
    /// declared renames rebinds deterministically.
    previous: BTreeMap<DefinitionId, Vec<DefinitionId>>,
}

impl ScopeAliasIndex {
    /// Build an index from a compiled alias table.
    ///
    /// Self-edges (`old == new`) are dropped — they carry no information and
    /// would make a scope look rebound when it matched directly.
    #[must_use]
    pub fn new(entries: &[AliasEntry]) -> Self {
        let mut current = BTreeMap::new();
        let mut previous: BTreeMap<DefinitionId, Vec<DefinitionId>> = BTreeMap::new();
        for entry in entries {
            if entry.old == entry.new {
                continue;
            }
            current.insert(entry.old, entry.new);
            previous.entry(entry.new).or_default().push(entry.old);
        }
        // The caller's slice is sorted by `old` in practice (the linker sorts
        // it), but sorting here makes the backward direction deterministic for
        // any input order — including hand-built tables in tests.
        for olds in previous.values_mut() {
            olds.sort_unstable();
            olds.dedup();
        }
        Self { current, previous }
    }

    /// Whether this index carries no rebinding edges at all. When true, every
    /// lookup misses and both callers behave exactly as they did before
    /// alias-awareness.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// The definition's identity *after* the declared rename, given an
    /// identity a stale translation file may still carry.
    #[must_use]
    pub fn current(&self, old: DefinitionId) -> Option<DefinitionId> {
        self.current.get(&old).copied()
    }

    /// Every identity a stale translation file may carry for the definition
    /// that is now `new`, ascending. Empty when nothing was renamed onto it.
    #[must_use]
    pub fn previous(&self, new: DefinitionId) -> &[DefinitionId] {
        self.previous.get(&new).map_or(&[], Vec::as_slice)
    }
}

/// Parse a `lines.json` / `brink:scope-id` scope id (`"0x"` + hex) into a
/// [`DefinitionId`], returning `None` rather than an error.
///
/// The lenient form exists for regeneration, which must never fail on a
/// non-canonical id: an XLIFF file that predates the `brink:scope-id`
/// extension falls back to `<file id>`, a *display name*, which does not
/// parse. Such a scope simply does not participate in rebinding.
pub(crate) fn parse_scope_id_lenient(id: &str) -> Option<DefinitionId> {
    let hex = id.strip_prefix("0x")?;
    let raw = u64::from_str_radix(hex, 16).ok()?;
    DefinitionId::from_raw(raw)
}

/// Render a [`DefinitionId`] in the canonical scope-id spelling used by
/// `lines.json`, `brink:scope-id`, and `<unit id>` — the same
/// `0x{:016x}` form [`crate::export_lines`] emits.
pub(crate) fn format_scope_id(id: DefinitionId) -> String {
    format!("0x{:016x}", id.to_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::DefinitionTag;

    fn id(raw: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, raw)
    }

    #[test]
    fn empty_table_is_inert() {
        let index = ScopeAliasIndex::new(&[]);
        assert!(index.is_empty());
        assert_eq!(index.current(id(1)), None);
        assert!(index.previous(id(1)).is_empty());
    }

    #[test]
    fn resolves_both_directions() {
        let index = ScopeAliasIndex::new(&[AliasEntry {
            old: id(1),
            new: id(2),
        }]);
        assert_eq!(index.current(id(1)), Some(id(2)));
        assert_eq!(index.previous(id(2)), &[id(1)]);
        // The reverse of an unaliased id is still empty.
        assert!(index.previous(id(1)).is_empty());
        assert_eq!(index.current(id(2)), None);
    }

    #[test]
    fn self_edges_are_dropped() {
        let index = ScopeAliasIndex::new(&[AliasEntry {
            old: id(7),
            new: id(7),
        }]);
        assert!(index.is_empty());
    }

    #[test]
    fn backward_direction_is_sorted_regardless_of_input_order() {
        let unsorted = ScopeAliasIndex::new(&[
            AliasEntry {
                old: id(9),
                new: id(1),
            },
            AliasEntry {
                old: id(3),
                new: id(1),
            },
        ]);
        assert_eq!(unsorted.previous(id(1)), &[id(3), id(9)]);
    }

    #[test]
    fn chains_are_not_followed() {
        // `a -> b`, `b -> c`: resolving `a` yields `b`, never `c`.
        let index = ScopeAliasIndex::new(&[
            AliasEntry {
                old: id(1),
                new: id(2),
            },
            AliasEntry {
                old: id(2),
                new: id(3),
            },
        ]);
        assert_eq!(index.current(id(1)), Some(id(2)));
    }

    #[test]
    fn scope_id_round_trips() {
        let hex = format_scope_id(id(0xDEAD_BEEF));
        assert_eq!(hex, "0x01000000deadbeef");
        assert_eq!(parse_scope_id_lenient(&hex), Some(id(0xDEAD_BEEF)));
    }

    #[test]
    fn display_name_scope_id_does_not_parse() {
        assert_eq!(parse_scope_id_lenient("intro"), None);
        assert_eq!(parse_scope_id_lenient("0xnothex"), None);
    }
}
