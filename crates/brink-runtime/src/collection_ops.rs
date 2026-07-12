//! T1b collection opcode implementations (`docs/format-v4-rfc.md` §3
//! "Collections (T1a)"; fault semantics from `docs/value-model-spec.md`
//! §11c).
//!
//! Every op here is total: out-of-bounds array indices and missing map keys
//! are turn-terminating `RuntimeError`s (propagated via `?`, unwinding
//! `step()` exactly like `DivisionByZero` does), never silent
//! growth/insertion. Mutation goes through `Value::array_make_mut`/
//! `map_make_mut` — the take → `make_mut` → write-back RMW discipline
//! (value-model-spec §5): an unshared collection mutates in place, a shared
//! one COWs exactly once.

use alloc::string::ToString;
use alloc::vec::Vec;

use brink_format::{MapKey, OrderedMap, Value, ValueType};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::Flow;

/// `ArrayNew(n)`: pop `n` values (in reverse push order), push
/// `Array([v0, …, vn-1])`.
pub(crate) fn array_new(flow: &mut Flow, n: u32) -> Result<(), RuntimeError> {
    let mut items = Vec::with_capacity(n as usize);
    for _ in 0..n {
        items.push(flow.pop_value()?);
    }
    items.reverse();
    flow.value_stack.push(Value::array(items));
    Ok(())
}

/// `MapNew(n)`: pop `n` key/value pairs (in reverse push order — value then
/// key per pair), push `Map({...})`. A repeated key keeps its first
/// position and takes the last value (`OrderedMap::insert` semantics). A
/// key outside the ratified domain (int/string/bool) is a fault — this is
/// the runtime-checked half of the "restricted to the ratified key domain
/// at runtime" ruling (`docs/t1b-surface-spec.md` §3).
pub(crate) fn map_new(flow: &mut Flow, n: u32) -> Result<(), RuntimeError> {
    let mut pairs = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let value = flow.pop_value()?;
        let key_value = flow.pop_value()?;
        let key = to_map_key(&key_value)?;
        pairs.push((key, value));
    }
    pairs.reverse();
    let mut map = OrderedMap::with_capacity(pairs.len());
    for (k, v) in pairs {
        map.insert(k, v);
    }
    flow.value_stack.push(Value::map(map));
    Ok(())
}

/// `IndexGet`: `[container, index]` → element/value. Turn-terminating fault
/// on out-of-bounds array index or missing map key.
pub(crate) fn index_get(flow: &mut Flow) -> Result<(), RuntimeError> {
    let index = flow.pop_value()?;
    let container = flow.pop_value()?;
    let result = read_index(&container, &index)?.clone();
    flow.value_stack.push(result);
    Ok(())
}

/// `IndexSet`: `[container, index, value]` → updated container. Take →
/// `make_mut` → write-back on `container` (already a stack value here — the
/// caller's job, per the RMW discipline the compiler emits, is to read the
/// root cell, call this, then write the result back). Fault on
/// out-of-bounds array index (no silent growth on write-past-end) or
/// missing map key (an indexed *write* never inserts — see
/// `RuntimeError::MapKeyNotFound`).
pub(crate) fn index_set(flow: &mut Flow) -> Result<(), RuntimeError> {
    let value = flow.pop_value()?;
    let index = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    write_index(&mut container, &index, value)?;
    flow.value_stack.push(container);
    Ok(())
}

/// `CollectionLen`: `[container]` → `Int(len)`. Array or map.
pub(crate) fn collection_len(flow: &mut Flow) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    #[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let len = match &container {
        Value::Array(items) => items.len() as i32,
        Value::Map(map) => map.len() as i32,
        other => return Err(RuntimeError::NotIndexable(type_name(other))),
    };
    flow.value_stack.push(Value::Int(len));
    Ok(())
}

/// `CollectionKeys`: `[map]` → `Array` of keys in insertion order.
///
/// For an `Array` input, returns the array itself unchanged (identity
/// pass-through). This deliberately makes `CollectionKeys` double as "the
/// canonical sequence to iterate" for either collection kind: brink has no
/// static array/map type distinction pre-typed-dialect, so `for x in
/// iterable { … }` (`docs/t1b-surface-spec.md` §2 — values for arrays, keys
/// in insertion order for maps) compiles to a *single* `CollectionKeys`
/// call regardless of which kind `iterable` turns out to hold at runtime;
/// dedicated iterator opcodes are deliberately not part of the T1b surface
/// (`docs/format-v4-rfc.md` §3 note).
pub(crate) fn collection_keys(flow: &mut Flow) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    let result = match &container {
        Value::Array(_) => container,
        Value::Map(map) => {
            let keys: Vec<Value> = map.keys().map(map_key_to_value).collect();
            Value::array(keys)
        }
        other => return Err(RuntimeError::NotIndexable(type_name(other))),
    };
    flow.value_stack.push(result);
    Ok(())
}

