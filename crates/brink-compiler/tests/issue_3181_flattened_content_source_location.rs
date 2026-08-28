#![allow(clippy::unwrap_used, clippy::expect_used)]

//! RED/GREEN proof for issue #3181.
//!
//! `brink-codegen-inkb`'s `add_line` hardcoded `source_location: None` for
//! every line reached through the `EmitContent`/`ChoiceOutput` *flattening*
//! path (content the pattern recognizer declines — e.g. a content line
//! mixing plain text with an inline conditional/alternation), even when a
//! real location was available upstream via `hir::Content::ptr`. The
//! recognized-line path already carried a real location; only the
//! flattened fallback dropped it.
//!
//! # Why a choice, not a bare top-level content line
//!
//! A top-level content line whose *only* dynamic part is one inline
//! conditional/alternation gets lifted out by `hir::normalize`
//! (`try_lift_inline`) into a block-level `Stmt::Conditional`/`Sequence`
//! with the surrounding text merged into *each branch's own* content —
//! which the recognizer then admits as an ordinary single-`Text` Phase 1
//! line per branch, never reaching the flattening path at all. A choice's
//! own display/output text is exempt from that lift (`hir::normalize`
//! never touches it — see `lir::lower::content::lower_inline_block`'s
//! doc), so a choice whose text mixes plain text with an inline
//! conditional reliably reaches `EmitContent`/`ChoiceOutput`'s flattening
//! fallback instead.
//!
//! Both source surfaces reach the same codegen fallback, so both are
//! covered here: `.ink` (`flattened_choice_content_gets_a_real_source_location_ink`)
//! and `.brink` (`flattened_choice_content_gets_a_real_source_location_brink`).
//! Each fixture's choice has no bracket/inner content, so its *start*
//! content plays both roles: it's the display text (`combine_choice_content`
//! in `container.rs`, evaluated before the choice is taken) and — once
//! chosen — the output text (`ChoiceOutput` in `lir::lower::mod`,
//! evaluated after). Both paths are exercised and asserted.
//!
//! The assertion is byte-exact, not merely "is `Some`": the recovered
//! `(range_start, range_end)` must slice the *actual* fixture source text
//! back out verbatim (computed via `str::find`, never a hardcoded offset,
//! so the test would fail loudly rather than silently pass if codegen ever
//! attributed a line to the wrong span).

use std::path::Path;

/// Find every line-table entry (across every scope) whose plain text is
/// `text` — the display-text and output-text uses of a choice's start
/// content each mint their own line-table entry with identical text, so a
/// fixture line here is expected to appear (at least) twice.
fn find_lines<'a>(
    story: &'a brink_format::StoryData,
    text: &str,
) -> Vec<&'a brink_format::LineEntry> {
    let found: Vec<_> = story
        .line_tables
        .iter()
        .flat_map(|lt| lt.lines.iter())
        .filter(
            |line| matches!(&line.content, brink_format::LineContent::Plain(s) if s.trim() == text),
        )
        .collect();
    assert!(
        !found.is_empty(),
        "no line table entry with plain text {text:?}"
    );
    found
}

/// Assert `entry` carries a real, byte-exact source location: the resolved
/// `file` matches, and slicing `source[range_start..range_end]` reproduces
/// `expected_slice` verbatim (not just "non-empty" — an actually-wrong
/// range would fail this just as loudly as an absent one).
fn assert_byte_exact_location(
    entry: &brink_format::LineEntry,
    source: &str,
    expected_file: &str,
    expected_slice: &str,
) {
    let loc = entry.source_location.as_ref();
    assert!(
        loc.is_some(),
        "expected a source_location on {:?}, found None (issue #3181 regression)",
        entry.content
    );
    let loc = loc.expect("just asserted above");
    assert_eq!(loc.file, expected_file, "wrong file attributed");
    let start = usize::try_from(loc.range_start).expect("range_start fits usize");
    let end = usize::try_from(loc.range_end).expect("range_end fits usize");
    assert!(
        start < end && end <= source.len(),
        "range {start}..{end} out of bounds for a {}-byte source",
        source.len()
    );
    let found = source.find(expected_slice);
    assert!(
        found.is_some(),
        "fixture source does not contain {expected_slice:?}"
    );
    let expected_start = found.expect("just asserted above");
    assert_eq!(
        (start, end),
        (expected_start, expected_start + expected_slice.len()),
        "range does not byte-exactly cover {expected_slice:?} in the source"
    );
    assert_eq!(
        &source[start..end],
        expected_slice,
        "sliced source does not match expected text verbatim"
    );
}

