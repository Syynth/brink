use std::collections::HashMap;

use bitflags::bitflags;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::Element;

/// ## Containers
/// There is only one type of generalised collection, and this is the
/// Container - it's used throughout the engine. In JSON it's represented as an
/// array type.
///
/// The root of a story is a container, and as a story is evaluated, the engine
/// steps through the sub-elements of containers.
///
/// Although containers primarily behave like arrays, they also have additional
/// attributes, including a way to reference named sub-elements that aren't
/// included in the array itself. To support this behaviour, the **final
/// element of the array is special**. The final element is either `null`, or
/// it's an object (dictionary) that contains a combination of named
/// sub-elements (for example, nested Containers), and optionally two other
/// properties: `#f`, which is used to hold an integer of bit flags, and `#n`,
/// which holds the name of the container itself, if that's not redundant due
/// to being a named field of a parent container.
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    /// Flags held in the `#f` field.
    pub flags: Option<ContainerFlags>,
    /// Name held in the `#n` field.
    pub name: Option<String>,
    /// Named sub-containers held in the final metadata object
    pub named_content: HashMap<String, Element>,
    /// All sub-elements of the container, excluding the final element.
    pub contents: Vec<Element>,
}

impl Serialize for Container {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let has_metadata =
            self.flags.is_some() || self.name.is_some() || !self.named_content.is_empty();

        let len = self.contents.len() + 1; // +1 for the trailing null/metadata object
        let mut seq = serializer.serialize_seq(Some(len))?;

        for element in &self.contents {
            seq.serialize_element(element)?;
        }

        if has_metadata {
            // Build the metadata object
            let mut obj = serde_json::Map::new();

            if let Some(flags) = self.flags {
                obj.insert("#f".to_string(), Value::from(flags.bits()));
            }
            if let Some(ref name) = self.name {
                obj.insert("#n".to_string(), Value::from(name.as_str()));
            }
            for (key, element) in &self.named_content {
                let value = serde_json::to_value(element).map_err(serde::ser::Error::custom)?;
                obj.insert(key.clone(), value);
            }

            seq.serialize_element(&obj)?;
        } else {
            seq.serialize_element(&Value::Null)?;
        }

        seq.end()
    }
}

impl<'de> Deserialize<'de> for Container {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<Value>::deserialize(deserializer)?;

        if values.is_empty() {
            return Ok(Container {
                flags: None,
                name: None,
                named_content: HashMap::new(),
                contents: Vec::new(),
            });
        }

        // The last element is special - it's either null or an object with metadata
        let (content_values, last) = values.split_at(values.len() - 1);
        let last = &last[0];

        let mut flags = None;
        let mut name = None;
        let mut named_content = HashMap::new();

        if !last.is_null()
            && let Some(obj) = last.as_object()
        {
            // Extract flags
            if let Some(f) = obj.get("#f")
                && let Some(f_int) = f.as_u64()
            {
                #[expect(clippy::cast_possible_truncation)]
                let bits = f_int as u32; // flags are always small
                flags = Some(ContainerFlags::from_bits_truncate(bits));
            }

            // Extract name
            if let Some(n) = obj.get("#n")
                && let Some(n_str) = n.as_str()
            {
                name = Some(n_str.to_string());
            }

            // Extract named sub-elements (everything except #f and #n)
            for (key, value) in obj {
                if key != "#f" && key != "#n" {
                    let element: Element =
                        serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
                    named_content.insert(key.clone(), element);
                }
            }
        }

        // Deserialize the content elements
        let mut contents = Vec::new();
        for value in content_values {
            let element: Element =
                serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
            contents.push(element);
        }

        Ok(Container {
            flags,
            name,
            named_content,
            contents,
        })
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Hash, Eq, Ord)]
    pub struct ContainerFlags: u32 {
        /// `Visits`: The story should keep a record of the number of visits to
        /// this container.
        const VISITS = 0b0000_0001;

        /// `Turns`: The story should keep a record of the number of the turn
        /// index that this container was last visited.
        const TURNS = 0b0000_0010;

        /// `CountStartOnly`: For the above numbers, the story should only
        /// record changes when the story visits the very first subelement,
        /// rather than random entry at any point. Used to distinguish the
        /// different behaviour between knots and stitches (random access),
        /// versus gather points and choices (count start only).
        const COUNT_START_ONLY = 0b0000_0100;
    }
}
