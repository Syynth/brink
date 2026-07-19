//! T1e path-projection RMW walk (`docs/t1e-spec.md` §1/§3/§4).
//!
//! A [`Value::Projection`] is `(root cell, path segments)` — never an
//! interior pointer. Reads walk the segment chain against the root's
//! *current* value; writes desugar to root-cell RMW: take the root → walk →
//! `make_mut` spine → set the final segment → store back (spec §3). A path
//! that no longer resolves against the root's current value (a shrunk
//! array, a removed map key, a struct field dropped by recompile) is the
//! single ratified [`RuntimeError::ProjectionInvalidated`] fault (spec
//! §1(2)) — never a clamp, never silent.
//!
//! These are the functions the RFC names `ProjRead`/`ProjWrite`
//! (`docs/format-v4-rfc.md` §3) — the VM's `Opcode::ProjRead`/
//! `Opcode::ProjWrite` dispatch arms call them directly, and so does
//! `GetTemp`/`SetTemp`/`TakeTemp`'s additive `Value::Projection` dispatch
//! arm (a projection-bound `ref` parameter dereferences through the exact
//! same walk a bare `ProjRead`/`ProjWrite` opcode would perform).
//!
//! Array/map indexing reuses [`collection_ops::read_index`]/
//! [`collection_ops::write_index`] verbatim (identical bounds/key-exists
//! semantics — a projection just reaches them through a different route
//! than a direct `[…]` expression) and re-labels any fault as
//! `ProjectionInvalidated` (spec §1(2) unifies every path-invalidation cause
//! under one fault). Struct-field segments resolve by name against the
//! root's *current* shape (spec §4: "resolve by field name against the
//! shape at access time; a field removed by recompile faults at
//! rehydration" — the same rule applies at ordinary read/write time, not
//! just rehydration).

use alloc::format;
use alloc::string::String;

use brink_format::{DefinitionId, NameId, ProjSegment, ShapeId, Value};

use crate::collection_ops;
use crate::error::RuntimeError;
use crate::program::Program;
use crate::state::ContextAccess;

/// `ProjRead`: walk `segments` against `cell`'s *current* value, returning a
/// clone of the resolved value. Faults `ProjectionInvalidated` if the root
/// is unresolved or the path no longer resolves.
pub(crate) fn read<C: ContextAccess + ?Sized>(
    program: &Program,
    context: &C,
    cell: DefinitionId,
    segments: &[ProjSegment],
) -> Result<Value, RuntimeError> {
    let idx = resolve_root(program, cell)?;
    let mut current = context.global(idx);
    for seg in segments {
        current = step_read(current, seg, program)?;
    }
    Ok(current.clone())
}

/// `ProjWrite`: root-cell RMW write. Take `cell`'s current value, walk +
/// `make_mut` down to the final segment, assign `value`, store the whole
/// (possibly-COWed) value back. Faults `ProjectionInvalidated` on an
/// unresolved root or path — matching `collection_ops::write_index`'s own
/// established convention (`docs/t1c-spec.md`-adjacent precedent: a
/// turn-terminating fault propagates before any store-back, the same
/// "no partial silent state" discipline every other RMW op in this VM
/// follows), the taken value is not restored on fault since a
/// turn-terminating fault unwinds the current turn entirely.
pub(crate) fn write<C: ContextAccess + ?Sized>(
    program: &Program,
    context: &mut C,
    cell: DefinitionId,
    segments: &[ProjSegment],
    value: Value,
) -> Result<(), RuntimeError> {
    let idx = resolve_root(program, cell)?;
    let mut root = context.take_global(idx);
    write_recursive(&mut root, segments, value, program)?;
    context.set_global(idx, root);
    Ok(())
}

/// Move-out-and-null read: read the current value at the path (like
/// [`read`]), then write `Null` back at that same path — the projection
/// analog of `TakeGlobal`/`TakeTemp`'s "move out, leave Null" discipline
/// (value-model-spec §5's sharing/last-use-elision story). Two RMW passes
/// (read then write) rather than a single combined walk — correctness over
/// cleverness for this first implementation; no hot-path requirement is
/// documented for T1e.
pub(crate) fn take<C: ContextAccess + ?Sized>(
    program: &Program,
    context: &mut C,
    cell: DefinitionId,
    segments: &[ProjSegment],
) -> Result<Value, RuntimeError> {
    let taken = read(program, context, cell, segments)?;
    write(program, context, cell, segments, Value::Null)?;
    Ok(taken)
}

