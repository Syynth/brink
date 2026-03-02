//! Immutable linked program.

use std::collections::HashMap;

use brink_format::{CountingFlags, DefinitionId, LineEntry, NameId, Value};

/// A linked, ready-to-execute program.
///
/// Created from [`StoryData`](brink_format::StoryData) via [`link()`](crate::link).
/// Immutable after creation — mutable per-instance state lives in [`Story`](crate::Story).
pub struct Program {
    pub(crate) containers: Vec<LinkedContainer>,
    pub(crate) container_map: HashMap<DefinitionId, u32>,
    pub(crate) line_tables: Vec<Vec<LineEntry>>,
    pub(crate) globals: Vec<GlobalSlot>,
    pub(crate) global_map: HashMap<DefinitionId, u32>,
    pub(crate) name_table: Vec<String>,
    pub(crate) root_idx: u32,
}

pub(crate) struct LinkedContainer {
    #[expect(dead_code)]
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

impl Program {
    /// Look up a container index by its definition id.
    pub(crate) fn resolve_container(&self, id: DefinitionId) -> Option<u32> {
        self.container_map.get(&id).copied()
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
}
