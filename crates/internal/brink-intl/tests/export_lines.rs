#![allow(clippy::unwrap_used)]

use brink_json::InkJson;

fn convert_story(json_text: &str) -> brink_format::StoryData {
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(json_text);
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    brink_converter::convert(&story).unwrap()
}

#[test]
fn snapshot_i001_minimal_story() {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let data = convert_story(json_text);
    let lines_json = brink_intl::export_lines(&data, 0);
    insta::assert_json_snapshot!(lines_json);
}

#[test]
fn empty_scopes_are_omitted() {
    // A story with only empty scopes should produce zero scopes in the export.
    let data = brink_format::StoryData {
        containers: vec![],
        line_tables: vec![brink_format::ScopeLineTable {
            scope_id: brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, 1),
            lines: vec![],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        name_table: vec![],
        list_literals: vec![],
    };
    let lines_json = brink_intl::export_lines(&data, 0);
    assert!(lines_json.scopes.is_empty());
}

#[test]
fn source_checksum_formatting() {
    let data = brink_format::StoryData {
        containers: vec![],
        line_tables: vec![],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        name_table: vec![],
        list_literals: vec![],
    };
    let lines_json = brink_intl::export_lines(&data, 0xDEAD_BEEF);
    assert_eq!(lines_json.source_checksum, "0xdeadbeef");
}

#[test]
fn audio_ref_exported() {
    let scope_id = brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, 1);
    let data = brink_format::StoryData {
        containers: vec![],
        line_tables: vec![brink_format::ScopeLineTable {
            scope_id,
            lines: vec![brink_format::LineEntry {
                content: brink_format::LineContent::Plain("hello\n".to_string()),
                source_hash: 0,
                audio_ref: Some("sfx/line1.ogg".to_string()),
            }],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        name_table: vec![],
        list_literals: vec![],
    };
    let lines_json = brink_intl::export_lines(&data, 0);
    assert_eq!(lines_json.scopes.len(), 1);
    assert_eq!(lines_json.scopes[0].lines.len(), 1);
    assert_eq!(
        lines_json.scopes[0].lines[0].audio,
        Some("sfx/line1.ogg".to_string())
    );
}