#[test]
fn flattened_choice_content_gets_a_real_source_location_ink() {
    let src = "VAR highRoll = true\n-> intro\n\n== intro ==\nReady?\n\
               * Roll the dice: {highRoll: high|low} result!\n    -> END\n";
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .expect("fixture should compile");

    // The leading and trailing text fragments of the choice's flattened
    // start content are each their own line-table entry
    // (`emit_content_parts`'s per-`Text`-part `add_line` call, reached via
    // both `combine_choice_content`'s display use and `ChoiceOutput`'s
    // output use) — every one of them now carries the *same* real
    // location: the whole choice-region's span
    // (`lir::Content::source_location`, built from `hir::Content::ptr` —
    // populated for choice regions by this same fix, see
    // `hir::lower::choice::lower_choice`'s `start_content`), matching the
    // one-location-per-line granularity the recognized path already used.
    let whole_region = "Roll the dice: {highRoll: high|low} result!";
    for entry in find_lines(&output.data, "Roll the dice:") {
        assert_byte_exact_location(entry, src, "story.ink", whole_region);
    }
    for entry in find_lines(&output.data, "result!") {
        assert_byte_exact_location(entry, src, "story.ink", whole_region);
    }
}

#[test]
fn flattened_choice_content_gets_a_real_source_location_brink() {
    let dir = std::env::temp_dir().join(format!(
        "brink-issue-3181-flattened-native-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let src = "var highRoll = true\n\nflow main() {\n  Ready?\n  {?\n    \
               * Roll the dice: {if highRoll { high } else { low }} result! -> END\n  }\n}\n";
    std::fs::write(dir.join("main.brink"), src).expect("write fixture");

    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();
    let output = result.expect("fixture should compile");

    // Unlike ink (where a choice's trailing divert is a CST sibling of its
    // content region, out of `hir::Content::ptr`'s range), native's
    // `CHOICE_START_CONTENT` node encloses the trailing `-> END` divert
    // too — so the real, byte-exact region genuinely extends through it.
    let whole_region = "Roll the dice: {if highRoll { high } else { low }} result! -> END";
    let expected_file = Path::new("main.brink").to_string_lossy().into_owned();
    for entry in find_lines(&output.data, "Roll the dice:") {
        assert_byte_exact_location(entry, src, &expected_file, whole_region);
    }
    for entry in find_lines(&output.data, "result!") {
        assert_byte_exact_location(entry, src, &expected_file, whole_region);
    }
}

/// Regression (review finding, #3202): a bracket/inner choice's own
/// location must be a real *cover* of the composed regions, not
/// "start's location wins" — `combine_choice_content` (display side,
/// `container.rs`) and `lower_choice_with_child`'s `ChoiceOutput`
/// composition (output side, `lir/lower/mod.rs`) both used to keep only
/// the *first* region's `source_location` and stamp it on every fragment
/// `emit_content_parts` emits, including fragments that came from the
/// *other* region. For `* Start {h: A|B} [Bracket] Inner`, that meant the
/// `"Bracket"` and `"Inner"` line-table entries carried the *start*
/// region's own span — a range that does not even contain the text
/// `"Bracket"`/`"Inner"` — instead of `None` (which the flattening
/// fallback would honestly have produced pre-#3181). The fix takes the
/// union of both regions' ranges, which is not byte-exact to a single
/// fragment but is always honest: it contains every fragment it is
/// attributed to.
#[test]
fn bracket_and_inner_choice_content_get_a_covering_source_location_ink() {
    let src = "VAR h = true\n-> intro\n\n== intro ==\n\
               * Start {h: A|B} [Bracket] Inner\n    -> END\n";
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .expect("fixture should compile");

    // Display side: `combine_choice_content(start, bracket)` — the union
    // must cover from the start of the start-content region through the
    // end of the bracket-content region, i.e. the whole `"Start {h: A|B}
    // [Bracket]"` span (byte-exact, computed from the real fixture text).
    let display_cover = "Start {h: A|B} [Bracket]";
    for entry in find_lines(&output.data, "Bracket") {
        assert_byte_exact_location(entry, src, "story.ink", display_cover);
    }

    // Output side: `lower_choice_with_child`'s `ChoiceOutput` — the union
    // of start+inner (bracket is excluded from the output text itself,
    // but its byte range falls between start's and inner's in the source,
    // so the honest cover still spans through it) must cover the whole
    // `"Start {h: A|B} [Bracket] Inner"` span.
    let output_cover = "Start {h: A|B} [Bracket] Inner";
    for entry in find_lines(&output.data, "Inner") {
        assert_byte_exact_location(entry, src, "story.ink", output_cover);
    }
}
