//! Shared `proptest` strategies for the issue #672 workstream B ("laws")
//! test suites in this crate. A `tests/<name>/mod.rs` submodule (rather than
//! a top-level `tests/<name>.rs` file) so cargo does not treat it as its own
//! test binary — each law file does `mod law_support;` to pull it in.
//!
//! `arb_value_full` covers every [`Value`] variant except the runtime-only
//! `TempPointer` (which never round-trips through any wire format — it
//! collapses to `VAL_NULL` on write, see `law_transcript_roundtrip.rs`'s
//! doc), unlike the writer/reader-specific `arb_value` helpers in
//! `proptest_inkb.rs` / `proptest_inkt.rs`, which deliberately restrict
//! themselves to what their wire format can round-trip (see those files'
//! own doc comments). This module's consumers
//! ([`law_savestate_roundtrip`](super), [`law_deep_equality`](super))
//! round-trip/compare through paths with no such restriction.

// Each `tests/law_*.rs` binary compiles this module independently (`mod
// law_support;`) and dead-code analysis runs per binary — a helper only
// some consumers need (e.g. `arb_wake_policy`/`arb_suspended_flow`, used by
// `law_savestate_roundtrip` alone) reads as genuinely dead in every other
// binary that pulls this module in. Mirrors the identical `#![allow]` on
// `brink-test-harness/tests/law_support/mod.rs` for the same reason.
#![allow(dead_code)]

use brink_format::{
    ClosureEnvEntry, DefinitionId, DefinitionTag, ListValue, MapKey, NameId, OrderedMap,
    ProjSegment, SUSPENDED_FLOW_SECTION_VERSION, ShapeId, SuspendedFlow, Value, WakePolicy,
    WakeSource,
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

/// One `Value::Projection` path segment: `Index` (a leaf `i32`) or `Key`
/// (nests an arbitrary `Value` — a non-`Int` map key or a struct field name),
/// mirroring `proptest_inkt.rs`'s `arb_proj_segment`.
fn arb_proj_segment(
    inner: impl Strategy<Value = Value> + Clone,
) -> impl Strategy<Value = ProjSegment> {
    prop_oneof![
        any::<i32>().prop_map(ProjSegment::Index),
        inner.prop_map(ProjSegment::Key),
    ]
}

fn arb_value_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        any::<f32>().prop_map(Value::Float),
        any::<bool>().prop_map(Value::Bool),
        ".*".prop_map(|s: String| Value::String(s.into())),
        // Ink LIST values (value-model-spec §4): issue #746's named gap in
        // this generator, closed here so `arb_value_full` (and every law
        // suite built on it) reaches this variant.
        (
            prop::collection::vec(arb_def_id(), 0..3),
            prop::collection::vec(arb_def_id(), 0..3),
        )
            .prop_map(|(items, origins)| Value::List(ListValue { items, origins }.into())),
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
        // Handle values (T1d, docs/t1d-spec.md §2).
        (arb_name_id(), any::<u64>()).prop_map(|(kind, id)| Value::handle(kind, id)),
    ]
}

/// A [`WakePolicy`] (`docs/flow-suspension-spec.md` §2 point 4): a
/// `Condition`-sourced policy always carries a condition fn token; a
/// `Host`-sourced one never does (§3 — no compiled ink fn exists for a
/// host-driven wake source).
pub fn arb_wake_policy() -> impl Strategy<Value = WakePolicy> {
    (arb_def_id(), any::<bool>(), arb_def_id()).prop_map(|(site, is_condition, condition)| {
        if is_condition {
            WakePolicy {
                site,
                condition: Some(condition),
                source: WakeSource::Condition,
            }
        } else {
            WakePolicy {
                site,
                condition: None,
                source: WakeSource::Host,
            }
        }
    })
}

/// A [`SuspendedFlow`] (the `FlowFrame`, `docs/flow-suspension-spec.md` §2):
/// current container, a bounded tunnel-return stack, a name-keyed frame
/// record (an arbitrary [`Value`], typically a map), and a wake policy.
pub fn arb_suspended_flow() -> impl Strategy<Value = SuspendedFlow> {
    (
        arb_def_id(),
        prop::collection::vec(arb_def_id(), 0..4),
        arb_value_full(),
        arb_wake_policy(),
    )
        .prop_map(|(current, return_stack, frame, wake)| SuspendedFlow {
            version: SUSPENDED_FLOW_SECTION_VERSION,
            current,
            return_stack,
            frame,
            wake,
        })
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
            (any::<u32>(), prop::collection::vec(inner.clone(), 0..4))
                .prop_map(|(shape, fields)| Value::record(ShapeId(shape), fields)),
            // Projection values (T1e, docs/t1e-spec.md §3): the other #667
            // gap in this generator — `arb_value_full`'s doc claims "every
            // `Value` variant" but `Projection` was absent until now.
            (
                arb_def_id(),
                prop::collection::vec(arb_proj_segment(inner), 0..3),
            )
                .prop_map(|(cell, segments)| Value::projection(cell, segments)),
        ]
    })
}

/// Structural exhaustiveness guard (issue #667, mirroring the identical guard
/// `proptest_inkt.rs` added for #883/#397): a match over every current
/// [`Value`] variant with **no wildcard arm**, so this fails to compile the
/// moment a new variant is added to the enum. Never called — the only
/// purpose is the compile-time forcing function: whoever adds a `Value`
/// variant must also add an arm here, and teach `arb_value_leaf`/
/// `arb_value_full` above to generate it, instead of the new variant silently
/// escaping the `SaveState`/`SuspendedFlow` round-trip laws this module feeds
/// the way `List` (#746) and `Projection` (this PR) both did.
#[expect(dead_code, reason = "compile-time-only exhaustiveness guard, see doc")]
fn assert_value_variants_exhaustive(value: &Value) {
    match value {
        Value::Int(_)
        | Value::Float(_)
        | Value::Bool(_)
        | Value::String(_)
        | Value::List(_)
        | Value::DivertTarget(_)
        | Value::VariablePointer(_)
        | Value::TempPointer { .. }
        | Value::Null
        | Value::FragmentRef(_)
        | Value::Array(_)
        | Value::Map(_)
        | Value::Record { .. }
        | Value::FnRef(_)
        | Value::Closure(_)
        | Value::Handle { .. }
        | Value::Projection(_) => {}
    }
}
