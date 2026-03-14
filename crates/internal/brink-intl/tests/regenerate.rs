#![allow(clippy::unwrap_used)]

use brink_format::{read_inkb_index, write_inkb};
use brink_intl::{ContentJson, LineJson, LinesJson, ScopeJson, export_lines, regenerate_lines};
use brink_json::InkJson;

fn make_base_data() -> brink_format::StoryData {
    let json_text =
        include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink.json");
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(json_text);
    let story: InkJson = serde_json::from_str(json_text).unwrap();
    brink_converter::convert(&story).unwrap()
}

fn make_base_inkb() -> Vec<u8> {
    let data = make_base_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    buf
}

fn export_from_base() -> LinesJson {
    let inkb = make_base_inkb();
    let data = make_base_data();
    let index = read_inkb_index(&inkb).unwrap();
    export_lines(&data, index.checksum)
}

fn translate_all(lines: &mut LinesJson, prefix: &str) {
    for scope in &mut lines.scopes {
        for line in &mut scope.lines {
            if let Some(ContentJson::Plain(text)) = &line.content {
                line.content = Some(ContentJson::Plain(format!("[{prefix}] {text}")));
            }
        }
    }
}

#[test]
fn identity_preserves_translations_exactly() {
    let new_export = export_from_base();
    let mut existing = export_from_base();
    translate_all(&mut existing, "ES");

    let result = regenerate_lines(&new_export, &existing);

    assert_eq!(result.scopes.len(), existing.scopes.len());
    for (result_scope, existing_scope) in result.scopes.iter().zip(existing.scopes.iter()) {
        assert_eq!(result_scope.lines.len(), existing_scope.lines.len());
        for (result_line, existing_line) in
            result_scope.lines.iter().zip(existing_scope.lines.iter())
        {
            assert_eq!(result_line.content, existing_line.content);
            assert_eq!(result_line.hash, existing_line.hash);
        }
    }
}

#[test]
fn insertion_leaves_new_line_untranslated() {
    let mut new_export = export_from_base();
    let mut existing = export_from_base();
    translate_all(&mut existing, "JA");

    // Insert a new line into the first scope of new_export.
    assert!(!new_export.scopes.is_empty());
    assert!(!new_export.scopes[0].lines.is_empty());
    let new_line = LineJson {
        index: 999,
        content: Some(ContentJson::Plain("brand new line\n".to_string())),
        hash: "ffffffffffffffff".to_string(),
        audio: None,
    };
    new_export.scopes[0].lines.insert(1, new_line);

    let result = regenerate_lines(&new_export, &existing);
    let scope = &result.scopes[0];

    // The original first line should be translated.
    assert!(scope.lines[0].content.is_some());
    // The inserted line should have no translation.
    assert!(
        scope.lines[1].content.is_none(),
        "expected inserted line to have no content, got {:?}",
        scope.lines[1].content
    );
}

#[test]
fn deletion_preserves_remaining_translations() {
    let new_export = export_from_base();
    let mut existing = export_from_base();
    translate_all(&mut existing, "FR");

    // Remove a line from new_export (simulate deletion in source).
    let mut trimmed_export = new_export.clone();
    assert!(!trimmed_export.scopes.is_empty());
    if trimmed_export.scopes[0].lines.len() > 1 {
        trimmed_export.scopes[0].lines.remove(0);
    }

    let result = regenerate_lines(&trimmed_export, &existing);
    // All remaining lines should have translations.
    for line in &result.scopes[0].lines {
        assert!(
            line.content.is_some(),
            "expected translation preserved, got None"
        );
    }
}

#[test]
fn edit_preserves_old_translation_with_new_hash() {
    let mut new_export = export_from_base();
    let mut existing = export_from_base();
    translate_all(&mut existing, "DE");

    // Change the hash of the first line to simulate source text edit.
    assert!(!new_export.scopes.is_empty());
    assert!(!new_export.scopes[0].lines.is_empty());
    let original_translation = existing.scopes[0].lines[0].content.clone();
    new_export.scopes[0].lines[0].hash = "deadbeefdeadbeef".to_string();

    let result = regenerate_lines(&new_export, &existing);
    let line = &result.scopes[0].lines[0];

    // Old translation should be preserved.
    assert_eq!(line.content, original_translation);
    // But hash should be the new one (signals needs_review).
    assert_eq!(line.hash, "deadbeefdeadbeef");
}