/// `CollectionValues`: `[map]` → `Array` of values in insertion order.
pub(crate) fn collection_values(flow: &mut Flow) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    let Value::Map(map) = &container else {
        return Err(RuntimeError::NotIndexable(type_name(&container)));
    };
    let values: Vec<Value> = map.values().cloned().collect();
    flow.value_stack.push(Value::array(values));
    Ok(())
}

/// `MapGet`: `[map, key]` → value. Turn-terminating fault on missing key or
/// a non-map container.
pub(crate) fn map_get(flow: &mut Flow) -> Result<(), RuntimeError> {
    let key = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Map(map) = &container else {
        return Err(RuntimeError::NotIndexable(type_name(&container)));
    };
    let map_key = to_map_key(&key)?;
    let value = map
        .get(&map_key)
        .cloned()
        .ok_or_else(|| RuntimeError::MapKeyNotFound {
            key: map_key_display(&map_key),
        })?;
    flow.value_stack.push(value);
    Ok(())
}

/// `MapInsert`: `[container, key_or_index, value]` → updated container.
/// This is the stdlib `insert()`/`push()` mutators' primitive (T1b-3,
/// `docs/t1b-surface-spec.md` §5) — generalized over both collection kinds
/// despite the opcode's `Map*` name (the RFC's frozen collection-opcode
/// block has no dedicated array-append/insert opcode; §5's ruling is that
/// `insert(x, k_or_i, v)` and `push(a, v)` work on either kind, so the
/// existing map-shaped opcodes are the natural, frozen-numbering-compatible
/// targets — see the T1b-3 PR description):
///
/// - **Map**: insert-or-overwrite by key. Unlike `IndexSet`, a missing key
///   is not a fault.
/// - **Array**: `Vec::insert(index, value)`, shifting later elements right.
///   `index` must be an `Int` in `[0, len]` inclusive — `index == len` is
///   "insert at the end", i.e. `push(a, v)` lowers to
///   `insert(a, len(a), v)`. Unlike `IndexSet`'s strict `< len`, this is the
///   one array write that's allowed to reach the end, by construction (it
///   grows the array by exactly one element, never further) — still no
///   *silent* growth: any other out-of-range index is a fault.
pub(crate) fn map_insert(flow: &mut Flow) -> Result<(), RuntimeError> {
    let value = flow.pop_value()?;
    let key = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    match container.value_type() {
        ValueType::Map => {
            let map_key = to_map_key(&key)?;
            let Some(map) = container.map_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(&container)));
            };
            map.insert(map_key, value);
        }
        ValueType::Array => {
            let len = container.as_array().map_or(0, |items| items.len());
            let idx = insert_index(&key, len)?;
            let Some(items) = container.array_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(&container)));
            };
            items.insert(idx, value);
        }
        _ => return Err(RuntimeError::NotIndexable(type_name(&container))),
    }
    flow.value_stack.push(container);
    Ok(())
}

/// `MapRemove`: `[container, key_or_index]` → updated container. The stdlib
/// `remove()` mutator's primitive (T1b-3) — generalized like `MapInsert`
/// above:
///
/// - **Map**: remove by key, no-op if the key was already absent.
/// - **Array**: `Vec::remove(index)`, shifting later elements left. `index`
///   must be an `Int` in `[0, len)` — strictly less than `len`, unlike
///   `MapInsert`'s append-friendly `<=`, since there is no element to remove
///   at `len`. Out-of-range is a fault (`IndexOutOfBounds`), matching
///   `IndexGet`/`IndexSet`.
pub(crate) fn map_remove(flow: &mut Flow) -> Result<(), RuntimeError> {
    let key = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    match container.value_type() {
        ValueType::Map => {
            let map_key = to_map_key(&key)?;
            let Some(map) = container.map_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(&container)));
            };
            map.remove(&map_key);
        }
        ValueType::Array => {
            let len = container.as_array().map_or(0, |items| items.len());
            let idx = array_index(&key, len)?;
            let Some(items) = container.array_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(&container)));
            };
            items.remove(idx);
        }
        _ => return Err(RuntimeError::NotIndexable(type_name(&container))),
    }
    flow.value_stack.push(container);
    Ok(())
}

