//! TM-3 completion: `int(x)`/`float(x)`/`string(x)` pure conversion
//! intrinsic opcode implementations (`docs/typed-mode-spec.md` §4,
//! maintainer ruling 2026-07-13, issue #659; fault semantics from
//! `docs/value-model-spec.md` §11c).
//!
//! Domains (ruling 2, "permissive numerics + bool"):
//! - `int(x)`: `Int` (identity), `Float` (truncate toward zero — ruling 3,
//!   matches vanilla ink's `INT()` exactly), `Bool` (`true` → 1, `false` →
//!   0), `String` (parse).
//! - `float(x)`: `Float` (identity), `Int` (widen), `Bool` (`true` → 1.0,
//!   `false` → 0.0), `String` (parse).
//! - `string(x)`: every value, display form — reuses [`value_ops::stringify`]
//!   directly, the exact function interpolation (`{x}`) already calls
//!   (typed-mode-spec §4: "display is universal, not a coercion"). Total —
//!   never faults.
//!
//! `int`/`float` are turn-terminating faults (never zero-defaulting) on a
//! string that fails to parse, or on any value outside the domain above
//! (divert targets, LIST values, arrays, maps, records) — ruling 1/2. This
//! is a **new, distinct** fault-carrying path from the pre-existing
//! uppercase `INT()`/`FLOAT()` builtins (`value_ops::cast_to_int`/
//! `cast_to_float`): those keep their legacy silent-0-on-string-parse-failure
//! behavior untouched within their own reachable `Int`/`Float`/`Bool`/
//! `String` domain, since that behavior is oracle-anchored
//! (byte-identical requirement). Outside that domain, `cast_to_int`/
//! `cast_to_float` used to silently fold every other `Value` variant to
//! `0`/`0.0` through a wildcard arm; issue #955 closed that hazard by
//! making them fault too (`RuntimeError::InvalidConversionDomain`, same
//! variant this module raises, distinguished by an uppercase `target`
//! label) — those variants were never oracle-reachable, so the ratchet is
//! unaffected.

use alloc::string::ToString;

use brink_format::Value;

use crate::error::RuntimeError;
use crate::program::Program;
use crate::story::Flow;
use crate::value_ops;

/// `ConvertInt`: `[x]` → `Int`. See module doc for domain/fault rules.
pub(crate) fn convert_to_int(flow: &mut Flow) -> Result<(), RuntimeError> {
    let val = flow.pop_value()?;
    let result =
        match &val {
            Value::Int(_) => val,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "`as i32` on f32 truncates toward zero and saturates on \
                       overflow/NaN — exactly ruling 3's semantics, no wrap-around UB"
            )]
            Value::Float(f) => Value::Int(*f as i32),
            Value::Bool(b) => Value::Int(i32::from(*b)),
            Value::String(s) => s.parse::<i32>().map(Value::Int).map_err(|_| {
                RuntimeError::ConversionParseFailure {
                    target: "int",
                    input: s.to_string(),
                }
            })?,
            other => {
                return Err(RuntimeError::InvalidConversionDomain {
                    target: "int",
                    got: type_name(other),
                });
            }
        };
    flow.value_stack.push(result);
    Ok(())
}

/// `ConvertFloat`: `[x]` → `Float`. See module doc for domain/fault rules.
pub(crate) fn convert_to_float(flow: &mut Flow) -> Result<(), RuntimeError> {
    let val = flow.pop_value()?;
    let result = match &val {
        Value::Float(_) => val,
        #[expect(
            clippy::cast_precision_loss,
            reason = "int->float widening matches ink's own implicit coercion; \
                       i32 -> f32 precision loss is the same accepted tradeoff \
                       `value_ops::cast_to_float` already makes"
        )]
        Value::Int(n) => Value::Float(*n as f32),
        Value::Bool(b) => Value::Float(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.parse::<f32>().map(Value::Float).map_err(|_| {
            RuntimeError::ConversionParseFailure {
                target: "float",
                input: s.to_string(),
            }
        })?,
        other => {
            return Err(RuntimeError::InvalidConversionDomain {
                target: "float",
                got: type_name(other),
            });
        }
    };
    flow.value_stack.push(result);
    Ok(())
}

