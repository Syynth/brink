//! Parser for inklecate's `.ink.json` output format.
//!
//! Deserializes the reference ink compiler's JSON output into typed Rust
//! structures for consumption by `brink-converter`.

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

mod choice_point;
mod container;
mod control_command;
mod divert;
mod element;
mod native_function;

pub use choice_point::*;
pub use container::*;
pub use control_command::*;
pub use divert::*;
pub use element::*;
pub use native_function::*;

/// Paths won't ever appear on their own in a Container, but are used by
/// various objects (for example, see Diverts) to reference content
/// within the hierarchy.
///
/// Paths are a dot-separated syntax:
///
/// path.to.target
///
/// Where each element of the path references a sub-object, drilling down into the hierarchy.
///
/// However, paths can have several element types between the dots:
///
///    Names - to reference particular knots, stitches, gathers and named choices. These specify a named content object within a Container.
///    Indices - integers that specify the index of a content object within the ordered array section of a Container.
///    Parent - Denoted with a ^. (Similar to using ".." in a file system.)
///
/// Relative paths lead with a dot rather than starting with a name or index.
///
/// Examples:
///
///    building.entrance.3.0 - the first element of a Container at the fourth element of a Container named entrance within a Container named building of the root Container.
///    .^.1 - the second element of the parent Container.
pub type Path = String;

pub type Variable = String;

/// ## Top Level
/// At the top level of the JSON file are two properties.
/// inkVersion is an integer that denotes the format version,
/// and root, which is the outer-most Container for the entire story.
///
/// Broadly speaking, the entire format is composed of Containers, and
/// individual sub-elements of the Story, within those Containers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InkJson {
    pub ink_version: u32,
    pub root: Container,
}

/// An ink list value: a set of named items, each with an integer value,
/// and optionally the origin list names.
#[derive(Debug, Clone, PartialEq)]
pub struct InkList {
    /// Map of "ListName.ItemName" to integer value
    pub items: std::collections::HashMap<String, i64>,
    /// Origin list names (present when the list is empty to preserve type info)
    pub origins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InkValue {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    DivertTarget(Path),
    VariablePointer(Variable),
    List(InkList),
}

impl Serialize for InkValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            InkValue::String(s) if s == "\n" => serializer.serialize_str("\n"),
            InkValue::String(s) => serializer.serialize_str(&format!("^{s}")),
            InkValue::Integer(i) => serializer.serialize_i64(*i),
            InkValue::Float(f) => serializer.serialize_f64(*f),
            InkValue::Bool(b) => serializer.serialize_bool(*b),
            InkValue::DivertTarget(path) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("^->", path)?;
                map.end()
            }
            InkValue::VariablePointer(var) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("^var", var)?;
                map.serialize_entry("ci", &0)?;
                map.end()
            }
            InkValue::List(list) => {
                let len = 1 + usize::from(!list.origins.is_empty());
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("list", &list.items)?;
                if !list.origins.is_empty() {
                    map.serialize_entry("origins", &list.origins)?;
                }
                map.end()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariableAssignment {
    /// { "VAR=": "money", "re": true }
    /// Pop a value from the evaluation stack, and assign it to the
    /// already-declared global variable money.
    GlobalAssignment { variable: String },

    /// { "temp=": "x" }
    /// Pop a value from the evaluation stack, and assign it to a newly
    /// declared temporary variable named x.
    TemporaryAssignment { variable: String, reassign: bool },
}

impl Serialize for VariableAssignment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            VariableAssignment::GlobalAssignment { variable } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("VAR=", variable)?;
                map.serialize_entry("re", &true)?;
                map.end()
            }
            VariableAssignment::TemporaryAssignment { variable, reassign } => {
                let len = 1 + usize::from(*reassign);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("temp=", variable)?;
                if *reassign {
                    map.serialize_entry("re", &true)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for VariableAssignment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let obj = serde_json::Map::deserialize(deserializer)?;

        // Global assignment: { "VAR=": "money", "re": true }
        if let Some(name) = obj.get("VAR=").and_then(Value::as_str) {
            return Ok(VariableAssignment::GlobalAssignment {
                variable: name.to_string(),
            });
        }

        // Temporary assignment: { "temp=": "x" }
        // Reassignment: { "temp=": "x", "re": true }
        if let Some(name) = obj.get("temp=").and_then(Value::as_str) {
            let reassign = obj.get("re").and_then(Value::as_bool).unwrap_or(false);
            return Ok(VariableAssignment::TemporaryAssignment {
                variable: name.to_string(),
                reassign,
            });
        }

        Err(serde::de::Error::custom(format!(
            "Unknown variable assignment: {obj:?}"
        )))
    }
}

/// { "VAR?": "danger" }
/// Get an existing global or temporary variable named danger and push its
/// value to the evaluation stack.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableReference {
    pub variable: String,
}

impl Serialize for VariableReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("VAR?", &self.variable)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for VariableReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let obj = serde_json::Map::deserialize(deserializer)?;

        if let Some(name) = obj.get("VAR?").and_then(Value::as_str) {
            return Ok(VariableReference {
                variable: name.to_string(),
            });
        }

        Err(serde::de::Error::custom(format!(
            "Unknown variable reference: {obj:?}"
        )))
    }
}

/// `{ "CNT?": "the_hall.light_switch" }`
/// Obtain the read count of a particular named knot, stitch, choice or gather.
/// Note that this is implemented as a Variable Reference with particular flag
/// in the C# ink runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadCountReference {
    pub variable: String,
}

impl Serialize for ReadCountReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("CNT?", &self.variable)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for ReadCountReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let obj = serde_json::Map::deserialize(deserializer)?;

        if let Some(name) = obj.get("CNT?").and_then(Value::as_str) {
            return Ok(ReadCountReference {
                variable: name.to_string(),
            });
        }

        Err(serde::de::Error::custom(format!(
            "Unknown read count reference: {obj:?}"
        )))
    }
}

#[cfg(test)]
mod tests;
