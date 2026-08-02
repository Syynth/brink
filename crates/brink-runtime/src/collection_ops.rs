//! T1b collection opcode implementations (`docs/format-v4-rfc.md` §3
//! "Collections (T1a)"; fault semantics from `docs/value-model-spec.md`
//! §11c).
//!
//! Reads and out-of-bounds array writes are total: a missing map key on a
//! *read* (`IndexGet`/`MapGet`) or an out-of-bounds array index (read or
//! write) is a turn-terminating `RuntimeError` (propagated via `?`,
//! unwinding `step()` exactly like `DivisionByZero` does), never silent
//! growth. A missing map key on an indexed *write* (`IndexSet`, i.e.
//! `m[k] = v`) instead **inserts** (JS/Python semantics, issue #856,
//! ruled 2026-07-15) — array writes still never grow past the end (no
//! silent growth there; `push`/`insert` are the stdlib mutators for that).
//! Mutation goes through `Value::array_make_mut`/`map_make_mut` — the take
//! → `make_mut` → write-back RMW discipline (value-model-spec §5): an
//! unshared collection mutates in place, a shared one COWs exactly once.
//!
//! `write_index` (used by path-projection writes, `proj_ops::write` —
//! `docs/t1e-spec.md` §4) keeps the strict fault-on-missing-key behavior:
//! that spec explicitly ratifies "a missing key ... at write time is the
//! §1(2) fault, consistent with §11c" for writes through a `ref` projection.
//! `write_index_upsert` is the insert-on-absent variant, used only by
//! `index_set` (the direct `IndexSet` opcode — #856's assign path).

use alloc::string::ToString;
use alloc::vec::Vec;

use brink_format::{MapKey, OrderedMap, Value, ValueType};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::{ExecMode, Flow};

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
    // Ranges (NS-A5, F7) index like a virtual array of their elements —
    // `(2..5)[0] == 2`, OOB faults exactly like arrays. Handled here (not
    // in `read_index`) because a range element is *computed*, never
    // borrowed: `read_index`'s `&Value` return can't hand one out.
    if let Value::Range { .. } = &container {
        let result = range_element(&container, &index)?;
        flow.value_stack.push(result);
        return Ok(());
    }
    let result = read_index(&container, &index)?.clone();
    flow.value_stack.push(result);
    Ok(())
}

/// The `i`-th element of a range value: `start + i`, faulting OOB exactly
/// like an array read (`docs/value-model-spec.md` §11c — no silent clamp).
/// Non-range containers are the caller's job; a non-int index is the same
/// `InvalidArrayIndex` fault arrays raise.
fn range_element(range: &Value, index: &Value) -> Result<Value, RuntimeError> {
    let Value::Int(i) = index else {
        return Err(RuntimeError::InvalidArrayIndex(type_name(index)));
    };
    let (Some((start, _, _)), Some(len)) = (range.as_range(), range.range_len()) else {
        return Err(RuntimeError::NotIndexable(type_name(range)));
    };
    if i64::from(*i) < 0 || i64::from(*i) >= len {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Err(RuntimeError::IndexOutOfBounds {
            index: *i,
            len: len.min(i64::from(u32::MAX)) as usize,
        });
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "start + i is an element of the range by construction, so it fits i32"
    )]
    Ok(Value::Int((i64::from(start) + i64::from(*i)) as i32))
}

/// `IndexSet`: `[container, index, value]` → updated container. Take →
/// `make_mut` → write-back on `container` (already a stack value here — the
/// caller's job, per the RMW discipline the compiler emits, is to read the
/// root cell, call this, then write the result back). Fault on
/// out-of-bounds array index (no silent growth on write-past-end); a
/// missing map key **inserts** rather than faulting (issue #856, ruled
/// 2026-07-15: `memo[k] = v` on a fresh key works, JS/Python semantics) —
/// see [`write_index_upsert`].
pub(crate) fn index_set(flow: &mut Flow) -> Result<(), RuntimeError> {
    let value = flow.pop_value()?;
    let index = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    write_index_upsert(&mut container, &index, value)?;
    flow.value_stack.push(container);
    Ok(())
}

/// `CollectionLen`: `[container]` → `Int(len)`. Array, map, range, or
/// string.
pub(crate) fn collection_len(flow: &mut Flow) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    #[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let len = match &container {
        Value::Array(items) => items.len() as i32,
        Value::Map(map) => map.len() as i32,
        // Ranges (NS-A5, F7): the denoted element count, saturated at
        // i32::MAX for the degenerate 2³¹+-element ranges (a loop that
        // long exceeds the VM step limit regardless; `rand::int` uses the
        // exact i64 length internally, never this op).
        Value::Range { .. } => container.range_len().unwrap_or(0).min(i64::from(i32::MAX)) as i32,
        // Strings (#1171): char count, i.e. Unicode scalar values via
        // `str::chars`, never UTF-8 byte length. Matches every other
        // string-indexing op in this runtime — `char_at` and `find`
        // (`string_ops.rs`) both count USVs, per the ruled "author sanity"
        // posture (`docs/stdlib-spec.md`, issue #857) and the verb table's
        // `len(… | string): int` = char count.
        Value::String(s) => s.chars().count() as i32,
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
    // Ranges (NS-A5, F7) pass through unchanged — the identity, exactly
    // like arrays: a range IS its own canonical sequence (`len`/`IndexGet`
    // read elements straight off the bounds), so the `for` desugar's
    // snapshot is O(1) and `for i in 0..1_000_000` never materializes a
    // million-element array. This is also what parks in a FlowFrame spill
    // across `await` — the durable wire form F7 exists for.
    if matches!(container, Value::Range { .. }) {
        flow.value_stack.push(container);
        return Ok(());
    }
    let result = Value::Array(iteration_sequence(container)?);
    flow.value_stack.push(result);
    Ok(())
}

