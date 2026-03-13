use crate::counting::CountingFlags;
use crate::id::{DefinitionId, NameId};
use crate::line::LineContent;
use crate::value::{Value, ValueType};

/// A compiled container (knot, stitch, gather, or anonymous flow block).
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerDef {
    pub id: DefinitionId,
    pub bytecode: Vec<u8>,
    pub content_hash: u64,
    pub counting_flags: CountingFlags,
    /// Sum of char values from the container's ink path string.
    /// Used to seed the RNG for shuffle sequences.
    pub path_hash: i32,
}

/// One entry in a container's line table.
#[derive(Debug, Clone, PartialEq)]
pub struct LineEntry {
    pub content: LineContent,
    pub source_hash: u64,
}

/// Per-container line table, stored separately from [`ContainerDef`] for
/// locale overlay swapping (`.inkl`).
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerLineTable {
    pub container_id: DefinitionId,
    pub lines: Vec<LineEntry>,
}

/// A global variable definition.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalVarDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub value_type: ValueType,
    pub default_value: Value,
    pub mutable: bool,
}

/// A list (enum-like set) definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// `(item_name, ordinal)` pairs in declaration order.
    pub items: Vec<(NameId, i32)>,
}

/// A single list item definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItemDef {
    pub id: DefinitionId,
    pub origin: DefinitionId,
    pub ordinal: i32,
    pub name: NameId,
}

/// An address pointing to a specific byte offset within a container.
///
/// Addresses are used for divert targets, visit tracking, and any definition
/// that maps to a position within a container. A "primary" address has
/// `byte_offset == 0` and the same `id` as its `container_id`, functioning
/// like the old `Container` tag. Intra-container addresses have non-zero
/// offsets and distinct IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressDef {
    pub id: DefinitionId,
    pub container_id: DefinitionId,
    pub byte_offset: u32,
}

/// Compute a deterministic hash of line content text.
///
/// Used by both the compiler codegen and the converter to populate
/// [`LineEntry::source_hash`]. The hash detects when source text has
/// changed across builds, enabling the regeneration workflow in the
/// internationalization pipeline.
pub fn content_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// An externally-bound function definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalFnDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub arg_count: u8,
    pub fallback: Option<DefinitionId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let a = content_hash("Hello, world!");
        let b = content_hash("Hello, world!");
        assert_eq!(a, b);
    }

    #[test]
    fn content_hash_non_zero_for_non_empty() {
        assert_ne!(content_hash("some text"), 0);
        assert_ne!(content_hash("x"), 0);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        assert_ne!(content_hash("hello"), content_hash("world"));
    }
}
