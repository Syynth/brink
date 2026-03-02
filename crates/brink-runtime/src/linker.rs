//! Links [`StoryData`] into an executable [`Program`].

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag, StoryData};

use crate::error::RuntimeError;
use crate::program::{GlobalSlot, LinkedContainer, Program};

/// Link a [`StoryData`] into an executable [`Program`].
///
/// Builds lookup tables mapping [`DefinitionId`]s to flat array indices.
/// The root container is identified as the one with `hash("")` using the same
/// hash function as the converter.
#[expect(clippy::cast_possible_truncation)]
pub fn link(data: &StoryData) -> Result<Program, RuntimeError> {
    let mut containers = Vec::with_capacity(data.containers.len());
    let mut container_map = HashMap::with_capacity(data.containers.len());
    let mut line_tables = Vec::with_capacity(data.containers.len());

    for (i, cdef) in data.containers.iter().enumerate() {
        let idx = i as u32;
        container_map.insert(cdef.id, idx);
        containers.push(LinkedContainer {
            id: cdef.id,
            bytecode: cdef.bytecode.clone(),
            counting_flags: cdef.counting_flags,
        });
    }

    // Build line tables parallel to containers.
    let lt_map: HashMap<DefinitionId, &[brink_format::LineEntry]> = data
        .line_tables
        .iter()
        .map(|lt| (lt.container_id, lt.lines.as_slice()))
        .collect();

    for cdef in &data.containers {
        let lines = lt_map
            .get(&cdef.id)
            .map_or_else(Vec::new, |entries| entries.to_vec());
        line_tables.push(lines);
    }

    // Build globals.
    let mut globals = Vec::with_capacity(data.variables.len());
    let mut global_map = HashMap::with_capacity(data.variables.len());
    for (i, gvar) in data.variables.iter().enumerate() {
        let idx = i as u32;
        global_map.insert(gvar.id, idx);
        globals.push(GlobalSlot {
            id: gvar.id,
            name: gvar.name,
            default: gvar.default_value.clone(),
        });
    }

    // Find root container: hash("") using DefaultHasher to match the converter.
    let root_id = DefinitionId::new(DefinitionTag::Container, hash_path(""));
    let root_idx = container_map
        .get(&root_id)
        .copied()
        .ok_or(RuntimeError::NoRootContainer)?;

    let name_table = data.name_table.clone();

    Ok(Program {
        containers,
        container_map,
        line_tables,
        globals,
        global_map,
        name_table,
        root_idx,
    })
}

/// Hash a path string using the same algorithm as the converter.
fn hash_path(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}
