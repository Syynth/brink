//! Arithmetic, comparison, coercion, truthiness, and stringify for [`Value`].

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::{ListValue, Value};

use crate::error::RuntimeError;
use crate::program::Program;

/// Returns whether a value is truthy in ink semantics.
pub(crate) fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(n) => *n != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        // A record is a value with fields, not a collection with a size —
        // it's always truthy, same as every other non-collection variant
        // here (divert targets, pointers, fragment refs).
        // A function value is a value with an identity, not a collection with a
        // size — always truthy, like every other non-collection variant here.
        // A handle (T1d) is the same shape of argument: an opaque token with
        // an identity, not a collection — always truthy.
        Value::DivertTarget(_)
        | Value::VariablePointer(_)
        | Value::TempPointer { .. }
        | Value::FragmentRef(_)
        | Value::Record { .. }
        | Value::FnRef(_)
        | Value::Closure(_)
        | Value::Handle { .. } => true,
        Value::List(lv) => !lv.items.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Map(map) => !map.is_empty(),
    }
}

/// Stringify a value for output.
pub(crate) fn stringify(v: &Value, program: &Program) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(n) => format!("{n}"),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::String(s) => s.to_string(),
        Value::Null => String::new(),
        Value::List(lv) => stringify_list(lv, program),
        Value::DivertTarget(id) | Value::VariablePointer(id) => format!("{id}"),
        Value::TempPointer { slot, frame_depth } => {
            format!("TempPointer({slot}@{frame_depth})")
        }
        // FragmentRef resolution happens in resolve_part, not stringify.
        // This fallback is for computation contexts where FragmentRef shouldn't appear.
        Value::FragmentRef(idx) => format!("<fragment:{idx}>"),
        // Collections are runtime-only until T1b emits their opcodes; the
        // author-facing output format is a T1b concern. This provisional
        // rendering keeps `stringify` total and is not reachable while the
        // collection opcodes are inert.
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(|v| stringify(v, program)).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", stringify_map_key(k), stringify(v, program)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        // Same provisional-rendering rationale as the collection arms above:
        // no compiler surface constructs a `Record` yet, so this format is
        // not user-facing-authoritative — it exists only so `stringify`
        // stays total.
        Value::Record { fields, .. } => {
            let parts: Vec<String> = fields.iter().map(|v| stringify(v, program)).collect();
            format!("{{{}}}", parts.join(", "))
        }
        // Function values (T1c-3, spec §5). The **authoritative** author-facing
        // display form — signature-like, with bound args rendered as defaults:
        // `fn heal(ref hp = player_hp, amount)`. Bound `val` args print their
        // value's display form, bound `ref` args print the captured cell name,
        // unbound params print as bare names. This is a permanently observable
        // surface (spec §5, ratified 2026-07-13) — deliberately boring and
        // stable, exercised via `string(f)`/`{f}` and property-tested. Both
        // `string(f)` and interpolation route here (typed-mode-spec §4:
        // "display is universal").
        Value::FnRef(target) => display_fn_value(*target, &[], program),
        Value::Closure(c) => display_fn_value(c.target, &c.env, program),
        // Handle values (T1d, `docs/t1d-spec.md` §6). The **authoritative**
        // display form — deliberately boring and stable, same
        // observable-surface-forever reasoning as the fn-value display
        // ruling above: `handle <Kind>#<id>`. Total: an unresolvable kind
        // (a stale `NameId` from a different compile) renders `?`, matching
        // `display_fn_value`'s convention for its own unresolvable names.
        Value::Handle { kind, id } => {
            let kind_name = program.name_checked(*kind).unwrap_or("?");
            format!("handle {kind_name}#{id}")
        }
    }
}

