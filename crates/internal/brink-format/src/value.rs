use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::id::DefinitionId;

/// The runtime type of a [`Value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
    List,
    DivertTarget,
    VariablePointer,
    TempPointer,
    Null,
    FragmentRef,
}

/// A runtime value in the ink VM.
///
/// Heap-allocating variants (`String`, `List`) are wrapped in `Arc` so that
/// cloning a `Value` is always O(1) — a refcount bump, not a deep copy.
/// This matches C#'s reference-type semantics and makes call-frame cloning
/// (during `fork_thread`) essentially free. Atomic refcounts are used so
/// `Value` can flow through Bevy's parallel scheduler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(Arc<str>),
    List(Arc<ListValue>),
    DivertTarget(DefinitionId),
    /// A reference to a global variable, used for `ref` parameters.
    VariablePointer(DefinitionId),
    /// A runtime-only pointer to a temp in a specific call frame.
    /// Used for `ref` parameters that target temp variables.
    TempPointer {
        slot: u16,
        frame_depth: u16,
    },
    Null,
    /// A reference to a fragment in the output buffer's fragment store.
    /// Fragments preserve structural output parts for locale re-rendering.
    FragmentRef(u32),
}

impl Value {
    /// Return the type discriminant for this value.
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Int(_) => ValueType::Int,
            Self::Float(_) => ValueType::Float,
            Self::Bool(_) => ValueType::Bool,
            Self::String(_) => ValueType::String,
            Self::List(_) => ValueType::List,
            Self::DivertTarget(_) => ValueType::DivertTarget,
            Self::VariablePointer(_) => ValueType::VariablePointer,
            Self::TempPointer { .. } => ValueType::TempPointer,
            Self::Null => ValueType::Null,
            Self::FragmentRef(_) => ValueType::FragmentRef,
        }
    }

    /// Extract an `i32` if this value is an [`Int`](Self::Int).
    ///
    /// Strict: does not coerce floats or booleans. Returns `None` for any
    /// other variant. For binding authors that want to read an integer
    /// argument from ink.
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Extract an `f32` if this value is numeric.
    ///
    /// Lenient on the int → float direction only: an [`Int`](Self::Int) is
    /// widened to `f32` (matching ink's implicit int→float promotion), but a
    /// float is never truncated to an int by [`as_int`](Self::as_int).
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(f) => Some(*f),
            #[expect(
                clippy::cast_precision_loss,
                reason = "int->float promotion matches ink coercion semantics"
            )]
            Self::Int(i) => Some(*i as f32),
            _ => None,
        }
    }

    /// Extract a `bool` if this value is a [`Bool`](Self::Bool).
    ///
    /// Strict: does not treat nonzero numbers as truthy. Use the VM's own
    /// truthiness rules if you need ink-style coercion.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Borrow the string contents if this value is a [`String`](Self::String).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Int(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Self::Float(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(Arc::from(v))
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(Arc::from(v))
    }
}

impl From<Arc<str>> for Value {
    fn from(v: Arc<str>) -> Self {
        Self::String(v)
    }
}

impl From<()> for Value {
    /// The unit type maps to [`Null`](Self::Null) — the natural return for a
    /// fire-and-forget external that produces no value.
    fn from((): ()) -> Self {
        Self::Null
    }
}

/// An ink list value: a set of list items plus their origin list definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListValue {
    /// The active items in this list (each a `ListItem` `DefinitionId`).
    pub items: Vec<DefinitionId>,
    /// The origin list definitions this value was derived from.
    pub origins: Vec<DefinitionId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DefinitionTag;

    #[test]
    fn value_type_discriminant() {
        assert_eq!(Value::Int(0).value_type(), ValueType::Int);
        assert_eq!(Value::Float(0.0).value_type(), ValueType::Float);
        assert_eq!(Value::Bool(true).value_type(), ValueType::Bool);
        assert_eq!(Value::String("".into()).value_type(), ValueType::String);
        assert_eq!(Value::Null.value_type(), ValueType::Null);

        let list = ListValue {
            items: vec![],
            origins: vec![],
        };
        assert_eq!(Value::List(list.into()).value_type(), ValueType::List);

        let target = DefinitionId::new(DefinitionTag::Address, 1);
        assert_eq!(
            Value::DivertTarget(target).value_type(),
            ValueType::DivertTarget
        );
    }

    #[test]
    fn from_impls_roundtrip() {
        assert_eq!(Value::from(7_i32), Value::Int(7));
        assert_eq!(Value::from(1.5_f32), Value::Float(1.5));
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from("hi"), Value::String("hi".into()));
        assert_eq!(Value::from(String::from("hi")), Value::String("hi".into()));
        assert_eq!(Value::from(()), Value::Null);
    }

    #[test]
    fn accessors_are_strict_except_int_to_float() {
        assert_eq!(Value::Int(3).as_int(), Some(3));
        assert_eq!(Value::Float(3.0).as_int(), None);
        assert_eq!(Value::Bool(true).as_int(), None);

        // int->float promotion is allowed (matches ink coercion);
        // float->int truncation is not.
        assert_eq!(Value::Int(3).as_float(), Some(3.0));
        assert_eq!(Value::Float(2.5).as_float(), Some(2.5));

        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Int(1).as_bool(), None);

        assert_eq!(Value::String("x".into()).as_str(), Some("x"));
        assert_eq!(Value::Int(1).as_str(), None);
    }
}
