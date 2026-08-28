#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

//! Golden/snapshot coverage for `source_location`, issue #3213.
//!
//! # Why this file exists
//!
//! Twice in two waves a fully green gate shipped wrong provenance (PR #3189,
//! PR #3202's own review round) because outside
//! `issue_3181_flattened_content_source_location.rs`'s one test, **every**
//! `source_location` mention in the test suite was a hand-built `None` —
//! provenance correctness was unfalsifiable by CI. This file asserts real,
//! byte-exact `source_location` values over one fixture per emission path,
//! on both source surfaces, so a regression in any of them goes red here
//! before it goes red in the studio in front of an author.
//!
//! # The granularity contract (`docs/intl-spec.md`, "`source_location`")
//!
//! One `source_location` per **content line** (or composed choice
//! display/output region) — never one per emitted fragment. Every flattened
//! `Text` fragment of a multi-part line carries a *clone* of the same
//! `Content`-level location. [`assert_recorded_range_contains_its_source`]
//! enforces this mechanically, as a blanket sweep over every entry in every
//! fixture compiled here, not just at the specific assertion sites below —
//! this is the shape of check that would have caught PR #3202's review
//! finding 2 (composing two choice regions kept the *first* region's
//! location and stamped it on fragments from the *other* region, so a
//! line's recorded range did not contain its own text).
//!
//! # Paths covered, per surface (`.ink` and `.brink`)
//!
//! 1. **Recognized content line** (`recognize.rs`'s Phase 1 `Plain` path) —
//!    a bare narrative line with no dynamic content.
//! 2. **Lifted inline conditional/alternation** — the exact #3202 regression
//!    shape: a top-level content line mixing plain text with `{cond: A|B}`
//!    gets lifted (`hir::normalize::try_lift_inline`) into one `Stmt` per
//!    branch, and `splice_around` must let the *enclosing line's* location
//!    win so each branch's recorded range covers the **whole source line**,
//!    not just its own narrower branch text.
//! 3. **Choice display text** (pre-selection, `combine_choice_content` in
//!    `brink-codegen-inkb/src/container.rs`).
//! 4. **Choice output text** (post-selection, `ChoiceOutput` in
//!    `brink-ir/src/lir/lower/mod.rs`).
//! 5. **Tag entries** — currently and deliberately `None`. **Pinned
//!    deliberately**: `lir::Content::tags` is a bare `Vec<Vec<ContentPart>>`
//!    — `hir::Tag::ptr` (a real provenance) is discarded flattening a tag
//!    into that shape, and reusing the enclosing content's range would
//!    misattribute a tag's own byte span (a tag can sit on the same source
//!    line as content it isn't co-extensive with) — exactly the
//!    "confidently wrong" location the issue warns against. A known,
//!    separate-scope follow-up (`lir::Content::tags` would need to carry
//!    per-tag provenance, a data-shape change). The day someone fixes it,
//!    [`assert_pinned_none`] on this fixture turns red — an intentional,
//!    visible change, not silent drift.
//! 6. **`StringPart::Literal` inside interpolation** — currently and
//!    deliberately `None`, same reasoning: `hir::StringPart::Literal`
//!    carries no span at all today (`hir::expr_span`'s `Expr::String` arm
//!    unions only interpolation sub-expression spans). Also pinned, same
//!    follow-up-visibility rationale as tags.
//!
//! Both fixtures' choice has no bracket/inner content, so its *start*
//! content plays both the display-text role (#3) and, once chosen, the
//! output-text role (#4) — mirroring
//! `issue_3181_flattened_content_source_location.rs`'s own fixture shape,
//! which this file does not duplicate but extends with the paths that test
//! left uncovered.
//!
//! # Proof this suite is worth anything
//!
//! A coverage suite that passes against a known-bad state is worthless —
//! this is exactly the failure mode issue #3213 exists to prevent. Proof
//! (reported in the PR body, not re-derivable from this file alone): with
//! PR #3202's own review-round fix commit (`e725b3952`, "apply adversarial
//! review findings — span regression + wrong choice locations") temporarily
//! reverted from the working tree, every test in this file that touches the
//! lifted-inline-conditional path or the choice-region composition goes
//! RED; restoring the commit turns them GREEN again.
//!
//! # Why issue #3183's `lir::Expr` provenance is NOT extended here
//!
//! Issue #3183 (D5's remainder) landed a bare `Provenance` on `lir::Expr`,
//! mirroring `lir::Stmt`'s split. That is a pure **in-memory IR** addition —
//! deliberately no codegen change (D6/#3184's job) — so nothing in this
//! file's `source_location` wire-format assertions could exercise it: this
//! suite reads `SourceLocation` off `StoryData`/`LineEntry`, which
//! `lir::Expr::provenance` is not (yet) threaded into. The byte-exact
//! round-trip proof for `lir::Expr` provenance lives at the layer that
//! actually carries it — `crates/internal/brink-ir/tests/
//! issue_3183_lir_provenance.rs`'s "`lir::Expr` provenance" section —
//! following that file's own established `Stmt`/`Container` round-trip
//! discipline rather than duplicating it at the wrong granularity here.

