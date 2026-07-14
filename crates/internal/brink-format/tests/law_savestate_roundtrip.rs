//! Law: **`SaveState` round-trip is identity** — issue #672 workstream B
//! item 2 ("serialization round-trips … `SaveState` round-trip incl. fn
//! values + structs + nested collections").
//!
//! [`SaveState`] (`docs/external-binding-foundation.md`) is the durable,
//! name-keyed save format `Story::save_state`/`load_state` produce and
//! reconcile; its `globals` map holds arbitrary [`Value`]s. Per
//! `docs/value-model-spec.md` §1 ("every script is pausable, saveable,
//! replayable … downstream of one property: script state is serializable
//! data, always") and §10 (resumability audit: "state-only saves stay
//! name-keyed trees"), a save is only trustworthy if every value shape that
//! can land in a global — scalars, function values (§11), and nested
//! collections/structs (§4) — survives a save/load cycle byte-for-byte.
//! `crates/internal/brink-format/src/save.rs`'s own unit test proves one
//! hand-picked case; this suite generalizes it across the full [`Value`]
//! variant space with `proptest`.
//!
//! The persistence format exercised here is `serde_json`, the same backend
//! `save.rs`'s existing `save_state_round_trips_collection_globals` test
//! uses — `SaveState` derives `Serialize`/`Deserialize` with no format
//! opinion of its own, so any serde backend is representative; JSON is the
//! one already in this crate's dev-dependencies.
//!
//! Deterministic seeds (house determinism rule): `ProptestConfig::with_cases`
//! is fixed and no `PROPTEST_*` env var is read, so failures reproduce
//! identically across runs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod law_support;

use std::collections::BTreeMap;

use brink_format::{SAVE_FORMAT_VERSION, SaveState, Value, VisitEntry};
use law_support::{arb_def_id, arb_value_full};
use proptest::prelude::*;

fn arb_visit_entry() -> impl Strategy<Value = VisitEntry> {
    (
        arb_def_id(),
        prop::option::of("[a-z][a-z0-9_.]{0,15}".prop_map(String::from)),
        any::<u32>(),
    )
        .prop_map(|(id, path, count)| VisitEntry { id, path, count })
}

/// `globals` keyed by author-chosen name — any string is a legal global
/// name, so no charset restriction here (unlike `MapKey`/inkt string
/// generators elsewhere, which dodge format-specific escaping edge cases).
fn arb_globals() -> impl Strategy<Value = BTreeMap<String, Value>> {
    prop::collection::vec(("[a-zA-Z_][a-zA-Z0-9_]{0,12}", arb_value_full()), 0..6).prop_map(
        |entries| {
            let mut globals = BTreeMap::new();
            for (name, value) in entries {
                globals.insert(name, value);
            }
            globals
        },
    )
}

fn arb_save_state() -> impl Strategy<Value = SaveState> {
    (
        arb_globals(),
        prop::collection::vec(arb_visit_entry(), 0..6),
        prop::collection::vec(arb_visit_entry(), 0..6),
        any::<u32>(),
        any::<i32>(),
        any::<i32>(),
    )
        .prop_map(
            |(globals, visits, turns, turn_index, rng_seed, previous_random)| SaveState {
                version: SAVE_FORMAT_VERSION,
                globals,
                visits,
                turns,
                turn_index,
                rng_seed,
                previous_random,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `save -> serde_json -> load` is identity for arbitrary `SaveState`
    /// values, including globals holding function values (`FnRef`/
    /// `Closure`), structs (`Record`), and collections nested inside each
    /// other (`Array`-of-`Map`-of-`Record`, etc).
    #[test]
    fn savestate_json_roundtrip_is_identity(save in arb_save_state()) {
        let json = serde_json::to_string(&save).expect("serialize SaveState");
        let recovered: SaveState = serde_json::from_str(&json).expect("deserialize SaveState");
        prop_assert_eq!(save, recovered);
    }

    /// Same law restricted to a single global holding a deeply nested
    /// collection value — the shape the issue calls out by name ("nested
    /// collections") — so a shrink failure isolates to the value tree
    /// itself rather than the surrounding `SaveState` scaffolding.
    #[test]
    fn savestate_json_roundtrip_preserves_nested_value(
        name in "[a-zA-Z_][a-zA-Z0-9_]{0,12}",
        value in arb_value_full(),
    ) {
        let mut globals = BTreeMap::new();
        globals.insert(name.clone(), value.clone());
        let save = SaveState {
            version: SAVE_FORMAT_VERSION,
            globals,
            visits: Vec::new(),
            turns: Vec::new(),
            turn_index: 0,
            rng_seed: 0,
            previous_random: 0,
        };

        let json = serde_json::to_string(&save).expect("serialize SaveState");
        let recovered: SaveState = serde_json::from_str(&json).expect("deserialize SaveState");
        prop_assert_eq!(recovered.globals.get(&name), Some(&value));
    }
}
