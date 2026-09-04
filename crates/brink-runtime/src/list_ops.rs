//! List opcode implementations.

use alloc::format;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use brink_format::{DefinitionId, ListValue, Value};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::state::ContextAccess;
use crate::story::Flow;

/// `ListContains` (`?`): `[lhs, rhs]` → `Bool(rhs ⊆ lhs)`, except that an
/// EMPTY operand on either side is `false` — ink's `InkList.Contains`
/// returns false when either list is empty rather than the vacuous
/// subset answer (issue #3531: `l ? ()` is `false`, `l !? ()` is `true`).
///
/// Also handles string operands: `"hello" ? "ell"` → substring check.
pub(crate) fn list_contains(flow: &mut Flow) -> Result<(), RuntimeError> {
    let rhs = flow.pop_value()?;
    let lhs = flow.pop_value()?;
    let result = if let (Value::String(a), Value::String(b)) = (&lhs, &rhs) {
        a.contains(&**b)
    } else {
        let rhs = to_list(rhs)?;
        let lhs = to_list(lhs)?;
        list_has(&lhs, &rhs)
    };
    flow.value_stack.push(Value::Bool(result));
    Ok(())
}

/// `ListNotContains` (`!?`): `[lhs, rhs]` → the negation of
/// [`list_contains`], empty operands included (`() !? ()` is `true`).
///
/// Also handles string operands.
pub(crate) fn list_not_contains(flow: &mut Flow) -> Result<(), RuntimeError> {
    let rhs = flow.pop_value()?;
    let lhs = flow.pop_value()?;
    let result = if let (Value::String(a), Value::String(b)) = (&lhs, &rhs) {
        !a.contains(&**b)
    } else {
        let rhs = to_list(rhs)?;
        let lhs = to_list(lhs)?;
        !list_has(&lhs, &rhs)
    };
    flow.value_stack.push(Value::Bool(result));
    Ok(())
}

/// ink's `InkList.Contains(other)`: false when either list is empty,
/// otherwise every item of `other` is in `list`.
fn list_has(list: &ListValue, other: &ListValue) -> bool {
    !list.items.is_empty()
        && !other.items.is_empty()
        && other.items.iter().all(|id| list.items.contains(id))
}

/// ink's `InkList.originNames` (issue #3532): a NON-EMPTY list's origins
/// are the definitions its items belong to, recomputed from the items on
/// every read — the stored `origins` field only speaks for an EMPTY list,
/// where it is what `LIST_ALL`, `LIST_INVERT` and `l(n)` enumerate.
pub(crate) fn effective_origins(program: &Program, lv: &ListValue) -> Vec<DefinitionId> {
    if lv.items.is_empty() {
        lv.origins.clone()
    } else {
        derive_origins(program, &lv.items)
    }
}

/// The distinct origin definitions of `items`, in first-seen order.
fn derive_origins(program: &Program, items: &[DefinitionId]) -> Vec<DefinitionId> {
    let mut origins = Vec::new();
    for &id in items {
        if let Some(entry) = program.list_item(id)
            && !origins.contains(&entry.origin)
        {
            origins.push(entry.origin);
        }
    }
    origins
}

/// Builds a list value the way ink's list operations do (issue #3532): a
/// non-empty result derives its origins from its items; an empty one
/// carries `empty_origins` — none for the operations that start from a
/// fresh `new InkList()` (`^`, `LIST_MIN`/`MAX`/`ALL`/`INVERT`, `+ int`),
/// the left operand's for `Union`/`Without` (`+`, `-`) and the input's
/// for a non-empty `LIST_RANGE`.
pub(crate) fn build(
    program: &Program,
    items: Vec<DefinitionId>,
    empty_origins: Vec<DefinitionId>,
) -> ListValue {
    let origins = if items.is_empty() {
        empty_origins
    } else {
        derive_origins(program, &items)
    };
    ListValue { items, origins }
}