use std::path::Path;

use brink_format::{LineContent, LineEntry, SourceLocation, StoryData};

// ─── Fixtures ───────────────────────────────────────────────────────

/// `.ink` fixture exercising all six paths. Built as a joined line array
/// (not one long string literal) so the source stays visually identical to
/// what a `story.ink` file on disk would look like — every expected slice
/// below is recovered from this text via `str::find`, never a hardcoded
/// offset.
fn ink_fixture_source() -> String {
    [
        "VAR h = true",
        "-> intro",
        "",
        "== intro ==",
        "Ready?",
        "Ready {h: high|low} now.",
        "Hello #mood: calm",
        "* Roll the dice: {h: high|low} result!",
        "    -> aside",
        "",
        "== aside ==",
        "~ temp greeting = \"Hi {h} there\"",
        "-> END",
        "",
    ]
    .join("\n")
}

/// `.brink` fixture exercising the same six paths through the native
/// resolver. `greet`'s `len(...)` call forces its string-literal argument
/// through `emit_string_expr`'s multi-part (`BeginStringEval`) path rather
/// than the single-literal fast path (`emit_string_expr` folds a
/// *one*-part `StringExpr` straight to `PushString`, bypassing `add_line`
/// entirely — a local `var greeting = "Hi {pname} there"` alone does not
/// reliably avoid that fold, so the literal is threaded through a function
/// argument instead, matching how #3202's own scope-note case was
/// exercised).
fn brink_fixture_source() -> String {
    [
        "var h = true",
        "",
        "flow greet(pname) {",
        "  Length is {len(\"Hi {pname} there\")}.",
        "}",
        "",
        "flow main() {",
        "  Line one.",
        "  Ready {if h { high } else { low }} now.",
        "  Hello #mood: calm",
        "  {?",
        "    * Roll the dice: {if h { high } else { low }} result! -> END",
        "  }",
        "}",
        "",
    ]
    .join("\n")
}

// ─── Shared assertion machinery ────────────────────────────────────

/// Every line-table entry (across every scope) whose `Plain` text is
/// `text` — several of these paths mint more than one entry with the same
/// text (e.g. a choice's start content plays both the display-text and
/// output-text role), so callers should expect (and check) more than one
/// hit where that's the documented shape.
fn find_plain<'a>(story: &'a StoryData, text: &str) -> Vec<&'a LineEntry> {
    let found: Vec<_> = story
        .line_tables
        .iter()
        .flat_map(|lt| lt.lines.iter())
        .filter(|line| matches!(&line.content, LineContent::Plain(s) if s == text))
        .collect();
    assert!(
        !found.is_empty(),
        "no line table entry with plain text {text:?}"
    );
    found
}