/// Resolve a projection's root cell to its global slot index, faulting
/// `ProjectionInvalidated` (not the generic `UnresolvedGlobal`) since a
/// dangling root is exactly the spec §1(2) fault family — e.g. `#@was`
/// couldn't rebind the root across a recompile.
fn resolve_root(program: &Program, cell: DefinitionId) -> Result<u32, RuntimeError> {
    program
        .resolve_global(cell)
        .ok_or_else(|| RuntimeError::ProjectionInvalidated(format!("root cell {cell} unresolved")))
}

/// Convert one [`ProjSegment`] into the plain [`Value`] `collection_ops`'
/// index helpers expect (`Index(n)` → `Int(n)`, `Key(v)` → `v`).
fn seg_as_value(seg: &ProjSegment) -> Value {
    match seg {
        ProjSegment::Index(n) => Value::Int(*n),
        ProjSegment::Key(v) => v.clone(),
    }
}

/// A struct-field segment's name, if `seg` is shaped like one (a `Key`
/// segment carrying a `String` — struct field names are always literal
/// strings, spec §3/§4). `None` for an `Index` segment or a non-`String`
/// key, which can never name a field.
fn seg_as_field_name(seg: &ProjSegment) -> Option<&str> {
    match seg {
        ProjSegment::Key(Value::String(s)) => Some(s),
        _ => None,
    }
}

/// Resolve a struct field name to its flat offset within `shape`
/// (`docs/t1e-spec.md` §4: "resolve by field name against the shape at
/// access time").
fn resolve_field_offset(program: &Program, shape: ShapeId, name: &str) -> Option<usize> {
    let entry = program.struct_shape(shape)?;
    entry
        .fields
        .iter()
        .position(|&id: &NameId| program.name_checked(id) == Some(name))
}

/// Re-label any `RuntimeError` as `ProjectionInvalidated`, preserving the
/// underlying cause in the message (spec §1(2): one fault, every cause).
fn invalidated(e: &RuntimeError) -> RuntimeError {
    RuntimeError::ProjectionInvalidated(alloc::string::ToString::to_string(e))
}

fn field_not_found_fault(name: &str) -> RuntimeError {
    RuntimeError::ProjectionInvalidated(format!("struct has no field {name:?}"))
}

fn not_a_struct_fault(seg: &ProjSegment) -> RuntimeError {
    RuntimeError::ProjectionInvalidated(format!(
        "field-access segment against a non-struct value: {}",
        describe_segment(seg)
    ))
}

fn describe_segment(seg: &ProjSegment) -> String {
    match seg {
        ProjSegment::Index(n) => format!("[{n}]"),
        ProjSegment::Key(v) => format!("[{v:?}]"),
    }
}

/// One read step: walk `seg` against `current`, returning a borrow into it.
fn step_read<'v>(
    current: &'v Value,
    seg: &ProjSegment,
    program: &Program,
) -> Result<&'v Value, RuntimeError> {
    match current {
        Value::Record { shape, fields } => {
            let name = seg_as_field_name(seg).ok_or_else(|| not_a_struct_fault(seg))?;
            let offset = resolve_field_offset(program, *shape, name)
                .ok_or_else(|| field_not_found_fault(name))?;
            fields
                .get(offset)
                .ok_or_else(|| field_not_found_fault(name))
        }
        Value::Array(_) | Value::Map(_) => {
            collection_ops::read_index(current, &seg_as_value(seg)).map_err(|e| invalidated(&e))
        }
        other => Err(RuntimeError::ProjectionInvalidated(format!(
            "cannot index into a {} value",
            value_kind(other)
        ))),
    }
}

