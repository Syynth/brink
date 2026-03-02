//! Converts inklecate `.ink.json` files to brink `.inkb` format.
//!
//! Reads the reference ink compiler's JSON output (via `brink-json`),
//! maps reference instructions to brink opcodes, and produces a
//! `StoryData` (via `brink-format`). Used to bootstrap runtime testing
//! against the 937 golden test files without needing the brink compiler.

mod codegen;
mod error;
mod index;
mod path;

pub use error::ConvertError;

use brink_format::StoryData;
use brink_json::InkJson;

/// Convert a parsed ink.json story into a `StoryData`.
pub fn convert(story: &InkJson) -> Result<StoryData, ConvertError> {
    // Pass 1: build the index of all definitions
    let story_index = index::build_index(story)?;

    // Pass 2: emit bytecode for all containers
    let mut name_table = codegen::NameTableWriter::new();

    let mut temps = codegen::TempScope::new();
    let pairs =
        codegen::process_container(&story_index, &story.root, "", &mut name_table, &mut temps)?;
    let (containers, line_tables): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();

    let variables = codegen::extract_globals(&story_index, story, &mut name_table)?;
    let (list_defs, list_items) = codegen::build_list_defs(&story_index, &mut name_table)?;
    let externals = codegen::build_externals(&story_index, &mut name_table)?;

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        name_table: name_table.into_vec(),
    })
}

#[cfg(test)]
mod tests;
