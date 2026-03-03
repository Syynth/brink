//! List opcode implementations.

use brink_format::{ListValue, Value};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::Story;

/// `ListContains` (`?`): `[lhs, rhs]` → `Bool(rhs ⊆ lhs)`
pub(crate) fn list_contains(story: &mut Story) -> Result<(), RuntimeError> {
    let rhs = pop_list(story)?;
    let lhs = pop_list(story)?;
    let result = rhs.items.iter().all(|id| lhs.items.contains(id));
    story.flow.value_stack.push(Value::Bool(result));
    Ok(())
}

/// `ListNotContains` (`!?`): `[lhs, rhs]` → `Bool(¬(rhs ⊆ lhs))`
pub(crate) fn list_not_contains(story: &mut Story) -> Result<(), RuntimeError> {
    let rhs = pop_list(story)?;
    let lhs = pop_list(story)?;
    let result = !rhs.items.iter().all(|id| lhs.items.contains(id));
    story.flow.value_stack.push(Value::Bool(result));
    Ok(())
}

/// `ListIntersect` (`L^`): `[a, b]` → `List(a ∩ b)`
pub(crate) fn list_intersect(story: &mut Story) -> Result<(), RuntimeError> {
    let b = pop_list(story)?;
    let a = pop_list(story)?;
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
    story
        .flow
        .value_stack
        .push(Value::List(ListValue { items, origins }));
    Ok(())
}

/// `ListCount`: `[list]` → `Int(len)`
#[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
pub(crate) fn list_count(story: &mut Story) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
    story
        .flow
        .value_stack
        .push(Value::Int(lv.items.len() as i32));
    Ok(())
}

/// `ListMin`: `[list]` → `List(single item with lowest ordinal)`
pub(crate) fn list_min(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
    let min_item = lv
        .items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| (id, e.ordinal)))
        .min_by_key(|&(_, ord)| ord)
        .map(|(id, _)| id);
    let items = min_item.map_or_else(Vec::new, |id| vec![id]);
    story.flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListMax`: `[list]` → `List(single item with highest ordinal)`
pub(crate) fn list_max(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
    let max_item = lv
        .items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| (id, e.ordinal)))
        .max_by_key(|&(_, ord)| ord)
        .map(|(id, _)| id);
    let items = max_item.map_or_else(Vec::new, |id| vec![id]);
    story.flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListValue`: `[list]` → `Int(ordinal of single-item list)`
pub(crate) fn list_value(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
    let ordinal = if lv.items.len() == 1 {
        program.list_item(lv.items[0]).map_or(0, |e| e.ordinal)
    } else {
        0
    };
    story.flow.value_stack.push(Value::Int(ordinal));
    Ok(())
}

/// `ListAll`: `[list]` → `List(all items from origins)`
pub(crate) fn list_all(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
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
    story.flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListInvert`: `[list]` → `List(ALL \ list)` — complement within origins.
pub(crate) fn list_invert(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
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
    story.flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListRange`: `[list, min, max]` → `List(items with ordinal in [min,max])`
pub(crate) fn list_range(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let max_val = pop_int_or_list_ordinal(story, program)?;
    let min_val = pop_int_or_list_ordinal(story, program)?;
    let lv = pop_list(story)?;
    let mut items = Vec::new();
    for &origin_id in &lv.origins {
        if let Some(def) = program.list_def(origin_id) {
            for &item_id in &def.items {
                if let Some(entry) = program.list_item(item_id)
                    && entry.ordinal >= min_val
                    && entry.ordinal <= max_val
                {
                    items.push(item_id);
                }
            }
        }
    }
    story.flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

/// `ListFromInt`: `[list_origin, ordinal]` → `List(single item by ordinal in origin)`
pub(crate) fn list_from_int(story: &mut Story, program: &Program) -> Result<(), RuntimeError> {
    let ordinal = pop_int(story)?;
    let origin_list = pop_list(story)?;
    let mut items = Vec::new();
    let mut origins = origin_list.origins.clone();
    // Use origins from the list to find the target item.
    for &origin_id in &origin_list.origins {
        if let Some(def) = program.list_def(origin_id) {
            for &item_id in &def.items {
                if let Some(entry) = program.list_item(item_id)
                    && entry.ordinal == ordinal
                {
                    items.push(item_id);
                    break;
                }
            }
        }
    }
    // If the list had no origins but had items, derive origins from items.
    if origins.is_empty() {
        for &item_id in &origin_list.items {
            if let Some(entry) = program.list_item(item_id)
                && !origins.contains(&entry.origin)
            {
                origins.push(entry.origin);
            }
        }
        for &origin_id in &origins {
            if let Some(def) = program.list_def(origin_id) {
                for &candidate_id in &def.items {
                    if let Some(e) = program.list_item(candidate_id)
                        && e.ordinal == ordinal
                    {
                        items.push(candidate_id);
                        break;
                    }
                }
            }
        }
    }
    story
        .flow
        .value_stack
        .push(Value::List(ListValue { items, origins }));
    Ok(())
}

/// `ListRandom`: `[list]` → `List(random item)` — deterministic for now (first item).
pub(crate) fn list_random(story: &mut Story) -> Result<(), RuntimeError> {
    let lv = pop_list(story)?;
    let items = if lv.items.is_empty() {
        vec![]
    } else {
        // TODO: use proper RNG when Random/SeedRandom are implemented.
        vec![lv.items[0]]
    };
    story.flow.value_stack.push(Value::List(ListValue {
        items,
        origins: lv.origins,
    }));
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn pop_list(story: &mut Story) -> Result<ListValue, RuntimeError> {
    let val = story.flow.pop_value()?;
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

fn pop_int(story: &mut Story) -> Result<i32, RuntimeError> {
    let val = story.flow.pop_value()?;
    match val {
        Value::Int(n) => Ok(n),
        _ => Err(RuntimeError::TypeError(format!(
            "expected int, got {:?}",
            val.value_type()
        ))),
    }
}

/// Pop a value that's either an Int or a single-item List (extract its ordinal).
fn pop_int_or_list_ordinal(story: &mut Story, program: &Program) -> Result<i32, RuntimeError> {
    let val = story.flow.pop_value()?;
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
