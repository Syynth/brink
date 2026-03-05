//! List opcode implementations.

use brink_format::{ListValue, Value};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::rng::StoryRng;
use crate::story::{Context, Flow};

/// `ListContains` (`?`): `[lhs, rhs]` → `Bool(rhs ⊆ lhs)`
pub(crate) fn list_contains(flow: &mut Flow) -> Result<(), RuntimeError> {
    let rhs = pop_list(flow)?;
    let lhs = pop_list(flow)?;
    let result = rhs.items.iter().all(|id| lhs.items.contains(id));
    flow.value_stack.push(Value::Bool(result));
    Ok(())
}

/// `ListNotContains` (`!?`): `[lhs, rhs]` → `Bool(¬(rhs ⊆ lhs))`
pub(crate) fn list_not_contains(flow: &mut Flow) -> Result<(), RuntimeError> {
    let rhs = pop_list(flow)?;
    let lhs = pop_list(flow)?;
    let result = !rhs.items.iter().all(|id| lhs.items.contains(id));
    flow.value_stack.push(Value::Bool(result));
    Ok(())
}

/// `ListIntersect` (`L^`): `[a, b]` → `List(a ∩ b)`
pub(crate) fn list_intersect(flow: &mut Flow) -> Result<(), RuntimeError> {
    let b = pop_list(flow)?;
    let a = pop_list(flow)?;
    let items: Vec<_> = a
        .items
        .iter()
        .filter(|id| b.items.contains(id))
        .copied()
        .collect();
    let mut origins = a.origins.clone();
    for &o in &b.origins {
        if !origins.contains(&o) {
            origins.push(o);
        }
    }
    flow.value_stack
        .push(Value::List(ListValue { items, origins }));
    Ok(())
}

/// `ListCount`: `[list]` → `Int(len)`
#[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
pub(crate) fn list_count(flow: &mut Flow) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    flow.value_stack.push(Value::Int(lv.items.len() as i32));
    Ok(())
}

/// `ListMin`: `[list]` → `List(single item with lowest ordinal)`
pub(crate) fn list_min(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let min_item = lv
        .items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| (id, e.ordinal)))
        .min_by_key(|&(_, ord)| ord)
        .map(|(id, _)| id);
    let items = min_item.map_or_else(Vec::new, |id| vec![id]);
    flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListMax`: `[list]` → `List(single item with highest ordinal)`
pub(crate) fn list_max(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let max_item = lv
        .items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| (id, e.ordinal)))
        .max_by_key(|&(_, ord)| ord)
        .map(|(id, _)| id);
    let items = max_item.map_or_else(Vec::new, |id| vec![id]);
    flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
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

/// `ListAll`: `[list]` → `List(all items from origins)`
pub(crate) fn list_all(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let mut items = Vec::new();
    for &origin_id in &lv.origins {
        if let Some(def) = program.list_def(origin_id) {
            for &item_id in &def.items {
                if !items.contains(&item_id) {
                    items.push(item_id);
                }
            }
        }
    }
    flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListInvert`: `[list]` → `List(ALL \ list)` — complement within origins.
pub(crate) fn list_invert(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let mut items = Vec::new();
    for &origin_id in &lv.origins {
        if let Some(def) = program.list_def(origin_id) {
            for &item_id in &def.items {
                if !lv.items.contains(&item_id) && !items.contains(&item_id) {
                    items.push(item_id);
                }
            }
        }
    }
    flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListRange`: `[list, min, max]` → `List(items with ordinal in [min,max])`
///
/// Filters the list's *own* items by ordinal bounds (not all items from origins).
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
    flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
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
        Value::List(lv) => {
            let mut defs = Vec::new();
            // Use explicit origins first.
            for &origin_id in &lv.origins {
                if let Some(def) = program.list_def(origin_id) {
                    defs.push(def);
                }
            }
            // If no origins, derive from items.
            if defs.is_empty() {
                for &item_id in &lv.items {
                    if let Some(entry) = program.list_item(item_id)
                        && let Some(def) = program.list_def(entry.origin)
                        && !defs.iter().any(|d| d.name == def.name)
                    {
                        defs.push(def);
                    }
                }
            }
            defs
        }
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
        .push(Value::List(ListValue { items, origins }));
    Ok(())
}

/// `ListRandom`: `[list]` → `List(random item)` — picks one item using the story RNG.
pub(crate) fn list_random<R: StoryRng>(
    flow: &mut Flow,
    context: &mut Context<R>,
) -> Result<(), RuntimeError> {
    let lv = pop_list(flow)?;
    let items = if lv.items.is_empty() {
        vec![]
    } else {
        let result_seed = context.rng_seed.wrapping_add(context.previous_random);
        let mut rng = R::from_seed(result_seed);
        let next_random = rng.next_int();
        #[expect(clippy::cast_sign_loss)]
        let idx = (next_random as usize) % lv.items.len();
        context.previous_random = next_random;
        vec![lv.items[idx]]
    };
    flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn pop_list(flow: &mut Flow) -> Result<ListValue, RuntimeError> {
    let val = flow.pop_value()?;
    match val {
        Value::List(lv) => Ok(lv),
        // An empty list can appear as Null in some contexts.
        Value::Null => Ok(ListValue {
            items: vec![],
            origins: vec![],
        }),
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