/// `ConvertString`: `[x]` → `String`. Total — every `Value` variant has a
/// display form (`value_ops::stringify` is already total, the same function
/// interpolation uses), so this never faults.
pub(crate) fn convert_to_string(flow: &mut Flow, program: &Program) -> Result<(), RuntimeError> {
    let val = flow.pop_value()?;
    let text = value_ops::stringify(&val, program);
    flow.value_stack.push(Value::String(text.into()));
    Ok(())
}

/// Type-name label for [`RuntimeError::InvalidConversionDomain`] — mirrors
/// `collection_ops`'/`record_ops`' own small hand-duplicated `type_name`
/// helpers (no shared export exists for this purpose across the ops
/// modules).
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
        // NS-A1: an Option is outside `int`/`float`'s numeric+bool domain —
        // the ruled `Option[T] ≠ T` strictness means no implicit unwrap,
        // even of a `some(3)`. (`string(x)` stays total via `stringify`.)
        Value::OptionVal(_) => "option",
        Value::Range { .. } => "range",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Vec4(_) => "vec4",
        Value::Quat(_) => "quat",
        Value::Mat2(_) => "mat2",
        Value::Mat3(_) => "mat3",
        Value::Mat4(_) => "mat4",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputBuffer;
    use crate::program::LinkedContainer;
    use brink_format::{CountingFlags, DefinitionId, DefinitionTag, ListValue, OrderedMap};
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

    fn empty_program() -> Program {
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
            struct_shapes: vec![],
            private_defs: Vec::new(),
            alias_table: Vec::new(),
        }
    }

    fn push(flow: &mut Flow, v: Value) {
        flow.value_stack.push(v);
    }

    // ── int() ────────────────────────────────────────────────────────────

    #[test]
    fn int_identity() {
        let mut flow = test_flow();
        push(&mut flow, Value::Int(7));
        convert_to_int(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(7));
    }

    #[test]
    fn int_truncates_positive_float_toward_zero() {
        let mut flow = test_flow();
        push(&mut flow, Value::Float(2.9));
        convert_to_int(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(2));
    }

    #[test]
    fn int_truncates_negative_float_toward_zero() {
        // Ruling 3's pinned case: int(-2.9) == -2, not -3 (floor).
        let mut flow = test_flow();
        push(&mut flow, Value::Float(-2.9));
        convert_to_int(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(-2));
    }

    #[test]
    fn int_from_bool() {
        let mut flow = test_flow();
        push(&mut flow, Value::Bool(true));
        convert_to_int(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(1));

        let mut flow = test_flow();
        push(&mut flow, Value::Bool(false));
        convert_to_int(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(0));
    }

    #[test]
    fn int_parses_numeric_string() {
        let mut flow = test_flow();
        push(&mut flow, Value::String("42".into()));
        convert_to_int(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Int(42));
    }

    #[test]
    fn int_parse_failure_faults() {
        let mut flow = test_flow();
        push(&mut flow, Value::String("potato".into()));
        let err = convert_to_int(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::ConversionParseFailure {
                target: "int",
                input: "potato".to_string(),
            }
        );
    }

    #[test]
    fn int_out_of_domain_divert_faults() {
        let mut flow = test_flow();
        push(
            &mut flow,
            Value::DivertTarget(DefinitionId::new(DefinitionTag::Address, 0)),
        );
        let err = convert_to_int(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "int",
                got: "divert_target",
            }
        );
    }

    #[test]
    fn int_out_of_domain_list_faults() {
        let mut flow = test_flow();
        push(
            &mut flow,
            Value::List(
                ListValue {
                    items: vec![],
                    origins: vec![],
                }
                .into(),
            ),
        );
        let err = convert_to_int(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "int",
                got: "list",
            }
        );
    }

    #[test]
    fn int_out_of_domain_array_faults() {
        let mut flow = test_flow();
        push(&mut flow, Value::array(vec![Value::Int(1)]));
        let err = convert_to_int(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "int",
                got: "array",
            }
        );
    }

    #[test]
    fn int_out_of_domain_map_faults() {
        let mut flow = test_flow();
        push(&mut flow, Value::map(OrderedMap::new()));
        let err = convert_to_int(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "int",
                got: "map",
            }
        );
    }

    // ── float() ──────────────────────────────────────────────────────────

    #[test]
    fn float_identity() {
        let mut flow = test_flow();
        push(&mut flow, Value::Float(1.5));
        convert_to_float(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Float(1.5));
    }

    #[test]
    fn float_widens_int() {
        let mut flow = test_flow();
        push(&mut flow, Value::Int(3));
        convert_to_float(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Float(3.0));
    }

    #[test]
    fn float_from_bool() {
        let mut flow = test_flow();
        push(&mut flow, Value::Bool(true));
        convert_to_float(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Float(1.0));

        let mut flow = test_flow();
        push(&mut flow, Value::Bool(false));
        convert_to_float(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Float(0.0));
    }

    #[test]
    fn float_parses_numeric_string() {
        let mut flow = test_flow();
        push(&mut flow, Value::String("2.5".into()));
        convert_to_float(&mut flow).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::Float(2.5));
    }

    #[test]
    fn float_parse_failure_faults() {
        let mut flow = test_flow();
        push(&mut flow, Value::String("nope".into()));
        let err = convert_to_float(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::ConversionParseFailure {
                target: "float",
                input: "nope".to_string(),
            }
        );
    }

    #[test]
    fn float_out_of_domain_struct_faults() {
        let mut flow = test_flow();
        push(
            &mut flow,
            Value::record(brink_format::ShapeId(0), vec![Value::Int(1)]),
        );
        let err = convert_to_float(&mut flow).unwrap_err();
        assert_eq!(
            err,
            RuntimeError::InvalidConversionDomain {
                target: "float",
                got: "record",
            }
        );
    }

    // ── string() ─────────────────────────────────────────────────────────

    #[test]
    fn string_of_int() {
        let program = empty_program();
        let mut flow = test_flow();
        push(&mut flow, Value::Int(42));
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("42".into()));
    }

    #[test]
    fn string_of_record_and_option_use_the_one_display_path() {
        // F1 (ruled 2026-07-19): `string()` and interpolation dispatch
        // through ONE display path (`value_ops::stringify`) — this pins the
        // structural struct default and the Option forms on the `string()`
        // leg specifically (the interpolation leg is pinned in
        // `value_ops`' own tests and the `struct-display-default`
        // tier1-brink case).
        let mut program = empty_program();
        program.name_table = vec!["Point".to_string(), "x".to_string(), "y".to_string()];
        program.struct_shapes = vec![crate::program::StructShapeEntry {
            name: brink_format::NameId(0),
            fields: vec![brink_format::NameId(1), brink_format::NameId(2)],
        }];
        let mut flow = test_flow();
        push(
            &mut flow,
            Value::record(brink_format::ShapeId(0), vec![Value::Int(1), Value::Int(2)]),
        );
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(
            flow.pop_value().unwrap(),
            Value::String("Point { x: 1, y: 2 }".into())
        );
        push(&mut flow, Value::none());
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("none".into()));
    }

    #[test]
    fn string_of_float() {
        let program = empty_program();
        let mut flow = test_flow();
        push(&mut flow, Value::Float(3.25));
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("3.25".into()));
    }

    #[test]
    fn string_of_bool() {
        let program = empty_program();
        let mut flow = test_flow();
        push(&mut flow, Value::Bool(true));
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("true".into()));
    }

    #[test]
    fn string_of_string_is_identity() {
        let program = empty_program();
        let mut flow = test_flow();
        push(&mut flow, Value::String("hi".into()));
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("hi".into()));
    }

    #[test]
    fn string_of_array_never_faults() {
        // Ruling 2: `string()` accepts everything — a collection input is
        // never a fault, unlike `int`/`float`.
        let program = empty_program();
        let mut flow = test_flow();
        push(&mut flow, Value::array(vec![Value::Int(1), Value::Int(2)]));
        convert_to_string(&mut flow, &program).unwrap();
        assert_eq!(flow.pop_value().unwrap(), Value::String("[1, 2]".into()));
    }

    #[test]
    fn string_of_divert_never_faults() {
        let program = empty_program();
        let mut flow = test_flow();
        push(
            &mut flow,
            Value::DivertTarget(DefinitionId::new(DefinitionTag::Address, 3)),
        );
        convert_to_string(&mut flow, &program).unwrap();
        assert!(matches!(flow.pop_value().unwrap(), Value::String(_)));
    }
}