/// The authoritative function-value display form (`docs/t1c-spec.md` §5):
/// `fn <name>(<param…>)` where each declared param renders as `ref name =
/// cell` / `name = value` when bound, or a bare `name` when unbound.
///
/// Total — never faults, never panics: an unresolvable target renders `?` for
/// the name, an unresolvable param `NameId` renders `?`, and a `ref` payload
/// that isn't a resolvable global cell falls back to its value display form.
fn display_fn_value(
    target: brink_format::DefinitionId,
    env: &[brink_format::ClosureEnvEntry],
    program: &Program,
) -> String {
    let name = program
        .divert_target_path(target)
        .unwrap_or_else(|| "?".to_owned());
    let empty: &[brink_format::ParamMeta] = &[];
    let params = program
        .resolve_target(target)
        .map_or(empty, |(idx, _)| program.container_params(idx));

    // Render every declared param, in order. The bound prefix comes from
    // `env`; any trailing params beyond `env.len()` are unbound. `env` can be
    // longer than `params` only for a stale rehydrated value — still rendered
    // so the form stays total (the invoke-time rehydration check is the fault
    // path, not display).
    let count = params.len().max(env.len());
    let mut parts = Vec::with_capacity(count);
    for i in 0..count {
        if let Some(entry) = env.get(i) {
            let pname = program.name_checked(entry.name).unwrap_or("?");
            if entry.is_ref {
                let cell = match &entry.payload {
                    Value::VariablePointer(id) => {
                        program.global_var_name(*id).map(ToOwned::to_owned)
                    }
                    _ => None,
                }
                .unwrap_or_else(|| stringify(&entry.payload, program));
                parts.push(format!("ref {pname} = {cell}"));
            } else {
                parts.push(format!("{pname} = {}", stringify(&entry.payload, program)));
            }
        } else {
            let pname = params
                .get(i)
                .map_or("?", |p| program.name_checked(p.name).unwrap_or("?"));
            parts.push(pname.to_owned());
        }
    }
    format!("fn {name}({})", parts.join(", "))
}

/// Provisional stringify for a map key (see [`stringify`]'s collection arms).
fn stringify_map_key(key: &brink_format::MapKey) -> String {
    match key {
        brink_format::MapKey::Int(n) => n.to_string(),
        brink_format::MapKey::Str(s) => s.to_string(),
        brink_format::MapKey::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
    }
}

