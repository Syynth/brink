//! Law: **sharing is unobservable** (`docs/value-model-spec.md` §3, §5) —
//! workstream B of issue #672 (`epic #672`, item 1: "COW/sharing-unobservable").
//!
//! §3's invariant: "Programs and hosts can never distinguish two structurally
//! equal values — no pointer identity, no refcounts, no copy timing". §5
//! spells out the mechanism this proves sound: `Value::Array`/`Map`/`Record`
//! share an `Arc` backing store until a write forces a `make_mut` copy
//! (`array_make_mut`/`map_make_mut`/`record_make_mut`). This suite generates
//! random collection values, holds several independent clones (an `Arc` bump
//! — the "shared" representation) alongside a hand-built deep copy (a fresh
//! `Arc` tree with no shared backing — the "deep-copied" representation), and
//! proves two things no author or host program could ever tell apart:
//!
//! 1. Mutating one clone never perturbs a sibling clone (the aliasing safety
//!    §5's take → `make_mut` → write-back discipline exists to guarantee).
//! 2. Applying the identical mutation to the shared clone and to the
//!    independently-deep-copied value produces byte-identical results — the
//!    "no observable difference between shared and deep-copied values" the
//!    issue calls for directly.
//!
//! Deterministic seeds (house determinism rule, `CLAUDE.md`): every
//! `proptest!` block below fixes `ProptestConfig::with_cases` and relies on
//! proptest's own default fixed RNG seed (no `PROPTEST_*` env override is
//! read), so a failing case reproduces identically on every run and in CI.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use brink_format::{MapKey, OrderedMap, ShapeId, Value};
use proptest::prelude::*;

/// A fixed 3-field shape id used throughout — `law_cow_sharing` never reads
/// the real `StructShapes` table (out of scope; it only exercises the `Arc`
/// mechanics `record_make_mut` shares with `array_make_mut`/`map_make_mut`),
/// so any stable id works as long as every generated `Record` uses it.
fn shape() -> ShapeId {
    ShapeId(7)
}

// ── Array ────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Mutating one `Arc`-shared clone of an array never perturbs a sibling
    /// clone, and the mutated result matches an independently deep-copied
    /// value put through the identical mutation (value-model-spec §3, §5).
    #[test]
    fn array_clone_mutation_is_unobservable_to_siblings(
        base in prop::collection::vec(any::<i32>(), 0..8),
        idx in 0usize..8,
        new_val in any::<i32>(),
    ) {
        prop_assume!(!base.is_empty());
        let idx = idx % base.len();

        let original = Value::array(base.iter().copied().map(Value::Int).collect());

        // Several independent holders of the "same" value: a plain `.clone()`
        // (an `Arc` bump — genuinely shared backing) and a hand-rebuilt deep
        // copy (a fresh `Arc` tree, `strong_count == 1`).
        let shared_a = original.clone();
        let shared_b = original.clone();
        let mut mutant_shared = original.clone();
        let mut mutant_deep = deep_copy_array(&original);

        // Sanity: the shared clones really do share backing; the deep copy
        // really doesn't. If this ever fails the test below would pass
        // vacuously, so assert it explicitly.
        prop_assert!(std::sync::Arc::ptr_eq(
            original.as_array().unwrap(),
            shared_a.as_array().unwrap()
        ));
        prop_assert!(!std::sync::Arc::ptr_eq(
            original.as_array().unwrap(),
            mutant_deep.as_array().unwrap()
        ));

        mutant_shared.array_make_mut().unwrap()[idx] = Value::Int(new_val);
        mutant_deep.array_make_mut().unwrap()[idx] = Value::Int(new_val);

        // 1. Siblings that never had the mutation applied are untouched.
        prop_assert_eq!(&original, &shared_a);
        prop_assert_eq!(&original, &shared_b);

        // 2. The shared-then-mutated value and the deep-copied-then-mutated
        //    value are indistinguishable — the sharing-unobservable law.
        prop_assert_eq!(&mutant_shared, &mutant_deep);

        let mut expected = base;
        expected[idx] = new_val;
        prop_assert_eq!(mutant_shared, Value::array(expected.into_iter().map(Value::Int).collect()));
    }
}