/// One human-readable line describing a `LineEntry`'s recorded location:
/// the line's own text, the file, the byte range, and — critically — the
/// *source slice that range denotes*. Comparing two of these strings
/// (actual vs. expected, the latter built the same way from a
/// `str::find`-derived range) makes a wrong range self-evident in an
/// `assert_eq!` diff: a reviewer sees `@ story.ink:87..123 denotes "Roll
/// the dice: {h: high|low} result!"` right next to the entry's own line
/// text and doesn't have to re-derive anything to spot the bug. A bare
/// pair of offset numbers does not have that property.
/// Every fixture path exercised in this file mints a `Plain` line-table
/// entry; a `Template` here would mean a fixture line accidentally matched
/// `recognize.rs`'s Phase 3 (interpolation) shape instead of the intended
/// path, which is itself worth failing loudly on rather than silently
/// treating as absent data.
fn plain_text(entry: &LineEntry) -> &str {
    assert!(
        matches!(entry.content, LineContent::Plain(_)),
        "expected a Plain entry, got {:?}",
        entry.content
    );
    match &entry.content {
        LineContent::Plain(s) => s.as_str(),
        LineContent::Template(_) => unreachable!("just asserted a Plain entry above"),
    }
}

fn describe(text: &str, loc: Option<&SourceLocation>, source: &str) -> String {
    match loc {
        Some(loc) => {
            let start = usize::try_from(loc.range_start).expect("range_start fits usize");
            let end = usize::try_from(loc.range_end).expect("range_end fits usize");
            let slice = source.get(start..end).unwrap_or("<range out of bounds>");
            format!(
                "line {text:?} @ {}:{start}..{end} denotes {slice:?}",
                loc.file
            )
        }
        None => format!("line {text:?} @ None"),
    }
}

/// Assert `entry` carries a real, byte-exact `source_location`: slicing
/// `source[range_start..range_end]` reproduces `expected_slice` verbatim.
/// `expected_slice`'s own position is recovered from `source` via
/// `str::find` — never a hardcoded offset (the precedent PR #3202 set,
/// `issue_3181_flattened_content_source_location.rs`).
fn assert_byte_exact(entry: &LineEntry, source: &str, expected_file: &str, expected_slice: &str) {
    let entry_text = plain_text(entry);
    let found = source.find(expected_slice);
    assert!(
        found.is_some(),
        "fixture source does not contain {expected_slice:?}"
    );
    let expected_start = found.expect("just asserted above");
    let expected_end = expected_start + expected_slice.len();
    let actual = describe(entry_text, entry.source_location.as_ref(), source);
    let expected = describe(
        entry_text,
        Some(&SourceLocation {
            file: expected_file.to_owned(),
            #[expect(clippy::cast_possible_truncation, reason = "test fixtures are tiny")]
            range_start: expected_start as u32,
            #[expect(clippy::cast_possible_truncation, reason = "test fixtures are tiny")]
            range_end: expected_end as u32,
        }),
        source,
    );
    assert_eq!(actual, expected, "source_location mismatch (issue #3213)");
}

/// Assert `entry` carries the pinned, deliberate `None` — a tag entry or a
/// `StringPart::Literal` (see this file's module doc, paths 5 and 6). The
/// day either of those gets a real threaded location, this assertion goes
/// red instead of drifting silently.
fn assert_pinned_none(entry: &LineEntry, source: &str) {
    let entry_text = plain_text(entry);
    assert_eq!(
        describe(entry_text, entry.source_location.as_ref(), source),
        describe(entry_text, None, source),
        "expected the pinned None for {entry_text:?} (issue #3213) — if this now fails because \
         a real location appeared, that is the intentional day tag/string-literal provenance \
         gets fixed (see this file's module doc) — update the pin, don't just silence it"
    );
}

