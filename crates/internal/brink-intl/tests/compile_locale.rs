#![allow(clippy::panic, clippy::unwrap_used)]

use brink_format::{read_inkb_index, read_inkl, write_inkb};
use brink_intl::{ContentJson, IntlError, LinesJson, compile_locale, export_lines};

fn make_base_data() -> brink_format::StoryData {
    // Compile from an in-memory string with a fixed entry name so snapshots
    // and checksums stay machine-independent.
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data
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
fn error_slot_index_out_of_range() {
    let inkb = make_base_inkb();
    let mut lines = export_from_inkb(&inkb);

    // I001's line has plain content — zero slots in the base. A translated
    // template referencing slot 0 has no corresponding base slot.
    assert!(!lines.scopes.is_empty());
    assert!(!lines.scopes[0].lines.is_empty());
    lines.scopes[0].lines[0].content = Some(ContentJson::Template {
        template: vec![
            brink_intl::PartJson::Literal("Hola ".to_string()),
            brink_intl::PartJson::Slot { slot: 0 },
        ],
    });

    let err = compile_locale(&inkb, &lines, "es").unwrap_err();
    assert!(
        matches!(
            err,
            IntlError::SlotIndexOutOfRange {
                slot: 0,
                slot_count: 0,
                ..
            }
        ),
        "expected SlotIndexOutOfRange{{slot: 0, slot_count: 0, ..}}, got {err:?}"
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
    use brink_runtime::{DotNetRng, Line, LocaleMode, Story};

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
    let (program, base_tables) = brink_runtime::link(&data).unwrap();
    // Program's source_checksum defaults to 0, and locale's base_checksum comes from inkb.
    // Override the program's checksum to match.
    // Since we can't set source_checksum directly (pub(crate)), we build locale with matching checksum.
    // Actually, the locale's base_checksum comes from the inkb, so we need to match it.
    // We'll create a locale manually with base_checksum=0 to match the program.
    let mut adjusted_locale = locale;
    adjusted_locale.base_checksum = 0;
    let line_tables = brink_runtime::apply_locale(
        &program,
        &adjusted_locale,
        &base_tables,
        LocaleMode::Overlay,
    )
    .unwrap();

    // Run the story and verify the localized text appears
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let lines = story.continue_maximally().unwrap();
    let text: String = lines.iter().map(Line::text).collect();
    assert!(
        text.contains("[ES]"),
        "expected localized text containing '[ES]', got: {text}"
    );
}

// ── #1442/#1671: `#@was` rebinding at compile-locale time ──
//
// A stale locale file carries pre-rename scope ids. `compile_locale` reads
// the base `.inkb`'s own `AliasTable` section and rebinds; the happy path is
// covered end-to-end in `rename_identity.rs`, so these pin the two edges.

/// A knot plus a stitch. Renaming the knot re-keys both, and `#@was` on the
/// knot mints an edge for each — the knot's own plus the stitch's
/// transitive bridge (#1671).
const RENAME_BEFORE: &str = "\
== hub ==
Welcome to the hub.
-> END

= market
Fish, mostly.
-> END
";

const RENAME_AFTER: &str = "\
== plaza ==
#@was(hub)
Welcome to the hub.
-> END

= market
Fish, mostly.
-> END
";

/// `#@was` is a brink-dialect extension (`E051` under strict ink).
fn compile_brink(src: &str) -> brink_format::StoryData {
    let options = brink_compiler::AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink,
        ..brink_compiler::AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("story.ink", |_p| Ok(src.to_owned()), options)
        .unwrap()
        .data
}

fn scope_id_of(story: &brink_format::StoryData, name: &str) -> String {
    export_lines(story, 0)
        .scopes
        .into_iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap()
        .id
}

/// The stitch is re-keyed by the rename too (its qualified name embeds the
/// knot's), and #1671 gives it its own transitive alias entry, so a locale
/// file carrying only its pre-rename id still rebinds through
/// `compile_locale` — it is not left to report `ScopeNotInBase`.
#[test]
fn a_transitively_rekeyed_scope_still_rebinds_through_compile_locale() {
    let before = compile_brink(RENAME_BEFORE);
    let after = compile_brink(RENAME_AFTER);
    let mut inkb = Vec::new();
    write_inkb(&after, &mut inkb);

    let old_stitch = scope_id_of(&before, "hub.market");
    let mut stale = export_lines(&before, 0);
    stale.scopes.retain(|s| s.id == old_stitch);
    assert_eq!(stale.scopes.len(), 1);

    let locale_bytes = compile_locale(&inkb, &stale, "es")
        .expect("the stitch's transitive alias entry (#1671) must rebind it");
    let locale = read_inkl(&locale_bytes).unwrap();

    let new_stitch = scope_id_of(&after, "plaza.market");
    let bound: Vec<String> = locale
        .line_tables
        .iter()
        .map(|t| format!("0x{:016x}", t.scope_id.to_raw()))
        .collect();
    assert_eq!(
        bound,
        vec![new_stitch],
        "the overlay must be keyed on the post-rename stitch id"
    );
}

/// A hand-merged file carrying *both* the pre- and post-rename ids for the
/// same knot would silently drop one of them if the last write won. It is
/// rejected instead.
#[test]
fn two_scopes_rebinding_onto_one_base_scope_are_rejected() {
    let before = compile_brink(RENAME_BEFORE);
    let after = compile_brink(RENAME_AFTER);
    let mut inkb = Vec::new();
    write_inkb(&after, &mut inkb);

    let old_knot = scope_id_of(&before, "hub");
    let new_knot = scope_id_of(&after, "plaza");

    let mut merged = export_lines(&before, 0);
    merged.scopes.retain(|s| s.id == old_knot);
    let mut post_rename = merged.scopes[0].clone();
    post_rename.id = new_knot.clone();
    merged.scopes.push(post_rename);

    let err = compile_locale(&inkb, &merged, "es").unwrap_err();
    match err {
        IntlError::AmbiguousScopeRebind {
            scope_ids,
            base_scope_id,
        } => {
            assert_eq!(scope_ids, format!("{old_knot}, {new_knot}"));
            assert_eq!(base_scope_id, new_knot);
        }
        other => panic!("expected AmbiguousScopeRebind, got {other:?}"),
    }
}
