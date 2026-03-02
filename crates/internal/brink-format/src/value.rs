use crate::id::DefinitionId;

/// The runtime type of a [`Value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
    List,
    DivertTarget,
    Null,
}

/// A runtime value in the ink VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    List(ListValue),
    DivertTarget(DefinitionId),
    Null,
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
            Self::Null => ValueType::Null,
        }
    }
}

/// An ink list value: a set of list items plus their origin list definitions.
#[derive(Debug, Clone, PartialEq)]
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
        assert_eq!(Value::String(String::new()).value_type(), ValueType::String);
        assert_eq!(Value::Null.value_type(), ValueType::Null);

        let list = ListValue {
            items: vec![],
            origins: vec![],
        };
        assert_eq!(Value::List(list).value_type(), ValueType::List);

        let target = DefinitionId::new(DefinitionTag::Container, 1);
        assert_eq!(
            Value::DivertTarget(target).value_type(),
            ValueType::DivertTarget
        );
    }
}
