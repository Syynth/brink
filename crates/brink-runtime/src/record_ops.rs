//! TM-4 record opcode implementations (`docs/typed-mode-spec.md` §6;
//! fault semantics from `docs/value-model-spec.md` §11c).
//!
//! Every op here is total in the same sense [`collection_ops`] is: a
//! missing field is a turn-terminating `RuntimeError`, never a silent
//! `Null`. Mutation goes through `Value::record_make_mut` — the take →
//! `make_mut` → write-back RMW discipline (value-model-spec §5): an
//! unshared record mutates in place, a shared one COWs exactly once.
//!
//! `RecordGetDyn`/`RecordSetDyn` resolve a field by name against the
//! record's own shape (looked up in [`Program::struct_shape`] by the
//! record's `ShapeId`) — the by-name field ops every dialect can use
//! correctly, per typed-mode-spec §6 ("gradual: Unknown head defers to
//! runtime field lookup with a turn-terminating fault on missing field").
//! Static-offset field ops (`RecordGet`/`RecordSet`, the strict-mode
//! performance payoff the spec anticipates) are reserved at the format layer
//! but not implemented here — see the PR description's scope note.
//!
//! [`collection_ops`]: crate::collection_ops

use alloc::format;
use alloc::vec::Vec;

use brink_format::{NameId, ShapeId, Value};

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::Flow;

/// `RecordNew(shape_id)`: pop the shape's declared field count worth of
/// values (in reverse push order — the caller/compiler is responsible for
/// pushing them in shape declaration order), push `Record { shape, fields }`.
pub(crate) fn record_new(
    flow: &mut Flow,
    program: &Program,
    shape_id: u32,
) -> Result<(), RuntimeError> {
    let shape = ShapeId(shape_id);
    let entry = program
        .struct_shape(shape)
        .ok_or(RuntimeError::InvalidShapeId(shape_id))?;
    let field_count = entry.fields.len();
    let mut fields = Vec::with_capacity(field_count);
    for _ in 0..field_count {
        fields.push(flow.pop_value()?);
    }
    fields.reverse();
    flow.value_stack.push(Value::record(shape, fields));
    Ok(())
}

/// `RecordGetDyn(name_id)`: `[record]` → field value, looked up by name in
/// the record's own shape. Turn-terminating fault if the popped value isn't
/// a `Record`, or if its shape has no field by that name.
pub(crate) fn record_get_dyn(
    flow: &mut Flow,
    program: &Program,
    name_id: u16,
) -> Result<(), RuntimeError> {
    let record = flow.pop_value()?;
    let value = read_field(program, &record, NameId(name_id))?.clone();
    flow.value_stack.push(value);
    Ok(())
}

/// `RecordSetDyn(name_id)`: `[record, value]` → updated record (take →
/// `make_mut` → write-back), field selected by name. Turn-terminating fault
/// if the popped container isn't a `Record`, or if its shape has no field
/// by that name.
pub(crate) fn record_set_dyn(
    flow: &mut Flow,
    program: &Program,
    name_id: u16,
) -> Result<(), RuntimeError> {
    let value = flow.pop_value()?;
    let mut record = flow.pop_value()?;
    let idx = field_index(program, &record, NameId(name_id))?;
    note_record_mutation(&record);
    let Some(fields) = record.record_make_mut() else {
        return Err(RuntimeError::NotARecord(type_name(&record)));
    };
    #[expect(
        clippy::indexing_slicing,
        reason = "index validated by field_index above"
    )]
    {
        fields[idx] = value;
    }
    flow.value_stack.push(record);
    Ok(())
}

/// `RecordGet(offset)`: `[record]` → field value, looked up by flat offset
/// into the record's own field vector (TM-4c, the strict-mode static-offset
/// payoff typed-mode-spec §6 anticipates). Unlike [`record_get_dyn`], this
/// never touches `Program::struct_shapes` — the offset is trusted to
/// already be correct for whatever shape the compiler proved at LIR
/// lowering time, and only the record's own field count is checked, which
/// is exactly the lookup this op exists to skip. Turn-terminating fault if
/// the popped value isn't a `Record`, or the offset is out of range.
pub(crate) fn record_get(flow: &mut Flow, offset: u16) -> Result<(), RuntimeError> {
    let record = flow.pop_value()?;
    let Some((_, fields)) = record.as_record() else {
        return Err(RuntimeError::NotARecord(type_name(&record)));
    };
    let Some(value) = fields.get(offset as usize).cloned() else {
        return Err(RuntimeError::RecordFieldOffsetOutOfRange {
            offset,
            len: fields.len(),
        });
    };
    flow.value_stack.push(value);
    Ok(())
}