/// `MapContains`: `[container, needle]` → `Bool`. The stdlib `contains(x,
/// v)` primitive (T1b-3) — generalized like `MapInsert`/`MapRemove`:
///
/// - **Map**: key containment — `needle` is coerced through the ratified key
///   domain (int/string/bool), matching `MapGet`/`IndexGet`'s key handling.
///   A `needle` outside the key domain (float, array, map, …) can never be a
///   key, so it's simply not contained — `false`, not a fault. This is
///   **total on both branches**, matching the array branch below and
///   value-model-spec §11c ("total operations with specified failure
///   values where defined"); unlike `MapGet`/indexing (§11c), `contains` has
///   no "the key isn't there" failure mode to escalate to a fault — a
///   non-key-domain needle *is* "the key isn't there." Ruled 2026-07-12,
///   see `docs/decision-log.md`.
/// - **Array**: element containment — a linear scan for a `needle` that
///   compares structurally equal (`Value`'s `PartialEq`, value-model-spec
///   §4/§5) to any element. `needle` may be any value, not just a scalar.
pub(crate) fn map_contains(flow: &mut Flow) -> Result<(), RuntimeError> {
    let needle = flow.pop_value()?;
    let container = flow.pop_value()?;
    let found = match &container {
        Value::Map(map) => MapKey::from_value(&needle).is_some_and(|k| map.contains_key(&k)),
        Value::Array(items) => items.iter().any(|item| item == &needle),
        other => return Err(RuntimeError::NotIndexable(type_name(other))),
    };
    flow.value_stack.push(Value::Bool(found));
    Ok(())
}

/// `PushLiteral(idx)`: push a clone of `program.literal_pool[idx]` — an
/// `Arc` bump for collections (zero-allocation load).
pub(crate) fn push_literal(
    flow: &mut Flow,
    program: &Program,
    idx: u32,
) -> Result<(), RuntimeError> {
    let value = program
        .literal_pool_entry(idx)
        .cloned()
        .ok_or(RuntimeError::InvalidLiteralIndex(idx))?;
    flow.value_stack.push(value);
    Ok(())
}

// ── Shared helpers ───────────────────────────────────────────────────────

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::DivertTarget(_) => "divert_target",
        Value::VariablePointer(_) => "var_pointer",
        Value::TempPointer { .. } => "temp_pointer",
        Value::Null => "null",
        Value::FragmentRef(_) => "fragment_ref",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
    }
}

fn to_map_key(v: &Value) -> Result<MapKey, RuntimeError> {
    MapKey::from_value(v).ok_or_else(|| RuntimeError::InvalidMapKeyType(type_name(v)))
}

fn map_key_to_value(k: &MapKey) -> Value {
    match k {
        MapKey::Int(n) => Value::Int(*n),
        MapKey::Str(s) => Value::String(alloc::sync::Arc::clone(s)),
        MapKey::Bool(b) => Value::Bool(*b),
    }
}

fn map_key_display(k: &MapKey) -> alloc::string::String {
    match k {
        MapKey::Int(n) => n.to_string(),
        MapKey::Str(s) => s.to_string(),
        MapKey::Bool(b) => b.to_string(),
    }
}

/// Read `container[index]`, borrowing from `container` — the shared read
/// half of `IndexGet` (via `.clone()`) and the intermediate reads inside
/// `write_index`'s chain walk.
fn read_index<'a>(container: &'a Value, index: &Value) -> Result<&'a Value, RuntimeError> {
    match container {
        Value::Array(items) => {
            let i = array_index(index, items.len())?;
            // `array_index` already validated `0 <= i < len`.
            #[expect(clippy::indexing_slicing, reason = "bounds validated above")]
            Ok(&items[i])
        }
        Value::Map(map) => {
            let key = to_map_key(index)?;
            map.get(&key).ok_or_else(|| RuntimeError::MapKeyNotFound {
                key: map_key_display(&key),
            })
        }
        other => Err(RuntimeError::NotIndexable(type_name(other))),
    }
}

/// Write `container[index] = value` in place via the take → `make_mut` →
/// write-back discipline. Array writes never grow past the end (no
/// out-of-bounds write ever succeeds); map writes never insert a new key
/// (missing key is a fault — see `RuntimeError::MapKeyNotFound`'s doc).
fn write_index(container: &mut Value, index: &Value, value: Value) -> Result<(), RuntimeError> {
    match container {
        Value::Array(_) => {
            let len = container.as_array().map_or(0, |items| items.len());
            let i = array_index(index, len)?;
            let Some(items) = container.array_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(container)));
            };
            #[expect(clippy::indexing_slicing, reason = "bounds validated above")]
            {
                items[i] = value;
            }
            Ok(())
        }
        Value::Map(_) => {
            let key = to_map_key(index)?;
            let has_key = container.as_map().is_some_and(|map| map.contains_key(&key));
            if !has_key {
                return Err(RuntimeError::MapKeyNotFound {
                    key: map_key_display(&key),
                });
            }
            let Some(map) = container.map_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(container)));
            };
            map.insert(key, value);
            Ok(())
        }
        other => Err(RuntimeError::NotIndexable(type_name(other))),
    }
}

