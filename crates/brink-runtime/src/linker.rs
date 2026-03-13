//! Links [`StoryData`] into an executable [`Program`].

use std::collections::HashMap;

use brink_format::{DefinitionId, StoryData};

use crate::error::RuntimeError;
use crate::program::{
    ExternalFnEntry, GlobalSlot, LinkedContainer, ListDefEntry, ListItemEntry, Program,
};

/// Link a [`StoryData`] into an executable [`Program`].
///
/// Builds lookup tables mapping [`DefinitionId`]s to flat array indices.
/// The root container is `containers[0]` by convention — both the converter
/// and the brink compiler emit the root first.
#[expect(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn link(data: &StoryData) -> Result<Program, RuntimeError> {
    let mut container_map = HashMap::with_capacity(data.containers.len());

    for (i, cdef) in data.containers.iter().enumerate() {
        let idx = i as u32;
        container_map.insert(cdef.id, idx);
    }

    // Build scope line tables and a map from scope_id → table index.
    let mut scope_table_map: HashMap<DefinitionId, u32> =
        HashMap::with_capacity(data.line_tables.len());
    let mut line_tables: Vec<Vec<brink_format::LineEntry>> =
        Vec::with_capacity(data.line_tables.len());
    let mut scope_ids: Vec<DefinitionId> = Vec::with_capacity(data.line_tables.len());
    for lt in &data.line_tables {
        let idx = line_tables.len() as u32;
        scope_table_map.insert(lt.scope_id, idx);
        scope_ids.push(lt.scope_id);
        line_tables.push(lt.lines.clone());
    }

    // Build containers with scope_table_idx.
    let mut containers = Vec::with_capacity(data.containers.len());
    for cdef in &data.containers {
        let scope_table_idx = scope_table_map.get(&cdef.scope_id).copied().unwrap_or(0);
        containers.push(LinkedContainer {
            id: cdef.id,
            bytecode: cdef.bytecode.clone(),
            counting_flags: cdef.counting_flags,
            path_hash: cdef.path_hash,
            scope_table_idx,
        });
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

    // Build unified address map from containers and address defs.
    // Containers get offset 0 (primary addresses).
    let mut address_map = HashMap::with_capacity(data.containers.len() + data.addresses.len());
    for (i, cdef) in data.containers.iter().enumerate() {
        address_map.insert(cdef.id, (i as u32, 0usize));
    }
    // Address defs add intra-container targets (and primary addresses from converter).
    for addr in &data.addresses {
        let container_idx = container_map
            .get(&addr.container_id)
            .copied()
            .ok_or(RuntimeError::UnresolvedDefinition(addr.container_id))?;
        address_map.insert(addr.id, (container_idx, addr.byte_offset as usize));
    }

    // Root container is always the first entry by convention.
    if data.containers.is_empty() {
        return Err(RuntimeError::NoRootContainer);
    }
    let root_idx = 0;

    let name_table = data.name_table.clone();

    // Build list item map.
    let mut list_item_map = HashMap::with_capacity(data.list_items.len());
    for li in &data.list_items {
        list_item_map.insert(
            li.id,
            ListItemEntry {
                name: li.name,
                ordinal: li.ordinal,
                origin: li.origin,
            },
        );
    }

    // Build list defs and list def map.
    let mut list_defs = Vec::with_capacity(data.list_defs.len());
    let mut list_def_map = HashMap::with_capacity(data.list_defs.len());
    for ldef in &data.list_defs {
        let idx = list_defs.len();
        // Collect all items belonging to this list, sorted by ordinal.
        let mut items: Vec<_> = data
            .list_items
            .iter()
            .filter(|li| li.origin == ldef.id)
            .collect();
        items.sort_by_key(|li| li.ordinal);
        let item_ids: Vec<_> = items.iter().map(|li| li.id).collect();

        list_def_map.insert(ldef.id, idx);
        list_defs.push(ListDefEntry {
            name: ldef.name,
            items: item_ids,
        });
    }

    // Clone list literals.
    let list_literals = data.list_literals.clone();

    // Build external function map.
    let mut external_fns = HashMap::with_capacity(data.externals.len());
    for ext in &data.externals {
        external_fns.insert(
            ext.id,
            ExternalFnEntry {
                name: ext.name,
                fallback: ext.fallback,
            },
        );
    }

    Ok(Program {
        containers,
        address_map,
        line_tables,
        scope_ids,
        source_checksum: 0,
        globals,
        global_map,
        name_table,
        root_idx,
        list_literals,
        list_item_map,
        list_defs,
        list_def_map,
        external_fns,
    })
}
