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
///
/// Fallible since F27 (`docs/stdlib-spec.md` §1.6, ruled 2026-07-19, issue
/// #1120): a `Value::OptionVal` in truthiness position is the one input with
/// no truthiness at all — see the arm's own comment.
pub(crate) fn is_truthy(v: &Value) -> Result<bool, RuntimeError> {
    Ok(match v {
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
        // A projection (T1e) is a value with an identity (root cell + path),
        // not a collection — always truthy, same reasoning as every other
        // non-collection variant here.
        // A tower value (NS-A8) is a value with components, not a collection
        // with a size — always truthy, following the record/handle/
        // projection precedent here. (No zero-vector falsiness: that would
        // be a quiet numeric coercion nothing ruled — flagged as a finding
        // for the queue, not invented here.)
        Value::DivertTarget(_)
        | Value::VariablePointer(_)
        | Value::TempPointer { .. }
        | Value::FragmentRef(_)
        | Value::Record { .. }
        | Value::FnRef(_)
        | Value::Closure(_)
        | Value::Handle { .. }
        | Value::Projection(_)
        | Value::Vec2(_)
        | Value::Vec3(_)
        | Value::Vec4(_)
        | Value::Quat(_)
        // A weighted table (NS-A7) is never empty — evidence-by-
        // construction (§8) refuses empty tables at the only producer —
        // so unlike the collections below there is no falsy case to
        // express: always truthy, the record/handle/tower precedent.
        | Value::Mat2(_)
        | Value::Mat3(_)
        | Value::Mat4(_)
        | Value::Weighted(_) => true,
        // F27 (ruled 2026-07-19, issue #1120): Option has **no** truthiness
        // — a condition-position `Option[T]` is a compile error under
        // `types = strict` (E116) and this turn-terminating fault under
        // gradual. The falsy-none arm NS-A1 shipped here (`none` falsy,
        // `some(x)` truthy — the `{r: …}` "did we find one?" guard) was a
        // quiet coercion of exactly the kind `Option[T] ≠ T` exists to ban;
        // authors write `== none` / `== some(x)` instead.
        Value::OptionVal(_) => return Err(RuntimeError::OptionTruthiness),
        // Range truthiness (NS-A5, F7): emptiness is load-bearing for
        // ranges (`for i in 0..n` with n = 0 runs zero times), so a range
        // reads like the collections below — truthy iff it denotes at
        // least one element.
        Value::Range { .. } => v.range_len().unwrap_or(0) > 0,
        Value::List(lv) => !lv.items.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Map(map) => !map.is_empty(),
    })
}

/// Stringify a value for output.
#[expect(
    clippy::too_many_lines,
    reason = "one display arm per Value variant — the NS-A7 Weighted arm pushed this past 100"
)]
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
        // Structs (TM-4 `Value::Record`): the **structural display
        // default** of the protocol registry (NS-A3, issue #1109,
        // docs/stdlib-spec.md §9.6) — shape name + fields in declared
        // order, mirroring the construction literal: `Point { x: 1, y: 2 }`.
        // Field values recurse through this same function, so nested
        // structs/options render consistently. This IS the display
        // protocol's default path: both interpolation (`{p}`, via
        // `EmitValue` → `resolve_part`) and the `string()` conversion
        // intrinsic (`ConvertString` → `convert_to_string`) dispatch
        // through `stringify` — F1's one-display-path ruling (2026-07-19);
        // a registered user `display` impl would override this default
        // once the impl spelling lands (⏳ code-dialect sitting). Total by
        // construction: a stale/mismatched `ShapeId` (a record loaded from
        // a save against a different compile) falls back to the positional
        // brace form rather than faulting — `string()`'s ruled totality
        // survives the dispatch (F1's rider).
        Value::Record { shape, fields } => {
            let entry = program.struct_shapes.get(shape.0 as usize);
            match entry {
                Some(entry) if entry.fields.len() == fields.len() => {
                    let name = program.name_checked(entry.name).unwrap_or("?");
                    if fields.is_empty() {
                        format!("{name} {{}}")
                    } else {
                        let parts: Vec<String> = entry
                            .fields
                            .iter()
                            .zip(fields.iter())
                            .map(|(field_name, v)| {
                                format!(
                                    "{}: {}",
                                    program.name_checked(*field_name).unwrap_or("?"),
                                    stringify(v, program)
                                )
                            })
                            .collect();
                        format!("{name} {{ {} }}", parts.join(", "))
                    }
                }
                _ => {
                    let parts: Vec<String> = fields.iter().map(|v| stringify(v, program)).collect();
                    format!("{{{}}}", parts.join(", "))
                }
            }
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
        // Projection values (T1e, `docs/t1e-spec.md` §4 PROPOSED): `ref
        // <root>` / `ref <root>.<field>[<index>]…` — root + rendered path,
        // deliberately boring and stable. An unresolvable root cell (a stale
        // `DefinitionId` from a different compile) renders `?`, matching
        // `display_fn_value`'s convention for its own unresolvable names.
        Value::Projection(p) => format!("ref {}", display_projection_path(p, program)),
        // Option values (NS-A1, `docs/stdlib-spec.md` §1.4): the boring,
        // stable form — `none` / `some(<inner display form>)`, matching the
        // source-level construction vocabulary. NOTE: the §1.6
        // display-boundary forgiveness (a final-None interpolation renders
        // as *nothing*) is Track B4 and deliberately NOT implemented here —
        // this is the strict-era rendering, total like every other arm.
        Value::OptionVal(inner) => match inner {
            None => "none".to_owned(),
            Some(v) => format!("some({})", stringify(v, program)),
        },
        // Range values (NS-A5, `docs/stdlib-spec.md` §7, F7): the written
        // form, preserved — `0..10` / `1..=6` — matching the source-level
        // literal vocabulary exactly (content equality may say two forms
        // are the same sequence; display stays faithful to the spelling).
        Value::Range {
            start,
            end,
            inclusive,
        } => {
            if *inclusive {
                format!("{start}..={end}")
            } else {
                format!("{start}..{end}")
            }
        }
        // Tower values (NS-A8): the **structural display default** of the
        // protocol registry (NS-A3 precedent, the `Record` arm above) —
        // kind name + named components in glam's declared order, mirroring
        // the §K construction grammar's tower row (`vec3 { x: 1, y: 2,
        // z: 3 }`). Matrices render their glam column fields (`x_axis` …)
        // as nested vectors through this same function. Lanes format
        // exactly like bare `Float` (`{n}`). Total by construction; a
        // registered user `display` impl can never override this — tower
        // kinds are compiler-known, not user structs (the registry rejects
        // them, E118).
        // Weighted tables (NS-A7, `docs/stdlib-spec.md` §8): mirror the
        // chartered construction literal — `Weighted { 3: sword, 1:
        // shield }`, entries in construction order (order is semantic for
        // display; equality alone is order-insensitive). Values recurse.
        Value::Weighted(w) => {
            let parts: Vec<String> = w
                .entries
                .iter()
                .map(|(weight, val)| format!("{weight}: {}", stringify(val, program)))
                .collect();
            format!("Weighted {{ {} }}", parts.join(", "))
        }
        Value::Vec2(v) => format!("vec2 {{ x: {}, y: {} }}", v.x, v.y),
        Value::Vec3(v) => format!("vec3 {{ x: {}, y: {}, z: {} }}", v.x, v.y, v.z),
        Value::Vec4(v) => format!("vec4 {{ x: {}, y: {}, z: {}, w: {} }}", v.x, v.y, v.z, v.w),
        Value::Quat(q) => format!("quat {{ x: {}, y: {}, z: {}, w: {} }}", q.x, q.y, q.z, q.w),
        Value::Mat2(m) => format!(
            "mat2 {{ x_axis: {}, y_axis: {} }}",
            stringify(&Value::Vec2(m.x_axis), program),
            stringify(&Value::Vec2(m.y_axis), program)
        ),
        Value::Mat3(m) => format!(
            "mat3 {{ x_axis: {}, y_axis: {}, z_axis: {} }}",
            stringify(&Value::Vec3(m.x_axis), program),
            stringify(&Value::Vec3(m.y_axis), program),
            stringify(&Value::Vec3(m.z_axis), program)
        ),
        Value::Mat4(m) => format!(
            "mat4 {{ x_axis: {}, y_axis: {}, z_axis: {}, w_axis: {} }}",
            stringify(&Value::Vec4(m.x_axis), program),
            stringify(&Value::Vec4(m.y_axis), program),
            stringify(&Value::Vec4(m.z_axis), program),
            stringify(&Value::Vec4(m.w_axis), program)
        ),
    }
}

/// Render a projection's `root.field[index]…` path text — no leading `ref `
/// prefix. [`stringify`]'s own `Value::Projection` arm prepends `ref ` for a
/// projection displayed as an ordinary value (`ref npc.inventory[3]`,
/// `docs/t1e-spec.md` §4 PROPOSED); [`display_fn_value`]'s bound-`ref`-param
/// arm calls this directly so a projection-bound `ref` parameter renders
/// `ref hp = npc.hp`, not the doubled `ref hp = ref npc.hp` that prepending
/// `ref ` twice would produce (issue #850).
fn display_projection_path(p: &brink_format::ProjectionValue, program: &Program) -> String {
    let root = program
        .global_var_name(p.cell)
        .map_or_else(|| "?".to_owned(), ToOwned::to_owned);
    let mut out = root;
    for seg in &p.segments {
        display_proj_segment(seg, program, &mut out);
    }
    out
}

