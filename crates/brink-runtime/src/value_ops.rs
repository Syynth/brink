//! Arithmetic, comparison, coercion, truthiness, and stringify for [`Value`].

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
        Value::DivertTarget(_) | Value::VariablePointer(_) => true,
        Value::List(lv) => !lv.items.is_empty(),
    }
}

/// Stringify a value for output.
pub(crate) fn stringify(v: &Value, program: &Program) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(n) => format!("{n}"),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::List(lv) => stringify_list(lv, program),
        Value::DivertTarget(id) | Value::VariablePointer(id) => format!("{id}"),
    }
}

/// Stringify a list value: sort items by (ordinal, origin name), join names with ", ".
fn stringify_list(lv: &ListValue, program: &Program) -> String {
    let mut entries: Vec<(i32, &str, &str)> = lv
        .items
        .iter()
        .filter_map(|&id| {
            program.list_item(id).map(|entry| {
                let origin_name = program
                    .list_def(entry.origin)
                    .map_or("", |def| program.name(def.name));
                (entry.ordinal, origin_name, program.name(entry.name))
            })
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    let names: Vec<&str> = entries.iter().map(|&(_, _, name)| name).collect();
    names.join(", ")
}

/// Binary arithmetic/comparison operation.
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
            Ok(Value::List(list_ordinal_shift(a, shift, program)))
        }
        (Value::Int(a), Value::Int(b)) => int_op(op, *a, *b),
        (Value::Float(a), Value::Float(b)) => Ok(float_op(op, *a, *b)),
        #[expect(clippy::cast_precision_loss)]
        (Value::Int(a), Value::Float(b)) => Ok(float_op(op, *a as f32, *b)),
        #[expect(clippy::cast_precision_loss)]
        (Value::Float(a), Value::Int(b)) => Ok(float_op(op, *a, *b as f32)),
        (Value::String(a), Value::String(b)) => string_op(op, a, b),
        // Int + String coercion: stringify the int
        (Value::String(a), Value::Int(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}")))
        }
        (Value::Int(a), Value::String(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}")))
        }
        // Float + String coercion
        (Value::String(a), Value::Float(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}")))
        }
        (Value::Float(a), Value::String(b)) if op == BinaryOp::Add => {
            Ok(Value::String(format!("{a}{b}")))
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
        (Value::Bool(a), Value::Float(b)) => Ok(float_op(op, if *a { 1.0 } else { 0.0 }, *b)),
        (Value::Float(a), Value::Bool(b)) => Ok(float_op(op, *a, if *b { 1.0 } else { 0.0 })),
        // DivertTarget equality
        (Value::DivertTarget(a), Value::DivertTarget(b)) if op == BinaryOp::Equal => {
            Ok(Value::Bool(a == b))
        }
        (Value::DivertTarget(a), Value::DivertTarget(b)) if op == BinaryOp::NotEqual => {
            Ok(Value::Bool(a != b))
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

/// Binary operations on two list values.
fn list_binary_op(
    op: BinaryOp,
    a: &ListValue,
    b: &ListValue,
    _program: &Program,
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
            Ok(Value::List(ListValue { items, origins }))
        }
        BinaryOp::Subtract => {
            // Except (a \ b)
            let items: Vec<_> = a
                .items
                .iter()
                .filter(|id| !b.items.contains(id))
                .copied()
                .collect();
            Ok(Value::List(ListValue {
                items,
                origins: a.origins.clone(),
            }))
        }
        BinaryOp::Equal => {
            // Same item set (order-independent)
            let eq =
                a.items.len() == b.items.len() && a.items.iter().all(|id| b.items.contains(id));
            Ok(Value::Bool(eq))
        }
        BinaryOp::NotEqual => {
            let eq =
                a.items.len() == b.items.len() && a.items.iter().all(|id| b.items.contains(id));
            Ok(Value::Bool(!eq))
        }
        BinaryOp::Greater => {
            // Strict superset: a contains all of b, and a has more items.
            let superset =
                b.items.iter().all(|id| a.items.contains(id)) && a.items.len() > b.items.len();
            Ok(Value::Bool(superset))
        }
        BinaryOp::GreaterOrEqual => {
            // Superset: a contains all of b.
            let superset = b.items.iter().all(|id| a.items.contains(id));
            Ok(Value::Bool(superset))
        }
        BinaryOp::Less => {
            // Strict subset: b contains all of a, and b has more items.
            let subset =
                a.items.iter().all(|id| b.items.contains(id)) && b.items.len() > a.items.len();
            Ok(Value::Bool(subset))
        }
        BinaryOp::LessOrEqual => {
            // Subset: b contains all of a.
            let subset = a.items.iter().all(|id| b.items.contains(id));
            Ok(Value::Bool(subset))
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
        #[expect(clippy::cast_precision_loss)]
        BinaryOp::Pow => float_op(op, a as f32, b as f32),
    })
}

fn float_op(op: BinaryOp, a: f32, b: f32) -> Value {
    match op {
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
        BinaryOp::Pow => Value::Float(a.powf(b)),
    }
}

fn string_op(op: BinaryOp, a: &str, b: &str) -> Result<Value, RuntimeError> {
    Ok(match op {
        BinaryOp::Add => Value::String(format!("{a}{b}")),
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
    use crate::program::LinkedContainer;
    use std::collections::HashMap;

    fn dummy_program() -> Program {
        use brink_format::{DefinitionId, DefinitionTag};
        Program {
            containers: vec![LinkedContainer {
                id: DefinitionId::new(DefinitionTag::Container, 0),
                bytecode: vec![],
                counting_flags: brink_format::CountingFlags::empty(),
                path_hash: 0,
            }],
            container_map: {
                let mut m = HashMap::new();
                m.insert(DefinitionId::new(DefinitionTag::Container, 0), 0);
                m
            },
            label_map: HashMap::new(),
            line_tables: vec![vec![]],
            globals: vec![],
            global_map: HashMap::new(),
            name_table: vec![],
            root_idx: 0,
            list_literals: vec![],
            list_item_map: HashMap::new(),
            list_defs: vec![],
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
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
        assert!(!is_truthy(&Value::String(String::new())));
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
}