/// The granularity-contract invariant (`docs/intl-spec.md`): every entry
/// with a real `source_location` must have that range *contain* the raw
/// source position of its own text — never a range that misattributes a
/// fragment to a location it doesn't overlap (PR #3202 review finding 2's
/// exact shape: the choice-composition regression stamped the *first*
/// region's range on fragments belonging to the *second* region).
///
/// Applies only where the entry's own (trimmed) text is independently
/// findable, byte-for-byte, somewhere in the raw source: a *literal*
/// flattened fragment (choice text, tag text — always verbatim CST text)
/// always is. A *lifted-inline-conditional* branch's resolved text
/// (`"Ready high now."`) legitimately is **not** — the raw source only
/// ever contains the unresolved `"Ready {h: high|low} now."`, so there is
/// no raw position to check containment against, and this invariant is
/// correctly silent there (those lines get their own byte-exact assertion
/// above instead, which the resolved text can't literally satisfy either
/// way). A fragment whose text happens not to appear anywhere in source at
/// all is likewise skipped as inapplicable, not treated as a pass.
fn assert_recorded_range_contains_its_source(story: &StoryData, source: &str) {
    for lt in &story.line_tables {
        for entry in &lt.lines {
            let Some(loc) = entry.source_location.as_ref() else {
                continue;
            };
            let LineContent::Plain(text) = &entry.content else {
                continue;
            };
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let occurrences: Vec<(usize, usize)> = source
                .match_indices(trimmed)
                .map(|(pos, m)| (pos, pos + m.len()))
                .collect();
            if occurrences.is_empty() {
                // Not literally present in source (a lifted/resolved
                // branch line) — inapplicable, not a pass.
                continue;
            }
            let start = usize::try_from(loc.range_start).expect("range_start fits usize");
            let end = usize::try_from(loc.range_end).expect("range_end fits usize");
            let contained = occurrences
                .iter()
                .any(|&(raw_start, raw_end)| start <= raw_start && raw_end <= end);
            assert!(
                contained,
                "granularity contract violated (issue #3213): recorded range {start}..{end} \
                 ({:?}) does not contain any raw source occurrence of its own text {trimmed:?} \
                 (occurrences at {occurrences:?})",
                source.get(start..end).unwrap_or("<out of bounds>"),
            );
        }
    }
}

// ─── `.ink` surface ─────────────────────────────────────────────────

#[test]
fn ink_recognized_content_line_gets_byte_exact_location() {
    let src = ink_fixture_source();
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.clone())).expect("compile");
    assert_recorded_range_contains_its_source(&output.data, &src);

    // The recognized-line path's provenance covers the whole source line
    // including its terminating newline (the CST content-line node's own
    // span) — not just the trimmed text the line table stores.
    for entry in find_plain(&output.data, "Ready?") {
        assert_byte_exact(entry, &src, "story.ink", "Ready?\n");
    }
}

#[test]
fn ink_lifted_inline_conditional_covers_the_whole_line() {
    let src = ink_fixture_source();
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.clone())).expect("compile");
    assert_recorded_range_contains_its_source(&output.data, &src);

    // The #3202 regression shape, verbatim: a top-level content line
    // mixing plain text with an inline conditional gets lifted into one
    // Stmt per branch, and each branch's recorded location must cover the
    // *whole* source line "Ready {h: high|low} now." — not just its own
    // branch text (" high" / "low").
    // Includes the trailing newline — see the recognized-line test's note;
    // `.ink`'s content-line CST node spans through it.
    let whole_line = "Ready {h: high|low} now.\n";
    for entry in find_plain(&output.data, "Ready high now.") {
        assert_byte_exact(entry, &src, "story.ink", whole_line);
    }
    for entry in find_plain(&output.data, "Ready low now.") {
        assert_byte_exact(entry, &src, "story.ink", whole_line);
    }
}

#[test]
fn ink_choice_display_and_output_text_cover_the_whole_choice_region() {
    let src = ink_fixture_source();
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.clone())).expect("compile");
    assert_recorded_range_contains_its_source(&output.data, &src);

    // No bracket/inner content, so the start content's leading and
    // trailing flattened fragments carry this same whole-region location
    // for both the display use (`combine_choice_content`, pre-selection)
    // and the output use (`ChoiceOutput`, post-selection) — see this
    // file's module doc.
    let whole_region = "Roll the dice: {h: high|low} result!";
    for entry in find_plain(&output.data, "Roll the dice:") {
        assert_byte_exact(entry, &src, "story.ink", whole_region);
    }
    for entry in find_plain(&output.data, "result!") {
        assert_byte_exact(entry, &src, "story.ink", whole_region);
    }
}

#[test]
fn ink_tag_entry_source_location_is_pinned_none() {
    let src = ink_fixture_source();
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.clone())).expect("compile");
    assert_recorded_range_contains_its_source(&output.data, &src);

    for entry in find_plain(&output.data, "mood: calm") {
        assert_pinned_none(entry, &src);
    }
}