/// The canonical iteration sequence of a builtin iterable — the iterate
/// protocol's closed v1 roster reified as ONE function (NS-A3, issue
/// #1109, docs/stdlib-spec.md §9.6): arrays iterate their values (identity
/// — the array's own storage, no copy), maps iterate their **keys** in
/// insertion order, snapshotted eagerly (F10, ruled 2026-07-19: maps' `for`
/// is a deliberate exception to live pull iteration). Everything else is
/// not iterable and faults `NotIndexable`.
///
/// Both consumers of the contract go through here so they can never drift:
/// the `for` desugar's [`collection_keys`] opcode (index walk, LIR-lowered)
/// and the pull-shaped [`crate::iter::ValueIter`] machine form (the law
/// harness's subject).
pub(crate) fn iteration_sequence(
    container: Value,
) -> Result<alloc::sync::Arc<Vec<Value>>, RuntimeError> {
    match container {
        Value::Array(items) => Ok(items),
        Value::Map(map) => {
            let keys: Vec<Value> = map.keys().map(map_key_to_value).collect();
            Ok(alloc::sync::Arc::new(keys))
        }
        other => Err(RuntimeError::NotIndexable(type_name(&other))),
    }
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
/// - **Map**: insert-or-overwrite by key — same insert-on-absent semantics
///   `IndexSet` now has too (issue #856); this opcode predates that ruling
///   and remains the stdlib `insert()`/`push()` mutators' primitive.
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
            note_map_mutation(&container);
            let Some(map) = container.map_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(&container)));
            };
            map.insert(map_key, value);
        }
        ValueType::Array => {
            let len = container.as_array().map_or(0, |items| items.len());
            let idx = insert_index(&key, len)?;
            note_array_mutation(&container);
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

/// `MapRemove`: `[map, key]` → updated map with `key` removed, no-op if the
/// key was already absent. The stdlib `remove()` mutator's primitive
/// (T1b-3). **Map-only as of issue #1484** (decision log "Quick-docket
/// closures" 2026-07-26): `remove` uniformly names identity-based,
/// idempotent-total removal (map keys, flags values), so a non-map
/// container is a fault (`NotIndexable`) rather than falling back to
/// index-based removal — the array-index leg this op used to generalize
/// over is [`seq_remove_at`], the faulting-index `remove_at()` primitive.
pub(crate) fn map_remove(flow: &mut Flow) -> Result<(), RuntimeError> {
    let key = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    match container.value_type() {
        ValueType::Map => {
            let map_key = to_map_key(&key)?;
            note_map_mutation(&container);
            let Some(map) = container.map_make_mut() else {
                return Err(RuntimeError::NotIndexable(type_name(&container)));
            };
            map.remove(&map_key);
        }
        _ => return Err(RuntimeError::NotIndexable(type_name(&container))),
    }
    flow.value_stack.push(container);
    Ok(())
}

/// `SeqRemoveAt`: `[a, i]` → updated array with the element at `i` removed,
/// shifting later elements left (`Vec::remove(i)`). The stdlib
/// `remove_at()` mutator's primitive (issue #1484, joining the `_at`
/// faulting-index family with `char_at`) — the array leg [`map_remove`]
/// generalized over before this PR. Array-only: a non-array `a` is a fault
/// (`NotIndexable`). `i` must be an `Int` in `[0, len)` — strictly less
/// than `len`, unlike `MapInsert`'s append-friendly `<=`, since there is no
/// element to remove at `len`. Out-of-range is a fault
/// (`IndexOutOfBounds`), matching `IndexGet`/`IndexSet`.
pub(crate) fn seq_remove_at(flow: &mut Flow) -> Result<(), RuntimeError> {
    let index = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    match container.value_type() {
        ValueType::Array => {
            let len = container.as_array().map_or(0, |items| items.len());
            let idx = array_index(&index, len)?;
            note_array_mutation(&container);
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
/// v)` primitive (T1b-3) — generalized like `MapInsert` (`MapRemove` was
/// generalized the same way until issue #1484 split it into map-only
/// `MapRemove` / array-only `SeqRemoveAt`):
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

// ── NS-A1: the ruled stdlib flips (`docs/stdlib-spec.md` §§4-5; issue
// #1107). Absence returns `Option` (`none`, never a fault); a malformed
// *question* — wrong container type, unorderable elements, a non-scalar map
// key — stays a turn-terminating fault (the ruled fault-vs-absence
// doctrine: "a fault says 'your program is wrong'; Option says 'the world
// didn't have one'"). ──────────────────────────────────────────────────────

/// `SeqIndexOf`: `[a, x]` → `Option[int]` — index of the first element
/// structurally equal to `x` (`Value`'s `PartialEq`, the built-in content
/// equality — F22: search verbs depend on `eq`, never `compare`), or
/// `none` when absent (martyr #2 redeemed).
pub(crate) fn seq_index_of(flow: &mut Flow) -> Result<(), RuntimeError> {
    let needle = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "index_of",
            expected: "an array",
            found: type_name(&container),
        });
    };
    #[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let result = items
        .iter()
        .position(|item| item == &needle)
        .map_or_else(Value::none, |i| Value::some(Value::Int(i as i32)));
    flow.value_stack.push(result);
    Ok(())
}

/// `SeqFirst`: `[a]` → `Option[T]` — first element, `none` on empty.
pub(crate) fn seq_first(flow: &mut Flow) -> Result<(), RuntimeError> {
    seq_edge(flow, "first", <[Value]>::first)
}

/// `SeqLast`: `[a]` → `Option[T]` — last element, `none` on empty.
pub(crate) fn seq_last(flow: &mut Flow) -> Result<(), RuntimeError> {
    seq_edge(flow, "last", <[Value]>::last)
}

fn seq_edge(
    flow: &mut Flow,
    verb: &'static str,
    pick: impl Fn(&[Value]) -> Option<&Value>,
) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb,
            expected: "an array",
            found: type_name(&container),
        });
    };
    let result = pick(items).map_or_else(Value::none, |v| Value::some(v.clone()));
    flow.value_stack.push(result);
    Ok(())
}

/// `SeqMin`: `[a]` → `Option[T]` — least element per [`total_order_cmp`],
/// `none` on empty. Ties keep the first occurrence (deterministic).
pub(crate) fn seq_min(flow: &mut Flow) -> Result<(), RuntimeError> {
    seq_extremum(flow, "min", core::cmp::Ordering::Less)
}

/// `SeqMax`: `[a]` → `Option[T]` — greatest element, `none` on empty.
pub(crate) fn seq_max(flow: &mut Flow) -> Result<(), RuntimeError> {
    seq_extremum(flow, "max", core::cmp::Ordering::Greater)
}

fn seq_extremum(
    flow: &mut Flow,
    verb: &'static str,
    keep_when: core::cmp::Ordering,
) -> Result<(), RuntimeError> {
    let mode = flow.exec_mode;
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb,
            expected: "an array",
            found: type_name(&container),
        });
    };
    // NS-A4 (§4b): in DEV mode a NaN comparand in an ordering context is a
    // turn-terminating fault, comparison or no comparison (`min([nan])`
    // never compares, but the NaN operand is the upstream bug all the
    // same). PROD mode skips the scan and places NaN by the pinned order
    // in `total_order_cmp`.
    if mode == ExecMode::Dev {
        nan_scan(verb, items, 0)?;
    }
    let mut best: Option<&Value> = None;
    for item in items.iter() {
        best = Some(match best {
            None => item,
            // Strict ordering comparison: only a strictly-better candidate
            // replaces the incumbent, so ties keep the first occurrence.
            Some(b) if total_order_cmp(verb, item, b)? == keep_when => item,
            Some(b) => b,
        });
    }
    let result = best.map_or_else(Value::none, |v| Value::some(v.clone()));
    flow.value_stack.push(result);
    Ok(())
}

/// The maximum array-nesting depth the recursive ordering walk
/// ([`total_order_cmp`]'s lexicographic arm, [`nan_scan`]) will follow
/// before faulting — a Rust-stack guard against pathological
/// self-referential nesting built up at runtime (`a = [a]` in a loop).
const ORDERING_NEST_LIMIT: u32 = 64;

/// The §4b orderable roster (`docs/stdlib-spec.md` §4b, RULED 2026-07-18;
/// NS-A4 completes it): int · float (numeric promotion between them) ·
/// bool (`false < true`) · string (lexicographic by Unicode scalar value —
/// Rust's `str` ordering, which is USV order for valid UTF-8) · arrays
/// lexicographic element-wise (elements recursively orderable, shorter
/// prefix first). Floats use the pinned non-fabricating total order —
/// ordinary IEEE order with `-0 == +0` as a tie, NaN greater than
/// everything, NaN-vs-NaN ties (deliberately NOT IEEE `totalOrder`). The
/// dev-mode NaN fault is NOT here: it is the [`nan_scan`] pre-scan the
/// ordering verbs run before any comparison — the comparison itself is
/// mode-free (the mode changes where execution stops, never how elements
/// place), so on the data that reaches it the two modes agree by
/// construction.
///
/// Structs/enums order ONLY via an explicit registry `compare` impl (§9.6,
/// no structural auto-order); no impl registration surface reaches the
/// runtime yet (the impl spelling is ⏳ code-dialect sitting), so records
/// stay [`RuntimeError::NotOrderable`] here — as do maps, flags subsets,
/// divert targets, and every cross-type pair.
fn total_order_cmp(
    verb: &'static str,
    a: &Value,
    b: &Value,
) -> Result<core::cmp::Ordering, RuntimeError> {
    total_order_cmp_at(verb, a, b, 0)
}

fn total_order_cmp_at(
    verb: &'static str,
    a: &Value,
    b: &Value,
    depth: u32,
) -> Result<core::cmp::Ordering, RuntimeError> {
    let not_orderable = |v: &Value| RuntimeError::NotOrderable {
        verb,
        found: type_name(v),
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(_) | Value::Int(_), Value::Float(_) | Value::Int(_)) => {
            // At least one side is a Float (the Int/Int arm matched above),
            // so both promote — `as_float` covers both variants.
            let (Some(x), Some(y)) = (a.as_float(), b.as_float()) else {
                return Err(not_orderable(a));
            };
            Ok(pinned_float_cmp(x, y))
        }
        (Value::Bool(x), Value::Bool(y)) => Ok(x.cmp(y)),
        (Value::String(x), Value::String(y)) => Ok(x.as_ref().cmp(y.as_ref())),
        // NS-A4: arrays lexicographic element-wise, recursively (§4b's
        // roster). First divergent element decides; a full prefix ties to
        // the shorter array ("shorter is less").
        (Value::Array(xs), Value::Array(ys)) => {
            if depth >= ORDERING_NEST_LIMIT {
                return Err(RuntimeError::NotOrderable {
                    verb,
                    found: "an array nested past the ordering depth limit",
                });
            }
            for (x, y) in xs.iter().zip(ys.iter()) {
                let ord = total_order_cmp_at(verb, x, y, depth + 1)?;
                if ord != core::cmp::Ordering::Equal {
                    return Ok(ord);
                }
            }
            Ok(xs.len().cmp(&ys.len()))
        }
        // Cross-type / unorderable. Name the most useful offender: an
        // element outside the orderable set entirely (`a` first — the
        // newly-visited element in the extremum walk — then `b`), or, for
        // a cross-type pair of individually-orderable elements
        // (`[1, "x"]`), the newly-visited `a` that diverged from the
        // incumbent.
        _ => {
            let orderable = |v: &Value| {
                matches!(
                    v,
                    Value::Int(_)
                        | Value::Float(_)
                        | Value::Bool(_)
                        | Value::String(_)
                        | Value::Array(_)
                )
            };
            if orderable(a) && !orderable(b) {
                Err(not_orderable(b))
            } else {
                Err(not_orderable(a))
            }
        }
    }
}