/// ink's `RetainListOriginsForAssignment` (issue #3532): an EMPTY list
/// assigned over a list value takes over that old value's origins,
/// whatever origins the new value arrived with — so `~ l = m - m` on an
/// `l` declared from `LIST l` still enumerates `l`'s items under
/// `LIST_ALL`, and `~ l = m - m` on an `l` currently holding `(c)` from
/// `LIST m` enumerates `m`'s. Applies to globals and to `SetTemp`
/// (directly or through a `ref` pointer); `DeclareTemp` is left alone —
/// ink retains there too when a same-named temp already exists, but a
/// brink temp slot is reused across the knots of one frame, so the slot's
/// previous value is not evidence of a same-named temp.
pub(crate) fn retain_origins_on_assign(program: &Program, old: &Value, new: &mut Value) {
    if let Value::List(new_lv) = new
        && new_lv.items.is_empty()
        && let Value::List(old_lv) = old
    {
        Arc::make_mut(new_lv).origins = effective_origins(program, old_lv);
    }
}

/// `ListIntersect` (`L^`): `[a, b]` → `List(a ∩ b)` — a fresh list: no origins when empty.
pub(crate) fn list_intersect(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let b = pop_list(flow)?;
    let a = pop_list(flow)?;
    let items: Vec<_> = a
        .items
        .iter()
        .filter(|id| b.items.contains(id))
        .copied()
        .collect();
    flow.value_stack
        .push(Value::List(Arc::new(build(program, items, vec![]))));
    Ok(())
}

/// `ListCount`: `[list]` → `Int(len)`
#[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
pub(crate) fn list_count(flow: &mut Flow) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    flow.value_stack.push(Value::Int(lv.items.len() as i32));
    Ok(())
}

/// `ListMin`: `[list]` → `List(single item with lowest ordinal)` — a fresh list: no origins when empty.
pub(crate) fn list_min(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let min_item = lv
        .items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| (id, e.ordinal)))
        .min_by_key(|&(_, ord)| ord)
        .map(|(id, _)| id);
    let items = min_item.map_or_else(Vec::new, |id| vec![id]);
    flow.value_stack
        .push(Value::List(Arc::new(build(program, items, vec![]))));
    Ok(())
}

/// `ListMax`: `[list]` → `List(single item with highest ordinal)` — a fresh list: no origins when empty.
pub(crate) fn list_max(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let max_item = lv
        .items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| (id, e.ordinal)))
        .max_by_key(|&(_, ord)| ord)
        .map(|(id, _)| id);
    let items = max_item.map_or_else(Vec::new, |id| vec![id]);
    flow.value_stack
        .push(Value::List(Arc::new(build(program, items, vec![]))));
    Ok(())
}

/// `ListValue`: `[list]` → `Int(ordinal of single-item list)`
pub(crate) fn list_value(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let ordinal = if lv.items.len() == 1 {
        program.list_item(lv.items[0]).map_or(0, |e| e.ordinal)
    } else {
        0
    };
    flow.value_stack.push(Value::Int(ordinal));
    Ok(())
}

/// `ListAll`: `[list]` → `List(all items from origins)` — a fresh list: no origins when empty.
pub(crate) fn list_all(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let mut items = Vec::new();
    for origin_id in effective_origins(program, &lv) {
        if let Some(def) = program.list_def(origin_id) {
            for &item_id in &def.items {
                if !items.contains(&item_id) {
                    items.push(item_id);
                }
            }
        }
    }
    flow.value_stack
        .push(Value::List(Arc::new(build(program, items, vec![]))));
    Ok(())
}

/// `ListInvert`: `[list]` → `List(ALL \ list)` — complement within origins.
pub(crate) fn list_invert(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let mut items = Vec::new();
    for origin_id in effective_origins(program, &lv) {
        if let Some(def) = program.list_def(origin_id) {
            for &item_id in &def.items {
                if !lv.items.contains(&item_id) && !items.contains(&item_id) {
                    items.push(item_id);
                }
            }
        }
    }
    flow.value_stack
        .push(Value::List(Arc::new(build(program, items, vec![]))));
    Ok(())
}

/// `ListRange`: `[list, min, max]` → `List(items with ordinal in [min,max])`
///
/// Filters the list's *own* items by ordinal bounds (not all items from origins).
/// An empty input yields a fresh list (no origins); a non-empty one keeps
/// its origins even when the range filters every item out.
pub(crate) fn list_range(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let max_val = pop_int_or_list_ordinal(flow, program)?;
    let min_val = pop_int_or_list_ordinal(flow, program)?;
    let lv = pop_list(flow)?;
    let items: Vec<_> = lv
        .items
        .iter()
        .copied()
        .filter(|&id| {
            program
                .list_item(id)
                .is_some_and(|e| e.ordinal >= min_val && e.ordinal <= max_val)
        })
        .collect();
    let empty_origins = if lv.items.is_empty() {
        vec![]
    } else {
        effective_origins(program, &lv)
    };
    flow.value_stack
        .push(Value::List(Arc::new(build(program, items, empty_origins))));
    Ok(())
}

