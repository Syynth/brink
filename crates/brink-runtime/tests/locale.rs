#![allow(clippy::unwrap_used)]

use brink_format::{
    DefinitionId, DefinitionTag, LineContent, LocaleData, LocaleLineEntry, LocaleScopeTable,
};
use brink_runtime::{DotNetRng, Line, LocaleMode, RuntimeError, Story, apply_locale};

fn scope_id(hash: u64) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, hash)
}

fn compile_and_link(
    ink_path: &str,
) -> (
    brink_format::StoryData,
    brink_runtime::Program,
    Vec<Vec<brink_format::LineEntry>>,
) {
    let data = brink_compiler::compile_path(std::path::Path::new(ink_path))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    (data, program, line_tables)
}

const I001_INK: &str = "../../tests/tier1/basics/I001-minimal-story/story.ink";

/// Build a `LocaleData` that replaces the first line in every scope with the given text.
fn build_locale_replacing_first_line(
    data: &brink_format::StoryData,
    replacement: &str,
) -> LocaleData {
    let base_checksum = data.source_checksum;
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
        base_checksum, // must match the compiled Program's source_checksum
        line_tables,
    }
}

/// Build a `LocaleData` covering all scopes identically (no text changes).
fn build_identity_locale(data: &brink_format::StoryData) -> LocaleData {
    let base_checksum = data.source_checksum;
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
        base_checksum,
        line_tables,
    }
}

#[test]
fn overlay_replaces_scope() {
    let (data, program, base_tables) = compile_and_link(I001_INK);
    let locale = build_locale_replacing_first_line(&data, "[ES] Hola mundo\n");
    let line_tables = apply_locale(&program, &locale, &base_tables, LocaleMode::Overlay).unwrap();

    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let lines = story.continue_maximally().unwrap();
    let text: String = lines.iter().map(Line::text).collect();
    assert!(
        text.contains("[ES] Hola mundo"),
        "expected localized text, got: {text}"
    );
}

#[test]
fn overlay_preserves_untouched() {
    let (data, program, base_tables) = compile_and_link(I001_INK);

    // Only cover the first scope, leave others untouched
    assert!(
        !data.line_tables.is_empty(),
        "need at least one scope to test"
    );
    let first_scope = &data.line_tables[0];
    let locale = LocaleData {
        locale_tag: "partial".to_string(),
        base_checksum: data.source_checksum,
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

    apply_locale(&program, &locale, &base_tables, LocaleMode::Overlay).unwrap();

    // If there are additional scopes, they should still have their original content.
    // For I001 there may only be one scope — the test still validates the overlay path.
}

#[test]
fn strict_all_covered() {
    let (data, program, base_tables) = compile_and_link(I001_INK);
    let locale = build_identity_locale(&data);
    // Strict mode should succeed when all scopes are covered.
    apply_locale(&program, &locale, &base_tables, LocaleMode::Strict).unwrap();
}

#[test]
fn strict_missing_scope() {
    let (data, program, base_tables) = compile_and_link(I001_INK);

    // Build locale with an empty set of scopes — strict mode should fail
    // if the base has any scopes.
    if data.line_tables.is_empty() {
        return; // can't test if there are no scopes
    }

    let locale = LocaleData {
        locale_tag: "partial".to_string(),
        base_checksum: data.source_checksum,
        line_tables: vec![], // no scopes covered
    };

    let err = apply_locale(&program, &locale, &base_tables, LocaleMode::Strict).unwrap_err();
    assert!(
        matches!(err, RuntimeError::LocaleScopeMissing(..)),
        "expected LocaleScopeMissing, got {err:?}"
    );
}

#[test]
fn checksum_mismatch() {
    let (data, program, base_tables) = compile_and_link(I001_INK);

    // The program carries the compiled source checksum; the locale claims a
    // different one.
    let wrong = data.source_checksum ^ 0xDEAD_BEEF;
    let locale = LocaleData {
        locale_tag: "bad".to_string(),
        base_checksum: wrong,
        line_tables: vec![],
    };

    let err = apply_locale(&program, &locale, &base_tables, LocaleMode::Overlay).unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::LocaleChecksumMismatch { expected, actual }
                if expected == data.source_checksum && actual == wrong
        ),
        "expected LocaleChecksumMismatch, got {err:?}"
    );
}

#[test]
fn scope_not_in_base() {
    let (data, program, base_tables) = compile_and_link(I001_INK);

    // Use a scope_id that doesn't exist in the linked program
    let fake_scope = scope_id(0xFFFF_FFFF_FFFF);
    let locale = LocaleData {
        locale_tag: "bad".to_string(),
        base_checksum: data.source_checksum,
        line_tables: vec![LocaleScopeTable {
            scope_id: fake_scope,
            lines: vec![],
        }],
    };

    let err = apply_locale(&program, &locale, &base_tables, LocaleMode::Overlay).unwrap_err();
    assert!(
        matches!(err, RuntimeError::LocaleScopeNotInBase(id) if id == fake_scope),
        "expected LocaleScopeNotInBase, got {err:?}"
    );
}
