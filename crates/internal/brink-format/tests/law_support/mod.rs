//! Shared `proptest` strategies for the issue #672 workstream B ("laws")
//! test suites in this crate. A `tests/<name>/mod.rs` submodule (rather than
//! a top-level `tests/<name>.rs` file) so cargo does not treat it as its own
//! test binary — each law file does `mod law_support;` to pull it in.
//!
//! `arb_value_full` covers every [`Value`] variant, unlike the
//! writer/reader-specific `arb_value` helpers in `proptest_inkb.rs` /
//! `proptest_inkt.rs`, which deliberately restrict themselves to what their
//! wire format can round-trip (see those files' own doc comments). This
//! module's consumer ([`law_savestate_roundtrip`](super)) round-trips
//! through `serde_json`, which has no such restriction.

use brink_format::{
    ClosureEnvEntry, DefinitionId, DefinitionTag, MapKey, NameId, OrderedMap, ShapeId, Value,
};
use proptest::prelude::*;

pub fn arb_tag() -> impl Strategy<Value = DefinitionTag> {
    prop_oneof![
        Just(DefinitionTag::Address),
        Just(DefinitionTag::GlobalVar),
        Just(DefinitionTag::ListDef),
        Just(DefinitionTag::ListItem),
        Just(DefinitionTag::ExternalFn),
    ]
}

pub fn arb_def_id() -> impl Strategy<Value = DefinitionId> {
    (arb_tag(), any::<u64>()).prop_map(|(tag, hash)| DefinitionId::new(tag, hash))
}

pub fn arb_name_id() -> impl Strategy<Value = NameId> {
    any::<u16>().prop_map(NameId)
}

pub fn arb_map_key() -> impl Strategy<Value = MapKey> {
    prop_oneof![
        any::<i32>().prop_map(MapKey::Int),
        ".*".prop_map(|s: String| MapKey::Str(s.into())),
        any::<bool>().prop_map(MapKey::Bool),
    ]
}

/// A `Value::Closure` env entry: a `val` (`Int` snapshot) or `ref`
/// (`VariablePointer`) payload — the two shapes T1c produces
/// (`docs/value-model-spec.md` §11).
fn arb_closure_env_entry() -> impl Strategy<Value = ClosureEnvEntry> {
    (arb_name_id(), any::<bool>(), arb_def_id(), any::<i32>()).prop_map(
        |(name, is_ref, cell, n)| ClosureEnvEntry {
            name,
            is_ref,
            payload: if is_ref {
                Value::VariablePointer(cell)
            } else {
                Value::Int(n)
            },
        },
    )
}

fn arb_value_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        any::<f32>().prop_map(Value::Float),
        any::<bool>().prop_map(Value::Bool),
        ".*".prop_map(|s: String| Value::String(s.into())),
        arb_def_id().prop_map(Value::DivertTarget),
        arb_def_id().prop_map(Value::VariablePointer),
        Just(Value::Null),
        any::<u32>().prop_map(Value::FragmentRef),
        // Function values (value-model-spec §11): a zero-bound `FnRef` and a
        // `Closure` with a small bound-arg env.
        arb_def_id().prop_map(Value::FnRef),
        (
            arb_def_id(),
            prop::collection::vec(arb_closure_env_entry(), 0..3)
        )
            .prop_map(|(target, env)| Value::closure(target, env)),
        // Handle values (T1d, docs/t1d-spec.md §2): the #746 List-gap class
        // this generator is written to avoid — every `Value` variant, this
        // one included, must be reachable from `arb_value_full`.
        (arb_name_id(), any::<u64>()).prop_map(|(kind, id)| Value::handle(kind, id)),
    ]
}

/// Every [`Value`] variant, with `Array`/`Map`/`Record` (value-model-spec §4)
/// nested to a bounded depth (3 levels, up to 16 nodes, width 4) so
/// generated cases stay small and shrinkable.
pub fn arb_value_full() -> impl Strategy<Value = Value> {
    arb_value_leaf().prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::array),
            prop::collection::vec((arb_map_key(), inner.clone()), 0..4).prop_map(|entries| {
                let mut map = OrderedMap::new();
                for (key, value) in entries {
                    map.insert(key, value);
                }
                Value::map(map)
            }),
            (any::<u32>(), prop::collection::vec(inner, 0..4))
                .prop_map(|(shape, fields)| Value::record(ShapeId(shape), fields)),
        ]
    })
}
