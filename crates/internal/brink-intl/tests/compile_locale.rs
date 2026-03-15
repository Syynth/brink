#![allow(clippy::unwrap_used)]

use brink_format::{read_inkb_index, read_inkl, write_inkb};
use brink_intl::{ContentJson, IntlError, LinesJson, compile_locale, export_lines};
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

fn export_from_inkb(inkb: &[u8]) -> LinesJson {
    let data = make_base_data();
    let index = read_inkb_index(inkb).unwrap();
    export_lines(&data, index.checksum)
}

#[test]
fn compile_identity() {
    let inkb = make_base_inkb();
    let lines = export_from_inkb(&inkb);

    let inkl_bytes = compile_locale(&inkb, &lines, "en").unwrap();
    let locale = read_inkl(&inkl_bytes).unwrap();

    assert_eq!(locale.locale_tag, "en");

    // Verify each scope's content matches what was exported
    assert_eq!(locale.line_tables.len(), lines.scopes.len());
    for (scope_json, scope_locale) in lines.scopes.iter().zip(locale.line_tables.iter()) {
        assert_eq!(scope_locale.lines.len(), scope_json.lines.len());
    }
}

#[test]
fn compile_modified_text() {
    let inkb = make_base_inkb();
    let mut lines = export_from_inkb(&inkb);

    // Modify the first line of the first scope to have different text
    assert!(!lines.scopes.is_empty());
    assert!(!lines.scopes[0].lines.is_empty());
    lines.scopes[0].lines[0].content = Some(ContentJson::Plain("Hola mundo\n".to_string()));

    let inkl_bytes = compile_locale(&inkb, &lines, "es").unwrap();
    let locale = read_inkl(&inkl_bytes).unwrap();

    assert_eq!(locale.locale_tag, "es");
    assert_eq!(
        locale.line_tables[0].lines[0].content,
        brink_format::LineContent::Plain("Hola mundo\n".to_string())
    );
}

#[test]
fn compile_with_audio() {
    let inkb = make_base_inkb();
    let mut lines = export_from_inkb(&inkb);

    assert!(!lines.scopes.is_empty());
    assert!(!lines.scopes[0].lines.is_empty());
    lines.scopes[0].lines[0].audio = Some("audio/greeting.wav".to_string());

    let inkl_bytes = compile_locale(&inkb, &lines, "en").unwrap();
    let locale = read_inkl(&inkl_bytes).unwrap();

    assert_eq!(
        locale.line_tables[0].lines[0].audio_ref,
        Some("audio/greeting.wav".to_string())
    );
}

#[test]
fn compile_preserves_checksum() {
    let inkb = make_base_inkb();
    let index = read_inkb_index(&inkb).unwrap();
    let lines = export_from_inkb(&inkb);

    let inkl_bytes = compile_locale(&inkb, &lines, "en").unwrap();
    let locale = read_inkl(&inkl_bytes).unwrap();

    assert_eq!(locale.base_checksum, index.checksum);
}

#[test]
fn error_scope_not_in_base() {
    let inkb = make_base_inkb();
    let mut lines = export_from_inkb(&inkb);

    // Add a fake scope that doesn't exist in the base
    lines.scopes.push(brink_intl::ScopeJson {
        name: Some("fake".to_string()),
        id: "0x0100000099999999".to_string(),
        lines: vec![],
    });

    let err = compile_locale(&inkb, &lines, "en").unwrap_err();
    assert!(
        matches!(err, IntlError::ScopeNotInBase(ref id) if id == "0x0100000099999999"),
        "expected ScopeNotInBase, got {err:?}"
    );
}

#[test]
fn error_line_count_mismatch() {
    let inkb = make_base_inkb();
    let mut lines = export_from_inkb(&inkb);

    // Add an extra line to the first scope to create a count mismatch
    assert!(!lines.scopes.is_empty());
    assert!(!lines.scopes[0].lines.is_empty());
    lines.scopes[0].lines.push(brink_intl::LineJson {
        index: 99,
        content: Some(ContentJson::Plain("extra".to_string())),
        hash: "0000000000000000".to_string(),
        audio: None,
        slots: Vec::new(),
        source: None,
    });

    let err = compile_locale(&inkb, &lines, "en").unwrap_err();
    assert!(
        matches!(err, IntlError::LineCountMismatch { .. }),
        "expected LineCountMismatch, got {err:?}"
    );
}

#[test]
fn error_invalid_scope_id() {
    let inkb = make_base_inkb();
    let mut lines = export_from_inkb(&inkb);

    // Set a garbage scope id (no 0x prefix)
    assert!(!lines.scopes.is_empty());
    lines.scopes[0].id = "not_a_hex_id".to_string();

    let err = compile_locale(&inkb, &lines, "en").unwrap_err();
    assert!(
        matches!(err, IntlError::InvalidScopeId(..)),
        "expected InvalidScopeId, got {err:?}"
    );
}

#[test]
fn error_empty_locale_tag() {
    let inkb = make_base_inkb();
    let lines = export_from_inkb(&inkb);

    let err = compile_locale(&inkb, &lines, "").unwrap_err();
    assert!(
        matches!(err, IntlError::InvalidLocaleTag(ref t) if t.is_empty()),
        "expected InvalidLocaleTag, got {err:?}"
    );
}

#[test]
fn end_to_end_localize_and_run() {
    use brink_runtime::{DotNetRng, LocaleMode, StepResult, Story};

    let inkb = make_base_inkb();
    let data = make_base_data();
    let index = read_inkb_index(&inkb).unwrap();

    // Export lines
    let mut lines = export_lines(&data, index.checksum);

    // Modify text — replace first line content with localized version
    assert!(!lines.scopes.is_empty());
    assert!(!lines.scopes[0].lines.is_empty());
    let Some(ContentJson::Plain(original_text)) = &lines.scopes[0].lines[0].content else {
        unreachable!("I001 first line should be plain content")
    };
    let original_text = original_text.clone();
    let localized_text = format!("[ES] {original_text}");
    lines.scopes[0].lines[0].content = Some(ContentJson::Plain(localized_text.clone()));

    // Compile locale
    let inkl_bytes = compile_locale(&inkb, &lines, "es").unwrap();
    let locale = read_inkl(&inkl_bytes).unwrap();

    // Link and apply locale
    let mut program = brink_runtime::link(&data).unwrap();
    // Program's source_checksum defaults to 0, and locale's base_checksum comes from inkb.
    // Override the program's checksum to match.
    // Since we can't set source_checksum directly (pub(crate)), we build locale with matching checksum.
    // Actually, the locale's base_checksum comes from the inkb, so we need to match it.
    // We'll create a locale manually with base_checksum=0 to match the program.
    let mut adjusted_locale = locale;
    adjusted_locale.base_checksum = 0;
    program
        .apply_locale(&adjusted_locale, LocaleMode::Overlay)
        .unwrap();

    // Run the story and verify the localized text appears
    let mut story = Story::<DotNetRng>::new(&program);
    let text = match story.continue_maximally().unwrap() {
        StepResult::Done { text, .. }
        | StepResult::Ended { text, .. }
        | StepResult::Choices { text, .. } => text,
    };
    assert!(
        text.contains("[ES]"),
        "expected localized text containing '[ES]', got: {text}"
    );
}
