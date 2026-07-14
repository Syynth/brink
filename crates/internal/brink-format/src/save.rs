//! Persistent, name-keyed save state for a story's game state.
//!
//! [`SaveState`] is the **durable** save format: globals, visit/turn counts,
//! turn index, and RNG, keyed by stable identities (variable name; scope
//! [`DefinitionId`]). It captures *game state only* — not execution position
//! (call stack / PC), which can't be made version-tolerant — so a save
//! survives a story recompile/patch as long as the relevant names/paths are
//! unchanged. The runtime (`Story::save_state` / `load_state`) produces and
//! reconciles it. See `docs/external-binding-foundation.md`.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{DefinitionId, Value};

/// Current [`SaveState`] format version. Bump when the *format* changes
/// (independent of the story's own content); `version` lets a loader migrate.
pub const SAVE_FORMAT_VERSION: u16 = 1;

/// A persistent, name-keyed snapshot of a story's game state.
///
/// Globals are keyed by variable name; visit/turn counts by scope
/// [`DefinitionId`] (which serializes as a stable `"$tt_hash"` string), with an
/// advisory author path attached when the scope is named.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SaveState {
    /// Save-format version (see [`SAVE_FORMAT_VERSION`]).
    pub version: u16,
    /// Global variables by name. `BTreeMap` for deterministic serialization.
    pub globals: BTreeMap<String, Value>,
    /// Each saved global's compiled `DefinitionId` at save time, keyed by
    /// the same name as [`Self::globals`] (M-3 rehydration miss-path lookup,
    /// `docs/modules-spec.md` §5). A VAR/CONST/LIST living in a **declared**
    /// module hashes its identity as `(module, name)`, so a bare name alone
    /// can't reconstruct the id a `#@was` alias-table entry was compiled
    /// against — this is the "module qualifier" the miss path needs,
    /// round-tripped as the id itself rather than the module name, so no
    /// hashing scheme has to be re-derived at load time. Consulted only when
    /// a saved global's name no longer matches any current global slot;
    /// absent entries (older saves predating this field) simply fall back
    /// to the pre-M-3 unknown-global report, same tolerant-of-patches
    /// behavior as before. `#[serde(default)]` so an older save missing
    /// this field entirely still deserializes.
    #[serde(default)]
    pub global_ids: BTreeMap<String, DefinitionId>,
    /// Visit counts by scope id, sorted by id for deterministic output.
    pub visits: Vec<VisitEntry>,
    /// Turn-since counts by scope id, sorted by id.
    pub turns: Vec<VisitEntry>,
    /// Global turn index.
    pub turn_index: u32,
    /// RNG seed.
    pub rng_seed: i32,
    /// Last drawn random value (so the RNG sequence resumes correctly).
    pub previous_random: i32,
}

/// One visit/turn-count entry: a scope id and its count, plus (when the scope
/// is a named knot/stitch) an advisory author path for human inspection. The
/// `id` is the load key; `path` is cosmetic.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VisitEntry {
    /// The counted scope's definition id (load key).
    pub id: DefinitionId,
    /// Author path for a named scope, e.g. `"forest.clearing"`. Absent for
    /// anonymous counted containers (gathers, choice points).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The count.
    pub count: u32,
}

/// What `Story::load_state` couldn't apply, so a host can surface it rather
/// than have data silently vanish. Globals whose name no longer exists are
/// **dropped** (no slot to hold them) and reported here. Visit/turn counts are
/// never dropped — counts for scopes the current program lacks are retained
/// harmlessly (unused until/unless the scope returns), so they aren't reported
/// *except* when the miss-path alias lookup (M-3, docs/modules-spec.md §5)
/// still can't place them — see `unresolved_renames`.
#[derive(Default, Clone, Debug, PartialEq, Serialize)]
pub struct LoadReport {
    /// Saved global names with no matching global in the current program.
    pub unknown_globals: Vec<String>,
    /// M-3 rehydration miss-path teaching messages (docs/modules-spec.md
    /// §5): a saved fn token, divert value, or visit/turn-count key that
    /// didn't match the current program even after consulting the compiled
    /// `#@was` alias table. Only populated for a program that actually
    /// carries alias-table entries (i.e. uses `#@was` somewhere) — an
    /// ordinary content edit with no rename directive stays silent, same
    /// as before M-3.
    pub unresolved_renames: Vec<String>,
}

impl LoadReport {
    /// Whether the load applied cleanly (nothing dropped, nothing left
    /// unresolved after the rename miss-path lookup).
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unknown_globals.is_empty() && self.unresolved_renames.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{MapKey, OrderedMap};

    /// A `SaveState` whose globals hold collection values must round-trip
    /// through serde as full trees — no lossy fold to null (T1a-3 / #525). The
    /// `globals` map is a `BTreeMap` (name-keyed, deterministic order), and
    /// each `Value` serializes through its own derived tree encoding, so a
    /// map-of-array global survives a save/load byte-for-byte.
    #[test]
    fn save_state_round_trips_collection_globals() {
        let inventory: OrderedMap = [
            (
                MapKey::from("weapons"),
                Value::array(vec![Value::from("sword"), Value::from("bow")]),
            ),
            (MapKey::from("gold"), Value::Int(42)),
        ]
        .into_iter()
        .collect();

        let mut globals = BTreeMap::new();
        globals.insert(String::from("inventory"), Value::map(inventory));
        globals.insert(
            String::from("scores"),
            Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        );

        let save = SaveState {
            version: SAVE_FORMAT_VERSION,
            globals,
            global_ids: BTreeMap::new(),
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 7,
            previous_random: 0,
        };

        let json = serde_json::to_string(&save).expect("serialize save");
        let back: SaveState = serde_json::from_str(&json).expect("deserialize save");
        assert_eq!(back, save);
    }
}
