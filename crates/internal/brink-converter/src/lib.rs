//! Converts inklecate `.ink.json` files to brink `.inkb` format.
//!
//! Reads the reference ink compiler's JSON output (via `brink-json`),
//! maps reference instructions to brink opcodes, and produces a
//! `StoryData` (via `brink-format`). Used to bootstrap runtime testing
//! against the 937 golden test files without needing the brink compiler.

mod canonicalize;
mod codegen;
mod error;
mod index;
mod path;

pub use error::ConvertError;

use std::collections::HashMap;

use brink_format::{AddressDef, DefinitionId, LineEntry, ScopeLineTable, StoryData};
use brink_json::InkJson;

/// Convert a parsed ink.json story into a `StoryData`.
pub fn convert(story: &InkJson) -> Result<StoryData, ConvertError> {
    // Pass 0: canonicalize paths, remove $r ceremony, resolve list item refs
    let canonical_root = canonicalize::canonicalize(&story.root, &story.list_defs);

    // Pass 1: build the index of all definitions (including labels)
    let story_index = index::build_index(story, &canonical_root)?;

    // Pass 2: emit bytecode for all containers, tracking element byte offsets
    let mut name_table = codegen::NameTableWriter::new();
    let mut element_offsets = codegen::ElementOffsets::new();

    let mut temps = codegen::TempScope::new();
    let mut list_literals = Vec::new();
    let mut scope_line_tables: HashMap<DefinitionId, Vec<LineEntry>> = HashMap::new();
    let containers = codegen::process_container(
        &story_index,
        &canonical_root,
        "",
        &mut name_table,
        &mut temps,
        &mut element_offsets,
        &mut list_literals,
        &mut scope_line_tables,
    )?;

    // Convert scope line tables to sorted Vec<ScopeLineTable>.
    let mut line_tables: Vec<ScopeLineTable> = scope_line_tables
        .into_iter()
        .map(|(scope_id, lines)| ScopeLineTable { scope_id, lines })
        .collect();
    line_tables.sort_by_key(|lt| lt.scope_id.to_raw());

    let variables = codegen::extract_globals(&story_index, &canonical_root, &mut name_table)?;
    let (list_defs, list_items) = codegen::build_list_defs(&story_index, &mut name_table)?;
    let externals = codegen::build_externals(&story_index, &mut name_table)?;

    // Pass 3: build address table — primary addresses for every container,
    // plus intra-container addresses from registered targets.
    let addresses = build_addresses(&story_index, &containers, &element_offsets);

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses,
        name_table: name_table.into_vec(),
        list_literals,
    })
}

/// Build `AddressDef`s: primary addresses for every container (`byte_offset` 0)
/// plus intra-container addresses from the index's registered targets.
fn build_addresses(
    index: &index::StoryIndex,
    containers: &[brink_format::ContainerDef],
    element_offsets: &codegen::ElementOffsets,
) -> Vec<AddressDef> {
    let mut addresses = Vec::new();

    // Primary address for every container: id == container_id, byte_offset 0.
    for cdef in containers {
        addresses.push(AddressDef {
            id: cdef.id,
            container_id: cdef.id,
            byte_offset: 0,
        });
    }

    // Intra-container addresses (formerly labels).
    for (path, &addr_id) in &index.intra_addresses {
        // Decompose the path: last component is the element index,
        // everything before is the container path.
        let Some(dot) = path.rfind('.') else {
            continue;
        };
        let container_path = &path[..dot];
        let index_str = &path[dot + 1..];
        let Ok(element_index) = index_str.parse::<usize>() else {
            continue;
        };

        // Look up the container's ID.
        let Some(&container_id) = index.containers.get(container_path) else {
            continue;
        };

        // Look up the byte offset for this element index.
        let byte_offset = element_offsets
            .get(&container_id)
            .and_then(|offsets| offsets.get(&element_index))
            .copied()
            .unwrap_or(0);

        #[expect(clippy::cast_possible_truncation)]
        addresses.push(AddressDef {
            id: addr_id,
            container_id,
            byte_offset: byte_offset as u32,
        });
    }

    addresses
}

#[cfg(test)]
mod tests;