// ── Map ──────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Same law as the array case, over `Value::Map` (value-model-spec §3,
    /// §4, §5): map keys are restricted to the v1 scalar domain, iteration
    /// order is insertion order, and `map_make_mut`'s copy-on-write path is
    /// unobservable the same way `array_make_mut`'s is.
    #[test]
    fn map_clone_mutation_is_unobservable_to_siblings(
        entries in prop::collection::vec(("[a-z]{1,4}", any::<i32>()), 1..6),
        write_key_idx in 0usize..6,
        new_val in any::<i32>(),
    ) {
        let write_key_idx = write_key_idx % entries.len();
        let write_key = entries[write_key_idx].0.clone();

        let mut map = OrderedMap::new();
        for (k, v) in &entries {
            map.insert(MapKey::from(k.as_str()), Value::Int(*v));
        }
        let original = Value::map(map);

        let shared_sibling = original.clone();
        let mut mutant_shared = original.clone();
        let mut mutant_deep = deep_copy_map(&original);

        prop_assert!(std::sync::Arc::ptr_eq(
            original.as_map().unwrap(),
            shared_sibling.as_map().unwrap()
        ));

        mutant_shared
            .map_make_mut()
            .unwrap()
            .insert(MapKey::from(write_key.as_str()), Value::Int(new_val));
        mutant_deep
            .map_make_mut()
            .unwrap()
            .insert(MapKey::from(write_key.as_str()), Value::Int(new_val));

        prop_assert_eq!(&original, &shared_sibling);
        prop_assert_eq!(&mutant_shared, &mutant_deep);
    }
}

// ── Record ───────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Same law over `Value::Record` (value-model-spec §3, §5): a closed
    /// shape's flat field vector shares `record_make_mut`'s take →
    /// `make_mut` → write-back discipline with arrays/maps.
    #[test]
    fn record_clone_mutation_is_unobservable_to_siblings(
        fields in prop::collection::vec(any::<i32>(), 1..5),
        field_idx in 0usize..5,
        new_val in any::<i32>(),
    ) {
        let field_idx = field_idx % fields.len();
        let original = Value::record(shape(), fields.iter().copied().map(Value::Int).collect());

        let shared_sibling = original.clone();
        let mut mutant_shared = original.clone();
        let mut mutant_deep = deep_copy_record(&original);

        prop_assert!(std::sync::Arc::ptr_eq(
            original.as_record().unwrap().1,
            shared_sibling.as_record().unwrap().1
        ));

        mutant_shared.record_make_mut().unwrap()[field_idx] = Value::Int(new_val);
        mutant_deep.record_make_mut().unwrap()[field_idx] = Value::Int(new_val);

        prop_assert_eq!(&original, &shared_sibling);
        prop_assert_eq!(&mutant_shared, &mutant_deep);
    }
}

// ── Deep-copy reference implementations ─────────────────────────────────
//
// Each `deep_copy_*` rebuilds a value from scratch through the public
// constructors (`Value::array`/`Value::map`/`Value::record`), which always
// allocate a fresh `Arc` (see their doc comments in `brink_format::value`).
// The result is structurally equal to its source but shares no backing
// allocation with it — the "deep-copied" side of the law.

fn deep_copy_array(v: &Value) -> Value {
    let items = v.as_array().unwrap();
    Value::array(items.iter().map(deep_copy_scalar_or_self).collect())
}

fn deep_copy_map(v: &Value) -> Value {
    let map = v.as_map().unwrap();
    let mut out = OrderedMap::with_capacity(map.len());
    for (k, val) in map.iter() {
        out.insert(k.clone(), deep_copy_scalar_or_self(val));
    }
    Value::map(out)
}

fn deep_copy_record(v: &Value) -> Value {
    let (shape, fields) = v.as_record().unwrap();
    Value::record(shape, fields.iter().map(deep_copy_scalar_or_self).collect())
}

/// Recurse for nested collections; scalars have no `Arc` sharing to defeat
/// so `.clone()` already yields an independent value for them.
fn deep_copy_scalar_or_self(v: &Value) -> Value {
    match v {
        Value::Array(_) => deep_copy_array(v),
        Value::Map(_) => deep_copy_map(v),
        Value::Record { .. } => deep_copy_record(v),
        other => other.clone(),
    }
}
