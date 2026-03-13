//! Core export logic for `lines.json`.

use std::collections::HashMap;

use brink_format::{DefinitionId, LineContent, LinePart, SelectKey, StoryData};

use crate::json_model::{ContentJson, LineJson, LinesJson, PartJson, ScopeJson, SelectJson};

/// Export line tables from a compiled story as a `LinesJson` structure.
///
/// `source_checksum` is the CRC-32 checksum from the `.inkb` header (or 0 if
/// the story was not loaded from `.inkb`).
pub fn export_lines(story: &StoryData, source_checksum: u32) -> LinesJson {
    // Build container name index: id → name string.
    let name_index: HashMap<DefinitionId, &str> = story
        .containers
        .iter()
        .filter_map(|c| {
            c.name
                .and_then(|nid| story.name_table.get(nid.0 as usize))
                .map(|s| (c.id, s.as_str()))
        })
        .collect();

    let scopes: Vec<ScopeJson> = story
        .line_tables
        .iter()
        .filter(|lt| !lt.lines.is_empty())
        .map(|lt| {
            let name = name_index.get(&lt.scope_id).map(|s| (*s).to_string());
            let lines: Vec<LineJson> = lt
                .lines
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    #[expect(clippy::cast_possible_truncation)]
                    let index = i as u16;
                    LineJson {
                        index,
                        content: convert_content(&entry.content),
                        hash: format!("{:016x}", entry.source_hash),
                    }
                })
                .collect();
            ScopeJson {
                name,
                id: format!("0x{:016x}", lt.scope_id.to_raw()),
                lines,
            }
        })
        .collect();

    LinesJson {
        version: 1,
        source_checksum: format!("0x{source_checksum:08x}"),
        scopes,
    }
}

fn convert_content(content: &LineContent) -> ContentJson {
    match content {
        LineContent::Plain(s) => ContentJson::Plain(s.clone()),
        LineContent::Template(parts) => ContentJson::Template {
            template: parts.iter().map(convert_part).collect(),
        },
    }
}

fn convert_part(part: &LinePart) -> PartJson {
    match part {
        LinePart::Literal(s) => PartJson::Literal(s.clone()),
        LinePart::Slot(idx) => PartJson::Slot { slot: *idx },
        LinePart::Select {
            slot,
            variants,
            default,
        } => PartJson::Select {
            select: SelectJson {
                slot: *slot,
                variants: variants
                    .iter()
                    .map(|(key, text)| {
                        let mut map = serde_json::Map::new();
                        map.insert(
                            format_select_key(key),
                            serde_json::Value::String(text.clone()),
                        );
                        map
                    })
                    .collect(),
                default: default.clone(),
            },
        },
    }
}

fn format_select_key(key: &SelectKey) -> String {
    match key {
        SelectKey::Cardinal(cat) => format!("cardinal:{cat:?}"),
        SelectKey::Ordinal(cat) => format!("ordinal:{cat:?}"),
        SelectKey::Exact(n) => format!("={n}"),
        SelectKey::Keyword(k) => format!("keyword:{k}"),
    }
}

#[cfg(test)]
mod tests {
    use brink_format::{DefinitionId, DefinitionTag};

    #[test]
    fn definition_id_hex_format() {
        let id = DefinitionId::new(DefinitionTag::Address, 0xDEAD_BEEF);
        assert_eq!(format!("0x{:016x}", id.to_raw()), "0x01000000deadbeef");
    }

    #[test]
    fn hash_format_no_prefix() {
        let hash: u64 = 0x626e_7681_b4e2_e7bc;
        assert_eq!(format!("{hash:016x}"), "626e7681b4e2e7bc");
    }
}
