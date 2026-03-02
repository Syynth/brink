//! Arithmetic, comparison, coercion, truthiness, and stringify for [`Value`].

use brink_format::Value;

use crate::error::RuntimeError;

/// Returns whether a value is truthy in ink semantics.
pub(crate) fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(n) => *n != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        Value::DivertTarget(_) | Value::List(_) => true,
    }
}

/// Stringify a value for output.
pub(crate) fn stringify(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(n) => format!("{n}"),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::String(s) => s.clone(),
        Value::Null | Value::List(_) => String::new(),
        Value::DivertTarget(id) => format!("{id}"),
    }
}

/// Binary arithmetic/comparison operation.
pub(crate) fn binary_op(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, RuntimeError> {
    // Coerce types: if both are numeric, promote to float if either is float.
    match (left, right) {
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
        let r = binary_op(BinaryOp::Add, &Value::Int(2), &Value::Int(3)).unwrap();
        assert_eq!(r, Value::Int(5));
    }

    #[test]
    fn int_float_promotion() {
        let r = binary_op(BinaryOp::Add, &Value::Int(2), &Value::Float(1.5)).unwrap();
        assert_eq!(r, Value::Float(3.5));
    }

    #[test]
    fn string_concat() {
        let r = binary_op(
            BinaryOp::Add,
            &Value::String("a".into()),
            &Value::String("b".into()),
        )
        .unwrap();
        assert_eq!(r, Value::String("ab".into()));
    }

    #[test]
    fn stringify_values() {
        assert_eq!(stringify(&Value::Int(42)), "42");
        assert_eq!(stringify(&Value::Bool(true)), "true");
        assert_eq!(stringify(&Value::Null), "");
    }
}