#[test]
fn new_scope_all_lines_untranslated() {
    let mut new_export = export_from_base();
    let existing = export_from_base();

    // Add a completely new scope.
    new_export.scopes.push(ScopeJson {
        name: Some("new_knot".to_string()),
        id: "0x0100000099999999".to_string(),
        lines: vec![LineJson {
            index: 0,
            content: Some(ContentJson::Plain("new knot text\n".to_string())),
            hash: "1111111111111111".to_string(),
            audio: None,
        }],
    });

    let result = regenerate_lines(&new_export, &existing);
    let new_scope = result.scopes.last().unwrap();
    assert_eq!(new_scope.id, "0x0100000099999999");
    assert!(new_scope.lines[0].content.is_none());
}

#[test]
fn removed_scope_dropped_from_output() {
    let new_export = export_from_base();
    let mut existing = export_from_base();

    // Add an extra scope to existing that won't be in new_export.
    existing.scopes.push(ScopeJson {
        name: Some("obsolete".to_string()),
        id: "0x0100000088888888".to_string(),
        lines: vec![LineJson {
            index: 0,
            content: Some(ContentJson::Plain("orphaned\n".to_string())),
            hash: "2222222222222222".to_string(),
            audio: None,
        }],
    });

    let result = regenerate_lines(&new_export, &existing);
    // Removed scope should not appear.
    assert!(
        !result.scopes.iter().any(|s| s.id == "0x0100000088888888"),
        "orphaned scope should be dropped"
    );
    assert_eq!(result.scopes.len(), new_export.scopes.len());
}

#[test]
fn multiple_changes_in_same_scope() {
    let mut new_export = export_from_base();
    let mut existing = export_from_base();
    translate_all(&mut existing, "IT");

    // Simulate: first line edited, new line inserted at end.
    assert!(!new_export.scopes.is_empty());
    let scope = &mut new_export.scopes[0];
    if !scope.lines.is_empty() {
        scope.lines[0].hash = "aaaaaaaaaaaaaaaa".to_string();
    }
    scope.lines.push(LineJson {
        index: 99,
        content: Some(ContentJson::Plain("appended\n".to_string())),
        hash: "bbbbbbbbbbbbbbbb".to_string(),
        audio: None,
    });

    let result = regenerate_lines(&new_export, &existing);
    let result_scope = &result.scopes[0];

    // First line: edit → old translation preserved, new hash.
    assert!(result_scope.lines[0].content.is_some());
    assert_eq!(result_scope.lines[0].hash, "aaaaaaaaaaaaaaaa");

    // Last line: insertion → no translation.
    let last = result_scope.lines.last().unwrap();
    assert!(last.content.is_none());
    assert_eq!(last.hash, "bbbbbbbbbbbbbbbb");
}

#[test]
fn audio_refs_preserved_through_regeneration() {
    let new_export = export_from_base();
    let mut existing = export_from_base();

    // Add audio to first line of existing.
    assert!(!existing.scopes.is_empty());
    assert!(!existing.scopes[0].lines.is_empty());
    existing.scopes[0].lines[0].audio = Some("audio/greeting.ogg".to_string());

    let result = regenerate_lines(&new_export, &existing);
    assert_eq!(
        result.scopes[0].lines[0].audio,
        Some("audio/greeting.ogg".to_string())
    );
}

#[test]
fn checksum_and_version_from_new_export() {
    let mut new_export = export_from_base();
    let existing = export_from_base();

    new_export.source_checksum = "0xcafebabe".to_string();
    new_export.version = 2;

    let result = regenerate_lines(&new_export, &existing);
    assert_eq!(result.source_checksum, "0xcafebabe");
    assert_eq!(result.version, 2);
}
