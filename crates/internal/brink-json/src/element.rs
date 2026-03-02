use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{
    ChoicePoint, Container, ControlCommand, Divert, InkValue, NativeFunction, ReadCountReference,
    VariableAssignment, VariableReference,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    Void,
    Container(Container),
    Value(InkValue),
    Divert(Divert),
    ControlCommand(ControlCommand),
    NativeFunction(NativeFunction),
    VariableAssignment(VariableAssignment),
    VariableReference(VariableReference),
    ReadCount(ReadCountReference),
    ChoicePoint(ChoicePoint),
}

impl Serialize for Element {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Element::Void => serializer.serialize_str("void"),
            Element::Container(c) => c.serialize(serializer),
            Element::Value(v) => v.serialize(serializer),
            Element::Divert(d) => d.serialize(serializer),
            Element::ControlCommand(cmd) => cmd.serialize(serializer),
            Element::NativeFunction(func) => func.serialize(serializer),
            Element::VariableAssignment(a) => a.serialize(serializer),
            Element::VariableReference(r) => r.serialize(serializer),
            Element::ReadCount(r) => r.serialize(serializer),
            Element::ChoicePoint(c) => c.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Element {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        // Handle arrays (containers)
        if let Some(arr) = value.as_array() {
            let container: Container = serde_json::from_value(Value::Array(arr.clone()))
                .map_err(serde::de::Error::custom)?;
            return Ok(Element::Container(container));
        }

        // Handle strings
        if let Some(s) = value.as_str() {
            // Check for control commands first
            if let Ok(cmd) = s.parse::<ControlCommand>() {
                return Ok(Element::ControlCommand(cmd));
            }

            // Check for native functions
            if let Ok(func) = s.parse::<NativeFunction>() {
                return Ok(Element::NativeFunction(func));
            }

            // Check for "void"
            if s == "void" {
                return Ok(Element::Void);
            }

            // Check for string values (starting with ^)
            if let Some(text) = s.strip_prefix('^') {
                return Ok(Element::Value(InkValue::String(text.to_string())));
            }

            // Handle special case: newline
            if s == "\n" {
                return Ok(Element::Value(InkValue::String("\n".to_string())));
            }

            return Err(serde::de::Error::custom(format!(
                "Unknown string element: {s}"
            )));
        }

        // Handle numbers
        if let Some(i) = value.as_i64() {
            return Ok(Element::Value(InkValue::Integer(i)));
        }
        if let Some(f) = value.as_f64() {
            return Ok(Element::Value(InkValue::Float(f)));
        }

        // Handle objects
        if let Some(obj) = value.as_object() {
            // Check for divert target value {"^->": "path"}
            if let Some(target) = obj.get("^->")
                && let Some(path) = target.as_str()
            {
                return Ok(Element::Value(InkValue::DivertTarget(path.to_string())));
            }

            // Check for variable pointer value {"^var": "varname", "ci": 0}
            if let Some(varname) = obj.get("^var")
                && let Some(name) = varname.as_str()
            {
                return Ok(Element::Value(InkValue::VariablePointer(name.to_string())));
            }

            // Check for diverts
            if obj.contains_key("->") {
                let divert: Divert =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::Divert(divert));
            }

            if obj.contains_key("f()") {
                let divert: Divert =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::Divert(divert));
            }

            if obj.contains_key("->t->") {
                let divert: Divert =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::Divert(divert));
            }

            if obj.contains_key("x()") {
                let divert: Divert =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::Divert(divert));
            }

            // Check for variable assignment
            if obj.contains_key("VAR=") || obj.contains_key("temp=") {
                let assignment: VariableAssignment =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::VariableAssignment(assignment));
            }

            // Check for variable reference
            if obj.contains_key("VAR?") {
                let reference: VariableReference =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::VariableReference(reference));
            }

            // Check for read count
            if obj.contains_key("CNT?") {
                let read_count: ReadCountReference =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::ReadCount(read_count));
            }

            // Check for choice point
            if obj.contains_key("*") {
                let choice_point: ChoicePoint =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                return Ok(Element::ChoicePoint(choice_point));
            }

            return Err(serde::de::Error::custom(format!(
                "Unknown object element: {obj:?}"
            )));
        }

        Err(serde::de::Error::custom(format!(
            "Unknown element type: {value:?}"
        )))
    }
}
