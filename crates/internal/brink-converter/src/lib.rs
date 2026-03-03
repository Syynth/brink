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

use brink_format::{LabelDef, StoryData};
use brink_json::InkJson;

/// Convert a parsed ink.json story into a `StoryData`.
pub fn convert(story: &InkJson) -> Result<StoryData, ConvertError> {
    // Pass 0: canonicalize paths and remove $r ceremony
    let canonical_root = canonicalize::canonicalize(&story.root);

    // Pass 1: build the index of all definitions (including labels)
    let story_index = index::build_index(story, &canonical_root)?;

    // Pass 2: emit bytecode for all containers, tracking element byte offsets
    let mut name_table = codegen::NameTableWriter::new();
    let mut element_offsets = codegen::ElementOffsets::new();

    let mut temps = codegen::TempScope::new();
    let mut list_literals = Vec::new();
    let pairs = codegen::process_container(
        &story_index,
        &canonical_root,
        "",
        &mut name_table,
        &mut temps,
        &mut element_offsets,
        &mut list_literals,
    )?;
    let (containers, line_tables): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();

    let variables = codegen::extract_globals(&story_index, &canonical_root, &mut name_table)?;
    let (list_defs, list_items) = codegen::build_list_defs(&story_index, &mut name_table)?;
    let externals = codegen::build_externals(&story_index, &mut name_table)?;

    // Pass 3: build label table from registered labels and element offsets
    let labels = build_labels(&story_index, &element_offsets);

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        labels,
        name_table: name_table.into_vec(),
        list_literals,
    })
}

/// Build `LabelDef`s from the index's label map and the element offset tables.
fn build_labels(
    index: &index::StoryIndex,
    element_offsets: &codegen::ElementOffsets,
) -> Vec<LabelDef> {
    let mut labels = Vec::new();

    for (path, &label_id) in &index.labels {
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
        labels.push(LabelDef {
            id: label_id,
            container_id,
            byte_offset: byte_offset as u32,
        });
    }

    labels
}

#[cfg(test)]
mod tests;