/// `RecordSet(offset)`: `[record, value]` → updated record (take →
/// `make_mut` → write-back — identical COW discipline to
/// [`record_set_dyn`]), field selected by flat offset. Turn-terminating
/// fault if the popped container isn't a `Record`, or the offset is out of
/// range.
pub(crate) fn record_set(flow: &mut Flow, offset: u16) -> Result<(), RuntimeError> {
    let value = flow.pop_value()?;
    let mut record = flow.pop_value()?;
    let len = record
        .as_record()
        .map(|(_, fields)| fields.len())
        .ok_or_else(|| RuntimeError::NotARecord(type_name(&record)))?;
    if offset as usize >= len {
        return Err(RuntimeError::RecordFieldOffsetOutOfRange { offset, len });
    }
    note_record_mutation(&record);
    let Some(fields) = record.record_make_mut() else {
        return Err(RuntimeError::NotARecord(type_name(&record)));
    };
    #[expect(
        clippy::indexing_slicing,
        reason = "offset validated against len above"
    )]
    {
        fields[offset as usize] = value;
    }
    flow.value_stack.push(record);
    Ok(())
}

// ── Shared helpers ───────────────────────────────────────────────────────

/// Record a COW-copy event if mutating `record` via the next
/// `record_make_mut()` call will find a shared `Arc` and pay the O(n) copy
/// — see `collection_ops::note_array_mutation` for the full rationale
/// (issue #821 Workstream B seed). No-op unless the `bench-counters`
/// feature is enabled.
#[cfg(feature = "bench-counters")]
#[inline]
fn note_record_mutation(record: &Value) {
    if let Value::Record { fields, .. } = record
        && alloc::sync::Arc::strong_count(fields) > 1
    {
        crate::bench_counters::record_cow_copy();
    }
}
#[cfg(not(feature = "bench-counters"))]
#[inline(always)]
fn note_record_mutation(_record: &Value) {}

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
        Value::Record { .. } => "record",
        Value::FnRef(_) | Value::Closure(_) => "fn",
        Value::Handle { .. } => "handle",
        Value::Projection(_) => "projection",
        Value::OptionVal(_) => "option",
        Value::Range { .. } => "range",
    }
}

/// Resolve `name` to its flat-field index within `record`'s own shape.
/// Fault if `record` isn't a `Record`, or its shape has no such field.
fn field_index(program: &Program, record: &Value, name: NameId) -> Result<usize, RuntimeError> {
    let Some((shape, _)) = record.as_record() else {
        return Err(RuntimeError::NotARecord(type_name(record)));
    };
    let entry = program
        .struct_shape(shape)
        .ok_or(RuntimeError::InvalidShapeId(shape.0))?;
    entry.fields.iter().position(|&f| f == name).ok_or_else(|| {
        // `name` is a caller-provided `NameId` operand — safe-guard the
        // lookup rather than calling `Program::name` (which indexes
        // unconditionally) so a malformed/out-of-range id still produces a
        // clean `RuntimeError`, not a panic.
        let display = program
            .name_table
            .get(name.0 as usize)
            .cloned()
            .unwrap_or_else(|| format!("<name#{}>", name.0));
        RuntimeError::RecordFieldNotFound(display)
    })
}

fn read_field<'a>(
    program: &Program,
    record: &'a Value,
    name: NameId,
) -> Result<&'a Value, RuntimeError> {
    let idx = field_index(program, record, name)?;
    let Some((_, fields)) = record.as_record() else {
        return Err(RuntimeError::NotARecord(type_name(record)));
    };
    #[expect(
        clippy::indexing_slicing,
        reason = "index validated by field_index above"
    )]
    Ok(&fields[idx])
}