/// NS-A4 (§4b): the DEV-mode pre-scan — fault on any float NaN comparand
/// in an ordering context, recursively through nested arrays ("same NaN
/// rule inside"). Run by `sort`/`sorted`/`min`/`max` before any
/// comparison; `sort_by`/`sorted_by` deliberately do NOT run it (F14: the
/// comparator owns the element semantics). PROD mode never calls this.
fn nan_scan(verb: &'static str, items: &[Value], depth: u32) -> Result<(), RuntimeError> {
    if depth >= ORDERING_NEST_LIMIT {
        return Err(RuntimeError::NotOrderable {
            verb,
            found: "an array nested past the ordering depth limit",
        });
    }
    for item in items {
        match item {
            Value::Float(f) if f.is_nan() => {
                return Err(RuntimeError::UnorderedComparand { verb });
            }
            Value::Array(inner) => nan_scan(verb, inner, depth + 1)?,
            _ => {}
        }
    }
    Ok(())
}

/// A fallible, stable, bottom-up merge sort over [`Value`]s. Exists
/// because both ordering comparators can fault mid-sort (`total_order_cmp`
/// on an unorderable element; a `sort_by` user comparator on anything its
/// body faults on), which rules out `slice::sort_by` (whose comparator is
/// infallible and which may panic on a detected non-total order — panics
/// are denied here). The §4b guarantee floor holds by construction: the
/// output is always some permutation of the input, and on success it is
/// the stable ascending order (equal elements keep their input order —
/// the left run wins ties).
///
/// `cmp(a, b)` is called with `a` from the earlier (left) run and `b`
/// from the later (right) run, in the source argument order a user
/// comparator expects.
pub(crate) fn fallible_stable_sort<F>(items: &mut [Value], cmp: &mut F) -> Result<(), RuntimeError>
where
    F: FnMut(&Value, &Value) -> Result<core::cmp::Ordering, RuntimeError>,
{
    let n = items.len();
    if n <= 1 {
        return Ok(());
    }
    let mut src: Vec<Value> = items.to_vec();
    let mut dst: Vec<Value> = items.to_vec();
    let mut width = 1usize;
    while width < n {
        let mut start = 0usize;
        while start < n {
            let mid = usize::min(start + width, n);
            let end = usize::min(start + 2 * width, n);
            let (mut l, mut r, mut o) = (start, mid, start);
            while l < mid && r < end {
                // Stable: the left element wins ties.
                if cmp(&src[l], &src[r])? == core::cmp::Ordering::Greater {
                    dst[o] = src[r].clone();
                    r += 1;
                } else {
                    dst[o] = src[l].clone();
                    l += 1;
                }
                o += 1;
            }
            while l < mid {
                dst[o] = src[l].clone();
                l += 1;
                o += 1;
            }
            while r < end {
                dst[o] = src[r].clone();
                r += 1;
                o += 1;
            }
            start = end;
        }
        core::mem::swap(&mut src, &mut dst);
        width *= 2;
    }
    items.clone_from_slice(&src);
    Ok(())
}

/// `SeqSorted`: `[a]` → `[a']` — the array sorted ascending by the §4b
/// doctrine order ([`total_order_cmp`]), stable. One op serves `sort(a)`
/// (statement-only; in-place-ness comes from the codegen RMW write-back)
/// and `sorted(a)` (functional) — the `RandShuffle` precedent, so faults
/// name the doctrine verb `sort`. DEV mode runs the [`nan_scan`] pre-scan
/// (NaN comparand → [`RuntimeError::UnorderedComparand`]); PROD mode
/// places NaN by the pinned total order and keeps moving.
pub(crate) fn seq_sorted(flow: &mut Flow) -> Result<(), RuntimeError> {
    let mode = flow.exec_mode;
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "sort",
            expected: "an array",
            found: type_name(&container),
        });
    };
    if mode == ExecMode::Dev {
        nan_scan("sort", items, 0)?;
    }
    let mut sorted: Vec<Value> = items.as_ref().clone();
    fallible_stable_sort(&mut sorted, &mut |a, b| total_order_cmp("sort", a, b))?;
    flow.value_stack.push(Value::array(sorted));
    Ok(())
}

/// The §4b pinned prod float order: IEEE `partial_cmp` where it's defined
/// (which already makes `-0 == +0` a tie), NaN greater than everything,
/// NaN-vs-NaN a tie.
fn pinned_float_cmp(x: f32, y: f32) -> core::cmp::Ordering {
    use core::cmp::Ordering;

    match x.partial_cmp(&y) {
        Some(ord) => ord,
        None => match (x.is_nan(), y.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            // partial_cmp only returns None when at least one side is NaN.
            (false, _) => Ordering::Less,
        },
    }
}

/// `SeqPop`: `[a]` → pushes `Option[T]` (the removed last element, `none`
/// on empty), then the shrunk array on top of it — stack-ordered so the
/// codegen bracket (`Take*` … `SeqPop` … `Set*`) stores the array back to
/// its root cell and leaves the Option as the expression value.
pub(crate) fn seq_pop(flow: &mut Flow) -> Result<(), RuntimeError> {
    let mut container = flow.pop_value()?;
    if !matches!(container, Value::Array(_)) {
        return Err(RuntimeError::StdlibWrongType {
            verb: "pop",
            expected: "an array",
            found: type_name(&container),
        });
    }
    note_array_mutation(&container);
    let popped = container
        .array_make_mut()
        .and_then(Vec::pop)
        .map_or_else(Value::none, Value::some);
    flow.value_stack.push(popped);
    flow.value_stack.push(container);
    Ok(())
}

/// `MapGetOpt`: `[m, k]` → `Option[V]` — the non-faulting map read
/// (`get(m, k)`, §5; martyr #3 redeemed). A missing key is absence
/// (`none`); a key outside the ratified int/string/bool key domain is a
/// malformed question and faults, matching `MapGet`/`IndexGet`'s key
/// handling (unlike `contains`, whose ruled totality has no failure mode
/// to escalate).
pub(crate) fn map_get_opt(flow: &mut Flow) -> Result<(), RuntimeError> {
    let key = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Map(map) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "get",
            expected: "a map",
            found: type_name(&container),
        });
    };
    let map_key = to_map_key(&key)?;
    let result = map
        .get(&map_key)
        .map_or_else(Value::none, |v| Value::some(v.clone()));
    flow.value_stack.push(result);
    Ok(())
}

/// `MapContainsValue`: `[m, v]` → `Bool` — content-equality scan over the
/// map's values (§5: "total, O(n) and honest about it"). The
/// `contains_key`/`contains_value` pair kills the ambiguity bare
/// `contains` would carry on maps.
pub(crate) fn map_contains_value(flow: &mut Flow) -> Result<(), RuntimeError> {
    let needle = flow.pop_value()?;
    let container = flow.pop_value()?;
    let Value::Map(map) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "contains_value",
            expected: "a map",
            found: type_name(&container),
        });
    };
    let found = map.values().any(|v| v == &needle);
    flow.value_stack.push(Value::Bool(found));
    Ok(())
}

/// `MapClear`: `[m]` → empty map. The `clear(m)` statement-only mutator's
/// primitive (§5: in-place, total); in-place-ness comes from the RMW
/// write-back bracket exactly like `MapInsert`/`MapRemove`.
pub(crate) fn map_clear(flow: &mut Flow) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    if !matches!(container, Value::Map(_)) {
        return Err(RuntimeError::StdlibWrongType {
            verb: "clear",
            expected: "a map",
            found: type_name(&container),
        });
    }
    flow.value_stack.push(Value::map(OrderedMap::new()));
    Ok(())
}