/// Validate an index value is an `Int` in `[0, len)`, returning it as
/// `usize`.
fn array_index(index: &Value, len: usize) -> Result<usize, RuntimeError> {
    let Value::Int(i) = index else {
        return Err(RuntimeError::InvalidArrayIndex(type_name(index)));
    };
    #[expect(clippy::cast_sign_loss)]
    if *i < 0 || *i as usize >= len {
        Err(RuntimeError::IndexOutOfBounds { index: *i, len })
    } else {
        Ok(*i as usize)
    }
}

/// Validate an index value is an `Int` in `[0, len]` — inclusive of `len`,
/// the one array write allowed to reach the end (`MapInsert`'s array
/// branch: `index == len` means "insert at the end", the `push()` stdlib
/// mutator's primitive). Distinct from [`array_index`]'s strict `< len`,
/// used by `IndexGet`/`IndexSet`/`MapRemove`'s array branch, none of which
/// ever grow the array.
fn insert_index(index: &Value, len: usize) -> Result<usize, RuntimeError> {
    let Value::Int(i) = index else {
        return Err(RuntimeError::InvalidArrayIndex(type_name(index)));
    };
    #[expect(clippy::cast_sign_loss)]
    if *i < 0 || *i as usize > len {
        Err(RuntimeError::IndexOutOfBounds { index: *i, len })
    } else {
        Ok(*i as usize)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────
//
// T1b-3 stdlib slice 1 (`docs/t1b-surface-spec.md` §5) is VM-native: the
// compiler emits `MapInsert`/`MapRemove`/`MapContains` for the generalized
// array/map semantics these tests prove directly against hand-assembled
// `Value` trees, one level below full bytecode — the same "op function,
// not full VM" granularity T1b-2's fault-semantics tests used. End-to-end
// compile+run coverage (source `.ink` -> bytecode -> VM) lives in the
// `tests/tier1-brink` corpus wing and `brink-test-harness`'s T1b property
// tests, which exercise these same primitives via the real
// `push`/`insert`/`remove`/`contains` call sites.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::story::Flow;
    use alloc::sync::Arc;

    /// A `Flow` with nothing but an empty value stack — every function in
    /// this module reads/writes only `flow.value_stack`.
    fn test_flow() -> Flow {
        Flow {
            threads: Vec::new(),
            value_stack: Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
        }
    }

    fn arr(items: Vec<Value>) -> Value {
        Value::array(items)
    }

    fn push_args(flow: &mut Flow, args: Vec<Value>) {
        for v in args {
            flow.value_stack.push(v);
        }
    }

    // ── MapInsert generalized for Array (push/insert primitive) ──────────

    #[test]
    fn map_insert_array_appends_at_len_index() {
        // push([1, 2], 3) == insert([1, 2], len([1,2]), 3) -> [1, 2, 3]
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![Value::Int(1), Value::Int(2)]),
                Value::Int(2),
                Value::Int(3),
            ],
        );
        map_insert(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(
            result,
            arr(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn map_insert_array_shifts_elements_right_at_interior_index() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![Value::Int(1), Value::Int(3)]),
                Value::Int(1),
                Value::Int(2),
            ],
        );
        map_insert(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(
            result,
            arr(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn map_insert_array_index_past_len_faults() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![Value::Int(1)]), Value::Int(5), Value::Int(9)],
        );
        let err = map_insert(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::IndexOutOfBounds { index: 5, len: 1 });
    }

    #[test]
    fn map_insert_array_negative_index_faults() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![Value::Int(1)]), Value::Int(-1), Value::Int(9)],
        );
        let err = map_insert(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::IndexOutOfBounds { index: -1, len: 1 });
    }

    #[test]
    fn map_insert_array_non_int_index_faults() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![Value::Int(1)]), Value::from("nope"), Value::Int(9)],
        );
        let err = map_insert(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::InvalidArrayIndex("string"));
    }

    #[test]
    fn map_insert_map_still_insert_or_overwrite_by_key() {
        // Unaffected regression check: existing Map behavior is unchanged
        // by the Array generalization.
        let mut map = OrderedMap::new();
        map.insert(MapKey::Str(Arc::from("a")), Value::Int(1));
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![Value::map(map), Value::from("b"), Value::Int(2)],
        );
        map_insert(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        let Value::Map(m) = result else {
            unreachable!("map_insert on a map must return a map")
        };
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&MapKey::Str(Arc::from("b"))), Some(&Value::Int(2)));
    }

    // ── MapRemove generalized for Array ───────────────────────────────────

    #[test]
    fn map_remove_array_shifts_elements_left() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                Value::Int(1),
            ],
        );
        map_remove(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(result, arr(vec![Value::Int(1), Value::Int(3)]));
    }

    #[test]
    fn map_remove_array_index_equal_to_len_faults() {
        // Unlike MapInsert's push-friendly `<= len`, remove has no element
        // to remove at `len` — strictly `< len`, same as IndexGet/IndexSet.
        let mut flow = test_flow();
        push_args(&mut flow, vec![arr(vec![Value::Int(1)]), Value::Int(1)]);
        let err = map_remove(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::IndexOutOfBounds { index: 1, len: 1 });
    }

    #[test]
    fn map_remove_map_no_op_when_key_absent() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![Value::map(OrderedMap::new()), Value::from("missing")],
        );
        map_remove(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        let Value::Map(m) = result else {
            unreachable!("map_remove on a map must return a map")
        };
        assert_eq!(m.len(), 0);
    }

    // ── MapContains generalized for Array (element containment) ──────────

    #[test]
    fn map_contains_array_element_present() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                Value::Int(2),
            ],
        );
        map_contains(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(true));
    }

    #[test]
    fn map_contains_array_element_absent() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![Value::Int(1), Value::Int(2)]), Value::Int(9)],
        );
        map_contains(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));
    }

    #[test]
    fn map_contains_array_non_scalar_needle() {
        // contains(x, v) accepts any expression argument (§5) — element
        // containment is structural equality, not restricted to the map-key
        // domain the way MapGet/MapContains's Map branch is.
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![arr(vec![Value::Int(1)]), arr(vec![Value::Int(2)])]),
                arr(vec![Value::Int(2)]),
            ],
        );
        map_contains(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(true));
    }

    #[test]
    fn map_contains_map_key_containment_unchanged() {
        let mut map = OrderedMap::new();
        map.insert(MapKey::Str(Arc::from("k")), Value::Int(1));
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::map(map), Value::from("k")]);
        map_contains(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(true));
    }

    #[test]
    fn map_contains_map_non_key_domain_float_needle_returns_false() {
        // RULED 2026-07-12 (#580, docs/decision-log.md): a needle outside
        // the map-key domain (int/string/bool) can never be a key, so
        // `contains` is total — `false`, not a fault — matching the array
        // branch's totality. Regression for the map branch specifically.
        let mut map = OrderedMap::new();
        map.insert(MapKey::Int(1), Value::Int(1));
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::map(map), Value::Float(1.0)]);
        map_contains(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));
    }

    #[test]
    fn map_contains_map_collection_needle_returns_false() {
        // A collection needle (array/map) is likewise outside the key
        // domain — total `false`, not `InvalidMapKeyType`.
        let mut map = OrderedMap::new();
        map.insert(MapKey::Str(Arc::from("k")), Value::Int(1));
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::map(map), arr(vec![Value::Int(1)])]);
        map_contains(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));
    }

    #[test]
    fn map_contains_non_collection_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Int(5), Value::Int(5)]);
        let err = map_contains(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::NotIndexable("int"));
    }

    // ── COW / RMW discipline ──────────────────────────────────────────────

    #[test]
    fn map_insert_array_cows_when_shared() {
        // Take -> make_mut -> write-back (value-model-spec §5): mutating a
        // shared Arc must not observably affect the other holder.
        let original = arr(vec![Value::Int(1)]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        push_args(&mut flow, vec![original, Value::Int(1), Value::Int(2)]);
        map_insert(&mut flow).unwrap();
        let mutated = flow.pop_value().unwrap();
        assert_eq!(snapshot, arr(vec![Value::Int(1)]), "snapshot unmutated");
        assert_eq!(mutated, arr(vec![Value::Int(1), Value::Int(2)]));
    }

    #[test]
    fn map_remove_array_cows_when_shared() {
        let original = arr(vec![Value::Int(1), Value::Int(2)]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        push_args(&mut flow, vec![original, Value::Int(0)]);
        map_remove(&mut flow).unwrap();
        let mutated = flow.pop_value().unwrap();
        assert_eq!(
            snapshot,
            arr(vec![Value::Int(1), Value::Int(2)]),
            "snapshot unmutated"
        );
        assert_eq!(mutated, arr(vec![Value::Int(2)]));
    }
}