/// Stringify a list value: sort items by (ordinal, origin name), join display names with ", ".
///
/// List item names are stored fully qualified (`ListName.ItemName`).
/// For display, we strip the origin prefix and show only the item name.
fn stringify_list(lv: &ListValue, program: &Program) -> String {
    let mut entries: Vec<(i32, &str, &str)> = lv
        .items
        .iter()
        .filter_map(|&id| {
            program.list_item(id).map(|entry| {
                let origin_name = program
                    .list_def(entry.origin)
                    .map_or("", |def| program.name(def.name));
                let full_name = program.name(entry.name);
                let display_name = full_name
                    .split_once('.')
                    .map_or(full_name, |(_, item)| item);
                (entry.ordinal, origin_name, display_name)
            })
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    let names: Vec<&str> = entries.iter().map(|&(_, _, name)| name).collect();
    names.join(", ")
}

/// Binary arithmetic/comparison operation.
#[expect(
    clippy::match_same_arms,
    reason = "the FnRef/Closure and Handle equality arms have identical bodies \
              (both delegate to Value's PartialEq) but must stay separate \
              match arms, not merged into one shared pattern: a fn value and \
              a handle are unrelated opaque-identity types, and comparing \
              across them must still hit the TypeError fault below, not \
              silently return false the way the Null cross-type rule does"
)]
pub(crate) fn binary_op(
    op: BinaryOp,
    left: &Value,
    right: &Value,
    program: &Program,
) -> Result<Value, RuntimeError> {
    // Coerce types: if both are numeric, promote to float if either is float.
    match (left, right) {
        // List + List
        (Value::List(a), Value::List(b)) => list_binary_op(op, a, b, program),
        // List + Int / List - Int → ordinal shift
        (Value::List(a), Value::Int(b)) if op == BinaryOp::Add || op == BinaryOp::Subtract => {
            let shift = if op == BinaryOp::Add { *b } else { -*b };
            Ok(Value::List(Arc::new(list_ordinal_shift(a, shift, program))))
        }
        (Value::Int(a), Value::Int(b)) => int_op(op, *a, *b),
        (Value::Float(a), Value::Float(b)) => float_op(op, *a, *b),
        #[expect(clippy::cast_precision_loss)]
        (Value::Int(a), Value::Float(b)) => float_op(op, *a as f32, *b),
        #[expect(clippy::cast_precision_loss)]
        (Value::Float(a), Value::Int(b)) => float_op(op, *a, *b as f32),
        (Value::String(a), Value::String(b)) => string_op(op, a, b),
        // Int + String coercion: stringify the int
        (Value::String(a), Value::Int(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}").into()))
        }
        (Value::Int(a), Value::String(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}").into()))
        }
        // Float + String coercion
        (Value::String(a), Value::Float(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}").into()))
        }
        (Value::Float(a), Value::String(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}").into()))
        }
        // String vs Int/Float equality: coerce numeric to string (ink type priority: String > Float > Int).
        (Value::String(a), Value::Int(b)) if op == BinaryOp::Equal || op == BinaryOp::NotEqual => {
            string_op(op, a, &b.to_string())
        }
        (Value::Int(a), Value::String(b)) if op == BinaryOp::Equal || op == BinaryOp::NotEqual => {
            string_op(op, &a.to_string(), b)
        }
        (Value::String(a), Value::Float(b))
            if op == BinaryOp::Equal || op == BinaryOp::NotEqual =>
        {
            string_op(op, a, &format!("{b}"))
        }
        (Value::Float(a), Value::String(b))
            if op == BinaryOp::Equal || op == BinaryOp::NotEqual =>
        {
            string_op(op, &format!("{a}"), b)
        }
        // Bool comparisons
        (Value::Bool(a), Value::Bool(b)) => bool_op(op, *a, *b),
        // Bool + Int coercion
        (Value::Bool(a), Value::Int(b)) => int_op(op, i32::from(*a), *b),
        (Value::Int(a), Value::Bool(b)) => int_op(op, *a, i32::from(*b)),
        // Bool + Float coercion
        (Value::Bool(a), Value::Float(b)) => float_op(op, if *a { 1.0 } else { 0.0 }, *b),
        (Value::Float(a), Value::Bool(b)) => float_op(op, *a, if *b { 1.0 } else { 0.0 }),
        // DivertTarget equality
        (Value::DivertTarget(a), Value::DivertTarget(b)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(a == b))
        }
        (Value::DivertTarget(a), Value::DivertTarget(b)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(a != b))
        }
        // Function-value equality (T1c-3, spec §5): structural — same fn
        // token and equal bound rows (`ref` entries by bound cell, `val` by
        // value), delegated to `Value`'s `PartialEq`. Only `==`/`!=` are
        // defined; any ordering op (`<`, `>=`, …) falls through to the
        // `TypeError` fault below — spec §5's "no ordering" rule (a runtime
        // fault in gradual mode, a compile error in strict). Deliberately
        // NOT merged with the `Handle` arm below despite the identical body
        // (see this function's `#[expect]`): a fn value compared against a
        // handle must still fault, not silently return `false`.
        (Value::FnRef(_) | Value::Closure(_), Value::FnRef(_) | Value::Closure(_))
            if op == BinaryOp::Equal =>
        {
            Ok(Value::Bool(left == right))
        }
        (Value::FnRef(_) | Value::Closure(_), Value::FnRef(_) | Value::Closure(_))
            if op == BinaryOp::NotEqual =>
        {
            Ok(Value::Bool(left != right))
        }
        // Handle equality (T1d, `docs/t1d-spec.md` §6): token equality — same
        // kind and same id, delegated to `Value`'s `PartialEq`. Only `==`/`!=`
        // are defined; any ordering op falls through to the `TypeError` fault
        // below — the spec's "no ordering" rule (a runtime fault in gradual
        // mode, a compile error under the typed dialect).
        (Value::Handle { .. }, Value::Handle { .. }) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::Handle { .. }, Value::Handle { .. }) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Equality for null
        (Value::Null, Value::Null) if op == BinaryOp::Equal => Ok(Value::Bool(true)),
        (Value::Null, Value::Null) if op == BinaryOp::NotEqual => Ok(Value::Bool(false)),
        (Value::Null, _) | (_, Value::Null) if op == BinaryOp::Equal => Ok(Value::Bool(false)),
        (Value::Null, _) | (_, Value::Null) if op == BinaryOp::NotEqual => Ok(Value::Bool(true)),
        _ => Err(RuntimeError::TypeError(format!(
            "cannot apply {op:?} to {:?} and {:?}",
            left.value_type(),
            right.value_type()
        ))),
    }
}

/// Get the minimum ordinal value in a list. Returns `None` if the list is empty.
fn list_min_ordinal(lv: &ListValue, program: &Program) -> Option<i32> {
    lv.items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| e.ordinal))
        .min()
}

/// Get the maximum ordinal value in a list. Returns `None` if the list is empty.
fn list_max_ordinal(lv: &ListValue, program: &Program) -> Option<i32> {
    lv.items
        .iter()
        .filter_map(|&id| program.list_item(id).map(|e| e.ordinal))
        .max()
}