// ── NS-A7: `Weighted[T]` + the humble heap (`docs/stdlib-spec.md` §8,
// issue #1113). The heap verbs maintain a MIN-HEAP invariant over an
// ordinary `[T]` (zero new value kinds — the Lua posture), ordered by the
// same §4b comparison core the sort family uses (`total_order_cmp` — one
// comparison path, mode-free). `Collect(WeightedNew)` is the one producer
// of `Value::Weighted`; `rand::roll`'s op lives in `rand_ops` (it draws).

/// `Collect(WeightedNew)`: `[pairs]` → `Weighted[T]`. Pops ONE array of
/// flattened `weight, value, …` entries (a transient artifact of the
/// codegen `ArrayNew` bracket — never observable) and validates the §8
/// evidence-by-construction invariant: non-empty, even pair row, every
/// weight a positive int. Weights the checker could classify are already
/// E120 compile errors; what reaches this op is the computed-weight
/// residual ([`RuntimeError::WeightedBadWeight`]).
pub(crate) fn weighted_new(flow: &mut Flow) -> Result<(), RuntimeError> {
    let row = flow.pop_value()?;
    let Value::Array(items) = &row else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "weighted",
            expected: "a flattened weight/value pair row",
            found: type_name(&row),
        });
    };
    if items.is_empty() {
        return Err(RuntimeError::WeightedMalformedTable {
            detail: "an empty table",
        });
    }
    if items.len() % 2 != 0 {
        return Err(RuntimeError::WeightedMalformedTable {
            detail: "an odd flattened pair row",
        });
    }
    let mut entries = Vec::with_capacity(items.len() / 2);
    for pair in items.chunks_exact(2) {
        let weight = match &pair[0] {
            Value::Int(w) if *w >= 1 => *w,
            Value::Int(w) => {
                return Err(RuntimeError::WeightedBadWeight {
                    found: w.to_string(),
                });
            }
            Value::Float(f) => {
                return Err(RuntimeError::WeightedBadWeight {
                    found: f.to_string(),
                });
            }
            other => {
                return Err(RuntimeError::WeightedBadWeight {
                    found: type_name(other).to_string(),
                });
            }
        };
        entries.push((weight, pair[1].clone()));
    }
    flow.value_stack.push(Value::weighted(entries));
    Ok(())
}

/// `Collect(HeapPush)`: `[a, x]` → `[a']` — append `x` and sift up,
/// restoring the min-heap invariant (assuming, per the humble-heap
/// posture, that `a` already satisfies it — the invariant "holds over
/// clean data" because every entry arrived through this op). §4b entry
/// check: DEV mode faults on a NaN anywhere in the entering element
/// ([`nan_scan`], the `sort`/`min`/`max` discipline applied at the door);
/// PROD mode places NaN by the pinned total order. The array itself is
/// NOT re-scanned — the entry check is the ruled contract, and clean
/// arrays stay clean by induction. In-place-ness comes from the RMW
/// write-back (the `SeqSorted` precedent).
pub(crate) fn heap_push(flow: &mut Flow) -> Result<(), RuntimeError> {
    let mode = flow.exec_mode;
    let element = flow.pop_value()?;
    let mut container = flow.pop_value()?;
    if !matches!(container, Value::Array(_)) {
        return Err(RuntimeError::StdlibWrongType {
            verb: "heap_push",
            expected: "an array",
            found: type_name(&container),
        });
    }
    if mode == ExecMode::Dev {
        nan_scan("heap_push", core::slice::from_ref(&element), 0)?;
    }
    note_array_mutation(&container);
    if let Some(items) = container.array_make_mut() {
        items.push(element);
        // Sift up: swaps only, so the §4b guarantee floor (always some
        // permutation of the input) holds even if a comparison faults
        // mid-sift on an unorderable element.
        let mut i = items.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if total_order_cmp("heap_push", &items[i], &items[parent])? == core::cmp::Ordering::Less
            {
                items.swap(i, parent);
                i = parent;
            } else {
                break;
            }
        }
    }
    flow.value_stack.push(container);
    Ok(())
}

/// `Collect(HeapPop)`: `[a]` → pushes `Option[T]` (the extracted minimum,
/// `none` on empty — absence, per the ruled doctrine), then the shrunk
/// re-heapified array on top of it — the `SeqPop` stack contract, so the
/// codegen take/store bracket writes the array back and leaves the Option
/// as the expression value. Not in the §4b dev NaN-fault list: extraction
/// compares with the mode-free pinned order and keeps moving (the fault
/// belongs at `heap_push`, the door).
pub(crate) fn heap_pop(flow: &mut Flow) -> Result<(), RuntimeError> {
    let mut container = flow.pop_value()?;
    if !matches!(container, Value::Array(_)) {
        return Err(RuntimeError::StdlibWrongType {
            verb: "heap_pop",
            expected: "an array",
            found: type_name(&container),
        });
    }
    note_array_mutation(&container);
    let mut popped = Value::none();
    if let Some(items) = container.array_make_mut()
        && !items.is_empty()
    {
        let last = items.len() - 1;
        items.swap(0, last);
        // `pop` cannot fail: `items` is non-empty by the guard above.
        if let Some(min) = items.pop() {
            popped = Value::some(min);
        }
        // Sift down from the root (swaps only — same permutation floor as
        // `heap_push`).
        let n = items.len();
        let mut i = 0usize;
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut smallest = i;
            if l < n
                && total_order_cmp("heap_pop", &items[l], &items[smallest])?
                    == core::cmp::Ordering::Less
            {
                smallest = l;
            }
            if r < n
                && total_order_cmp("heap_pop", &items[r], &items[smallest])?
                    == core::cmp::Ordering::Less
            {
                smallest = r;
            }
            if smallest == i {
                break;
            }
            items.swap(i, smallest);
            i = smallest;
        }
    }
    flow.value_stack.push(popped);
    flow.value_stack.push(container);
    Ok(())
}

/// `Collect(HeapPeek)`: `[a]` → `Option[T]` — the minimum without
/// extraction (`none` on empty). Over a heap-maintained array the minimum
/// IS the root element, so this is a pure O(1) read with no comparisons
/// (and therefore no ordering faults — its only fault is a non-array).
pub(crate) fn heap_peek(flow: &mut Flow) -> Result<(), RuntimeError> {
    let container = flow.pop_value()?;
    let Value::Array(items) = &container else {
        return Err(RuntimeError::StdlibWrongType {
            verb: "heap_peek",
            expected: "an array",
            found: type_name(&container),
        });
    };
    let result = items
        .first()
        .map_or_else(Value::none, |v| Value::some(v.clone()));
    flow.value_stack.push(result);
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

/// Record a COW-copy event if mutating `container` (an `Array`) via the
/// next `array_make_mut()` call will find a shared `Arc` and pay the O(n)
/// copy — must be called *before* `array_make_mut`, since that call itself
/// performs the clone (issue #821 Workstream B seed). No-op — and the
/// `Arc::strong_count` check itself compiles out — unless the
/// `bench-counters` feature is enabled.
#[cfg(feature = "bench-counters")]
#[inline]
fn note_array_mutation(container: &Value) {
    if let Value::Array(items) = container
        && alloc::sync::Arc::strong_count(items) > 1
    {
        crate::bench_counters::record_cow_copy();
    }
}
#[cfg(not(feature = "bench-counters"))]
#[inline(always)]
fn note_array_mutation(_container: &Value) {}

/// Same as [`note_array_mutation`] for `Map` containers.
#[cfg(feature = "bench-counters")]
#[inline]
fn note_map_mutation(container: &Value) {
    if let Value::Map(map) = container
        && alloc::sync::Arc::strong_count(map) > 1
    {
        crate::bench_counters::record_cow_copy();
    }
}
#[cfg(not(feature = "bench-counters"))]
#[inline(always)]
fn note_map_mutation(_container: &Value) {}

pub(crate) fn type_name(v: &Value) -> &'static str {
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
        Value::Record { .. } => "record",
        Value::FnRef(_) | Value::Closure(_) => "fn",
        Value::Handle { .. } => "handle",
        Value::Projection(_) => "projection",
        Value::OptionVal(_) => "option",
        Value::Range { .. } => "range",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Vec4(_) => "vec4",
        Value::Quat(_) => "quat",
        Value::Mat2(_) => "mat2",
        Value::Mat3(_) => "mat3",
        Value::Mat4(_) => "mat4",
        Value::Weighted(_) => "weighted",
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
/// `pub(crate)`: also reused by [`crate::proj_ops`] for the array/map legs
/// of a path-projection read walk (`docs/t1e-spec.md` §3/§4) — the identical
/// bounds/key-exists semantics, just reachable through a projection instead
/// of a direct `[…]` expression.
pub(crate) fn read_index<'a>(
    container: &'a Value,
    index: &Value,
) -> Result<&'a Value, RuntimeError> {
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