/// `ListFromInt`: `[origin, ordinal]` → `List(single item by ordinal in origin)`
///
/// The origin can be either a `String` (list def name, from `listInt` native fn)
/// or a `List` (from which origins are derived).
pub(crate) fn list_from_int(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let ordinal = pop_int(flow)?;
    let origin_val = flow.pop_value()?;

    // Collect origin list definitions to search.
    let origin_defs: Vec<&crate::program::ListDefEntry> = match &origin_val {
        Value::String(name) => program.list_def_by_name(name).into_iter().collect(),
        Value::List(lv) => effective_origins(program, lv)
            .into_iter()
            .filter_map(|origin_id| program.list_def(origin_id))
            .collect(),
        _ => Vec::new(),
    };

    let mut items = Vec::new();
    let mut origins = Vec::new();
    for def in &origin_defs {
        for &item_id in &def.items {
            if let Some(entry) = program.list_item(item_id)
                && entry.ordinal == ordinal
            {
                items.push(item_id);
                if !origins.contains(&entry.origin) {
                    origins.push(entry.origin);
                }
                break;
            }
        }
    }

    flow.value_stack
        .push(Value::List(Arc::new(ListValue { items, origins })));
    Ok(())
}

/// `ListRandom`: `[list]` → `List(random item)` — picks one item using the story RNG.
pub(crate) fn list_random<R: crate::rng::StoryRng>(
    flow: &mut Flow,
    context: &mut (impl ContextAccess + ?Sized),
) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let items = if lv.items.is_empty() {
        vec![]
    } else {
        let result_seed = context.rng_seed().wrapping_add(context.previous_random());
        let next_random = context.next_random::<R>(result_seed);
        #[expect(clippy::cast_sign_loss)]
        let idx = (next_random as usize) % lv.items.len();
        context.set_previous_random(next_random);
        vec![lv.items[idx]]
    };
    flow.value_stack.push(Value::List(Arc::new(ListValue {
        items,
        origins: lv.origins.clone(),
    })));
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Convert an already-popped value to a list.
fn to_list(val: Value) -> Result<Arc<ListValue>, RuntimeError> {
    match val {
        Value::List(lv) => Ok(lv),
        Value::Null => Ok(Arc::new(ListValue {
            items: vec![],
            origins: vec![],
        })),
        _ => Err(RuntimeError::TypeError(format!(
            "expected list, got {:?}",
            val.value_type()
        ))),
    }
}

fn pop_list(flow: &mut Flow) -> Result<Arc<ListValue>, RuntimeError> {
    let val = flow.pop_value()?;
    match val {
        Value::List(lv) => Ok(lv),
        // An empty list can appear as Null in some contexts.
        Value::Null => Ok(Arc::new(ListValue {
            items: vec![],
            origins: vec![],
        })),
        _ => Err(RuntimeError::TypeError(format!(
            "expected list, got {:?}",
            val.value_type()
        ))),
    }
}

fn pop_int(flow: &mut Flow) -> Result<i32, RuntimeError> {
    let val = flow.pop_value()?;
    match val {
        Value::Int(n) => Ok(n),
        _ => Err(RuntimeError::TypeError(format!(
            "expected int, got {:?}",
            val.value_type()
        ))),
    }
}

/// Pop a value that's either an Int or a single-item List (extract its ordinal).
fn pop_int_or_list_ordinal(flow: &mut Flow, program: &Program) -> Result<i32, RuntimeError> {
    let val = flow.pop_value()?;
    match val {
        Value::Int(n) => Ok(n),
        Value::List(lv) => {
            if lv.items.len() == 1 {
                Ok(program.list_item(lv.items[0]).map_or(0, |e| e.ordinal))
            } else {
                Ok(0)
            }
        }
        _ => Err(RuntimeError::TypeError(format!(
            "expected int or list, got {:?}",
            val.value_type()
        ))),
    }
}
