use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::Path;

/// Additionally, a "c" property set to true indicates that the divert is
/// conditional, and should therefore pop a value off the evaluation stack to
/// determine whether the divert should actually happen.
#[derive(Debug, Clone, PartialEq)]
pub enum Divert {
    /// { "->": "path.to.target" }
    /// a standard divert to content at a particular path.
    Target { conditional: bool, path: Path },

    /// { "->": "variableTarget", "var": true }
    /// as above, except that var specifies that the target is the name of a
    /// variable containing a divert target value.
    Variable { conditional: bool, path: Path },

    /// { "f()": "path.to.func" }
    /// a function-call, which is defined as a divert that pushes an element to
    /// the callstack. Note that it doesn't necessarily correspond directly to
    /// an ink function, since choices use them internally too.
    Function { conditional: bool, path: Path },

    /// { "->t->": "path.tunnel" }
    /// a tunnel, which works similarly to a function call by pushing an
    /// element to the callstack. The only difference is that the callstack is
    /// aware of the type of element that was pushed, for error checking.
    Tunnel { conditional: bool, path: Path },

    /// { "->t->": "variableTarget", "var": true }
    /// as above, except that var specifies that the target is the name of a
    /// variable containing a divert target value.
    TunnelVariable { conditional: bool, path: Path },

    /// { "x()": "externalFuncName", "exArgs": 5 }
    /// an external (game-side) function call, that optionally takes the specified number of arguments.
    ExternalFunction {
        conditional: bool,
        name: String,
        arg_count: u32,
    },
}

impl Serialize for Divert {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Divert::Target { conditional, path } => {
                let len = 1 + usize::from(*conditional);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("->", path)?;
                if *conditional {
                    map.serialize_entry("c", &true)?;
                }
                map.end()
            }
            Divert::Variable { conditional, path } => {
                let len = 2 + usize::from(*conditional);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("->", path)?;
                map.serialize_entry("var", &true)?;
                if *conditional {
                    map.serialize_entry("c", &true)?;
                }
                map.end()
            }
            Divert::Function { conditional, path } => {
                let len = 1 + usize::from(*conditional);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("f()", path)?;
                if *conditional {
                    map.serialize_entry("c", &true)?;
                }
                map.end()
            }
            Divert::Tunnel { conditional, path } => {
                let len = 1 + usize::from(*conditional);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("->t->", path)?;
                if *conditional {
                    map.serialize_entry("c", &true)?;
                }
                map.end()
            }
            Divert::TunnelVariable { conditional, path } => {
                let len = 2 + usize::from(*conditional);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("->t->", path)?;
                map.serialize_entry("var", &true)?;
                if *conditional {
                    map.serialize_entry("c", &true)?;
                }
                map.end()
            }
            Divert::ExternalFunction {
                conditional,
                name,
                arg_count,
            } => {
                let len = 2 + usize::from(*conditional);
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("x()", name)?;
                map.serialize_entry("exArgs", arg_count)?;
                if *conditional {
                    map.serialize_entry("c", &true)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Divert {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let obj = serde_json::Map::deserialize(deserializer)?;

        let conditional = obj.get("c").and_then(Value::as_bool).unwrap_or(false);

        // Standard divert: { "->": "path" } or variable divert: { "->": "path", "var": true }
        if let Some(target) = obj.get("->").and_then(Value::as_str) {
            let is_var = obj.get("var").and_then(Value::as_bool).unwrap_or(false);

            return if is_var {
                Ok(Divert::Variable {
                    conditional,
                    path: target.to_string(),
                })
            } else {
                Ok(Divert::Target {
                    conditional,
                    path: target.to_string(),
                })
            };
        }

        // Function call: { "f()": "path" }
        if let Some(target) = obj.get("f()").and_then(Value::as_str) {
            return Ok(Divert::Function {
                conditional,
                path: target.to_string(),
            });
        }

        // Tunnel: { "->t->": "path" } or variable tunnel: { "->t->": "path", "var": true }
        if let Some(target) = obj.get("->t->").and_then(Value::as_str) {
            let is_var = obj.get("var").and_then(Value::as_bool).unwrap_or(false);
            return if is_var {
                Ok(Divert::TunnelVariable {
                    conditional,
                    path: target.to_string(),
                })
            } else {
                Ok(Divert::Tunnel {
                    conditional,
                    path: target.to_string(),
                })
            };
        }

        // External function: { "x()": "name", "exArgs": n }
        if let Some(name) = obj.get("x()").and_then(Value::as_str) {
            #[expect(clippy::cast_possible_truncation)]
            let arg_count = obj.get("exArgs").and_then(Value::as_u64).unwrap_or(0) as u32; // arg counts are always small

            return Ok(Divert::ExternalFunction {
                conditional,
                name: name.to_string(),
                arg_count,
            });
        }

        Err(serde::de::Error::custom(format!(
            "Unknown divert type: {obj:?}"
        )))
    }
}
