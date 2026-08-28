//! Pest-based reader for the `.inkt` textual format.
//!
//! The grammar-rule clusters live in sibling modules (issue #685, pure `mod`
//! extraction — no logic changes): [`values`] (scalars and composite value
//! forms), [`defs`] (top-level definition tables), [`lines`] (container
//! metadata and the per-scope line table), [`instructions`] (the bytecode
//! `code` field), and [`primitives`] (shared token parsers). This file keeps
//! only the parser entry point (`read_inkt`/`parse_story`), the pest-derived
//! `Rule` enum, and the public [`InktParseError`] type.

use pest::Parser;
use pest_derive::Parser;

use crate::story::StoryData;

mod defs;
mod instructions;
mod lines;
mod primitives;
mod values;

#[derive(Parser)]
#[grammar = "inkt/inkt.pest"]
struct InktParser;

/// Error returned when parsing `.inkt` text fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InktParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl core::fmt::Display for InktParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for InktParseError {}

/// Parse `.inkt` text into a [`StoryData`].
pub fn read_inkt(input: &str) -> Result<StoryData, InktParseError> {
    let pairs = InktParser::parse(Rule::story, input).map_err(|e| {
        let (line, col) = match e.line_col {
            pest::error::LineColLocation::Pos(pos) => pos,
            pest::error::LineColLocation::Span(start, _) => start,
        };
        InktParseError {
            message: e.to_string(),
            line,
            col,
        }
    })?;

    let story_pair = pairs.into_iter().next().ok_or_else(|| InktParseError {
        message: "no story node".into(),
        line: 1,
        col: 1,
    })?;

    parse_story(story_pair)
}

type P<'a> = pest::iterators::Pair<'a, Rule>;

fn parse_story(pair: P<'_>) -> Result<StoryData, InktParseError> {
    let mut name_table = Vec::new();
    // Fuzz-found (#1102): a `.inkt` document declaring the same container
    // address twice is malformed input and must be rejected at read time.
    // Accepting it poisons the roundtrip downstream: `write_inkt` collapses
    // line tables through a `scope_id`-keyed `HashMap`, so the later
    // duplicate's lines silently replace the earlier one's on the next write
    // (same admission-check posture as the duplicate map key rejection, #985).
    let mut seen_container_ids = std::collections::HashSet::new();
    let mut variables = Vec::new();
    let mut list_defs = Vec::new();
    let mut list_items = Vec::new();
    let mut externals = Vec::new();
    let mut addresses = Vec::new();
    let mut address_paths = Vec::new();
    let mut containers = Vec::new();
    let mut line_tables = Vec::new();
    let mut list_literals = Vec::new();
    let mut literal_pool = Vec::new();
    let mut private_defs = Vec::new();
    let mut alias_table = Vec::new();
    let mut effect_rows = Vec::new();
    let mut frame_shapes = Vec::new();
    let mut struct_shapes = Vec::new();
    let mut debug_info = None;
    let mut source_checksum = 0u32;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::story_checksum => {
                if let Some(hex_pair) = inner.into_inner().next() {
                    source_checksum = primitives::parse_hex_u32(hex_pair.as_str());
                }
            }
            Rule::name_table => name_table = defs::parse_name_table(inner)?,
            Rule::globals => variables = defs::parse_globals(inner)?,
            Rule::lists => list_defs = defs::parse_lists(inner)?,
            Rule::list_items => list_items = defs::parse_list_items(inner)?,
            Rule::externals => externals = defs::parse_externals(inner)?,
            Rule::addresses => addresses = defs::parse_addresses(inner)?,
            Rule::address_paths => address_paths = defs::parse_address_paths(inner)?,
            Rule::list_literals => list_literals = defs::parse_list_literals(inner)?,
            Rule::literal_pool => literal_pool = values::parse_literal_pool(inner)?,
            Rule::struct_shapes => struct_shapes = defs::parse_struct_shapes(inner)?,
            Rule::visibility => private_defs = defs::parse_visibility(inner)?,
            Rule::alias_table => alias_table = defs::parse_alias_table(inner)?,
            Rule::effect_rows => effect_rows = defs::parse_effect_rows(inner)?,
            Rule::frame_shapes => frame_shapes = defs::parse_frame_shapes(inner)?,
            Rule::debug_info => debug_info = Some(defs::parse_debug_info(inner)?),
            Rule::container => {
                let (line, col) = inner.line_col();
                let (container, lt) = lines::parse_container(inner)?;
                if !seen_container_ids.insert(container.id) {
                    return Err(InktParseError {
                        message: format!("duplicate container address: {}", container.id),
                        line,
                        col,
                    });
                }
                let is_scope_owner = container.scope_id == container.id;
                containers.push(container);
                // Only add line tables for scope-owning containers.
                // Child containers (scope_id != id) have no lines in the text.
                if is_scope_owner {
                    line_tables.push(lt);
                }
            }
            _ => {}
        }
    }

    // Sort line tables by scope_id for deterministic ordering,
    // matching the converter's output.
    line_tables.sort_by_key(|lt| lt.scope_id.to_raw());

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses,
        address_paths,
        name_table,
        list_literals,
        literal_pool,
        struct_shapes,
        private_defs,
        alias_table,
        effect_rows,
        frame_shapes,
        debug_info,
        source_checksum,
    })
}
