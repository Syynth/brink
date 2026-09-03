//! Issue #3365, Part 1: `E195` — a `*`/`+` choice with neither
//! display/bracket text nor a divert, matching inklecate's own "Choice is
//! completely empty" warning (`InkParser/InkParser_Choices.cs:84-86`; line
//! 90 guards a different warning — "Blank choice", on the
//! `* [] some text` shape — which this code deliberately does not cover).
//!
//! Exercises the real production entry point
//! (`brink_ir::hir::lower::choice::LowerChoice::lower_choice`, called
//! through `hir::lower`) over real `.ink` source parsed by `brink_syntax` —
//! not a hand-built `Choice` fixture — so deleting the fix in
//! `crates/internal/brink-ir/src/hir/lower/choice.rs` turns every `fires`
//! case in this file red.

use brink_ir::DiagnosticCode;
use brink_ir::hir::FileId;

/// The diagnostic codes `hir::lower` produced for `src`, parsed as ink
/// source at file id 0.
fn lowering_codes(src: &str) -> Vec<DiagnosticCode> {
    let parsed = brink_syntax::parse(src);
    let tree = parsed.tree();
    let (_hir, _manifest, diags) = brink_ir::hir::lower(FileId(0), &tree);
    diags.into_iter().map(|d| d.code).collect()
}

fn assert_fires(name: &str, src: &str) {
    let codes = lowering_codes(src);
    assert!(
        codes.contains(&DiagnosticCode::E195),
        "{name}: expected E195 to fire for {src:?}, got {codes:?}"
    );
}

fn assert_does_not_fire(name: &str, src: &str) {
    let codes = lowering_codes(src);
    assert!(
        !codes.contains(&DiagnosticCode::E195),
        "{name}: expected E195 not to fire for {src:?}, got {codes:?}"
    );
}

// ─── Fires: the issue's own repro and its sibling shapes ──────────────

#[test]
fn fires_on_empty_bracket_with_nested_body() {
    // The issue's own repro (#3365).
    assert_fires(
        "empty_bracket_with_nested_body",
        "* []\n    Fallthrough body.\n- -> END\n",
    );
}

#[test]
fn fires_on_bare_star_with_nothing_at_all() {
    assert_fires("bare_star_truly_empty", "*\n- -> END\n");
}

#[test]
fn fires_on_bare_plus_with_nothing_at_all() {
    // The `+` (sticky) form is named explicitly in the issue.
    assert_fires("bare_plus_truly_empty", "+\n- -> END\n");
}

#[test]
fn fires_on_empty_bracket_even_when_body_has_its_own_divert() {
    // The nested body's own divert must not count — inklecate's own check
    // only looks at the choice's own line (docs/diagnostics/E195.md).
    assert_fires("empty_bracket_body_divert", "* []\n    -> END\n");
}

#[test]
fn fires_on_second_choice_in_a_mixed_set() {
    let codes = lowering_codes("* One\n* []\n- -> END\n");
    assert!(
        codes.contains(&DiagnosticCode::E195),
        "expected E195 for the set's second (empty) choice, got {codes:?}"
    );
}

// A `(label)` or `{condition}` guard does NOT exempt a choice from this
// check — the reference's own `emptyContent` computation has no such
// carve-out, and measurement against inklecate confirms it fires anyway
// for both shapes below. The blank line before the gather matters: an
// indented body line right after the label is absorbed into the choice's
// own `CHOICE_START_CONTENT` by a parser quirk (the newline-after-label
// bump in `parser/choice.rs`, and the matching bump in the
// condition-continuation loop), which would give the choice real text and
// make it fail to reach this check at all — a prior version of this file
// used exactly that shape and passed for the wrong reason (review finding,
// PR #3473): reverting the fires-check's `label.is_none() &&
// condition.is_none()` guards left it green.
#[test]
fn fires_with_a_label_and_no_other_exemption() {
    assert_fires("labeled_choice", "* (opt)\n\n- -> END\n");
}

#[test]
fn fires_with_a_condition_and_no_other_exemption() {
    assert_fires("conditioned_choice", "VAR x = true\n* {x}\n\n- -> END\n");
}

// ─── Does not fire: tag, divert, text ──────────────────────────────────

#[test]
fn does_not_fire_with_a_tag_only() {
    // `self.all_tags()` (not each content region's own `.tags`) is what
    // catches this: a tag directly on the choice line, with no preceding
    // content region node, has nowhere else to be attributed (review
    // finding, PR #3473) — matching inklecate's own silence on the same
    // shape (measured).
    assert_does_not_fire("tag_only_choice", "* #tag\n- -> END\n");
}

#[test]
fn does_not_fire_with_an_empty_divert() {
    // `* ->` is inklecate's own documented fix for this warning — a divert
    // token with no target still counts as "has a divert".
    assert_does_not_fire(
        "empty_divert_choice",
        "* ->\n    Fallthrough body.\n- -> END\n",
    );
}

#[test]
fn does_not_fire_with_start_text() {
    assert_does_not_fire("start_text_choice", "* Hello\n- -> END\n");
}

#[test]
fn does_not_fire_with_bracket_text() {
    assert_does_not_fire("bracket_text_choice", "* [Continue]\n- -> END\n");
}

// ─── Does not co-opt E034's own territory ──────────────────────────────
//
// E034 ("choice set has only fallback choices") is a `brink-analyzer`
// pass over already-lowered HIR (`validate::validate`), not a lowering-time
// diagnostic — `hir::lower` alone never produces it, so the interaction
// with E195 is exercised through the full compile pipeline instead, in
// `crates/brink-compiler/tests/issue_3365_empty_choice_warning.rs`'s
// `e034_and_e195_can_co_occur_without_interference`.

#[test]
fn does_not_fire_on_a_real_fallback_choice_with_an_empty_divert() {
    // `* ->` (empty divert, no target) is a real, deliberate fallback
    // (`is_fallback` is `true` for it) but still has a divert token, so
    // E195 must stay quiet — this is exactly the shape the message's own
    // hint (`Add a divert arrow … : * ->`) recommends writing.
    assert_does_not_fire("real_fallback_choice", "* ->\n=== a ===\n-> END\n");
}