/// Ordinal-based list comparison (ink semantics).
///
/// - `A > B`:  if A empty → false; if B empty → true; else min(A) > max(B)
/// - `A >= B`: if A empty → (B empty); if B empty → true; else min(A) >= min(B) AND max(A) >= max(B)
/// - `A < B`:  if B empty → false; if A empty → true; else max(A) < min(B)
/// - `A <= B`: if B empty → (A empty); if A empty → true; else max(A) <= max(B) AND min(A) <= min(B)
fn list_compare(op: BinaryOp, a: &ListValue, b: &ListValue, program: &Program) -> bool {
    match op {
        BinaryOp::Greater => {
            if a.items.is_empty() {
                return false;
            }
            if b.items.is_empty() {
                return true;
            }
            matches!(
                (list_min_ordinal(a, program), list_max_ordinal(b, program)),
                (Some(a_min), Some(b_max)) if a_min > b_max
            )
        }
        BinaryOp::GreaterOrEqual => {
            if a.items.is_empty() {
                return b.items.is_empty();
            }
            if b.items.is_empty() {
                return true;
            }
            matches!(
                (list_min_ordinal(a, program), list_min_ordinal(b, program),
                 list_max_ordinal(a, program), list_max_ordinal(b, program)),
                (Some(a_min), Some(b_min), Some(a_max), Some(b_max))
                    if a_min >= b_min && a_max >= b_max
            )
        }
        BinaryOp::Less => {
            if b.items.is_empty() {
                return false;
            }
            if a.items.is_empty() {
                return true;
            }
            matches!(
                (list_max_ordinal(a, program), list_min_ordinal(b, program)),
                (Some(a_max), Some(b_min)) if a_max < b_min
            )
        }
        BinaryOp::LessOrEqual => {
            if b.items.is_empty() {
                return a.items.is_empty();
            }
            if a.items.is_empty() {
                return true;
            }
            matches!(
                (list_max_ordinal(a, program), list_max_ordinal(b, program),
                 list_min_ordinal(a, program), list_min_ordinal(b, program)),
                (Some(a_max), Some(b_max), Some(a_min), Some(b_min))
                    if a_max <= b_max && a_min <= b_min
            )
        }
        _ => false,
    }
}

/// Binary operations on two list values.
fn list_binary_op(
    op: BinaryOp,
    a: &ListValue,
    b: &ListValue,
    program: &Program,
) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Add => {
            // Union
            let mut items = a.items.clone();
            for &id in &b.items {
                if !items.contains(&id) {
                    items.push(id);
                }
            }
            let mut origins = a.origins.clone();
            for &id in &b.origins {
                if !origins.contains(&id) {
                    origins.push(id);
                }
            }
            Ok(Value::List(Arc::new(ListValue { items, origins })))
        }
        BinaryOp::Subtract => {
            // Except (a \ b)
            let items: Vec<_> = a
                .items
                .iter()
                .filter(|id| !b.items.contains(id))
                .copied()
                .collect();
            Ok(Value::List(Arc::new(ListValue {
                items,
                origins: a.origins.clone(),
            })))
        }
        BinaryOp::Equal => {
            let eq =
                a.items.len() == b.items.len() && a.items.iter().all(|id| b.items.contains(id));
            Ok(Value::Bool(eq))
        }
        BinaryOp::NotEqual => {
            let eq =
                a.items.len() == b.items.len() && a.items.iter().all(|id| b.items.contains(id));
            Ok(Value::Bool(!eq))
        }
        BinaryOp::Greater | BinaryOp::GreaterOrEqual | BinaryOp::Less | BinaryOp::LessOrEqual => {
            Ok(Value::Bool(list_compare(op, a, b, program)))
        }
        BinaryOp::And => Ok(Value::Bool(!a.items.is_empty() && !b.items.is_empty())),
        BinaryOp::Or => Ok(Value::Bool(!a.items.is_empty() || !b.items.is_empty())),
        _ => Err(RuntimeError::TypeError(format!(
            "cannot apply {op:?} to lists"
        ))),
    }
}