/// Recursive write step: descend `current` along `segments`, `make_mut`-ing
/// each level, and assign `value` at the final segment. `segments` must be
/// non-empty — the empty case (assigning the whole root) is handled by
/// [`write`]'s caller contract (a projection with zero segments would be a
/// bare `ref`, which never reaches `Value::Projection` in the first place —
/// T1e-1's zero-segment fast path binds a plain `VariablePointer`).
fn write_recursive(
    current: &mut Value,
    segments: &[ProjSegment],
    value: Value,
    program: &Program,
) -> Result<(), RuntimeError> {
    let Some((seg, rest)) = segments.split_first() else {
        *current = value;
        return Ok(());
    };
    if rest.is_empty() {
        return set_segment(current, seg, value, program);
    }
    match current {
        Value::Record { shape, .. } => {
            let name = seg_as_field_name(seg)
                .ok_or_else(|| not_a_struct_fault(seg))?
                .to_owned();
            let offset = resolve_field_offset(program, *shape, &name)
                .ok_or_else(|| field_not_found_fault(&name))?;
            let fields = current
                .record_make_mut()
                .ok_or_else(|| field_not_found_fault(&name))?;
            let elem = fields
                .get_mut(offset)
                .ok_or_else(|| field_not_found_fault(&name))?;
            write_recursive(elem, rest, value, program)
        }
        Value::Array(_) => {
            let len = current.as_array().map_or(0, |items| items.len());
            let idx = array_index_of(seg, len)?;
            let items = current
                .array_make_mut()
                .ok_or_else(|| RuntimeError::ProjectionInvalidated("not an array".into()))?;
            let elem = items.get_mut(idx).ok_or_else(|| {
                RuntimeError::ProjectionInvalidated("array index vanished".into())
            })?;
            write_recursive(elem, rest, value, program)
        }
        Value::Map(_) => {
            let key = brink_format::MapKey::from_value(&seg_as_value(seg)).ok_or_else(|| {
                RuntimeError::ProjectionInvalidated("map key is not int/string/bool".into())
            })?;
            let has_key = current.as_map().is_some_and(|m| m.contains_key(&key));
            if !has_key {
                return Err(RuntimeError::ProjectionInvalidated(format!(
                    "map has no key {key:?}"
                )));
            }
            let map = current
                .map_make_mut()
                .ok_or_else(|| RuntimeError::ProjectionInvalidated("not a map".into()))?;
            let elem = map
                .get_mut(&key)
                .ok_or_else(|| RuntimeError::ProjectionInvalidated("map key vanished".into()))?;
            write_recursive(elem, rest, value, program)
        }
        other => Err(RuntimeError::ProjectionInvalidated(format!(
            "cannot index into a {} value",
            value_kind(other)
        ))),
    }
}

/// The final-segment assignment: set `seg` on `current` to `value`. Reuses
/// [`collection_ops::write_index`] for `Array`/`Map` verbatim (identical
/// bounds/key-exists discipline as a direct `[…] =` expression); `Record`
/// gets its own by-name field set (no `RecordSetDyn` reuse — that op works
/// on a whole popped/pushed stack value, not a `&mut Value` in place).
fn set_segment(
    current: &mut Value,
    seg: &ProjSegment,
    value: Value,
    program: &Program,
) -> Result<(), RuntimeError> {
    match current {
        Value::Record { shape, .. } => {
            let name = seg_as_field_name(seg)
                .ok_or_else(|| not_a_struct_fault(seg))?
                .to_owned();
            let offset = resolve_field_offset(program, *shape, &name)
                .ok_or_else(|| field_not_found_fault(&name))?;
            let fields = current
                .record_make_mut()
                .ok_or_else(|| field_not_found_fault(&name))?;
            let slot = fields
                .get_mut(offset)
                .ok_or_else(|| field_not_found_fault(&name))?;
            *slot = value;
            Ok(())
        }
        Value::Array(_) | Value::Map(_) => {
            collection_ops::write_index(current, &seg_as_value(seg), value)
                .map_err(|e| invalidated(&e))
        }
        other => Err(RuntimeError::ProjectionInvalidated(format!(
            "cannot index into a {} value",
            value_kind(other)
        ))),
    }
}

/// Validate an array index segment against `len`, same domain as
/// `collection_ops`' own (private) `array_index` — duplicated here in
/// terms of a `ProjectionInvalidated` fault rather than
/// `IndexOutOfBounds`/`InvalidArrayIndex` directly, since the intermediate
/// (non-final) leg of the recursive write spine needs the index *before*
/// calling `array_make_mut`, one step earlier than `write_index`'s own
/// internal call shape allows reuse.
fn array_index_of(seg: &ProjSegment, len: usize) -> Result<usize, RuntimeError> {
    let ProjSegment::Index(i) = seg else {
        return Err(RuntimeError::ProjectionInvalidated(format!(
            "array index segment must be an int, got {}",
            describe_segment(seg)
        )));
    };
    #[expect(clippy::cast_sign_loss)]
    if *i < 0 || *i as usize >= len {
        Err(RuntimeError::ProjectionInvalidated(format!(
            "array index {i} out of bounds (len {len})"
        )))
    } else {
        Ok(*i as usize)
    }
}