/// Render one path-projection segment onto `out` (`docs/t1e-spec.md` §4
/// PROPOSED): a struct-field-shaped string key as `.name`, anything else as
/// `[value]`.
fn display_proj_segment(seg: &brink_format::ProjSegment, program: &Program, out: &mut String) {
    use core::fmt::Write as _;
    match seg {
        brink_format::ProjSegment::Index(n) => {
            let _ = write!(out, "[{n}]");
        }
        brink_format::ProjSegment::Key(Value::String(s)) if is_field_like(s) => {
            let _ = write!(out, ".{s}");
        }
        brink_format::ProjSegment::Key(v) => {
            let _ = write!(out, "[{}]", stringify(v, program));
        }
    }
}

/// Whether a string looks like a bare identifier (`.field`-renderable)
/// rather than an arbitrary map key (`["field"]`-renderable).
fn is_field_like(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
                    // T1e (docs/t1e-spec.md §4 PROPOSED, issue #850): a
                    // path-projection-bound `ref` param (`#fn(heal, ref
                    // npc.hp)`) renders its captured *path*, not the bare
                    // `ref `-prefixed value form `stringify` would give a
                    // standalone projection — `display_projection_path`
                    // omits that prefix since the `ref {pname} = ` this
                    // arm builds already supplies it once.
                    Value::Projection(p) => Some(display_projection_path(p, program)),
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
    reason = "the FnRef/Closure, Handle, VariablePointer/TempPointer, and \
              Projection equality arms have identical bodies (all delegate \
              to Value's PartialEq) but must stay separate match arms, not \
              merged into one shared pattern: these are unrelated opaque- \
              identity types, and comparing across them must still hit the \
              TypeError fault below, not silently return false the way the \
              Null cross-type rule does"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one match per Value variant pair, each documenting a distinct \
              spec ruling (value-model-spec §4, t1c-spec §5, t1d-spec §6, \
              t1e-spec §4) — splitting the match would scatter the single \
              source of truth for == /!= semantics across helper functions \
              for no clarity gain"
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
        // VariablePointer/TempPointer equality (issue #939, value-model-spec
        // §4 total-equality sweep): token equality — same identity-carrying,
        // non-collection shape as `DivertTarget` above (both are "a value
        // with an identity, not a collection", per this module's
        // `is_truthy` grouping), delegated to `Value`'s own `PartialEq`
        // rather than re-spelled here, mirroring the `FnRef`/`Closure`/
        // `Handle` pattern PRs #918/#931 established. `VariablePointer`
        // compares by target `DefinitionId`; `TempPointer` compares by
        // `(slot, frame_depth)`. Only `==`/`!=` are defined; any ordering op
        // falls through to the `TypeError` fault below — these are call-
        // argument-passing tokens (T1c `ref` params, T1e `ref` projections'
        // root), never orderable values.
        (Value::VariablePointer(_), Value::VariablePointer(_)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::VariablePointer(_), Value::VariablePointer(_)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        (Value::TempPointer { .. }, Value::TempPointer { .. }) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::TempPointer { .. }, Value::TempPointer { .. }) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
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
        // Projection equality (T1e, issue #939, `docs/t1e-spec.md` §4
        // PROPOSED): "equality structural (same root cell + equal
        // segments); not a map key; no ordering" — delegated to `Value`'s
        // own `PartialEq`, which already applies the `Arc::ptr_eq` fast path
        // for this variant (same COW-heap-allocated shape as `Closure`).
        // Only `==`/`!=` are defined; any ordering op falls through to the
        // `TypeError` fault below, per the spec's explicit "no ordering"
        // ruling.
        (Value::Projection(_), Value::Projection(_)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::Projection(_), Value::Projection(_)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Array equality (value-model-spec §4): structural, with the
        // `Arc::ptr_eq` fast path — delegated to `Value`'s own `PartialEq`
        // impl, the same structural comparison `collection_ops::map_contains`'s
        // Array branch already exercises for element containment. Only
        // `==`/`!=` are defined; any ordering op falls through to the
        // `TypeError` fault below — arrays have no ordering. Unlike maps
        // (#909, parked), array equality is unambiguously order-sensitive by
        // construction — element order is observable array structure, not an
        // incidental insertion artifact, so there is no analogous ruling to
        // park here.
        (Value::Array(_), Value::Array(_)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::Array(_), Value::Array(_)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Map equality (value-model-spec §4): structural, with the
        // `Arc::ptr_eq` fast path — both delegated to `Value`'s own
        // `PartialEq` impl, the same structural comparison `contains()`
        // (`collection_ops::map_contains`'s Array branch) already exercises
        // for element containment. Only `==`/`!=` are defined here; any
        // ordering op falls through to the `TypeError` fault below — maps
        // have no ordering. NOTE: whether two maps with the same entries in
        // a different insertion order compare equal was ruled in #909
        // (2026-07-18, `docs/decision-log.md` "Map/record equality is
        // insertion-order-insensitive"): map equality is content-based and
        // insertion-order-insensitive — this arm forwards to `OrderedMap`'s
        // hand-written `PartialEq` (see `OrderedMap`'s doc comment in
        // `brink-format::value`), which now implements exactly that.
        (Value::Map(_), Value::Map(_)) if op == BinaryOp::Equal => Ok(Value::Bool(left == right)),
        (Value::Map(_), Value::Map(_)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Record equality (typed-dialect era, TM-4; `docs/value-model-spec.md`
        // §11c / `docs/typed-mode-spec.md` §6): structural — same shape and
        // equal fields, with the `Arc::ptr_eq` fast path — delegated to
        // `Value`'s `PartialEq`, which already refuses equality across
        // mismatched shapes even when the field vectors happen to coincide.
        // Only `==`/`!=` are defined; any ordering op falls through to the
        // `TypeError` fault below — records have no ordering.
        (Value::Record { .. }, Value::Record { .. }) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::Record { .. }, Value::Record { .. }) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Option equality (NS-A1, `docs/stdlib-spec.md` §1.4): structural —
        // `none == none`, `some(x) == some(y)` iff `x == y` — delegated to
        // `Value`'s `PartialEq` (which carries the `Arc::ptr_eq` fast path
        // on the `some` payload). Only `==`/`!=` are defined; any ordering
        // op falls through to the `TypeError` fault below. An Option
        // compared against a bare value (`some(1) == 1`) also falls through
        // to the fault — the ruled `Option[T] ≠ T` strictness; the checker
        // reports it statically under `types = strict`, and the runtime
        // fault is the gradual-mode backstop.
        (Value::OptionVal(_), Value::OptionVal(_)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::OptionVal(_), Value::OptionVal(_)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Range equality (NS-A5, F7 "content equality"): delegated to
        // `Value`'s `PartialEq` — two ranges are equal iff they denote the
        // same integer sequence (`1..=6 == 1..7`; every empty range equals
        // every other empty range). Only `==`/`!=` are defined; ordering a
        // range (or comparing one against a non-range) falls through to the
        // `TypeError` fault below — ranges are not orderable and never
        // coerce.
        (Value::Range { .. }, Value::Range { .. }) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::Range { .. }, Value::Range { .. }) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Weighted equality (NS-A7, `docs/stdlib-spec.md` §8): multiset
        // content — delegated to `Value`'s `PartialEq` (order-insensitive,
        // multiplicity-sensitive, the F17 policy; `Arc::ptr_eq` fast path
        // included). Only `==`/`!=` are defined; ordering a table (or
        // comparing one against a non-Weighted) falls through to the
        // `TypeError` fault below — no ordering, no quiet coercion.
        (Value::Weighted(_), Value::Weighted(_)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(left == right))
        }
        (Value::Weighted(_), Value::Weighted(_)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(left != right))
        }
        // Equality for null
        (Value::Null, Value::Null) if op == BinaryOp::Equal => Ok(Value::Bool(true)),
        (Value::Null, Value::Null) if op == BinaryOp::NotEqual => Ok(Value::Bool(false)),
        (Value::Null, _) | (_, Value::Null) if op == BinaryOp::Equal => Ok(Value::Bool(false)),
        (Value::Null, _) | (_, Value::Null) if op == BinaryOp::NotEqual => Ok(Value::Bool(true)),
        // Tower operators (NS-A8, `docs/tower-mini-spec.md` T3 — the ruled
        // §2b op table, conventions per glam wholesale). Placed after the
        // Null arms so `vec == null` keeps the universal cross-type-null
        // `false`, and guarded on either side being a tower value so the
        // scalar-scale forms (`float * vecN`) are reachable. Everything the
        // ruled table does not enumerate faults below (a malformed
        // question), never silently coerces.
        (l, r) if is_tower(l) || is_tower(r) => tower_binary_op(op, l, r),
        _ => Err(RuntimeError::TypeError(format!(
            "cannot apply {op:?} to {:?} and {:?}",
            left.value_type(),
            right.value_type()
        ))),
    }
}

