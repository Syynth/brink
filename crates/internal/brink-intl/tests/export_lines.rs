#![allow(clippy::unwrap_used)]

fn compile_story(src: &str) -> brink_format::StoryData {
    // Fixed entry name keeps snapshots machine-independent.
    brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data
}

#[test]
fn snapshot_i001_minimal_story() {
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    let data = compile_story(src);
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
        address_paths: vec![],
        name_table: vec![],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        alias_table: vec![],
        source_checksum: 0,
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
        address_paths: vec![],
        name_table: vec![],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        alias_table: vec![],
        source_checksum: 0,
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
                flags: brink_format::LineFlags::from_plain("hello\n"),
                source_hash: 0,
                audio_ref: Some("sfx/line1.ogg".to_string()),
                slot_info: Vec::new(),
                source_location: None,
            }],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        address_paths: vec![],
        name_table: vec![],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        alias_table: vec![],
        source_checksum: 0,
    };
    let lines_json = brink_intl::export_lines(&data, 0);
    assert_eq!(lines_json.scopes.len(), 1);
    assert_eq!(lines_json.scopes[0].lines.len(), 1);
    assert_eq!(
        lines_json.scopes[0].lines[0].audio,
        Some("sfx/line1.ogg".to_string())
    );
}
