//! Immutable linked program.

use std::collections::HashMap;

use brink_format::{CountingFlags, DefinitionId, ListValue, NameId, Value};

/// A linked, ready-to-execute program.
///
/// Created from [`StoryData`](brink_format::StoryData) via [`link()`](crate::link).
/// Immutable after creation — mutable per-instance state lives in [`Story`](crate::Story).
pub struct Program {
    pub(crate) containers: Vec<LinkedContainer>,
    /// Unified address map: `id → (container_idx, byte_offset)`.
    /// Contains both container IDs (offset 0) and intra-container addresses.
    pub(crate) address_map: HashMap<DefinitionId, (u32, usize)>,
    /// Scope `DefinitionId` for each entry in the line tables (parallel vec).
    /// Structural metadata — does not change with locale.
    pub(crate) scope_ids: Vec<DefinitionId>,
    /// CRC-32 checksum from the source `.inkb`, used for locale validation.
    pub(crate) source_checksum: u32,
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
    /// External function metadata keyed by the external function's `DefinitionId`.
    pub(crate) external_fns: HashMap<DefinitionId, ExternalFnEntry>,
}

pub(crate) struct LinkedContainer {
    pub id: DefinitionId,
    pub bytecode: Vec<u8>,
    pub counting_flags: CountingFlags,
    pub path_hash: i32,
    /// Index into `Program.line_tables` for this container's scope line table.
    pub scope_table_idx: u32,
}

pub(crate) struct GlobalSlot {
    #[expect(dead_code, reason = "needed for save/load serialization and debugging")]
    pub id: DefinitionId,
    #[expect(dead_code, reason = "needed for save/load serialization and debugging")]
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

/// Runtime metadata for an external function.
pub(crate) struct ExternalFnEntry {
    pub name: NameId,
    pub fallback: Option<DefinitionId>,
}

impl Program {
    /// Resolve any target (container or address) to `(container_idx, byte_offset)`.
    pub(crate) fn resolve_target(&self, id: DefinitionId) -> Option<(u32, usize)> {
        self.address_map.get(&id).copied()
    }

    /// Get a container by its index.
    pub(crate) fn container(&self, idx: u32) -> &LinkedContainer {
        &self.containers[idx as usize]
    }

    /// Get the scope line table index for a container.
    pub(crate) fn scope_table_idx(&self, container_idx: u32) -> u32 {
        self.containers[container_idx as usize].scope_table_idx
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
    pub fn global_defaults(&self) -> Vec<Value> {
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

    /// Look up an external function by its `DefinitionId`.
    pub(crate) fn external_fn(&self, id: DefinitionId) -> Option<&ExternalFnEntry> {
        self.external_fns.get(&id)
    }
}