/// How [`write_index_impl`] handles a map write whose key isn't already
/// present.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MissingMapKey {
    /// Fault (`RuntimeError::MapKeyNotFound`) — [`write_index`]'s behavior,
    /// used by path-projection writes.
    Fault,
    /// Insert the key (JS/Python semantics) — [`write_index_upsert`]'s
    /// behavior, used by the `IndexSet` opcode (issue #856).
    Insert,
}

/// Write `container[index] = value` in place via the take → `make_mut` →
/// write-back discipline. Array writes never grow past the end (no
/// out-of-bounds write ever succeeds). Map writes never insert a new key —
/// missing key is a fault (see `RuntimeError::MapKeyNotFound`'s doc). This
/// is the STRICT variant: `pub(crate)`, reused verbatim by
/// [`crate::proj_ops`] for path-projection writes, which must keep this
/// fault-on-missing-key behavior per `docs/t1e-spec.md` §4 ("a missing key
/// ... at write time is the §1(2) fault, consistent with §11c") — see
/// [`read_index`]. For the `IndexSet` opcode's insert-on-absent semantics
/// (issue #856), see [`write_index_upsert`] instead.
pub(crate) fn write_index(
    container: &mut Value,
    index: &Value,
    value: Value,
) -> Result<(), RuntimeError> {
    write_index_impl(container, index, value, MissingMapKey::Fault)
}

/// Write `container[index] = value`, inserting a new map key on a miss
/// (JS/Python semantics, issue #856, ruled 2026-07-15) instead of faulting.
/// Array bounds behavior is unchanged from [`write_index`] (never grows
/// past the end — no silent growth on write-past-end). Used only by
/// `IndexSet` (indexed assignment's assign-path opcode, `index_set`); reads
/// (`IndexGet`/`MapGet`) and path-projection writes still fault on a
/// missing key per value-model-spec §11c / t1e-spec.md §4.
pub(crate) fn write_index_upsert(
    container: &mut Value,
    index: &Value,
    value: Value,
) -> Result<(), RuntimeError> {
    write_index_impl(container, index, value, MissingMapKey::Insert)
}

