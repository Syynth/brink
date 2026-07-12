//! T1b collection opcode implementations (`docs/format-v4-rfc.md` §3
//! "Collections (T1a)"; fault semantics from `docs/value-model-spec.md`
//! §6/§11c).
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

use brink_format::{MapKey, OrderedMap, Value};

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

/// `MapInsert`: `[map, key, value]` → updated map (insert-or-overwrite).
/// Unlike `IndexSet`, a missing key is not a fault — this is the stdlib
/// `insert()` mutator's primitive (T1b-3).
pub(crate) fn map_insert(flow: &mut Flow) -> Result<(), RuntimeError> {
    let value = flow.pop_value()?;
    let key = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    let Some(map) = container.map_make_mut() else {
        return Err(RuntimeError::NotIndexable(type_name(&container)));
    };
    let map_key = to_map_key(&key)?;
    map.insert(map_key, value);
    flow.value_stack.push(container);
    Ok(())
}

/// `MapRemove`: `[map, key]` → updated map with `key` removed (no-op if
/// the key was already absent).
pub(crate) fn map_remove(flow: &mut Flow) -> Result<(), RuntimeError> {
    let key = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    let Some(map) = container.map_make_mut() else {
        return Err(RuntimeError::NotIndexable(type_name(&container)));
    };
    let map_key = to_map_key(&key)?;
    map.remove(&map_key);
    flow.value_stack.push(container);
    Ok(())
}

/// `MapContains`: `[map, key]` → `Bool`.
pub(crate) fn map_contains(flow: &mut Flow) -> Result<(), RuntimeError> {
    let key = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Map(map) = &container else {
        return Err(RuntimeError::NotIndexable(type_name(&container)));
    };
    let map_key = to_map_key(&key)?;
    flow.value_stack
        .push(Value::Bool(map.contains_key(&map_key)));
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