/// Whether a value is one of the NS-A8 numeric-tower kinds.
pub(crate) fn is_tower(v: &Value) -> bool {
    matches!(
        v,
        Value::Vec2(_)
            | Value::Vec3(_)
            | Value::Vec4(_)
            | Value::Quat(_)
            | Value::Mat2(_)
            | Value::Mat3(_)
            | Value::Mat4(_)
    )
}

/// The tower's operator family (NS-A8, `docs/tower-mini-spec.md` T3; the
/// §2b op table of `docs/stdlib-inventory.md`), semantics all glam's:
///
/// - `+`/`-`/`*` **componentwise** on same-kind vectors; `+`/`-` on quats
///   (glam's `Add`/`Sub`);
/// - **scalar scale**: `vecN * float` / `float * vecN` (ints promote,
///   matching ink's int→float coercion);
/// - `mat * vec` **transforms** (matching sizes);
/// - `quat * quat` **composes**; `quat * vec3` **rotates**;
/// - `mat * mat` **composes** (matching sizes) — F31 partial-b, issue #1145;
/// - `mat * scalar` **scales** (ints promote, one direction only — see the
///   arm's own comment) — F31 partial-b, issue #1145;
/// - `vec / scalar` **scales down** (ints promote, one direction only, IEEE
///   division so a zero divisor yields `inf`/`nan` lanes rather than a
///   fault) — F31 partial-b, issue #1145;
/// - `==`/`!=` on same-kind pairs — componentwise IEEE (T4), delegated to
///   `Value`'s `PartialEq`.
///
/// Everything else — ordering ops (T4: the tower is NOT orderable), `vec /
/// vec`, `mat / *`, `quat * scalar`, modulo, logic, cross-kind pairs,
/// tower-vs-scalar equality, and the still-un-ruled matrix±matrix form — is a
/// turn-terminating `TypeError` fault: the ruled table is the whole surface;
/// nothing is invented beyond it (F31's explicit "every other glam-native
/// form keeps faulting" clause).
fn tower_binary_op(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, RuntimeError> {
    use BinaryOp as B;
    // Same-kind structural equality first (componentwise IEEE, T4).
    // Cross-kind equality (vec2 vs vec3, vec vs float, …) deliberately
    // falls through to the fault below — the same no-silent-`false`
    // discipline as fn-vs-handle equality.
    if (op == B::Equal || op == B::NotEqual) && left.value_type() == right.value_type() {
        let eq = left == right;
        return Ok(Value::Bool(if op == B::Equal { eq } else { !eq }));
    }
    let fault = || {
        Err(RuntimeError::TypeError(format!(
            "cannot apply {op:?} to {:?} and {:?}",
            left.value_type(),
            right.value_type()
        )))
    };
    // Scalar operand for the scale forms — int promotes to f32 (ink's
    // int→float coercion), tower/other values are not scalars.
    let scalar = |v: &Value| -> Option<f32> {
        match v {
            Value::Float(f) => Some(*f),
            #[expect(
                clippy::cast_precision_loss,
                reason = "int->float promotion matches ink coercion semantics"
            )]
            Value::Int(n) => Some(*n as f32),
            _ => None,
        }
    };
    match (op, left, right) {
        // Componentwise `+`/`-` (vecN, quat).
        (B::Add, Value::Vec2(a), Value::Vec2(b)) => Ok(Value::Vec2(*a + *b)),
        (B::Add, Value::Vec3(a), Value::Vec3(b)) => Ok(Value::Vec3(*a + *b)),
        (B::Add, Value::Vec4(a), Value::Vec4(b)) => Ok(Value::Vec4(*a + *b)),
        (B::Add, Value::Quat(a), Value::Quat(b)) => Ok(Value::Quat(*a + *b)),
        (B::Subtract, Value::Vec2(a), Value::Vec2(b)) => Ok(Value::Vec2(*a - *b)),
        (B::Subtract, Value::Vec3(a), Value::Vec3(b)) => Ok(Value::Vec3(*a - *b)),
        (B::Subtract, Value::Vec4(a), Value::Vec4(b)) => Ok(Value::Vec4(*a - *b)),
        (B::Subtract, Value::Quat(a), Value::Quat(b)) => Ok(Value::Quat(*a - *b)),
        // Componentwise `*` (vecN).
        (B::Multiply, Value::Vec2(a), Value::Vec2(b)) => Ok(Value::Vec2(*a * *b)),
        (B::Multiply, Value::Vec3(a), Value::Vec3(b)) => Ok(Value::Vec3(*a * *b)),
        (B::Multiply, Value::Vec4(a), Value::Vec4(b)) => Ok(Value::Vec4(*a * *b)),
        // Quat composition and vec3 rotation.
        (B::Multiply, Value::Quat(a), Value::Quat(b)) => Ok(Value::Quat(*a * *b)),
        (B::Multiply, Value::Quat(q), Value::Vec3(v)) => Ok(Value::Vec3(*q * *v)),
        // Matrix transforms (matching sizes).
        (B::Multiply, Value::Mat2(m), Value::Vec2(v)) => Ok(Value::Vec2(*m * *v)),
        (B::Multiply, Value::Mat3(m), Value::Vec3(v)) => Ok(Value::Vec3(*m * *v)),
        (B::Multiply, Value::Mat4(m), Value::Vec4(v)) => Ok(Value::Vec4(*m * *v)),
        // Matrix composition (F31 partial-b, issue #1145): `mat * mat` of
        // matching size, delegated to glam's own `Mul<Mat*> for Mat*` (which
        // is composition, not componentwise — matrices don't get the
        // componentwise `*` vecN/quat have above). Placed before the
        // catch-all scalar-scale arms below so it isn't shadowed by them.
        (B::Multiply, Value::Mat2(a), Value::Mat2(b)) => Ok(Value::Mat2(*a * *b)),
        (B::Multiply, Value::Mat3(a), Value::Mat3(b)) => Ok(Value::Mat3(*a * *b)),
        (B::Multiply, Value::Mat4(a), Value::Mat4(b)) => Ok(Value::Mat4(*a * *b)),
        // Scalar scale, both orders.
        (B::Multiply, Value::Vec2(a), s) => match scalar(s) {
            Some(f) => Ok(Value::Vec2(*a * f)),
            None => fault(),
        },
        (B::Multiply, Value::Vec3(a), s) => match scalar(s) {
            Some(f) => Ok(Value::Vec3(*a * f)),
            None => fault(),
        },
        (B::Multiply, Value::Vec4(a), s) => match scalar(s) {
            Some(f) => Ok(Value::Vec4(*a * f)),
            None => fault(),
        },
        (B::Multiply, s, Value::Vec2(a)) => match scalar(s) {
            Some(f) => Ok(Value::Vec2(f * *a)),
            None => fault(),
        },
        (B::Multiply, s, Value::Vec3(a)) => match scalar(s) {
            Some(f) => Ok(Value::Vec3(f * *a)),
            None => fault(),
        },
        (B::Multiply, s, Value::Vec4(a)) => match scalar(s) {
            Some(f) => Ok(Value::Vec4(f * *a)),
            None => fault(),
        },
        // Matrix scale (F31 partial-b, issue #1145): `mat * scalar` only —
        // F31's row is named one direction (unlike the vecN row above, which
        // the T3 "wholesale" ruling explicitly documents both orders for);
        // the commuted `scalar * mat` form is left faulting rather than
        // guessed at, even though glam itself defines it (`Mul<Mat4> for
        // f32`) — not asked for by the ruling, so not added here. Int
        // operands promote via the same `scalar` closure used above (ink's
        // int→float coercion).
        (B::Multiply, Value::Mat2(m), s) => match scalar(s) {
            Some(f) => Ok(Value::Mat2(*m * f)),
            None => fault(),
        },
        (B::Multiply, Value::Mat3(m), s) => match scalar(s) {
            Some(f) => Ok(Value::Mat3(*m * f)),
            None => fault(),
        },
        (B::Multiply, Value::Mat4(m), s) => match scalar(s) {
            Some(f) => Ok(Value::Mat4(*m * f)),
            None => fault(),
        },
        // Vector scale-down (F31 partial-b, issue #1145): `vec / scalar`
        // only — no `scalar / vec` (not asked for; glam itself doesn't treat
        // that as the same operation shape), no `vec / vec` (stays faulting
        // per F31's explicit list). IEEE float division, NOT a
        // `RuntimeError::DivisionByZero` fault: a zero divisor produces
        // `inf`/`nan` lanes and flows, exactly like the scalar `Divide` arm
        // in `float_op` above (T4's NaN-totality) — division by the tower's
        // zero vector/matrix stays unruled and faults via the catch-all.
        (B::Divide, Value::Vec2(a), s) => match scalar(s) {
            Some(f) => Ok(Value::Vec2(*a / f)),
            None => fault(),
        },
        (B::Divide, Value::Vec3(a), s) => match scalar(s) {
            Some(f) => Ok(Value::Vec3(*a / f)),
            None => fault(),
        },
        (B::Divide, Value::Vec4(a), s) => match scalar(s) {
            Some(f) => Ok(Value::Vec4(*a / f)),
            None => fault(),
        },
        _ => fault(),
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
#[expect(
    clippy::float_cmp,
    reason = "exact IEEE == is the deliberate choice (issue #939, see the \
              Equal/NotEqual arm's own comment): it matches the C# reference \
              runtime's float == and Value's own PartialEq for floats nested \
              inside collections, not an oversight clippy's margin-of-error \
              suggestion would fix"
)]
fn float_op(op: BinaryOp, a: f32, b: f32) -> Result<Value, RuntimeError> {
    Ok(match op {
        BinaryOp::Add => Value::Float(a + b),
        BinaryOp::Subtract => Value::Float(a - b),
        BinaryOp::Multiply => Value::Float(a * b),
        BinaryOp::Divide => Value::Float(a / b),
        BinaryOp::Modulo => Value::Float(a % b),
        // Exact IEEE `==`/`!=` — NOT an epsilon-tolerant comparison (issue
        // #939, value-model-spec §4). Two facts settle this:
        //
        // 1. The C# reference runtime (trust-hierarchy tier 2,
        //    `ink-engine-runtime/NativeFunctionCall.cs`) defines float `==`
        //    as plain `(x, y) => x == y` — no tolerance. That's the oracle
        //    ground truth this VM is conforming to.
        // 2. `Value`'s own hand-written `PartialEq` (used for every
        //    float-inside-a-collection comparison — array/map/record
        //    elements, list ordinals via other arms, and this same
        //    `Equal`/`NotEqual` dispatch for nested values) already does
        //    exact `a == b` (`crates/internal/brink-format/src/value.rs`) and
        //    is the basis for the ratified "NaN-bearing collections never
        //    compare equal" rule (§4) — that rule is just IEEE composition,
        //    which only holds if the leaf float comparison is exact. An
        //    epsilon fudge at this scalar arm while collections stay exact
        //    was the inconsistency PR #931's review flagged: `5.0 == 5.0`
        //    outside a collection tolerated a `< f32::EPSILON` slop window
        //    that `[5.0] == [5.0]` never got, even though both routes
        //    ultimately answer "are these two floats the same value".
        //
        // The alternative (epsilon-tolerant scalar `==`, kept as `Value`'s
        // `PartialEq` stays exact) was considered and rejected: it would
        // leave direct float `==` diverging from the C# oracle on any
        // rounding-adjacent pair, and — since `Value::PartialEq` is also
        // what several format/save/dedup paths key equality off of, not
        // just this operator — going the other way (making collections
        // epsilon-tolerant too) would mean touching that hand-written impl
        // and contradicts the already-ratified NaN-composition wording in
        // §4. Exact equality is the one change that makes both paths agree
        // *and* matches the oracle; flagged here per issue #939 rather than
        // silently parked.
        BinaryOp::Equal => Value::Bool(a == b),
        BinaryOp::NotEqual => Value::Bool(a != b),
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

/// Cast value to int — the classic uppercase `INT()` builtin (vanilla ink,
/// `Opcode::CastToInt`). Distinct from the T1b lowercase `int()` intrinsic
/// (`conversion_ops::convert_to_int`), which has its own fault semantics.
///
/// The `Int`/`Float`/`Bool`/`String` domain and the string-parse-failure
/// silent-0 fallback are **unchanged** — kept exactly as-is per issue #955's
/// "do not change any currently-reachable behavior", regardless of which of
/// these arms the oracle corpus actually exercises today.
///
/// Every other `Value` variant used to fall through a `_ => Value::Int(0)`
/// wildcard — the wildcard-fan-out hazard (#667/#955): a future variant
/// would silently cast to zero instead of getting a considered answer. None
/// of `value-model-spec.md`, `t1c-spec.md`, `t1d-spec.md`, or `t1e-spec.md`
/// rule a conversion for `List`/`DivertTarget`/`VariablePointer`/
/// `TempPointer`/`Null`/`FragmentRef`/`Array`/`Map`/`Record`/`FnRef`/
/// `Closure`/`Handle`/`Projection` — genuinely unruled, so per the issue's
/// own conservative default and the value-model-spec §11c fault precedent
/// ("no silent garbage"), they now fault the same way the T1b lowercase
/// `int()` intrinsic already does for its own out-of-domain inputs
/// (`RuntimeError::InvalidConversionDomain`). The match is exhaustive by
/// name (no wildcard) so a future `Value` variant is a compile error here,
/// not a silent zero.
pub(crate) fn cast_to_int(v: &Value) -> Result<Value, RuntimeError> {
    Ok(match v {
        Value::Int(_) => v.clone(),
        #[expect(clippy::cast_possible_truncation)]
        Value::Float(f) => Value::Int(*f as i32),
        Value::Bool(b) => Value::Int(i32::from(*b)),
        Value::String(s) => Value::Int(s.parse::<i32>().unwrap_or(0)),
        Value::List(_)
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
        | Value::Projection(_)
        | Value::OptionVal(_)
        | Value::Range { .. }
        | Value::Vec2(_)
        | Value::Vec3(_)
        | Value::Vec4(_)
        | Value::Quat(_)
        | Value::Mat2(_)
        | Value::Mat3(_)
        | Value::Mat4(_)
        | Value::Weighted(_) => {
            return Err(RuntimeError::InvalidConversionDomain {
                target: "INT",
                got: cast_type_name(v),
            });
        }
    })
}

/// Cast value to float — the classic uppercase `FLOAT()` builtin. See
/// [`cast_to_int`]'s doc comment for the full rationale; the same
/// domain-preservation and exhaustiveness reasoning applies here.
pub(crate) fn cast_to_float(v: &Value) -> Result<Value, RuntimeError> {
    Ok(match v {
        Value::Float(_) => v.clone(),
        #[expect(clippy::cast_precision_loss)]
        Value::Int(n) => Value::Float(*n as f32),
        Value::Bool(b) => Value::Float(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => Value::Float(s.parse::<f32>().unwrap_or(0.0)),
        Value::List(_)
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
        | Value::Projection(_)
        | Value::OptionVal(_)
        | Value::Range { .. }
        | Value::Vec2(_)
        | Value::Vec3(_)
        | Value::Vec4(_)
        | Value::Quat(_)
        | Value::Mat2(_)
        | Value::Mat3(_)
        | Value::Mat4(_)
        | Value::Weighted(_) => {
            return Err(RuntimeError::InvalidConversionDomain {
                target: "FLOAT",
                got: cast_type_name(v),
            });
        }
    })
}

/// Type-name label for [`RuntimeError::InvalidConversionDomain`] as raised by
/// [`cast_to_int`]/[`cast_to_float`] — mirrors `collection_ops`'/
/// `record_ops`'/`conversion_ops`'s own small hand-duplicated `type_name`
/// helpers (no shared export exists for this purpose across the ops
/// modules). Named distinctly (`cast_type_name`) since this module already
/// has other private helpers.
fn cast_type_name(v: &Value) -> &'static str {
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

    /// A program with one declared shape `Point { x, y }` — the structural
    /// display default's lookup input (NS-A3, stdlib-spec §9.6).
    fn program_with_point_shape() -> Program {
        let mut program = dummy_program();
        program.name_table = vec!["Point".to_string(), "x".to_string(), "y".to_string()];
        program.struct_shapes = vec![crate::program::StructShapeEntry {
            name: NameId(0),
            fields: vec![NameId(1), NameId(2)],
        }];
        program
    }

    #[test]
    fn record_display_is_structural_by_field_order() {
        let program = program_with_point_shape();
        let p = Value::record(brink_format::ShapeId(0), vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(stringify(&p, &program), "Point { x: 1, y: 2 }");
    }

    #[test]
    fn record_display_nests_recursively() {
        let program = program_with_point_shape();
        let inner = Value::record(brink_format::ShapeId(0), vec![Value::Int(1), Value::Int(2)]);
        let opt = Value::some(inner);
        assert_eq!(stringify(&opt, &program), "some(Point { x: 1, y: 2 })");
    }

    #[test]
    fn record_display_with_stale_shape_falls_back_totally() {
        // F1 rider: `string()`'s totality survives — a record whose
        // `ShapeId` doesn't resolve in this program (e.g. loaded from a
        // save against a different compile) renders positionally instead
        // of faulting.
        let program = program_with_point_shape();
        let stale = Value::record(brink_format::ShapeId(7), vec![Value::Int(1)]);
        assert_eq!(stringify(&stale, &program), "{1}");
        let mismatched = Value::record(
            brink_format::ShapeId(0),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)],
        );
        assert_eq!(stringify(&mismatched, &program), "{1, 2, 3}");
    }

    #[test]
    fn option_display_forms_are_pinned() {
        // F28 (ruled 2026-07-19): `none`/`some(…)` render *totally* in
        // display until B4's boundary forgiveness arrives with the native
        // surface.
        let program = dummy_program();
        assert_eq!(stringify(&Value::none(), &program), "none");
        assert_eq!(stringify(&Value::some(Value::Int(3)), &program), "some(3)");
    }

    #[test]
    fn truthiness() {
        assert!(is_truthy(&Value::Bool(true)).unwrap());
        assert!(!is_truthy(&Value::Bool(false)).unwrap());
        assert!(is_truthy(&Value::Int(1)).unwrap());
        assert!(!is_truthy(&Value::Int(0)).unwrap());
        assert!(is_truthy(&Value::Float(0.1)).unwrap());
        assert!(!is_truthy(&Value::Float(0.0)).unwrap());
        assert!(is_truthy(&Value::String("hi".into())).unwrap());
        assert!(!is_truthy(&Value::String("".into())).unwrap());
        assert!(!is_truthy(&Value::Null).unwrap());
    }

    /// F27 (ruled 2026-07-19, issue #1120): Option has no truthiness —
    /// both `none` and `some(x)` fault in truthiness position (flipping
    /// NS-A1's falsy-none / truthy-some behavior). `some(0)` faults too:
    /// there is no "presence is truthy" carve-out.
    #[test]
    fn option_has_no_truthiness() {
        assert_eq!(
            is_truthy(&Value::none()),
            Err(RuntimeError::OptionTruthiness)
        );
        assert_eq!(
            is_truthy(&Value::some(Value::Int(0))),
            Err(RuntimeError::OptionTruthiness)
        );
        assert_eq!(
            is_truthy(&Value::some(Value::Bool(true))),
            Err(RuntimeError::OptionTruthiness)
        );
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

    // ── Map/Record equality (issue #908, value-model-spec §4) ───────────────
    //
    // `binary_op` had no Map/Map or Record/Record arm at all — `==`/`!=`
    // faulted with a `TypeError` instead of comparing, even though `Value`'s
    // own `PartialEq` already implements the ratified structural-equality-
    // with-`ptr_eq`-fast-path rule these tests exercise through the operator.

    use brink_format::{OrderedMap, ShapeId};

    // Regression tests for #922 (sibling of #908 above): `binary_op` had no
    // Array/Array arm at all — `==`/`!=` faulted with a `TypeError` instead
    // of comparing, even though `Value`'s own `PartialEq` already implements
    // the ratified structural-equality-with-`ptr_eq`-fast-path rule these
    // tests exercise through the operator. Unlike maps (#909, parked), array
    // equality is unambiguously order-sensitive by construction, so there is
    // no analogous ordering question to park here.

    #[test]
    fn array_equality_is_structural() {
        let prog = dummy_program();
        let arr1 = Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let arr2 = Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &arr1, &arr2, &prog).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &arr1, &arr2, &prog).unwrap(),
            Value::Bool(false)
        );

        // Different contents.
        let different_contents = Value::array(vec![Value::Int(1), Value::Int(99), Value::Int(3)]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &arr1, &different_contents, &prog).unwrap(),
            Value::Bool(false)
        );

        // Same elements, different order: arrays are order-sensitive by
        // construction (unlike the parked map-ordering question in #909) —
        // this must NOT compare equal.
        let reordered = Value::array(vec![Value::Int(3), Value::Int(2), Value::Int(1)]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &arr1, &reordered, &prog).unwrap(),
            Value::Bool(false)
        );

        // Different lengths.
        let shorter = Value::array(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &arr1, &shorter, &prog).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &arr1, &shorter, &prog).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn array_equality_ptr_eq_fast_path() {
        // Same Arc (a clone of the snapshot): the `ptr_eq` fast path wins
        // immediately, without needing structural comparison at all.
        let p = dummy_program();
        let a = Value::array(vec![Value::Int(1), Value::Int(2)]);
        let snapshot = a.clone();
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &snapshot, &p).unwrap(),
            Value::Bool(true)
        );
    }

    /// Nested arrays and mixed-type elements: structural comparison must
    /// recurse through nested collections and compare heterogeneous element
    /// types (Int vs String vs nested Array) elementwise.
    #[test]
    fn array_equality_nested_and_mixed_types() {
        let p = dummy_program();
        let a = Value::array(vec![
            Value::Int(1),
            Value::String("x".into()),
            Value::array(vec![Value::Bool(true), Value::Int(2)]),
        ]);
        let b = Value::array(vec![
            Value::Int(1),
            Value::String("x".into()),
            Value::array(vec![Value::Bool(true), Value::Int(2)]),
        ]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );

        // Divergent nested element.
        let c = Value::array(vec![
            Value::Int(1),
            Value::String("x".into()),
            Value::array(vec![Value::Bool(false), Value::Int(2)]),
        ]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &c, &p).unwrap(),
            Value::Bool(false)
        );
    }

    /// NaN composes structurally through array elements: two distinct arrays
    /// (distinct `Arc`s) each holding a NaN never compare equal, but an array
    /// compared against its own shared snapshot does — the `ptr_eq` fast
    /// path short-circuits before the NaN-bearing structural compare runs
    /// (value-model-spec §4: sharing an identical snapshot is equal to
    /// itself even with a NaN payload; this is stated as harmless).
    #[test]
    fn array_equality_nan_composition() {
        let p = dummy_program();
        let a = Value::array(vec![Value::Float(f32::NAN)]);
        let b = Value::array(vec![Value::Float(f32::NAN)]);
        // Distinct Arcs, NaN != NaN structurally.
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
        // Same Arc (snapshot): ptr_eq wins regardless of NaN.
        let snapshot = a.clone();
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &snapshot, &p).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn array_has_no_ordering() {
        let p = dummy_program();
        let a = Value::array(vec![Value::Int(1)]);
        let b = Value::array(vec![Value::Int(2)]);
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &a, &b, &p).is_err(),
                "{op:?} on two arrays must fault, not silently order them"
            );
        }
    }

    #[test]
    fn map_equality_is_structural() {
        let p = dummy_program();
        let mut m1 = OrderedMap::new();
        m1.insert("a".into(), Value::Int(1));
        m1.insert("b".into(), Value::Int(2));
        let mut m2 = OrderedMap::new();
        m2.insert("a".into(), Value::Int(1));
        m2.insert("b".into(), Value::Int(2));

        let a = Value::map(m1);
        let b = Value::map(m2);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &a, &b, &p).unwrap(),
            Value::Bool(false)
        );

        let mut m3 = OrderedMap::new();
        m3.insert("a".into(), Value::Int(1));
        m3.insert("b".into(), Value::Int(99));
        let c = Value::map(m3);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &c, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn map_equality_ptr_eq_fast_path() {
        // Same Arc (a clone of the snapshot): the `ptr_eq` fast path wins
        // immediately, without needing structural comparison at all.
        let p = dummy_program();
        let mut m = OrderedMap::new();
        m.insert("k".into(), Value::Int(1));
        let a = Value::map(m);
        let snapshot = a.clone();
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &snapshot, &p).unwrap(),
            Value::Bool(true)
        );
    }

    /// NaN composes structurally through map values: two distinct maps
    /// (distinct `Arc`s) each holding a NaN never compare equal, but a map
    /// compared against its own shared snapshot does — the `ptr_eq` fast
    /// path short-circuits before the NaN-bearing structural compare runs
    /// (value-model-spec §4: sharing an identical snapshot is equal to
    /// itself even with a NaN payload; this is stated as harmless).
    #[test]
    fn map_equality_nan_composition() {
        let p = dummy_program();
        let mut m1 = OrderedMap::new();
        m1.insert("n".into(), Value::Float(f32::NAN));
        let mut m2 = OrderedMap::new();
        m2.insert("n".into(), Value::Float(f32::NAN));

        let a = Value::map(m1);
        let b = Value::map(m2);
        // Distinct Arcs, NaN != NaN structurally.
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(false)
        );
        // Same Arc (snapshot): ptr_eq wins regardless of NaN.
        let snapshot = a.clone();
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &snapshot, &p).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn record_equality_is_structural() {
        let p = dummy_program();
        let shape = ShapeId(0);
        let a = Value::record(shape, vec![Value::Int(1), Value::String("x".into())]);
        let b = Value::record(shape, vec![Value::Int(1), Value::String("x".into())]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );

        let c = Value::record(shape, vec![Value::Int(2), Value::String("x".into())]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &c, &p).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &a, &c, &p).unwrap(),
            Value::Bool(true)
        );

        // Same field vectors, different shapes: never equal, even though the
        // fields happen to coincide.
        let other_shape = Value::record(ShapeId(1), vec![Value::Int(1), Value::String("x".into())]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &other_shape, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn record_equality_ptr_eq_fast_path() {
        let p = dummy_program();
        let a = Value::record(ShapeId(0), vec![Value::Int(1)]);
        let snapshot = a.clone();
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &snapshot, &p).unwrap(),
            Value::Bool(true)
        );
    }

    /// Nested collections: a map holding a record and an array, compared
    /// against a structurally-identical but distinct-Arc counterpart.
    #[test]
    fn nested_map_and_record_equality() {
        let p = dummy_program();
        let shape = ShapeId(0);
        let mut m1 = OrderedMap::new();
        m1.insert(
            "rec".into(),
            Value::record(shape, vec![Value::Array(Arc::new(vec![Value::Int(1)]))]),
        );
        let mut m2 = OrderedMap::new();
        m2.insert(
            "rec".into(),
            Value::record(shape, vec![Value::Array(Arc::new(vec![Value::Int(1)]))]),
        );
        let a = Value::map(m1);
        let b = Value::map(m2);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn map_has_no_ordering() {
        let p = dummy_program();
        let a = Value::map(OrderedMap::new());
        let b = Value::map(OrderedMap::new());
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &a, &b, &p).is_err(),
                "{op:?} on two maps must fault, not silently order them"
            );
        }
    }

    #[test]
    fn record_has_no_ordering() {
        let p = dummy_program();
        let a = Value::record(ShapeId(0), vec![Value::Int(1)]);
        let b = Value::record(ShapeId(0), vec![Value::Int(2)]);
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &a, &b, &p).is_err(),
                "{op:?} on two records must fault, not silently order them"
            );
        }
    }

    // ── VariablePointer/TempPointer/Projection equality (issue #939,
    //    value-model-spec §4 total-equality sweep) ──────────────────────
    //
    // `binary_op` had no arm at all for these three variants — `==`/`!=`
    // fell through to the catch-all and faulted with a `TypeError`, even
    // though `Value`'s own `PartialEq` already implements the correct
    // token/structural equality for all three (mirroring the #918/#931
    // pattern for `FnRef`/`Closure`/`Handle`/`Array`/`Map`/`Record`).

    use brink_format::ProjSegment;

    #[test]
    fn variable_pointer_equality_is_token_equality() {
        let p = dummy_program();
        let cell_a = DefinitionId::new(DefinitionTag::GlobalVar, 1);
        let cell_b = DefinitionId::new(DefinitionTag::GlobalVar, 2);
        let ptr_a1 = Value::VariablePointer(cell_a);
        let ptr_a2 = Value::VariablePointer(cell_a);
        let ptr_b = Value::VariablePointer(cell_b);

        assert_eq!(
            binary_op(BinaryOp::Equal, &ptr_a1, &ptr_a2, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &ptr_a1, &ptr_b, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::Equal, &ptr_a1, &ptr_b, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn variable_pointer_has_no_ordering() {
        let p = dummy_program();
        let a = Value::VariablePointer(DefinitionId::new(DefinitionTag::GlobalVar, 1));
        let b = Value::VariablePointer(DefinitionId::new(DefinitionTag::GlobalVar, 2));
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &a, &b, &p).is_err(),
                "{op:?} on two variable pointers must fault, not silently order them"
            );
        }
    }

    #[test]
    fn temp_pointer_equality_is_token_equality() {
        let p = dummy_program();
        let t_a1 = Value::TempPointer {
            slot: 3,
            frame_depth: 1,
        };
        let t_a2 = Value::TempPointer {
            slot: 3,
            frame_depth: 1,
        };
        // Same slot, different frame depth — not the same temp.
        let t_b = Value::TempPointer {
            slot: 3,
            frame_depth: 2,
        };
        // Different slot, same frame depth — not the same temp either.
        let t_c = Value::TempPointer {
            slot: 4,
            frame_depth: 1,
        };

        assert_eq!(
            binary_op(BinaryOp::Equal, &t_a1, &t_a2, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &t_a1, &t_b, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::Equal, &t_a1, &t_c, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn temp_pointer_has_no_ordering() {
        let p = dummy_program();
        let a = Value::TempPointer {
            slot: 1,
            frame_depth: 0,
        };
        let b = Value::TempPointer {
            slot: 2,
            frame_depth: 0,
        };
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &a, &b, &p).is_err(),
                "{op:?} on two temp pointers must fault, not silently order them"
            );
        }
    }

    #[test]
    fn projection_equality_is_structural() {
        let p = dummy_program();
        let cell = DefinitionId::new(DefinitionTag::GlobalVar, 10);
        let a = Value::projection(
            cell,
            vec![
                ProjSegment::Index(3),
                ProjSegment::Key(Value::String("hp".into())),
            ],
        );
        let b = Value::projection(
            cell,
            vec![
                ProjSegment::Index(3),
                ProjSegment::Key(Value::String("hp".into())),
            ],
        );
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &a, &b, &p).unwrap(),
            Value::Bool(false)
        );

        // Same root cell, different segments: not equal.
        let different_segments = Value::projection(cell, vec![ProjSegment::Index(4)]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &different_segments, &p).unwrap(),
            Value::Bool(false)
        );

        // Different root cell, same segments: not equal.
        let other_cell = DefinitionId::new(DefinitionTag::GlobalVar, 11);
        let different_root = Value::projection(
            other_cell,
            vec![
                ProjSegment::Index(3),
                ProjSegment::Key(Value::String("hp".into())),
            ],
        );
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &different_root, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn projection_equality_ptr_eq_fast_path() {
        let p = dummy_program();
        let cell = DefinitionId::new(DefinitionTag::GlobalVar, 10);
        let a = Value::projection(cell, vec![ProjSegment::Index(0)]);
        let snapshot = a.clone();
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &snapshot, &p).unwrap(),
            Value::Bool(true)
        );
    }

    /// A projection whose path contains a float-typed map key composes
    /// structurally through `ProjSegment::Key(Value::Float(_))`'s own
    /// `PartialEq` — same exact-IEEE composition as any other nested float
    /// (this module's `float_equality_is_exact_ieee_not_epsilon_tolerant`
    /// test covers the direct scalar case this must stay consistent with).
    #[test]
    fn projection_equality_nested_float_segment() {
        let p = dummy_program();
        let cell = DefinitionId::new(DefinitionTag::GlobalVar, 20);
        let a = Value::projection(cell, vec![ProjSegment::Key(Value::Float(1.5))]);
        let b = Value::projection(cell, vec![ProjSegment::Key(Value::Float(1.5))]);
        let c = Value::projection(cell, vec![ProjSegment::Key(Value::Float(1.500_000_1))]);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &c, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn projection_has_no_ordering() {
        let p = dummy_program();
        let cell = DefinitionId::new(DefinitionTag::GlobalVar, 10);
        let a = Value::projection(cell, vec![ProjSegment::Index(0)]);
        let b = Value::projection(cell, vec![ProjSegment::Index(1)]);
        for op in [
            BinaryOp::Less,
            BinaryOp::Greater,
            BinaryOp::LessOrEqual,
            BinaryOp::GreaterOrEqual,
        ] {
            assert!(
                binary_op(op, &a, &b, &p).is_err(),
                "{op:?} on two projections must fault, not silently order them"
            );
        }
    }

    /// Cross-variant identity types must still fault, not silently return
    /// `false` — same reasoning as `handle_equality_does_not_leak_across_to_
    /// other_identity_types` above (this function's `match_same_arms`
    /// `#[expect]` covers exactly this: identical arm bodies, deliberately
    /// unmerged patterns so unrelated identity types don't compare).
    #[test]
    fn pointer_and_projection_equality_does_not_leak_across_variants() {
        let p = dummy_program();
        let cell = DefinitionId::new(DefinitionTag::GlobalVar, 1);
        let var_ptr = Value::VariablePointer(cell);
        let temp_ptr = Value::TempPointer {
            slot: 0,
            frame_depth: 0,
        };
        let projection = Value::projection(cell, vec![]);
        let divert = Value::DivertTarget(cell);

        for (a, b) in [
            (&var_ptr, &temp_ptr),
            (&var_ptr, &projection),
            (&var_ptr, &divert),
            (&temp_ptr, &projection),
            (&temp_ptr, &divert),
            (&projection, &divert),
        ] {
            assert!(
                binary_op(BinaryOp::Equal, a, b, &p).is_err(),
                "{a:?} == {b:?} must fault, not silently compare across variants"
            );
            assert!(
                binary_op(BinaryOp::NotEqual, a, b, &p).is_err(),
                "{a:?} != {b:?} must fault, not silently compare across variants"
            );
        }
    }

    // ── Float equality: exact IEEE, not epsilon-tolerant (issue #939) ──────
    //
    // `float_op`'s `Equal`/`NotEqual` used to fudge with `(a - b).abs() <
    // f32::EPSILON` while every float nested inside a collection (array/map/
    // record/projection) compared via `Value`'s own exact `PartialEq`. That
    // was the inconsistency PR #931's review flagged. These tests pin down
    // that both routes now agree.

    /// One ULP at `0.5` is `f32::EPSILON / 2` (the exponent is one less than
    /// at `1.0`, halving the ULP) — a nonzero difference that still sits
    /// strictly inside the old `< f32::EPSILON` tolerance window. The old
    /// epsilon-fudged `float_op` would have called these equal; exact IEEE
    /// `==` must not.
    fn one_ulp_apart() -> (f32, f32) {
        let a = 0.5_f32;
        let b = f32::from_bits(a.to_bits() + 1);
        assert!(
            (a - b).abs() < f32::EPSILON,
            "fixture must stay inside the old epsilon window"
        );
        (a, b)
    }

    #[test]
    fn float_equality_is_exact_ieee_not_epsilon_tolerant() {
        let p = dummy_program();
        // Equal bit patterns compare equal, as before.
        assert_eq!(
            binary_op(BinaryOp::Equal, &Value::Float(1.5), &Value::Float(1.5), &p).unwrap(),
            Value::Bool(true)
        );
        // A difference well inside the old `f32::EPSILON` (~1.19e-7) window
        // must now compare NOT equal — the old epsilon fudge would have
        // silently called these equal.
        let (a, b) = one_ulp_apart();
        assert_ne!(
            a.to_bits(),
            b.to_bits(),
            "test fixture must pick genuinely distinct f32 bit patterns"
        );
        let (a, b) = (Value::Float(a), Value::Float(b));
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
    }

    /// Direct scalar float `==` and float-inside-an-array `==` must agree —
    /// the exact inconsistency PR #931's review flagged, now resolved by
    /// making both exact-IEEE.
    #[test]
    fn float_equality_direct_and_inside_array_are_consistent() {
        let p = dummy_program();
        let (a, b) = one_ulp_apart();
        let (near_a, near_b) = (Value::Float(a), Value::Float(b));

        let direct = binary_op(BinaryOp::Equal, &near_a, &near_b, &p).unwrap();

        let arr_a = Value::array(vec![near_a.clone()]);
        let arr_b = Value::array(vec![near_b.clone()]);
        let nested = binary_op(BinaryOp::Equal, &arr_a, &arr_b, &p).unwrap();

        assert_eq!(
            direct, nested,
            "direct float == and float-inside-array == must use the same semantics"
        );
        assert_eq!(direct, Value::Bool(false));
    }

    #[test]
    fn float_nan_never_equals_itself_directly() {
        // Matches the collection-level NaN rule (value-model-spec §4: "NaN-
        // bearing collections never compare equal") composing all the way
        // down to the scalar level now that both use exact IEEE `==`.
        let p = dummy_program();
        let a = Value::Float(f32::NAN);
        let b = Value::Float(f32::NAN);
        assert_eq!(
            binary_op(BinaryOp::Equal, &a, &b, &p).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &a, &b, &p).unwrap(),
            Value::Bool(true)
        );
    }

    // ── cast_to_int / cast_to_float (issue #955) ────────────────────────
    //
    // The in-domain arms (Int/Float/Bool/String) must keep their exact
    // pre-#955 behavior — oracle-anchored, byte-identical. The
    // out-of-domain arms used to silently fold to 0/0.0 through a wildcard;
    // they must now fault with `InvalidConversionDomain`.

    #[test]
    fn cast_to_int_identity_and_widening_domain_unchanged() {
        assert_eq!(cast_to_int(&Value::Int(7)).unwrap(), Value::Int(7));
        assert_eq!(cast_to_int(&Value::Float(2.9)).unwrap(), Value::Int(2));
        assert_eq!(cast_to_int(&Value::Float(-2.9)).unwrap(), Value::Int(-2));
        assert_eq!(cast_to_int(&Value::Bool(true)).unwrap(), Value::Int(1));
        assert_eq!(cast_to_int(&Value::Bool(false)).unwrap(), Value::Int(0));
        assert_eq!(
            cast_to_int(&Value::String("42".into())).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn cast_to_int_unparseable_string_keeps_legacy_silent_zero() {
        // The pre-#955 legacy behavior for a failed string parse — distinct
        // from the T1b lowercase `int()` intrinsic, which faults on this
        // exact input (`conversion_ops::int_parse_failure_faults`).
        assert_eq!(
            cast_to_int(&Value::String("potato".into())).unwrap(),
            Value::Int(0)
        );
    }

    #[test]
    fn cast_to_float_identity_and_widening_domain_unchanged() {
        assert_eq!(
            cast_to_float(&Value::Float(1.5)).unwrap(),
            Value::Float(1.5)
        );
        assert_eq!(cast_to_float(&Value::Int(3)).unwrap(), Value::Float(3.0));
        assert_eq!(
            cast_to_float(&Value::Bool(true)).unwrap(),
            Value::Float(1.0)
        );
        assert_eq!(
            cast_to_float(&Value::Bool(false)).unwrap(),
            Value::Float(0.0)
        );
        assert_eq!(
            cast_to_float(&Value::String("2.5".into())).unwrap(),
            Value::Float(2.5)
        );
    }

    #[test]
    fn cast_to_float_unparseable_string_keeps_legacy_silent_zero() {
        assert_eq!(
            cast_to_float(&Value::String("nope".into())).unwrap(),
            Value::Float(0.0)
        );
    }

    #[test]
    fn cast_to_int_out_of_domain_variants_fault_instead_of_folding_to_zero() {
        let cases: Vec<Value> = vec![
            Value::List(
                ListValue {
                    items: vec![],
                    origins: vec![],
                }
                .into(),
            ),
            Value::DivertTarget(DefinitionId::new(DefinitionTag::Address, 0)),
            Value::VariablePointer(DefinitionId::new(DefinitionTag::Address, 0)),
            Value::TempPointer {
                slot: 0,
                frame_depth: 0,
            },
            Value::Null,
            Value::FragmentRef(0),
            Value::array(vec![Value::Int(1)]),
            Value::map(OrderedMap::new()),
            Value::record(brink_format::ShapeId(0), vec![Value::Int(1)]),
            Value::FnRef(DefinitionId::new(DefinitionTag::Address, 0)),
            Value::Handle {
                kind: NameId(0),
                id: 0,
            },
        ];
        for v in cases {
            let err = cast_to_int(&v).unwrap_err();
            assert!(
                matches!(
                    err,
                    RuntimeError::InvalidConversionDomain { target: "INT", .. }
                ),
                "expected InvalidConversionDomain{{target: \"INT\", ..}} for {v:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn cast_to_float_out_of_domain_variants_fault_instead_of_folding_to_zero() {
        let v = Value::array(vec![Value::Int(1)]);
        let err = cast_to_float(&v).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "FLOAT",
                got: "array",
            }
        );

        let v = Value::record(brink_format::ShapeId(0), vec![Value::Int(1)]);
        let err = cast_to_float(&v).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "FLOAT",
                got: "record",
            }
        );
    }
}

