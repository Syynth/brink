//! Immutable linked program.

use std::collections::HashMap;

use brink_format::{CountingFlags, DefinitionId, LineEntry, ListValue, NameId, Value};

/// A linked, ready-to-execute program.
///
/// Created from [`StoryData`](brink_format::StoryData) via [`link()`](crate::link).
/// Immutable after creation — mutable per-instance state lives in [`Story`](crate::Story).
pub struct Program {
    pub(crate) containers: Vec<LinkedContainer>,
    pub(crate) container_map: HashMap<DefinitionId, u32>,
    /// Label targets: `id → (container_idx, byte_offset)`.
    pub(crate) label_map: HashMap<DefinitionId, (u32, usize)>,
    pub(crate) line_tables: Vec<Vec<LineEntry>>,
    pub(crate) globals: Vec<GlobalSlot>,
    pub(crate) global_map: HashMap<DefinitionId, u32>,
    pub(crate) name_table: Vec<String>,
    pub(crate) root_idx: u32,
    /// List literal values referenced by `PushList(idx)`.
    pub(crate) list_literals: Vec<ListValue>,
    /// Per-item metadata keyed by item `DefinitionId`.
    pub(crate) list_item_map: HashMap<DefinitionId, ListItemEntry>,
    /// List definitions indexed by position.
    pub(crate) list_defs: Vec<ListDefEntry>,
    /// Map from list def `DefinitionId` to index in `list_defs`.
    pub(crate) list_def_map: HashMap<DefinitionId, usize>,
}

pub(crate) struct LinkedContainer {
    pub id: DefinitionId,
    pub bytecode: Vec<u8>,
    pub counting_flags: CountingFlags,
}

pub(crate) struct GlobalSlot {
    #[expect(dead_code)]
    pub id: DefinitionId,
    #[expect(dead_code)]
    pub name: NameId,
    pub default: Value,
}

/// Runtime metadata for a list item.
pub(crate) struct ListItemEntry {
    pub name: NameId,
    pub ordinal: i32,
    pub origin: DefinitionId,
}

/// Runtime metadata for a list definition.
pub(crate) struct ListDefEntry {
    pub name: NameId,
    /// All item `DefinitionId`s belonging to this list, sorted by ordinal.
    pub items: Vec<DefinitionId>,
}

impl Program {
    /// Look up a container index by its definition id.
    pub(crate) fn resolve_container(&self, id: DefinitionId) -> Option<u32> {
        self.container_map.get(&id).copied()
    }

    /// Resolve any target (container or label) to `(container_idx, byte_offset)`.
    ///
    /// Containers resolve to offset 0. Labels resolve to their recorded byte offset.
    pub(crate) fn resolve_target(&self, id: DefinitionId) -> Option<(u32, usize)> {
        if let Some(&idx) = self.container_map.get(&id) {
            return Some((idx, 0));
        }
        self.label_map.get(&id).copied()
    }

    /// Get a container by its index.
    pub(crate) fn container(&self, idx: u32) -> &LinkedContainer {
        &self.containers[idx as usize]
    }

    /// Get the line table for a container by index.
    pub(crate) fn line_table(&self, idx: u32) -> &[LineEntry] {
        &self.line_tables[idx as usize]
    }

    /// Look up a name by id.
    pub(crate) fn name(&self, id: NameId) -> &str {
        &self.name_table[id.0 as usize]
    }

    /// Look up a global slot index.
    pub(crate) fn resolve_global(&self, id: DefinitionId) -> Option<u32> {
        self.global_map.get(&id).copied()
    }

    /// Get the root container index.
    pub(crate) fn root_idx(&self) -> u32 {
        self.root_idx
    }

    /// Build the initial globals vector from slot defaults.
    pub(crate) fn global_defaults(&self) -> Vec<Value> {
        self.globals.iter().map(|s| s.default.clone()).collect()
    }

    /// Get a list literal by index.
    pub(crate) fn list_literal(&self, idx: u16) -> &ListValue {
        &self.list_literals[idx as usize]
    }

    /// Look up a list item's metadata.
    pub(crate) fn list_item(&self, id: DefinitionId) -> Option<&ListItemEntry> {
        self.list_item_map.get(&id)
    }

    /// Get a list definition by its `DefinitionId`.
    pub(crate) fn list_def(&self, id: DefinitionId) -> Option<&ListDefEntry> {
        self.list_def_map.get(&id).map(|&idx| &self.list_defs[idx])
    }

    /// Find a list definition by its string name.
    pub(crate) fn list_def_by_name(&self, name: &str) -> Option<&ListDefEntry> {
        self.list_defs
            .iter()
            .find(|def| self.name(def.name) == name)
    }
}
