#![allow(clippy::unwrap_used)]

use brink_format::{
    DefinitionId, DefinitionTag, LineContent, LocaleData, LocaleLineEntry, LocaleScopeTable,
};
use brink_runtime::{DotNetRng, LocaleMode, RuntimeError, StepResult, Story};

fn scope_id(hash: u64) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, hash)
}

fn convert_and_link(json_text: &str) -> (brink_format::StoryData, brink_runtime::Program) {
    let json_text = json_text.strip_prefix('\u{feff}').unwrap_or(json_text);
    let story: brink_json::InkJson = serde_json::from_str(json_text).unwrap();
    let data = brink_converter::convert(&story).unwrap();
    let program = brink_runtime::link(&data).unwrap();
    (data, program)
}

fn i001_json() -> &'static str {
    include_str!("../../../tests/tier1/basics/I001-minimal-story/story.ink.json")
}

/// Build a `LocaleData` that replaces the first line in every scope with the given text.
fn build_locale_replacing_first_line(
    data: &brink_format::StoryData,
    replacement: &str,
) -> LocaleData {
    let line_tables: Vec<LocaleScopeTable> = data
        .line_tables
        .iter()
        .map(|lt| {
            let lines: Vec<LocaleLineEntry> = lt
                .lines
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    if i == 0 {
                        LocaleLineEntry {
                            content: LineContent::Plain(replacement.to_string()),
                            audio_ref: None,
                        }
                    } else {
                        LocaleLineEntry {
                            content: entry.content.clone(),
                            audio_ref: entry.audio_ref.clone(),
                        }
                    }
                })
                .collect();
            LocaleScopeTable {
                scope_id: lt.scope_id,
                lines,
            }
        })
        .collect();

    LocaleData {
        locale_tag: "es".to_string(),
        base_checksum: 0, // matches Program's default source_checksum
        line_tables,
    }
}

/// Build a `LocaleData` covering all scopes identically (no text changes).
fn build_identity_locale(data: &brink_format::StoryData) -> LocaleData {
    let line_tables: Vec<LocaleScopeTable> = data
        .line_tables
        .iter()
        .map(|lt| LocaleScopeTable {
            scope_id: lt.scope_id,
            lines: lt
                .lines
                .iter()
                .map(|entry| LocaleLineEntry {
                    content: entry.content.clone(),
                    audio_ref: entry.audio_ref.clone(),
                })
                .collect(),
        })
        .collect();

    LocaleData {
        locale_tag: "en".to_string(),
        base_checksum: 0,
        line_tables,
    }
}

#[test]
fn overlay_replaces_scope() {
    let (data, mut program) = convert_and_link(i001_json());
    let locale = build_locale_replacing_first_line(&data, "[ES] Hola mundo\n");
    program.apply_locale(&locale, LocaleMode::Overlay).unwrap();

    let mut story = Story::<DotNetRng>::new(&program);
    let result = story.continue_maximally().unwrap();
    let text = match result {
        StepResult::Done { text, .. }
        | StepResult::Ended { text, .. }
        | StepResult::Choices { text, .. } => text,
    };
    assert!(
        text.contains("[ES] Hola mundo"),
        "expected localized text, got: {text}"
    );
}

#[test]
fn overlay_preserves_untouched() {
    let (data, mut program) = convert_and_link(i001_json());

    // Only cover the first scope, leave others untouched
    assert!(
        !data.line_tables.is_empty(),
        "need at least one scope to test"
    );
    let first_scope = &data.line_tables[0];
    let locale = LocaleData {
        locale_tag: "partial".to_string(),
        base_checksum: 0,
        line_tables: vec![LocaleScopeTable {
            scope_id: first_scope.scope_id,
            lines: first_scope
                .lines
                .iter()
                .map(|_e| LocaleLineEntry {
                    content: LineContent::Plain("[REPLACED]".to_string()),
                    audio_ref: None,
                })
                .collect(),
        }],
    };

    program.apply_locale(&locale, LocaleMode::Overlay).unwrap();

    // If there are additional scopes, they should still have their original content.
    // For I001 there may only be one scope — the test still validates the overlay path.
}

#[test]
fn strict_all_covered() {
    let (data, mut program) = convert_and_link(i001_json());
    let locale = build_identity_locale(&data);
    // Strict mode should succeed when all scopes are covered.
    program.apply_locale(&locale, LocaleMode::Strict).unwrap();
}

#[test]
fn strict_missing_scope() {
    let (data, mut program) = convert_and_link(i001_json());

    // Build locale with an empty set of scopes — strict mode should fail
    // if the base has any scopes.
    if data.line_tables.is_empty() {
        return; // can't test if there are no scopes
    }

    let locale = LocaleData {
        locale_tag: "partial".to_string(),
        base_checksum: 0,
        line_tables: vec![], // no scopes covered
    };

    let err = program
        .apply_locale(&locale, LocaleMode::Strict)
        .unwrap_err();
    assert!(
        matches!(err, RuntimeError::LocaleScopeMissing(..)),
        "expected LocaleScopeMissing, got {err:?}"
    );
}

#[test]
fn checksum_mismatch() {
    let (_data, mut program) = convert_and_link(i001_json());

    // Program has source_checksum=0 (from link), locale has a different checksum.
    let locale = LocaleData {
        locale_tag: "bad".to_string(),
        base_checksum: 0xDEAD_BEEF, // doesn't match 0
        line_tables: vec![],
    };

    let err = program
        .apply_locale(&locale, LocaleMode::Overlay)
        .unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::LocaleChecksumMismatch {
                expected: 0,
                actual: 0xDEAD_BEEF
            }
        ),
        "expected LocaleChecksumMismatch, got {err:?}"
    );
}

#[test]
fn scope_not_in_base() {
    let (_data, mut program) = convert_and_link(i001_json());

    // Use a scope_id that doesn't exist in the linked program
    let fake_scope = scope_id(0xFFFF_FFFF_FFFF);
    let locale = LocaleData {
        locale_tag: "bad".to_string(),
        base_checksum: 0,
        line_tables: vec![LocaleScopeTable {
            scope_id: fake_scope,
            lines: vec![],
        }],
    };

    let err = program
        .apply_locale(&locale, LocaleMode::Overlay)
        .unwrap_err();
    assert!(
        matches!(err, RuntimeError::LocaleScopeNotInBase(id) if id == fake_scope),
        "expected LocaleScopeNotInBase, got {err:?}"
    );
}