/// NS-A8 tower semantics at the operator/display/equality layer
/// (`docs/tower-mini-spec.md` T3/T4).
#[cfg(test)]
mod tower_tests {
    use super::*;
    use crate::program::{LinkedContainer, Program};
    use brink_format::{DefinitionId, DefinitionTag};
    use glam::{Mat2, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
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

    fn v2(x: f32, y: f32) -> Value {
        Value::Vec2(Vec2::new(x, y))
    }

    fn v3(x: f32, y: f32, z: f32) -> Value {
        Value::Vec3(Vec3::new(x, y, z))
    }

    fn v4(x: f32, y: f32, z: f32, w: f32) -> Value {
        Value::Vec4(Vec4::new(x, y, z, w))
    }

    // ── T3: the ruled operator table, semantics per glam ─────────────

    #[test]
    fn vec_add_sub_mul_are_componentwise() {
        let p = dummy_program();
        assert_eq!(
            binary_op(BinaryOp::Add, &v2(1.0, 2.0), &v2(3.0, 4.0), &p).unwrap(),
            v2(4.0, 6.0)
        );
        assert_eq!(
            binary_op(BinaryOp::Subtract, &v2(3.0, 4.0), &v2(1.0, 2.0), &p).unwrap(),
            v2(2.0, 2.0)
        );
        assert_eq!(
            binary_op(BinaryOp::Multiply, &v2(2.0, 3.0), &v2(4.0, 5.0), &p).unwrap(),
            v2(8.0, 15.0)
        );
    }

    #[test]
    fn scalar_scale_both_orders_with_int_promotion() {
        let p = dummy_program();
        assert_eq!(
            binary_op(BinaryOp::Multiply, &v2(1.0, 2.0), &Value::Float(2.0), &p).unwrap(),
            v2(2.0, 4.0)
        );
        assert_eq!(
            binary_op(BinaryOp::Multiply, &Value::Int(3), &v2(1.0, 2.0), &p).unwrap(),
            v2(3.0, 6.0)
        );
    }

    #[test]
    fn quat_multiply_composes_and_rotates() {
        let p = dummy_program();
        let q = Quat::from_rotation_z(core::f32::consts::FRAC_PI_2);
        let composed = binary_op(BinaryOp::Multiply, &Value::Quat(q), &Value::Quat(q), &p).unwrap();
        assert_eq!(composed, Value::Quat(q * q));
        let rotated =
            binary_op(BinaryOp::Multiply, &Value::Quat(q), &v3(1.0, 0.0, 0.0), &p).unwrap();
        assert_eq!(rotated, Value::Vec3(q * Vec3::new(1.0, 0.0, 0.0)));
    }

    #[test]
    fn mat_vec_transforms() {
        let p = dummy_program();
        let m = Mat2::from_cols(Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0));
        let got = binary_op(BinaryOp::Multiply, &Value::Mat2(m), &v2(1.0, 0.0), &p).unwrap();
        assert_eq!(got, Value::Vec2(m * Vec2::new(1.0, 0.0)));
    }