/// Shift all list items by an ordinal delta within their origin lists.
fn list_ordinal_shift(lv: &ListValue, shift: i32, program: &Program) -> ListValue {
    let mut items = Vec::with_capacity(lv.items.len());
    for &item_id in &lv.items {
        if let Some(entry) = program.list_item(item_id) {
            let target_ordinal = entry.ordinal + shift;
            // Find the item with the target ordinal in the same origin.
            if let Some(def) = program.list_def(entry.origin) {
                for &candidate_id in &def.items {
                    if let Some(candidate) = program.list_item(candidate_id)
                        && candidate.ordinal == target_ordinal
                    {
                        items.push(candidate_id);
                        break;
                    }
                }
            }
        }
    }
    ListValue {
        items,
        origins: lv.origins.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    And,
    Or,
    Min,
    Max,
    Pow,
}

fn int_op(op: BinaryOp, a: i32, b: i32) -> Result<Value, RuntimeError> {
    Ok(match op {
        BinaryOp::Add => Value::Int(a.wrapping_add(b)),
        BinaryOp::Subtract => Value::Int(a.wrapping_sub(b)),
        BinaryOp::Multiply => Value::Int(a.wrapping_mul(b)),
        BinaryOp::Divide => {
            if b == 0 {
                return Err(RuntimeError::DivisionByZero);
            }
            Value::Int(a.wrapping_div(b))
        }
        BinaryOp::Modulo => {
            if b == 0 {
                return Err(RuntimeError::DivisionByZero);
            }
            Value::Int(a.wrapping_rem(b))
        }
        BinaryOp::Equal => Value::Bool(a == b),
        BinaryOp::NotEqual => Value::Bool(a != b),
        BinaryOp::Greater => Value::Bool(a > b),
        BinaryOp::GreaterOrEqual => Value::Bool(a >= b),
        BinaryOp::Less => Value::Bool(a < b),
        BinaryOp::LessOrEqual => Value::Bool(a <= b),
        BinaryOp::And => Value::Bool(a != 0 && b != 0),
        BinaryOp::Or => Value::Bool(a != 0 || b != 0),
        BinaryOp::Min => Value::Int(a.min(b)),
        BinaryOp::Max => Value::Int(a.max(b)),
        // `powf` needs `libm` — std-only. See `float_op`.
        #[cfg(feature = "std")]
        #[expect(clippy::cast_precision_loss)]
        BinaryOp::Pow => Value::Float((a as f32).powf(b as f32)),
        #[cfg(not(feature = "std"))]
        BinaryOp::Pow => {
            return Err(RuntimeError::Unimplemented(
                "POW() requires the `std` feature (no libm in no_std builds)".into(),
            ));
        }
    })
}

// `powf` needs a transcendental (`libm`) implementation that `core` doesn't
// provide — it's std-only. Every other case here is plain arithmetic or a
// core-available intrinsic (`abs`/`min`/`max`), so only `Pow` needs to
// branch; under `no_std` it reports `Unimplemented` instead of miscomputing
// or panicking. `std` behavior (the default, and the only path the oracle
// exercises) is unchanged.
//
// `Result` looks unnecessary from a `std`-only reading (clippy checks the
// default feature set) — the `Err` arm only exists under the mutually
// exclusive `not(feature = "std")` config.
#[cfg_attr(feature = "std", expect(clippy::unnecessary_wraps))]
fn float_op(op: BinaryOp, a: f32, b: f32) -> Result<Value, RuntimeError> {
    Ok(match op {
        BinaryOp::Add => Value::Float(a + b),
        BinaryOp::Subtract => Value::Float(a - b),
        BinaryOp::Multiply => Value::Float(a * b),
        BinaryOp::Divide => Value::Float(a / b),
        BinaryOp::Modulo => Value::Float(a % b),
        BinaryOp::Equal => Value::Bool((a - b).abs() < f32::EPSILON),
        BinaryOp::NotEqual => Value::Bool((a - b).abs() >= f32::EPSILON),
        BinaryOp::Greater => Value::Bool(a > b),
        BinaryOp::GreaterOrEqual => Value::Bool(a >= b),
        BinaryOp::Less => Value::Bool(a < b),
        BinaryOp::LessOrEqual => Value::Bool(a <= b),
        BinaryOp::And => Value::Bool(a != 0.0 && b != 0.0),
        BinaryOp::Or => Value::Bool(a != 0.0 || b != 0.0),
        BinaryOp::Min => Value::Float(a.min(b)),
        BinaryOp::Max => Value::Float(a.max(b)),
        #[cfg(feature = "std")]
        BinaryOp::Pow => Value::Float(a.powf(b)),
        #[cfg(not(feature = "std"))]
        BinaryOp::Pow => {
            return Err(RuntimeError::Unimplemented(
                "POW() requires the `std` feature (no libm in no_std builds)".into(),
            ));
        }
    })
}

fn string_op(op: BinaryOp, a: &str, b: &str) -> Result<Value, RuntimeError> {
    Ok(match op {
        BinaryOp::Add => Value::String(format!("{a}{b}").into()),
        BinaryOp::Equal => Value::Bool(a == b),
        BinaryOp::NotEqual => Value::Bool(a != b),
        _ => {
            return Err(RuntimeError::TypeError(format!(
                "cannot apply {op:?} to strings"
            )));
        }
    })
}

fn bool_op(op: BinaryOp, a: bool, b: bool) -> Result<Value, RuntimeError> {
    Ok(match op {
        BinaryOp::Equal => Value::Bool(a == b),
        BinaryOp::NotEqual => Value::Bool(a != b),
        BinaryOp::And => Value::Bool(a && b),
        BinaryOp::Or => Value::Bool(a || b),
        // Treat bools as 0/1 ints for arithmetic
        _ => int_op(op, i32::from(a), i32::from(b))?,
    })
}

/// Cast value to int.
pub(crate) fn cast_to_int(v: &Value) -> Value {
    match v {
        Value::Int(_) => v.clone(),
        #[expect(clippy::cast_possible_truncation)]
        Value::Float(f) => Value::Int(*f as i32),
        Value::Bool(b) => Value::Int(i32::from(*b)),
        Value::String(s) => Value::Int(s.parse::<i32>().unwrap_or(0)),
        _ => Value::Int(0),
    }
}

/// Cast value to float.
pub(crate) fn cast_to_float(v: &Value) -> Value {
    match v {
        Value::Float(_) => v.clone(),
        #[expect(clippy::cast_precision_loss)]
        Value::Int(n) => Value::Float(*n as f32),
        Value::Bool(b) => Value::Float(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => Value::Float(s.parse::<f32>().unwrap_or(0.0)),
        _ => Value::Float(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::{LinkedContainer, ListDefEntry, ListItemEntry};
    use brink_format::{DefinitionId, DefinitionTag, NameId};
    use std::collections::HashMap;

    fn dummy_program() -> Program {
        Program {
            containers: vec![LinkedContainer {
                id: DefinitionId::new(DefinitionTag::Address, 0),
                bytecode: vec![],
                counting_flags: brink_format::CountingFlags::empty(),
                path_hash: 0,
                param_count: 0,
                params: Vec::new(),
                scope_table_idx: 0,
            }],
            address_map: {
                let mut m = HashMap::new();
                m.insert(DefinitionId::new(DefinitionTag::Address, 0), (0u32, 0usize));
                m
            },
            scope_ids: vec![DefinitionId::new(DefinitionTag::Address, 0)],
            source_checksum: 0,
            globals: vec![],
            global_map: HashMap::new(),
            name_table: vec![],
            address_by_path: HashMap::new(),
            root_idx: 0,
            list_literals: vec![],
            literal_pool: vec![],
            list_item_map: HashMap::new(),
            list_defs: vec![],
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
            local_scope_defaults: Vec::new(),
            struct_shapes: Vec::new(),
            private_defs: Vec::new(),
            alias_table: Vec::new(),
        }
    }

    #[test]
    fn truthiness() {
        assert!(is_truthy(&Value::Bool(true)));
        assert!(!is_truthy(&Value::Bool(false)));
        assert!(is_truthy(&Value::Int(1)));
        assert!(!is_truthy(&Value::Int(0)));
        assert!(is_truthy(&Value::Float(0.1)));
        assert!(!is_truthy(&Value::Float(0.0)));
        assert!(is_truthy(&Value::String("hi".into())));
        assert!(!is_truthy(&Value::String("".into())));
        assert!(!is_truthy(&Value::Null));
    }

    #[test]
    fn int_arithmetic() {
        let p = dummy_program();
        let r = binary_op(BinaryOp::Add, &Value::Int(2), &Value::Int(3), &p).unwrap();
        assert_eq!(r, Value::Int(5));
    }

    #[test]
    fn int_float_promotion() {
        let p = dummy_program();
        let r = binary_op(BinaryOp::Add, &Value::Int(2), &Value::Float(1.5), &p).unwrap();
        assert_eq!(r, Value::Float(3.5));
    }

    #[test]
    fn string_concat() {
        let p = dummy_program();
        let r = binary_op(
            BinaryOp::Add,
            &Value::String("a".into()),
            &Value::String("b".into()),
            &p,
        )
        .unwrap();
        assert_eq!(r, Value::String("ab".into()));
    }

    #[test]
    fn stringify_values() {
        let p = dummy_program();
        assert_eq!(stringify(&Value::Int(42), &p), "42");
        assert_eq!(stringify(&Value::Bool(true), &p), "true");
        assert_eq!(stringify(&Value::Null, &p), "");
    }

    /// Build a program with a list definition "Rank" containing items low(1), mid(2), high(3).
    fn program_with_rank_list() -> (Program, DefinitionId, DefinitionId, DefinitionId) {
        let list_def_id = DefinitionId::new(DefinitionTag::ListDef, 100);
        let low_id = DefinitionId::new(DefinitionTag::ListItem, 1);
        let mid_id = DefinitionId::new(DefinitionTag::ListItem, 2);
        let high_id = DefinitionId::new(DefinitionTag::ListItem, 3);

        let mut p = dummy_program();
        // Names: 0="low", 1="mid", 2="high", 3="Rank"
        p.name_table = vec![
            "low".to_string(),
            "mid".to_string(),
            "high".to_string(),
            "Rank".to_string(),
        ];
        p.list_item_map.insert(
            low_id,
            ListItemEntry {
                name: NameId(0),
                ordinal: 1,
                origin: list_def_id,
            },
        );
        p.list_item_map.insert(
            mid_id,
            ListItemEntry {
                name: NameId(1),
                ordinal: 2,
                origin: list_def_id,
            },
        );
        p.list_item_map.insert(
            high_id,
            ListItemEntry {
                name: NameId(2),
                ordinal: 3,
                origin: list_def_id,
            },
        );
        p.list_defs.push(ListDefEntry {
            name: NameId(3),
            items: vec![low_id, mid_id, high_id],
        });
        p.list_def_map.insert(list_def_id, 0);

        (p, low_id, mid_id, high_id)
    }

    /// List comparisons use ordinal-based semantics, not set-theoretic.
    #[test]
    fn list_comparison_ordinal_semantics() {
        let (p, low_id, mid_id, high_id) = program_with_rank_list();
        let list_def_id = DefinitionId::new(DefinitionTag::ListDef, 100);

        let low = Value::List(Arc::new(ListValue {
            items: vec![low_id],
            origins: vec![list_def_id],
        }));
        let mid = Value::List(Arc::new(ListValue {
            items: vec![mid_id],
            origins: vec![list_def_id],
        }));
        let high = Value::List(Arc::new(ListValue {
            items: vec![high_id],
            origins: vec![list_def_id],
        }));
        let mid_high = Value::List(Arc::new(ListValue {
            items: vec![mid_id, high_id],
            origins: vec![list_def_id],
        }));

        // {mid} > {low} — ordinal 2 > ordinal 1 → true
        // (set-theoretic would say false because low ∉ {mid})
        assert_eq!(
            binary_op(BinaryOp::Greater, &mid, &low, &p).unwrap(),
            Value::Bool(true)
        );

        // {high} > {mid_high} — min(high)=3 > max(mid_high)=3 → false
        assert_eq!(
            binary_op(BinaryOp::Greater, &high, &mid_high, &p).unwrap(),
            Value::Bool(false)
        );

        // {low} < {mid} — max(low)=1 < min(mid)=2 → true
        assert_eq!(
            binary_op(BinaryOp::Less, &low, &mid, &p).unwrap(),
            Value::Bool(true)
        );

        // {mid_high} >= {mid} — min(mid_high)=2 >= min(mid)=2 AND max(mid_high)=3 >= max(mid)=2 → true
        assert_eq!(
            binary_op(BinaryOp::GreaterOrEqual, &mid_high, &mid, &p).unwrap(),
            Value::Bool(true)
        );

        // {mid} <= {mid_high} — max(mid)=2 <= max(mid_high)=3 AND min(mid)=2 <= min(mid_high)=2 → true
        assert_eq!(
            binary_op(BinaryOp::LessOrEqual, &mid, &mid_high, &p).unwrap(),
            Value::Bool(true)
        );

        // {low} >= {mid} — min(low)=1 >= min(mid)=2 → false
        assert_eq!(
            binary_op(BinaryOp::GreaterOrEqual, &low, &mid, &p).unwrap(),
            Value::Bool(false)
        );
    }

    /// Empty list edge cases for ordinal comparisons.
    #[test]
    fn list_comparison_empty() {
        let (p, low_id, _, _) = program_with_rank_list();
        let list_def_id = DefinitionId::new(DefinitionTag::ListDef, 100);

        let empty = Value::List(Arc::new(ListValue {
            items: vec![],
            origins: vec![list_def_id],
        }));
        let low = Value::List(Arc::new(ListValue {
            items: vec![low_id],
            origins: vec![list_def_id],
        }));

        // {low} > () → true (non-empty > empty)
        assert_eq!(
            binary_op(BinaryOp::Greater, &low, &empty, &p).unwrap(),
            Value::Bool(true)
        );
        // () > {low} → false
        assert_eq!(
            binary_op(BinaryOp::Greater, &empty, &low, &p).unwrap(),
            Value::Bool(false)
        );
        // () < {low} → true (empty < non-empty)
        assert_eq!(
            binary_op(BinaryOp::Less, &empty, &low, &p).unwrap(),
            Value::Bool(true)
        );
        // () >= () → true
        assert_eq!(
            binary_op(BinaryOp::GreaterOrEqual, &empty, &empty, &p).unwrap(),
            Value::Bool(true)
        );
        // () <= () → true
        assert_eq!(
            binary_op(BinaryOp::LessOrEqual, &empty, &empty, &p).unwrap(),
            Value::Bool(true)
        );
    }

    /// String == Int coerces Int to String (ink type priority: String > Int).
    #[test]
    fn string_int_equality_coercion() {
        let p = dummy_program();
        // "5" == 5 → "5" == "5" → true
        let r = binary_op(
            BinaryOp::Equal,
            &Value::String("5".into()),
            &Value::Int(5),
            &p,
        )
        .unwrap();
        assert_eq!(r, Value::Bool(true));

        // "blah" == 5 → "blah" == "5" → false
        let r = binary_op(
            BinaryOp::Equal,
            &Value::String("blah".into()),
            &Value::Int(5),
            &p,
        )
        .unwrap();
        assert_eq!(r, Value::Bool(false));

        // 5 == "5" (reversed operand order)
        let r = binary_op(
            BinaryOp::Equal,
            &Value::Int(5),
            &Value::String("5".into()),
            &p,
        )
        .unwrap();
        assert_eq!(r, Value::Bool(true));
    }

    // ── Handle (T1d, docs/t1d-spec.md §6) ───────────────────────────────────

    #[test]
    fn handle_equality_is_token_equality() {
        let p = dummy_program();
        let h1 = Value::handle(NameId(1), 42);
        let h1_again = Value::handle(NameId(1), 42);
        let h2 = Value::handle(NameId(2), 42);

        assert_eq!(
            binary_op(BinaryOp::Equal, &h1, &h1_again, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &h1, &h2, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::Equal, &h1, &h2, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn handle_has_no_ordering() {
        // No `<`/`>`/`<=`/`>=` is defined for Handle (spec §6) — every
        // ordering op is a runtime TypeError fault in gradual mode.
        let p = dummy_program();
        let h1 = Value::handle(NameId(1), 1);
        let h2 = Value::handle(NameId(1), 2);
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &h1, &h2, &p).is_err(),
                "{op:?} on two handles must fault, not silently order them"
            );
        }
    }

    #[test]
    fn handle_equality_does_not_leak_across_to_other_identity_types() {
        // A handle and a fn value are unrelated opaque-identity types — even
        // though both use the same "structural PartialEq" body in
        // `binary_op` (the reason for this function's `match_same_arms`
        // `#[expect]`), comparing across them must still fault, not silently
        // return `false` the way the `Null` cross-type rule does.
        let p = dummy_program();
        let h = Value::handle(NameId(1), 42);
        let f = Value::FnRef(DefinitionId::new(DefinitionTag::Address, 42));
        assert!(binary_op(BinaryOp::Equal, &h, &f, &p).is_err());
        assert!(binary_op(BinaryOp::NotEqual, &h, &f, &p).is_err());
    }

    /// List items stored with qualified names ("Rank.low") should display as just "low".
    #[test]
    fn stringify_list_strips_origin_prefix() {
        let list_def_id = DefinitionId::new(DefinitionTag::ListDef, 200);
        let a_id = DefinitionId::new(DefinitionTag::ListItem, 10);
        let b_id = DefinitionId::new(DefinitionTag::ListItem, 11);

        let mut p = dummy_program();
        p.name_table = vec![
            "Colors.red".to_string(),
            "Colors.blue".to_string(),
            "Colors".to_string(),
        ];
        p.list_item_map.insert(
            a_id,
            ListItemEntry {
                name: NameId(0),
                ordinal: 1,
                origin: list_def_id,
            },
        );
        p.list_item_map.insert(
            b_id,
            ListItemEntry {
                name: NameId(1),
                ordinal: 2,
                origin: list_def_id,
            },
        );
        p.list_defs.push(ListDefEntry {
            name: NameId(2),
            items: vec![a_id, b_id],
        });
        p.list_def_map.insert(list_def_id, 0);

        let lv = ListValue {
            items: vec![a_id, b_id],
            origins: vec![list_def_id],
        };
        assert_eq!(stringify(&Value::List(Arc::new(lv)), &p), "red, blue");
    }

    /// List items stored without a prefix (legacy/unqualified) still display correctly.
    #[test]
    fn stringify_list_unqualified_names_unchanged() {
        let (p, low_id, mid_id, _) = program_with_rank_list();
        let list_def_id = DefinitionId::new(DefinitionTag::ListDef, 100);

        let lv = ListValue {
            items: vec![low_id, mid_id],
            origins: vec![list_def_id],
        };
        assert_eq!(stringify(&Value::List(Arc::new(lv)), &p), "low, mid");
    }
}
