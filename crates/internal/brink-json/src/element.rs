use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{
    ChoicePoint, Container, ControlCommand, Divert, InkList, InkValue, NativeFunction,
    ReadCountReference, VariableAssignment, VariableReference,
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
    /// Placeholder inserted by the preprocessing pass to blank out elements
    /// (e.g. $r ceremony) without changing array indices. Never produced by
    /// JSON parsing. Codegen emits nothing for this variant.
    Nop,
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
            Element::Nop => serializer.serialize_str("nop"),
        }
    }
}

impl<'de> Deserialize<'de> for Element {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        deserialize_value(value).map_err(serde::de::Error::custom)
    }
}

fn deserialize_value(value: Value) -> Result<Element, String> {
    // Handle arrays (containers)
    if value.is_array() {
        let container: Container = serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Element::Container(container));
    }

    // Handle strings
    if let Some(s) = value.as_str() {
        return deserialize_string(s);
    }

    // Handle booleans (used in global variable declarations)
    if let Some(b) = value.as_bool() {
        return Ok(Element::Value(InkValue::Bool(b)));
    }

    // Handle numbers
    if let Some(i) = value.as_i64() {
        return Ok(Element::Value(InkValue::Integer(i)));
    }
    if let Some(f) = value.as_f64() {
        return Ok(Element::Value(InkValue::Float(f)));
    }

    // Handle objects
    if value.is_object() {
        return deserialize_object(value);
    }

    Err(format!("Unknown element type: {value:?}"))
}

fn deserialize_string(s: &str) -> Result<Element, String> {
    if let Ok(cmd) = s.parse::<ControlCommand>() {
        return Ok(Element::ControlCommand(cmd));
    }
    if let Ok(func) = s.parse::<NativeFunction>() {
        return Ok(Element::NativeFunction(func));
    }
    if s == "void" {
        return Ok(Element::Void);
    }
    if let Some(text) = s.strip_prefix('^') {
        return Ok(Element::Value(InkValue::String(text.to_string())));
    }
    if s == "\n" {
        return Ok(Element::Value(InkValue::String("\n".to_string())));
    }
    Err(format!("Unknown string element: {s}"))
}

fn deserialize_object(value: Value) -> Result<Element, String> {
    let obj = value.as_object().ok_or("expected object")?;

    // Divert target value {"^->": "path"}
    if let Some(target) = obj.get("^->").and_then(Value::as_str) {
        return Ok(Element::Value(InkValue::DivertTarget(target.to_string())));
    }

    // Variable pointer value {"^var": "varname", "ci": 0}
    if let Some(name) = obj.get("^var").and_then(Value::as_str) {
        return Ok(Element::Value(InkValue::VariablePointer(name.to_string())));
    }

    // Diverts
    if obj.contains_key("->")
        || obj.contains_key("f()")
        || obj.contains_key("->t->")
        || obj.contains_key("x()")
    {
        let divert: Divert = serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Element::Divert(divert));
    }

    // Variable assignment
    if obj.contains_key("VAR=") || obj.contains_key("temp=") {
        let assignment: VariableAssignment =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Element::VariableAssignment(assignment));
    }

    // Variable reference
    if obj.contains_key("VAR?") {
        let reference: VariableReference =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Element::VariableReference(reference));
    }

    // Read count
    if obj.contains_key("CNT?") {
        let read_count: ReadCountReference =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Element::ReadCount(read_count));
    }

    // Choice point
    if obj.contains_key("*") {
        let choice_point: ChoicePoint = serde_json::from_value(value).map_err(|e| e.to_string())?;
        return Ok(Element::ChoicePoint(choice_point));
    }

    // List value {"list": {...}, "origins": [...]}
    if obj.contains_key("list") {
        let mut items = std::collections::HashMap::new();
        if let Some(list_obj) = obj.get("list").and_then(Value::as_object) {
            for (k, v) in list_obj {
                if let Some(n) = v.as_i64() {
                    items.insert(k.clone(), n);
                }
            }
        }
        let origins = obj
            .get("origins")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Element::Value(InkValue::List(InkList { items, origins })));
    }

    Err(format!("Unknown object element: {obj:?}"))
}