    #[test]
    fn unruled_tower_ops_fault_not_coerce() {
        let p = dummy_program();
        // Cross-size arithmetic, ordering, and every glam-native form F31
        // (issue #1145) did NOT rule in still fault — only `mat * mat`,
        // `mat * scalar` (one direction), and `vec / scalar` (one direction)
        // became implemented rows; `mat ± mat`, `quat * scalar`, `vec /
        // vec`, `scalar / vec`, and `scalar * mat` are deliberately still
        // unruled.
        for (op, l, r) in [
            (BinaryOp::Add, v2(1.0, 2.0), v3(1.0, 2.0, 3.0)),
            (BinaryOp::Divide, v2(1.0, 2.0), v2(1.0, 2.0)),
            (BinaryOp::Divide, Value::Float(2.0), v2(1.0, 2.0)),
            (BinaryOp::Less, v2(1.0, 2.0), v2(3.0, 4.0)),
            (BinaryOp::Greater, v3(1.0, 2.0, 3.0), v3(1.0, 2.0, 3.0)),
            (
                BinaryOp::Add,
                Value::Mat3(Mat3::IDENTITY),
                Value::Mat3(Mat3::IDENTITY),
            ),
            (
                BinaryOp::Subtract,
                Value::Mat3(Mat3::IDENTITY),
                Value::Mat3(Mat3::IDENTITY),
            ),
            (
                BinaryOp::Multiply,
                Value::Quat(Quat::IDENTITY),
                Value::Float(2.0),
            ),
            (
                BinaryOp::Multiply,
                Value::Float(2.0),
                Value::Mat3(Mat3::IDENTITY),
            ),
            (BinaryOp::Add, v2(1.0, 2.0), Value::Float(1.0)),
        ] {
            let err = binary_op(op, &l, &r, &p).unwrap_err();
            assert!(matches!(err, RuntimeError::TypeError(_)), "{op:?}: {err:?}");
        }
    }