fn write_index_impl(
    container: &mut Value,
    index: &Value,
    value: Value,
    on_missing: MissingMapKey,
) -> Result<(), RuntimeError> {
    match container {
        Value::Array(_) => {
            let len = container.as_array().map_or(0, |items| items.len());
            let i = array_index(index, len)?;
            note_array_mutation(container);
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
            if on_missing == MissingMapKey::Fault {
                let has_key = container.as_map().is_some_and(|map| map.contains_key(&key));
                if !has_key {
                    return Err(RuntimeError::MapKeyNotFound {
                        key: map_key_display(&key),
                    });
                }
            }
            note_map_mutation(container);
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
/// used by `IndexGet`/`IndexSet`/`SeqRemoveAt`, none of which ever grow the
/// array.
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
// compiler emits `MapInsert`/`MapContains` for their generalized array/map
// semantics, and (as of issue #1484) `MapRemove` (map-only) / `SeqRemoveAt`
// (array-only) for the split `remove`/`remove_at` pair — these tests prove
// them directly against hand-assembled `Value` trees, one level below full
// bytecode — the same "op function, not full VM" granularity T1b-2's
// fault-semantics tests used. End-to-end compile+run coverage (source
// `.ink` -> bytecode -> VM) lives in the `tests/tier1-brink` corpus wing
// and `brink-test-harness`'s T1b property
// tests, which exercise these same primitives via the real
// `push`/`insert`/`remove`/`remove_at`/`contains` call sites.
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
            ran_out_of_content_cause: crate::RanOutOfContentCause::default(),
            exec_mode: crate::story::ExecMode::default(),
            pure_callback: crate::story::PureCallbackState::default(),
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

    // ── CollectionLen: string arm (issue #1171) ───────────────────────────

    #[test]
    fn collection_len_string_counts_chars_not_bytes() {
        // "café" is 4 USVs but 5 UTF-8 bytes — proves the char-count
        // semantics, not `str::len`.
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::from("café")]);
        collection_len(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(result, Value::Int(4));
    }

    #[test]
    fn collection_len_string_ascii() {
        // {len("cider")} — the issue's minimal repro.
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::from("cider")]);
        collection_len(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn collection_len_empty_string_is_zero() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::from("")]);
        collection_len(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(result, Value::Int(0));
    }

    // ── IndexSet: map[newKey] = value inserts (issue #856, ruled 2026-07-15) ──

    #[test]
    fn index_set_map_fresh_key_inserts() {
        // memo[k] = v on a fresh key: no fault, key present afterward
        // (JS/Python semantics — the whole point of #856).
        let mut map = OrderedMap::new();
        map.insert(MapKey::Str(Arc::from("a")), Value::Int(1));
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![Value::map(map), Value::from("fresh"), Value::Int(99)],
        );
        index_set(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        let Value::Map(m) = result else {
            unreachable!("index_set on a map must return a map")
        };
        assert_eq!(m.len(), 2);
        assert_eq!(
            m.get(&MapKey::Str(Arc::from("fresh"))),
            Some(&Value::Int(99))
        );
        assert_eq!(m.get(&MapKey::Str(Arc::from("a"))), Some(&Value::Int(1)));
    }

    #[test]
    fn index_set_map_existing_key_overwrites() {
        let mut map = OrderedMap::new();
        map.insert(MapKey::Str(Arc::from("a")), Value::Int(1));
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![Value::map(map), Value::from("a"), Value::Int(42)],
        );
        index_set(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        let Value::Map(m) = result else {
            unreachable!("index_set on a map must return a map")
        };
        assert_eq!(m.len(), 1, "overwrite must not grow the map");
        assert_eq!(m.get(&MapKey::Str(Arc::from("a"))), Some(&Value::Int(42)));
    }

    #[test]
    fn index_set_map_invalid_key_type_still_faults() {
        // The insert-on-absent ruling only waives the *presence* check —
        // the key-domain check (int/string/bool) is unaffected.
        let mut map = OrderedMap::new();
        map.insert(MapKey::Int(1), Value::Int(1));
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![Value::map(map), Value::Float(3.5), Value::Int(9)],
        );
        let err = index_set(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::InvalidMapKeyType("float"));
    }

    #[test]
    fn index_set_array_out_of_bounds_still_faults() {
        // Array indexed-assignment is untouched by #856: still no silent
        // growth on write-past-end.
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![Value::Int(1), Value::Int(2)]),
                Value::Int(5),
                Value::Int(9),
            ],
        );
        let err = index_set(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::IndexOutOfBounds { index: 5, len: 2 });
    }

    #[test]
    fn write_index_strict_still_faults_on_missing_key() {
        // `write_index` (the path-projection variant, `docs/t1e-spec.md`
        // §4) is untouched by #856 — only `write_index_upsert` (IndexSet's
        // implementation) gained insert-on-absent.
        let mut container = Value::map(OrderedMap::new());
        let err = write_index(&mut container, &Value::from("k"), Value::Int(1)).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::MapKeyNotFound {
                key: "k".to_string()
            }
        );
    }

    // ── MapRemove / SeqRemoveAt split (issue #1484) ───────────────────────
    //
    // `MapRemove` used to generalize over Array (index-based, faulting) the
    // same way `MapInsert`/`MapContains` still do. Issue #1484 splits that
    // one name into two: `MapRemove` is map-only now, and the array-index
    // leg moves to its own primitive, `SeqRemoveAt`.

    #[test]
    fn seq_remove_at_array_shifts_elements_left() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![
                arr(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                Value::Int(1),
            ],
        );
        seq_remove_at(&mut flow).unwrap();
        let result = flow.pop_value().unwrap();
        assert_eq!(result, arr(vec![Value::Int(1), Value::Int(3)]));
    }

    #[test]
    fn seq_remove_at_index_equal_to_len_faults() {
        // Unlike MapInsert's push-friendly `<= len`, remove_at has no
        // element to remove at `len` — strictly `< len`, same as
        // IndexGet/IndexSet.
        let mut flow = test_flow();
        push_args(&mut flow, vec![arr(vec![Value::Int(1)]), Value::Int(1)]);
        let err = seq_remove_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::IndexOutOfBounds { index: 1, len: 1 });
    }

    #[test]
    fn seq_remove_at_on_a_map_faults() {
        // The split's whole point: `remove_at` no longer accepts a map —
        // that's `remove`'s domain now.
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![Value::map(OrderedMap::new()), Value::Int(0)],
        );
        let err = seq_remove_at(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::NotIndexable("map"));
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

    #[test]
    fn map_remove_on_an_array_faults() {
        // The split's other half: `remove` no longer accepts an array —
        // that's `remove_at`'s domain now.
        let mut flow = test_flow();
        push_args(&mut flow, vec![arr(vec![Value::Int(1)]), Value::Int(0)]);
        let err = map_remove(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::NotIndexable("array"));
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
    fn seq_remove_at_cows_when_shared() {
        let original = arr(vec![Value::Int(1), Value::Int(2)]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        push_args(&mut flow, vec![original, Value::Int(0)]);
        seq_remove_at(&mut flow).unwrap();
        let mutated = flow.pop_value().unwrap();
        assert_eq!(
            snapshot,
            arr(vec![Value::Int(1), Value::Int(2)]),
            "snapshot unmutated"
        );
        assert_eq!(mutated, arr(vec![Value::Int(2)]));
    }

    // ── NS-A1 Option verb flips (docs/stdlib-spec.md §§4-5, #1107) ───────

    fn ints(ns: &[i32]) -> Value {
        arr(ns.iter().map(|n| Value::Int(*n)).collect())
    }

    #[test]
    fn seq_index_of_finds_first_occurrence_and_none_when_absent() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[7, 8, 7]), Value::Int(7)]);
        seq_index_of(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(0)));

        push_args(&mut flow, vec![ints(&[7, 8]), Value::Int(9)]);
        seq_index_of(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());
    }

    #[test]
    fn seq_index_of_uses_structural_equality() {
        // Element equality is content equality — a nested array needle
        // matches structurally, never by identity.
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![ints(&[1, 2]), ints(&[3])]), ints(&[3])],
        );
        seq_index_of(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(1)));
    }

    #[test]
    fn seq_index_of_on_non_array_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Int(1), Value::Int(1)]);
        let err = seq_index_of(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "index_of",
                expected: "an array",
                found: "int",
            }
        );
    }

    #[test]
    fn seq_first_last_on_empty_are_none() {
        for op in [seq_first, seq_last] {
            let mut flow = test_flow();
            push_args(&mut flow, vec![ints(&[])]);
            op(&mut flow).unwrap();
            assert_eq!(flow.pop_value().unwrap(), Value::none());
        }
    }

    #[test]
    fn seq_first_last_pick_the_edges() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[4, 5, 6])]);
        seq_first(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(4)));
        push_args(&mut flow, vec![ints(&[4, 5, 6])]);
        seq_last(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(6)));
    }

    #[test]
    fn seq_min_max_over_ints_and_empty() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[3, 1, 2])]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(1)));
        push_args(&mut flow, vec![ints(&[3, 1, 2])]);
        seq_max(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(3)));
        push_args(&mut flow, vec![ints(&[])]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());
    }

    #[test]
    fn seq_min_max_promote_mixed_numerics_and_return_the_element() {
        // [2, 1.5] — min is the float, max is the int; the *element* comes
        // back unwidened (an Int stays an Int).
        let mixed = || arr(vec![Value::Int(2), Value::Float(1.5)]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![mixed()]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Float(1.5)));
        push_args(&mut flow, vec![mixed()]);
        seq_max(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(2)));
    }

    #[test]
    fn seq_min_max_order_strings_and_bools() {
        let strs = arr(vec![Value::from("pear"), Value::from("apple")]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![strs]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::from("apple")));

        let bools = arr(vec![Value::Bool(true), Value::Bool(false)]);
        push_args(&mut flow, vec![bools]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Bool(false)));
    }

    #[test]
    fn seq_min_ties_keep_the_first_occurrence() {
        // -0.0 and +0.0 tie under the pinned order; the first stays.
        let a = arr(vec![Value::Float(-0.0), Value::Float(0.0)]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![a]);
        seq_min(&mut flow).unwrap();
        let Value::OptionVal(Some(v)) = flow.pop_value().unwrap() else {
            unreachable!("min of a non-empty float array is some");
        };
        let Value::Float(f) = *v else {
            unreachable!("element is a float");
        };
        assert!(f.is_sign_negative(), "first (-0.0) kept on tie");
    }

    #[test]
    fn seq_max_places_nan_greatest_per_the_pinned_prod_order() {
        // §4b PROD mode: NaN greater than everything, execution keeps
        // moving. (NS-A4: dev mode faults instead — the test below.)
        let a = arr(vec![Value::Float(1.0), Value::Float(f32::NAN)]);
        let mut flow = test_flow();
        flow.exec_mode = ExecMode::Prod;
        push_args(&mut flow, vec![a.clone()]);
        seq_max(&mut flow).unwrap();
        let Value::OptionVal(Some(v)) = flow.pop_value().unwrap() else {
            unreachable!("max of a non-empty float array is some");
        };
        let Value::Float(f) = *v else {
            unreachable!("element is a float");
        };
        assert!(f.is_nan(), "NaN sorts greatest");

        push_args(&mut flow, vec![a]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Float(1.0)));
    }

    /// NS-A4 (§4b): DEV mode (the default) faults on a NaN comparand in an
    /// ordering context — comparison or no comparison (`min([nan])` never
    /// compares, and NaN nested inside an array element is the same
    /// upstream bug).
    #[test]
    fn seq_extremum_dev_mode_faults_on_nan_comparand() {
        let mut flow = test_flow();
        assert_eq!(flow.exec_mode, ExecMode::Dev, "dev is the default");

        push_args(&mut flow, vec![arr(vec![Value::Float(f32::NAN)])]);
        let err = seq_min(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::UnorderedComparand { verb: "min" });

        // Nested: "same NaN rule inside" (§4b).
        push_args(
            &mut flow,
            vec![arr(vec![arr(vec![Value::Float(f32::NAN)]), ints(&[1])])],
        );
        let err = seq_max(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::UnorderedComparand { verb: "max" });

        // NaN-free floats are fine in dev — the modes agree on clean data.
        push_args(
            &mut flow,
            vec![arr(vec![Value::Float(2.0), Value::Float(-1.0)])],
        );
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Float(-1.0)));
    }

    // ── NS-A4: `SeqSorted` (`sort`/`sorted`) ──────────────────────────

    #[test]
    fn seq_sorted_orders_ints_stably_ascending() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[3, 1, 2, 1])]);
        seq_sorted(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), ints(&[1, 1, 2, 3]));
    }

    #[test]
    fn seq_sorted_orders_strings_and_mixed_numerics() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![
                Value::from("pear"),
                Value::from("apple"),
                Value::from("fig"),
            ])],
        );
        seq_sorted(&mut flow).unwrap();
        assert_eq!(
            flow.pop_value().unwrap(),
            arr(vec![
                Value::from("apple"),
                Value::from("fig"),
                Value::from("pear"),
            ])
        );

        // Mixed int/float promote for comparison; elements come back
        // unwidened.
        push_args(
            &mut flow,
            vec![arr(vec![Value::Int(2), Value::Float(1.5), Value::Int(1)])],
        );
        seq_sorted(&mut flow).unwrap();
        assert_eq!(
            flow.pop_value().unwrap(),
            arr(vec![Value::Int(1), Value::Float(1.5), Value::Int(2)])
        );
    }

    /// Stability: `-0.0` and `+0.0` tie under the pinned order (`-0 ==
    /// +0`), so their input order survives the sort.
    #[test]
    fn seq_sorted_is_stable_across_pinned_ties() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![
                Value::Float(0.0),
                Value::Float(-0.0),
                Value::Float(-1.0),
            ])],
        );
        seq_sorted(&mut flow).unwrap();
        let Value::Array(items) = flow.pop_value().unwrap() else {
            unreachable!("sorted returns an array");
        };
        let signs: Vec<bool> = items
            .iter()
            .map(|v| {
                let Value::Float(f) = v else {
                    unreachable!("floats in, floats out")
                };
                f.is_sign_negative()
            })
            .collect();
        // -1.0 first, then +0.0 before -0.0 (input order preserved on tie).
        assert_eq!(signs, vec![true, false, true]);
    }

    #[test]
    fn seq_sorted_orders_arrays_lexicographically() {
        let mut flow = test_flow();
        push_args(
            &mut flow,
            vec![arr(vec![ints(&[2]), ints(&[1, 5]), ints(&[1])])],
        );
        seq_sorted(&mut flow).unwrap();
        assert_eq!(
            flow.pop_value().unwrap(),
            arr(vec![ints(&[1]), ints(&[1, 5]), ints(&[2])])
        );
    }

    /// NS-A4 (§4b): the dev/prod split on `sort` — dev faults on a NaN
    /// comparand, prod places it by the pinned total order (NaN greatest)
    /// and keeps moving. Same array, same op, mode decides WHERE execution
    /// stops — never the placement of the clean elements.
    #[test]
    fn seq_sorted_dev_faults_prod_places_nan() {
        let a = || {
            arr(vec![
                Value::Float(f32::NAN),
                Value::Float(1.0),
                Value::Float(-1.0),
            ])
        };
        let mut flow = test_flow();
        push_args(&mut flow, vec![a()]);
        let err = seq_sorted(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::UnorderedComparand { verb: "sort" });

        flow.exec_mode = ExecMode::Prod;
        push_args(&mut flow, vec![a()]);
        seq_sorted(&mut flow).unwrap();
        let Value::Array(items) = flow.pop_value().unwrap() else {
            unreachable!("sorted returns an array");
        };
        assert_eq!(items[0], Value::Float(-1.0));
        assert_eq!(items[1], Value::Float(1.0));
        let Value::Float(last) = items[2] else {
            unreachable!("floats in, floats out")
        };
        assert!(last.is_nan(), "prod places NaN greatest");
    }

    #[test]
    fn seq_sorted_faults_on_non_array_and_unorderable_elements() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Int(3)]);
        let err = seq_sorted(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "sort",
                expected: "an array",
                found: "int",
            }
        );

        // Cross-type pair: individually orderable, jointly malformed.
        push_args(&mut flow, vec![arr(vec![Value::Int(1), Value::from("x")])]);
        let err = seq_sorted(&mut flow).unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::NotOrderable { verb: "sort", .. }
        ));
    }

    /// The empty and singleton arrays sort to themselves in both modes —
    /// and a singleton NaN still faults in dev (the operand IS the bug).
    #[test]
    fn seq_sorted_edge_shapes() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[])]);
        seq_sorted(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), ints(&[]));

        push_args(&mut flow, vec![arr(vec![Value::Float(f32::NAN)])]);
        let err = seq_sorted(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::UnorderedComparand { verb: "sort" });
    }

    /// NS-A3 (issue #1109, stdlib-spec §4b): structs have no structural
    /// auto-order — without a registered `compare` impl (none registrable
    /// v1) a record element stays `NotOrderable`. Wave A4 wires registered
    /// compares into `total_order_cmp`; this pins the pre-A4 line.
    #[test]
    fn seq_min_over_records_faults_not_orderable() {
        let p = Value::record(brink_format::ShapeId(0), vec![Value::Int(1)]);
        let q = Value::record(brink_format::ShapeId(0), vec![Value::Int(2)]);
        let a = arr(vec![p, q]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![a]);
        let err = seq_min(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::NotOrderable {
                verb: "min",
                found: "record",
            }
        );
    }

    #[test]
    fn seq_min_cross_type_elements_fault_not_orderable() {
        let a = arr(vec![Value::Int(1), Value::from("x")]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![a]);
        let err = seq_min(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::NotOrderable {
                verb: "min",
                found: "string",
            }
        );
    }

    #[test]
    fn seq_min_unorderable_element_type_faults() {
        // Maps are outside the §4b roster in every wave.
        let a = arr(vec![
            Value::Map(Arc::new(OrderedMap::new())),
            Value::Map(Arc::new(OrderedMap::new())),
        ]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![a]);
        let err = seq_min(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::NotOrderable {
                verb: "min",
                found: "map",
            }
        );
    }

    /// NS-A4 (§4b roster completion): arrays order lexicographically,
    /// element-wise, recursively — `min` over `[[1, 2], [1]]` is `[1]`
    /// (a full prefix ties to the shorter array).
    #[test]
    fn seq_min_orders_arrays_lexicographically() {
        let a = arr(vec![ints(&[1, 2]), ints(&[1])]);
        let mut flow = test_flow();
        push_args(&mut flow, vec![a]);
        seq_min(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(ints(&[1])));
    }

    #[test]
    fn seq_pop_pushes_option_then_shrunk_array() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[1, 2])]);
        seq_pop(&mut flow).unwrap();
        // Stack order: [popped-Option, shrunk-array] — array on top so the
        // codegen bracket's store-back pops it, leaving the Option.
        assert_eq!(flow.pop_value().unwrap(), ints(&[1]));
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(2)));
    }

    #[test]
    fn seq_pop_on_empty_is_none_and_keeps_the_empty_array() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![ints(&[])]);
        seq_pop(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), ints(&[]));
        assert_eq!(flow.pop_value().unwrap(), Value::none());
    }

    #[test]
    fn seq_pop_on_non_array_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::from("nope")]);
        let err = seq_pop(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "pop",
                expected: "an array",
                found: "string",
            }
        );
    }

    #[test]
    fn seq_pop_cows_when_shared() {
        let original = ints(&[1, 2]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        push_args(&mut flow, vec![original]);
        seq_pop(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), ints(&[1]));
        assert_eq!(snapshot, ints(&[1, 2]), "snapshot unmutated");
    }

    fn simple_map() -> Value {
        let mut m = OrderedMap::new();
        m.insert(MapKey::from("hp"), Value::Int(10));
        m.insert(MapKey::from("name"), Value::from("gob"));
        Value::map(m)
    }

    #[test]
    fn map_get_opt_present_absent_and_wrong_container() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![simple_map(), Value::from("hp")]);
        map_get_opt(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(10)));

        push_args(&mut flow, vec![simple_map(), Value::from("mp")]);
        map_get_opt(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());

        push_args(&mut flow, vec![ints(&[1]), Value::Int(0)]);
        let err = map_get_opt(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "get",
                expected: "a map",
                found: "array",
            }
        );
    }

    #[test]
    fn map_get_opt_non_scalar_key_is_a_malformed_question_fault() {
        // Unlike a *missing* key (absence -> none), a key outside the
        // int/string/bool domain can never be a key at all — a bug, so it
        // faults exactly like `m[k]`'s own key handling.
        let mut flow = test_flow();
        push_args(&mut flow, vec![simple_map(), ints(&[1])]);
        let err = map_get_opt(&mut flow).unwrap_err();
        assert_eq!(err, RuntimeError::InvalidMapKeyType("array"));
    }

    #[test]
    fn map_contains_value_scans_content_equality() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![simple_map(), Value::Int(10)]);
        map_contains_value(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(true));

        push_args(&mut flow, vec![simple_map(), Value::Int(11)]);
        map_contains_value(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Bool(false));

        push_args(&mut flow, vec![ints(&[10]), Value::Int(10)]);
        let err = map_contains_value(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "contains_value",
                expected: "a map",
                found: "array",
            }
        );
    }

    #[test]
    fn map_clear_empties_and_faults_on_non_map() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![simple_map()]);
        map_clear(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::map(OrderedMap::new()));

        push_args(&mut flow, vec![ints(&[1])]);
        let err = map_clear(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "clear",
                expected: "a map",
                found: "array",
            }
        );
    }

    // ── NS-A7: Weighted[T] + the humble heap (docs/stdlib-spec.md §8,
    // #1113) ─────────────────────────────────────────────────────────────

    /// Run `weighted_new` over a flattened pair row.
    fn weighted_from(flow: &mut Flow, row: Vec<Value>) -> Result<Value, RuntimeError> {
        push_args(flow, vec![arr(row)]);
        weighted_new(flow)?;
        flow.pop_value()
    }

    #[test]
    fn weighted_new_builds_multiset_in_construction_order() {
        let mut flow = test_flow();
        let w = weighted_from(
            &mut flow,
            vec![
                Value::Int(3),
                Value::String("sword".into()),
                Value::Int(3),
                Value::String("shield".into()),
            ],
        )
        .unwrap();
        // F17: duplicate weights are legal and meaningful (multiset).
        let Value::Weighted(table) = &w else {
            unreachable!("expected a Weighted, got {w:?}");
        };
        assert_eq!(table.entries.len(), 2);
        assert_eq!(table.entries[0], (3, Value::String("sword".into())));
        assert_eq!(table.entries[1], (3, Value::String("shield".into())));
        assert_eq!(table.total_weight(), 6);
    }

    #[test]
    fn weighted_equality_is_multiset_content_not_order() {
        let a = Value::weighted(vec![
            (3, Value::String("a".into())),
            (1, Value::String("b".into())),
        ]);
        let b = Value::weighted(vec![
            (1, Value::String("b".into())),
            (3, Value::String("a".into())),
        ]);
        let c = Value::weighted(vec![
            (3, Value::String("a".into())),
            (3, Value::String("a".into())),
        ]);
        assert_eq!(a, b, "order-insensitive");
        assert_ne!(a, c, "multiplicity-sensitive");
    }

    #[test]
    fn weighted_new_refuses_bad_computed_weights() {
        // The E078-style split's runtime half: zero, negative, and
        // non-int computed weights are construction faults.
        let mut flow = test_flow();
        let err = weighted_from(&mut flow, vec![Value::Int(0), Value::Int(1)]).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::WeightedBadWeight {
                found: "0".to_string()
            }
        );
        let err = weighted_from(&mut flow, vec![Value::Int(-3), Value::Int(1)]).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::WeightedBadWeight {
                found: "-3".to_string()
            }
        );
        let err = weighted_from(&mut flow, vec![Value::Float(1.5), Value::Int(1)]).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::WeightedBadWeight {
                found: "1.5".to_string()
            }
        );
        let err =
            weighted_from(&mut flow, vec![Value::String("w".into()), Value::Int(1)]).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::WeightedBadWeight {
                found: "string".to_string()
            }
        );
    }

    #[test]
    fn weighted_new_guards_malformed_pair_rows() {
        // Unreachable through the compiler (E120 refuses these shapes
        // statically) — the malformed-bytecode guards.
        let mut flow = test_flow();
        let err = weighted_from(&mut flow, vec![]).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::WeightedMalformedTable {
                detail: "an empty table"
            }
        );
        let err = weighted_from(&mut flow, vec![Value::Int(1)]).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::WeightedMalformedTable {
                detail: "an odd flattened pair row"
            }
        );
    }

    /// Drive `heap_push` as the codegen bracket would: `[a, x]` → `[a']`.
    fn push_heap(flow: &mut Flow, heap: Value, x: Value) -> Result<Value, RuntimeError> {
        push_args(flow, vec![heap, x]);
        heap_push(flow)?;
        flow.pop_value()
    }

    /// Drive `heap_pop`: `[a]` → `(popped Option, shrunk array)`.
    fn pop_heap(flow: &mut Flow, heap: Value) -> Result<(Value, Value), RuntimeError> {
        push_args(flow, vec![heap]);
        heap_pop(flow)?;
        let shrunk = flow.pop_value()?;
        let popped = flow.pop_value()?;
        Ok((popped, shrunk))
    }

    #[test]
    fn heap_property_push_n_pop_all_drains_ascending() {
        // The heap-invariant property test (the §8 gate): push N values in
        // a scrambled order, then pop until empty — the drain must come
        // out in exactly the §4b doctrine order (ascending, min-heap).
        let mut flow = test_flow();
        let values = [7, 3, 11, 3, -2, 0, 42, 5, 5, -100, 19, 1];
        let mut heap = arr(vec![]);
        for v in values {
            heap = push_heap(&mut flow, heap, Value::Int(v)).unwrap();
        }
        let mut drained = Vec::new();
        loop {
            let (popped, shrunk) = pop_heap(&mut flow, heap).unwrap();
            heap = shrunk;
            match popped {
                Value::OptionVal(None) => break,
                Value::OptionVal(Some(v)) => match v.as_ref() {
                    Value::Int(n) => drained.push(*n),
                    other => unreachable!("unexpected pop payload {other:?}"),
                },
                other => unreachable!("heap_pop must produce an Option, got {other:?}"),
            }
        }
        let mut expected = values.to_vec();
        expected.sort_unstable();
        assert_eq!(drained, expected, "min-heap drains ascending");
        assert_eq!(heap, arr(vec![]), "drained heap is empty");
    }

    #[test]
    fn heap_peek_reads_min_without_extraction() {
        let mut flow = test_flow();
        let mut heap = arr(vec![]);
        for v in [5, 2, 9] {
            heap = push_heap(&mut flow, heap, Value::Int(v)).unwrap();
        }
        push_args(&mut flow, vec![heap.clone()]);
        heap_peek(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::some(Value::Int(2)));
        // Empty peek is absence.
        push_args(&mut flow, vec![arr(vec![])]);
        heap_peek(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::none());
    }

    #[test]
    fn heap_pop_on_empty_is_none_not_fault() {
        let mut flow = test_flow();
        let (popped, shrunk) = pop_heap(&mut flow, arr(vec![])).unwrap();
        assert_eq!(popped, Value::none());
        assert_eq!(shrunk, arr(vec![]));
    }

    #[test]
    fn heap_push_dev_mode_faults_on_nan_entry() {
        // §4b: `heap_push` checks at entry — a NaN in the entering element
        // (bare or nested) is the dev-mode fault; the invariant then holds
        // over clean data.
        let mut flow = test_flow();
        assert_eq!(flow.exec_mode, ExecMode::Dev, "dev is the default");
        let err = push_heap(&mut flow, arr(vec![]), Value::Float(f32::NAN)).unwrap_err();
        assert_eq!(err, RuntimeError::UnorderedComparand { verb: "heap_push" });
        // Nested: "same NaN rule inside".
        let err = push_heap(&mut flow, arr(vec![]), arr(vec![Value::Float(f32::NAN)])).unwrap_err();
        assert_eq!(err, RuntimeError::UnorderedComparand { verb: "heap_push" });
        // NaN-free floats are fine in dev.
        let heap = push_heap(&mut flow, arr(vec![]), Value::Float(1.5)).unwrap();
        assert_eq!(heap, arr(vec![Value::Float(1.5)]));
    }

    #[test]
    fn heap_push_prod_mode_places_nan_by_the_pinned_order() {
        // PROD: NaN enters and places greatest (never fabricated, never
        // lost) — the §4b pinned total order, mode-free comparison core.
        let mut flow = test_flow();
        flow.exec_mode = ExecMode::Prod;
        let mut heap = arr(vec![]);
        for v in [Value::Float(2.0), Value::Float(f32::NAN), Value::Float(1.0)] {
            heap = push_heap(&mut flow, heap, v).unwrap();
        }
        let (popped, shrunk) = pop_heap(&mut flow, heap).unwrap();
        assert_eq!(popped, Value::some(Value::Float(1.0)));
        let (popped, shrunk) = pop_heap(&mut flow, shrunk).unwrap();
        assert_eq!(popped, Value::some(Value::Float(2.0)));
        let (popped, shrunk) = pop_heap(&mut flow, shrunk).unwrap();
        let Value::OptionVal(Some(v)) = popped else {
            unreachable!("expected some(NaN)");
        };
        let Value::Float(f) = v.as_ref() else {
            unreachable!("expected a float");
        };
        assert!(f.is_nan(), "NaN pops last (greatest), never dropped");
        assert_eq!(shrunk, arr(vec![]));
    }

    #[test]
    fn heap_verbs_fault_on_unorderable_elements_and_non_arrays() {
        let mut flow = test_flow();
        // Unorderable element reached by a sift comparison.
        let heap = arr(vec![Value::Int(1)]);
        let err = push_heap(&mut flow, heap, simple_map()).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::NotOrderable {
                verb: "heap_push",
                found: "map",
            }
        );
        // Non-array receivers are the malformed-question fault.
        let err = push_heap(&mut flow, Value::Int(1), Value::Int(2)).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "heap_push",
                expected: "an array",
                found: "int",
            }
        );
        push_args(&mut flow, vec![Value::Int(1)]);
        let err = heap_pop(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "heap_pop",
                expected: "an array",
                found: "int",
            }
        );
        push_args(&mut flow, vec![Value::Int(1)]);
        let err = heap_peek(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::StdlibWrongType {
                verb: "heap_peek",
                expected: "an array",
                found: "int",
            }
        );
    }

    #[test]
    fn heap_push_cows_when_shared() {
        // Take -> make_mut -> write-back (value-model-spec §5): mutating a
        // shared heap array must not observably affect the other holder.
        let original = ints(&[1, 3]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        let mutated = push_heap(&mut flow, original, Value::Int(0)).unwrap();
        assert_eq!(snapshot, ints(&[1, 3]), "snapshot unmutated");
        assert_eq!(mutated, ints(&[0, 3, 1]), "sifted to the root");
    }
}