/// Human-readable type label — mirrors the per-module `type_name` helpers
/// elsewhere in this crate (`collection_ops`/`record_ops`/`vm`).
fn value_kind(v: &Value) -> &'static str {
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
    use crate::program::{GlobalSlot, LinkedContainer, StructShapeEntry};
    use crate::world::{ResolvedPolicy, World};
    use alloc::vec;
    use brink_format::{CountingFlags, DefinitionTag};
    use std::collections::HashMap;

    fn global_id(n: u64) -> DefinitionId {
        DefinitionId::new(DefinitionTag::GlobalVar, n)
    }

    /// A minimal `Program` with one global `VAR npc` (a `Record` at shape 0
    /// with fields `hp`, `name`) at global slot 0 and one plain `VAR gold`
    /// at slot 1 — same hand-built-`Program` style `record_ops`' own tests
    /// use, one level below full bytecode.
    fn test_program() -> Program {
        let npc_id = global_id(1);
        let gold_id = global_id(2);
        let mut global_map = HashMap::new();
        global_map.insert(npc_id, 0);
        global_map.insert(gold_id, 1);
        Program {
            containers: vec![LinkedContainer {
                id: DefinitionId::new(DefinitionTag::Address, 0),
                bytecode: vec![],
                counting_flags: CountingFlags::empty(),
                path_hash: 0,
                param_count: 0,
                params: Vec::new(),
                scope_table_idx: 0,
            }],
            address_map: HashMap::new(),
            scope_ids: vec![DefinitionId::new(DefinitionTag::Address, 0)],
            source_checksum: 0,
            globals: vec![
                GlobalSlot {
                    id: npc_id,
                    name: NameId(0),
                    default: Value::record(ShapeId(0), vec![Value::Int(10), Value::Null]),
                    local: false,
                },
                GlobalSlot {
                    id: gold_id,
                    name: NameId(3),
                    default: Value::Int(5),
                    local: false,
                },
            ],
            global_map,
            name_table: vec![
                "npc".to_string(),
                "hp".to_string(),
                "name".to_string(),
                "gold".to_string(),
            ],
            address_by_path: HashMap::new(),
            root_idx: 0,
            list_literals: vec![],
            literal_pool: vec![],
            list_item_map: HashMap::new(),
            list_defs: vec![],
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
            local_scope_defaults: Vec::new(),
            struct_shapes: vec![StructShapeEntry {
                name: NameId(0),
                fields: vec![NameId(1), NameId(2)],
            }],
            private_defs: Vec::new(),
            alias_table: Vec::new(),
        }
    }

    fn world_with_defaults(program: &Program) -> World {
        World::from_globals(program.global_defaults(), ResolvedPolicy::all_world())
    }

    #[test]
    fn read_walks_struct_field() {
        let program = test_program();
        let world = world_with_defaults(&program);
        let cell = global_id(1);
        let segments = vec![ProjSegment::Key(Value::String("hp".into()))];
        let v = read(&program, &world, cell, &segments).expect("read");
        assert_eq!(v, Value::Int(10));
    }

    #[test]
    fn write_updates_struct_field_root_cell() {
        let program = test_program();
        let mut world = world_with_defaults(&program);
        let cell = global_id(1);
        let segments = vec![ProjSegment::Key(Value::String("hp".into()))];
        write(&program, &mut world, cell, &segments, Value::Int(42)).expect("write");
        let v = read(&program, &world, cell, &segments).expect("read back");
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn write_missing_field_faults_projection_invalidated() {
        let program = test_program();
        let mut world = world_with_defaults(&program);
        let cell = global_id(1);
        let segments = vec![ProjSegment::Key(Value::String("mana".into()))];
        let err = write(&program, &mut world, cell, &segments, Value::Int(1)).unwrap_err();
        assert!(matches!(err, RuntimeError::ProjectionInvalidated(_)));
    }

    #[test]
    fn read_array_index_out_of_bounds_faults() {
        let program = test_program();
        let mut world = world_with_defaults(&program);
        world.set_global(0, Value::array(vec![Value::Int(1), Value::Int(2)]));
        let cell = global_id(1);
        let segments = vec![ProjSegment::Index(5)];
        let err = read(&program, &world, cell, &segments).unwrap_err();
        assert!(matches!(err, RuntimeError::ProjectionInvalidated(_)));
    }

    #[test]
    fn take_reads_then_leaves_null() {
        let program = test_program();
        let mut world = world_with_defaults(&program);
        let cell = global_id(2);
        let segments: Vec<ProjSegment> = Vec::new();
        let taken = take(&program, &mut world, cell, &segments).expect("take");
        assert_eq!(taken, Value::Int(5));
        assert_eq!(*world.global(1), Value::Null);
    }

    #[test]
    fn overlapping_projections_write_through_immediately() {
        // Two projections into the same root cell: a write through one is
        // visible to a read through the other (spec §1(3): "every write
        // applies to the root cell at the moment it happens").
        let program = test_program();
        let mut world = world_with_defaults(&program);
        let cell = global_id(1);
        let a = vec![ProjSegment::Key(Value::String("hp".into()))];
        let b = vec![ProjSegment::Key(Value::String("hp".into()))];
        write(&program, &mut world, cell, &a, Value::Int(99)).expect("write via a");
        let via_b = read(&program, &world, cell, &b).expect("read via b");
        assert_eq!(via_b, Value::Int(99));
    }
}