    // ── F31 partial-b (issue #1145): the three newly-implemented rows ──

    #[test]
    fn mat_mat_composes() {
        let p = dummy_program();
        let a2 = Mat2::from_cols(Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0));
        let b2 = Mat2::from_cols(Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0));
        assert_eq!(
            binary_op(BinaryOp::Multiply, &Value::Mat2(a2), &Value::Mat2(b2), &p).unwrap(),
            Value::Mat2(a2 * b2)
        );

        let a3 = Mat3::from_cols(
            Vec3::new(1.0, 2.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let b3 = Mat3::IDENTITY;
        assert_eq!(
            binary_op(BinaryOp::Multiply, &Value::Mat3(a3), &Value::Mat3(b3), &p).unwrap(),
            Value::Mat3(a3 * b3)
        );

        let a4 = Mat4::from_cols(
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 3.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        );
        let b4 = Mat4::IDENTITY;
        assert_eq!(
            binary_op(BinaryOp::Multiply, &Value::Mat4(a4), &Value::Mat4(b4), &p).unwrap(),
            Value::Mat4(a4 * b4)
        );
    }

    #[test]
    fn mat_scalar_scales_one_direction_with_int_promotion() {
        let p = dummy_program();
        let m = Mat2::from_cols(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0));
        assert_eq!(
            binary_op(BinaryOp::Multiply, &Value::Mat2(m), &Value::Float(2.0), &p).unwrap(),
            Value::Mat2(m * 2.0)
        );
        // int operand promotes like ink's int->float coercion.
        assert_eq!(
            binary_op(BinaryOp::Multiply, &Value::Mat2(m), &Value::Int(3), &p).unwrap(),
            Value::Mat2(m * 3.0)
        );
        // F31 named only "mat * scalar" — the commuted `scalar * mat` form
        // (which glam itself defines) is deliberately not added here; still
        // faults.
        let err =
            binary_op(BinaryOp::Multiply, &Value::Float(2.0), &Value::Mat2(m), &p).unwrap_err();
        assert!(matches!(err, RuntimeError::TypeError(_)));
    }

    #[test]
    fn vec_scalar_divides_one_direction_with_int_promotion() {
        let p = dummy_program();
        assert_eq!(
            binary_op(BinaryOp::Divide, &v2(4.0, 8.0), &Value::Float(2.0), &p).unwrap(),
            v2(2.0, 4.0)
        );
        // int operand promotes like ink's int->float coercion.
        assert_eq!(
            binary_op(BinaryOp::Divide, &v2(4.0, 8.0), &Value::Int(2), &p).unwrap(),
            v2(2.0, 4.0)
        );
        assert_eq!(
            binary_op(
                BinaryOp::Divide,
                &v3(4.0, 8.0, 12.0),
                &Value::Float(4.0),
                &p
            )
            .unwrap(),
            v3(1.0, 2.0, 3.0)
        );
        assert_eq!(
            binary_op(
                BinaryOp::Divide,
                &v4(4.0, 8.0, 12.0, 16.0),
                &Value::Float(4.0),
                &p
            )
            .unwrap(),
            v4(1.0, 2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn vec_divide_by_zero_is_ieee_not_a_fault() {
        // T4: division by zero yields inf/nan lanes, exactly like bare float
        // division (`float_op`'s `Divide` arm) — NOT `RuntimeError::DivisionByZero`.
        let p = dummy_program();
        let got = binary_op(BinaryOp::Divide, &v2(1.0, -1.0), &Value::Float(0.0), &p).unwrap();
        assert!(
            matches!(
                &got,
                Value::Vec2(r)
                    if r.x.is_infinite() && r.x.is_sign_positive()
                        && r.y.is_infinite() && r.y.is_sign_negative()
            ),
            "{got:?}"
        );
        let zero_over_zero =
            binary_op(BinaryOp::Divide, &v2(0.0, 0.0), &Value::Float(0.0), &p).unwrap();
        assert!(
            matches!(&zero_over_zero, Value::Vec2(r) if r.x.is_nan() && r.y.is_nan()),
            "{zero_over_zero:?}"
        );
    }

    // ── T4: componentwise IEEE equality; NOT orderable ───────────────

    #[test]
    fn equality_is_componentwise_ieee() {
        let p = dummy_program();
        assert_eq!(
            binary_op(BinaryOp::Equal, &v2(1.0, 2.0), &v2(1.0, 2.0), &p).unwrap(),
            Value::Bool(true)
        );
        // -0 == +0 per lane.
        assert_eq!(
            binary_op(BinaryOp::Equal, &v2(-0.0, 1.0), &v2(0.0, 1.0), &p).unwrap(),
            Value::Bool(true)
        );
        // A NaN lane makes a value unequal to itself.
        let nan = v2(f32::NAN, 0.0);
        assert_eq!(
            binary_op(BinaryOp::Equal, &nan, &nan, &p).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            binary_op(BinaryOp::NotEqual, &nan, &nan, &p).unwrap(),
            Value::Bool(true)
        );
        // Cross-kind equality faults (no silent false).
        assert!(binary_op(BinaryOp::Equal, &v2(1.0, 2.0), &v3(1.0, 2.0, 0.0), &p).is_err());
        // vec == null keeps the universal cross-type-null false.
        assert_eq!(
            binary_op(BinaryOp::Equal, &v2(1.0, 2.0), &Value::Null, &p).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn extremum_verbs_fault_not_orderable_for_tower_elements() {
        // T4/§4b: a vec in an ordering context is a NotOrderable fault.
        use crate::output::OutputBuffer;
        let mut flow = crate::story::Flow {
            threads: Vec::new(),
            value_stack: Vec::new(),
            output: OutputBuffer::new(),
            pending_choices: Vec::new(),
            current_tags: Vec::new(),
            in_tag: false,
            skipping_choice: false,
            did_safe_exit: false,
            did_unsafe_yield: false,
            exec_mode: crate::story::ExecMode::default(),
            comparator_depth: 0,
        };
        flow.value_stack
            .push(Value::array(vec![v2(1.0, 2.0), v2(3.0, 4.0)]));
        let err = crate::collection_ops::seq_min(&mut flow).unwrap_err();
        assert!(matches!(err, RuntimeError::NotOrderable { .. }), "{err:?}");
    }

    // ── Display: the structural default form ─────────────────────────

    #[test]
    fn display_is_structural_construction_form() {
        let p = dummy_program();
        assert_eq!(stringify(&v2(1.0, 2.5), &p), "vec2 { x: 1, y: 2.5 }");
        assert_eq!(
            stringify(&Value::Quat(Quat::from_xyzw(0.0, 0.0, 0.0, 1.0)), &p),
            "quat { x: 0, y: 0, z: 0, w: 1 }"
        );
        assert_eq!(
            stringify(
                &Value::Mat2(Mat2::from_cols(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0))),
                &p
            ),
            "mat2 { x_axis: vec2 { x: 1, y: 2 }, y_axis: vec2 { x: 3, y: 4 } }"
        );
    }

    // ── Conversion domain: tower is outside INT()/FLOAT() ────────────

    #[test]
    fn tower_is_outside_the_cast_domain() {
        assert!(matches!(
            cast_to_int(&v2(1.0, 2.0)),
            Err(RuntimeError::InvalidConversionDomain { .. })
        ));
        assert!(matches!(
            cast_to_float(&v3(1.0, 2.0, 3.0)),
            Err(RuntimeError::InvalidConversionDomain { .. })
        ));
    }

    // ── Truthiness: record/handle precedent (always truthy) ──────────

    #[test]
    fn tower_values_are_truthy_compounds() {
        assert!(is_truthy(&v2(0.0, 0.0)).unwrap());
        assert!(is_truthy(&Value::Mat3(Mat3::ZERO)).unwrap());
    }
}