// ── Tests ────────────────────────────────────────────────────────────────
//
// Same "op function, not full VM" granularity `collection_ops` uses — these
// exercise the primitives directly against hand-built `Program`/`Value`
// trees, one level below full bytecode.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::program::{LinkedContainer, StructShapeEntry};
    use brink_format::{CountingFlags, DefinitionId, DefinitionTag};
    use std::collections::HashMap;

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

    /// A minimal `Program` with a name table (`"x"` = 0, `"y"` = 1) and one
    /// declared struct shape `Point { x, y }` at `ShapeId(0)`.
    fn point_program() -> Program {
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
            globals: vec![],
            global_map: HashMap::new(),
            name_table: vec!["x".to_string(), "y".to_string()],
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
                fields: vec![NameId(0), NameId(1)],
            }],
            private_defs: Vec::new(),
            alias_table: Vec::new(),
        }
    }

    fn push_args(flow: &mut Flow, args: Vec<Value>) {
        for v in args {
            flow.value_stack.push(v);
        }
    }

    #[test]
    fn record_new_constructs_in_shape_order() {
        let program = point_program();
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Float(1.0), Value::Float(2.0)]);
        record_new(&mut flow, &program, 0).unwrap();
        let result = flow.pop_value().unwrap();
        let (shape, fields) = result.as_record().unwrap();
        assert_eq!(shape, ShapeId(0));
        assert_eq!(fields.as_slice(), &[Value::Float(1.0), Value::Float(2.0)]);
    }

    #[test]
    fn record_new_unknown_shape_faults() {
        let program = point_program();
        let mut flow = test_flow();
        let err = record_new(&mut flow, &program, 99).unwrap_err();
        assert_eq!(err, RuntimeError::InvalidShapeId(99));
    }

    #[test]
    fn record_get_dyn_reads_field_by_name() {
        let program = point_program();
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record]);
        record_get_dyn(&mut flow, &program, 1).unwrap(); // "y"
        assert_eq!(flow.pop_value().unwrap(), Value::Float(2.0));
    }

    #[test]
    fn record_get_dyn_missing_field_faults() {
        let program = point_program();
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record]);
        let err = record_get_dyn(&mut flow, &program, 5).unwrap_err();
        assert!(matches!(err, RuntimeError::RecordFieldNotFound(_)));
    }

    #[test]
    fn record_get_dyn_non_record_faults() {
        let program = point_program();
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Int(1)]);
        let err = record_get_dyn(&mut flow, &program, 0).unwrap_err();
        assert_eq!(err, RuntimeError::NotARecord("int"));
    }

    #[test]
    fn record_set_dyn_writes_field_by_name() {
        let program = point_program();
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record, Value::Float(9.0)]);
        record_set_dyn(&mut flow, &program, 1).unwrap(); // "y"
        let result = flow.pop_value().unwrap();
        let (_, fields) = result.as_record().unwrap();
        assert_eq!(fields.as_slice(), &[Value::Float(1.0), Value::Float(9.0)]);
    }

    #[test]
    fn record_set_dyn_missing_field_faults() {
        let program = point_program();
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record, Value::Float(9.0)]);
        let err = record_set_dyn(&mut flow, &program, 7).unwrap_err();
        assert!(matches!(err, RuntimeError::RecordFieldNotFound(_)));
    }

    // ── Static-offset ops (TM-4c) ───────────────────────────────────────────

    #[test]
    fn record_get_reads_field_by_offset() {
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record]);
        record_get(&mut flow, 1).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Float(2.0));
    }

    #[test]
    fn record_get_out_of_range_offset_faults() {
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record]);
        let err = record_get(&mut flow, 5).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::RecordFieldOffsetOutOfRange { offset: 5, len: 2 }
        );
    }

    #[test]
    fn record_get_non_record_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Int(1)]);
        let err = record_get(&mut flow, 0).unwrap_err();
        assert_eq!(err, RuntimeError::NotARecord("int"));
    }

    #[test]
    fn record_set_writes_field_by_offset() {
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record, Value::Float(9.0)]);
        record_set(&mut flow, 1).unwrap();
        let result = flow.pop_value().unwrap();
        let (_, fields) = result.as_record().unwrap();
        assert_eq!(fields.as_slice(), &[Value::Float(1.0), Value::Float(9.0)]);
    }

    #[test]
    fn record_set_out_of_range_offset_faults() {
        let mut flow = test_flow();
        let record = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        push_args(&mut flow, vec![record, Value::Float(9.0)]);
        let err = record_set(&mut flow, 7).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::RecordFieldOffsetOutOfRange { offset: 7, len: 2 }
        );
    }

    #[test]
    fn record_set_non_record_faults() {
        let mut flow = test_flow();
        push_args(&mut flow, vec![Value::Int(1), Value::Float(9.0)]);
        let err = record_set(&mut flow, 0).unwrap_err();
        assert_eq!(err, RuntimeError::NotARecord("int"));
    }

    #[test]
    fn record_set_cows_when_shared() {
        let original = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        push_args(&mut flow, vec![original, Value::Float(9.0)]);
        record_set(&mut flow, 0).unwrap();
        let mutated = flow.pop_value().unwrap();
        assert_eq!(
            snapshot,
            Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]),
            "snapshot unmutated"
        );
        assert_eq!(
            mutated,
            Value::record(ShapeId(0), vec![Value::Float(9.0), Value::Float(2.0)])
        );
    }

    // ── COW / sharing law ─────────────────────────────────────────────────

    #[test]
    fn record_set_dyn_cows_when_shared() {
        let program = point_program();
        let original = Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]);
        let snapshot = original.clone();
        let mut flow = test_flow();
        push_args(&mut flow, vec![original, Value::Float(9.0)]);
        record_set_dyn(&mut flow, &program, 0).unwrap(); // "x"
        let mutated = flow.pop_value().unwrap();
        assert_eq!(
            snapshot,
            Value::record(ShapeId(0), vec![Value::Float(1.0), Value::Float(2.0)]),
            "snapshot unmutated"
        );
        assert_eq!(
            mutated,
            Value::record(ShapeId(0), vec![Value::Float(9.0), Value::Float(2.0)])
        );
    }

    #[test]
    fn record_equality_requires_matching_shape() {
        let a = Value::record(ShapeId(0), vec![Value::Int(1)]);
        let b = Value::record(ShapeId(1), vec![Value::Int(1)]);
        assert_ne!(a, b, "same fields, different shape must not be equal");
        let c = Value::record(ShapeId(0), vec![Value::Int(1)]);
        assert_eq!(a, c);
    }
}