#[test]
fn ink_string_literal_interpolation_source_location_is_pinned_none() {
    let src = ink_fixture_source();
    let output = brink_compiler::compile("story.ink", |_p| Ok(src.clone())).expect("compile");
    assert_recorded_range_contains_its_source(&output.data, &src);

    for entry in find_plain(&output.data, "Hi ") {
        assert_pinned_none(entry, &src);
    }
    for entry in find_plain(&output.data, " there") {
        assert_pinned_none(entry, &src);
    }
}

// ─── `.brink` surface ───────────────────────────────────────────────

fn compile_brink(src: &str) -> brink_compiler::CompileOutput {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "brink-issue-3213-golden-{}-{unique}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(dir.join("main.brink"), src).expect("write fixture");
    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();
    result.expect("fixture should compile")
}

#[test]
fn brink_recognized_content_line_gets_byte_exact_location() {
    let src = brink_fixture_source();
    let output = compile_brink(&src);
    assert_recorded_range_contains_its_source(&output.data, &src);

    for entry in find_plain(&output.data, "Line one.") {
        assert_byte_exact(entry, &src, "main.brink", "Line one.");
    }
}

#[test]
fn brink_lifted_inline_conditional_covers_the_whole_line() {
    let src = brink_fixture_source();
    let output = compile_brink(&src);
    assert_recorded_range_contains_its_source(&output.data, &src);

    let whole_line = "Ready {if h { high } else { low }} now.";
    for entry in find_plain(&output.data, "Ready high now.") {
        assert_byte_exact(entry, &src, "main.brink", whole_line);
    }
    for entry in find_plain(&output.data, "Ready low now.") {
        assert_byte_exact(entry, &src, "main.brink", whole_line);
    }
}

#[test]
fn brink_choice_display_and_output_text_cover_the_whole_choice_region() {
    let src = brink_fixture_source();
    let output = compile_brink(&src);
    assert_recorded_range_contains_its_source(&output.data, &src);

    // Unlike `.ink` (whose choice's trailing divert is a CST sibling of
    // its content region, out of `hir::Content::ptr`'s range), native's
    // `CHOICE_START_CONTENT` node encloses the trailing `-> END` too — so
    // the real, byte-exact region genuinely extends through it (see
    // `issue_3181_flattened_content_source_location.rs`'s matching note).
    let whole_region = "Roll the dice: {if h { high } else { low }} result! -> END";
    for entry in find_plain(&output.data, "Roll the dice:") {
        assert_byte_exact(entry, &src, "main.brink", whole_region);
    }
    for entry in find_plain(&output.data, "result!") {
        assert_byte_exact(entry, &src, "main.brink", whole_region);
    }
}

#[test]
fn brink_tag_entry_source_location_is_pinned_none() {
    let src = brink_fixture_source();
    let output = compile_brink(&src);
    assert_recorded_range_contains_its_source(&output.data, &src);

    for entry in find_plain(&output.data, "mood: calm") {
        assert_pinned_none(entry, &src);
    }
}

#[test]
fn brink_string_literal_interpolation_source_location_is_pinned_none() {
    let src = brink_fixture_source();
    let output = compile_brink(&src);
    assert_recorded_range_contains_its_source(&output.data, &src);

    for entry in find_plain(&output.data, "Hi ") {
        assert_pinned_none(entry, &src);
    }
    for entry in find_plain(&output.data, " there") {
        assert_pinned_none(entry, &src);
    }
}

#[test]
fn brink_file_path_is_recorded_as_the_native_file_name() {
    // Sanity check for `expected_file` used throughout: `compile_path`
    // records the file relative to the project, not an absolute temp
    // path, matching `Path::new("main.brink").to_string_lossy()`.
    let src = brink_fixture_source();
    let output = compile_brink(&src);
    let entries = find_plain(&output.data, "Line one.");
    let expected_file = Path::new("main.brink").to_string_lossy().into_owned();
    for entry in entries {
        let loc = entry.source_location.as_ref().expect("has a location");
        assert_eq!(loc.file, expected_file);
    }
}
