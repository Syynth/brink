//! Links [`StoryData`] into an executable [`Program`].

use alloc::string::String;
use alloc::vec::Vec;

use brink_format::{DefinitionId, StoryData};

use crate::collections::{Map as HashMap, map_with_capacity};
use crate::error::RuntimeError;
use crate::program::{
    ExternalFnEntry, GlobalSlot, LinkedContainer, ListDefEntry, ListItemEntry, PathTarget, Program,
};

/// Link a [`StoryData`] into an executable [`Program`].
///
/// Builds lookup tables mapping [`DefinitionId`]s to flat array indices.
/// The root container is `containers[0]` by convention — the brink compiler
/// emits the root first.
#[expect(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn link(
    data: &StoryData,
) -> Result<(Program, Vec<Vec<brink_format::LineEntry>>), RuntimeError> {
    let mut container_map = map_with_capacity(data.containers.len());

    for (i, cdef) in data.containers.iter().enumerate() {
        let idx = i as u32;
        container_map.insert(cdef.id, idx);
    }

    // Build scope line tables and a map from scope_id → table index.
    let mut scope_table_map: HashMap<DefinitionId, u32> = map_with_capacity(data.line_tables.len());
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
            param_count: cdef.param_count,
            scope_table_idx,
        });
    }

    // Build globals.
    let mut globals = Vec::with_capacity(data.variables.len());
    let mut global_map = map_with_capacity(data.variables.len());
    for (i, gvar) in data.variables.iter().enumerate() {
        let idx = i as u32;
        global_map.insert(gvar.id, idx);
        globals.push(GlobalSlot {
            id: gvar.id,
            name: gvar.name,
            default: gvar.default_value.clone(),
            local: gvar.local,
        });
    }

    // Build unified address map from containers and address defs.
    // Containers get offset 0 (primary addresses).
    let mut address_map = map_with_capacity(data.containers.len() + data.addresses.len());
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
    let mut list_item_map = map_with_capacity(data.list_items.len());
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
    let mut list_def_map = map_with_capacity(data.list_defs.len());
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

    // Clone the T1b literal pool (`PushLiteral(idx)` targets).
    let literal_pool = data.literal_pool.clone();

    // Build external function map.
    let mut external_fns = map_with_capacity(data.externals.len());
    for ext in &data.externals {
        external_fns.insert(
            ext.id,
            ExternalFnEntry {
                name: ext.name,
                fallback: ext.fallback,
            },
        );
    }

    // Build the path → address lookup used by `Program::find_address`.
    //
    // When the program carries an explicit `address_paths` table (compiler
    // output), it is the source of truth: each entry's qualified path maps to
    // its target, resolved through `address_map`. This is what enables
    // qualified addressing of scopes (`knot`, `knot.stitch`) and author labels
    // (`knot.label`, `knot.stitch.label`).
    //
    // When the table is empty (legacy `.inkb` or converter output, which does
    // not emit it), fall back to deriving scope paths from container names —
    // the previous behavior, which already qualifies knot/stitch scope names.
    let mut address_by_path: HashMap<String, PathTarget> = HashMap::new();
    if data.address_paths.is_empty() {
        // `BTreeMap` has no `reserve` — no-op under `no_std`.
        #[cfg(feature = "std")]
        address_by_path.reserve(data.containers.len());
        for (i, cdef) in data.containers.iter().enumerate() {
            if let Some(name_id) = cdef.name {
                let name = data.name_table[name_id.0 as usize].clone();
                address_by_path.insert(
                    name,
                    PathTarget {
                        id: cdef.id,
                        container_idx: i as u32,
                        byte_offset: 0,
                    },
                );
            }
        }
    } else {
        // `BTreeMap` has no `reserve` — no-op under `no_std`.
        #[cfg(feature = "std")]
        address_by_path.reserve(data.address_paths.len());
        for ap in &data.address_paths {
            // Resolve the target through the address map; skip anything
            // unresolvable (defensive — should not happen for valid output).
            if let Some(&(idx, offset)) = address_map.get(&ap.target) {
                let name = data.name_table[ap.path.0 as usize].clone();
                address_by_path.insert(
                    name,
                    PathTarget {
                        id: ap.target,
                        container_idx: idx,
                        byte_offset: offset,
                    },
                );
            }
        }
    }

    // Compiled `#@local` knot/stitch defaults — the base layer of policy
    // resolution. Sorted by path so a knot expands before its stitches.
    let mut local_scope_defaults: Vec<(String, DefinitionId)> = data
        .containers
        .iter()
        .filter(|c| c.local)
        .filter_map(|c| {
            c.name
                .map(|n| (data.name_table[n.0 as usize].clone(), c.id))
        })
        .collect();
    local_scope_defaults.sort();

    let program = Program {
        containers,
        address_map,
        scope_ids,
        source_checksum: data.source_checksum,
        globals,
        global_map,
        name_table,
        address_by_path,
        root_idx,
        list_literals,
        literal_pool,
        list_item_map,
        list_defs,
        list_def_map,
        external_fns,
        local_scope_defaults,
    };
    Ok((program, line_tables))
}
