//! The prose block elements the native grammar gained in issue #1715
//! (`docs/prose-dialect-spec.md` §8b/§8d) parse cleanly but have **no HIR
//! lowering yet** — attachment and the conventions `lower:` column are
//! issue #1717's slice, and the per-flow tag API is #474's.
//!
//! That staging is only defensible if the gap is *loud*. This file is the
//! guard: every shape the grammar now recognizes must reach a diagnostic
//! rather than being lowered as ordinary prose or dropped on the floor
//! (`lower_native`'s standing posture — "every such construct is a loud
//! diagnostic, never a silent drop" — and CLAUDE.md's "flag silent data
//! drops" rule).
//!
//! An integration test rather than an in-`lib` unit test for the same
//! reason `b07_native_body.rs` gives: `brink-analyzer` is a dev-dependency
//! that itself depends on `brink-ir`.

use brink_ir::hir::lower_native;
use brink_ir::{DiagnosticCode, FileId};

fn lower(src: &str) -> Vec<brink_ir::Diagnostic> {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (_, _, diags) = lower_native::lower(FileId(0), &parse.tree());
    diags
}

fn e129_count(diags: &[brink_ir::Diagnostic]) -> usize {
    diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E129)
        .count()
}

#[test]
fn a_header_scoped_scene_stitch_is_reported_not_lowered_as_prose() {
    let diags = lower("INT. MARKET SQUARE - NIGHT [market]\nThe square is empty.\n");
    assert!(
        e129_count(&diags) >= 1,
        "a scene heading must be reported as not-yet-lowered, got {diags:?}"
    );
}

#[test]
fn a_block_cue_and_its_parenthetical_are_reported_not_lowered_as_prose() {
    let diags =
        lower("flow scene() {\n  @VENDOR #(v.o.)\n  (hushed)\n  You shouldn't be here.\n}\n");
    assert!(
        e129_count(&diags) >= 2,
        "the cue and the parenthetical must each be reported, got {diags:?}"
    );
}

#[test]
fn a_compact_cue_is_reported_not_lowered_as_prose() {
    let diags = lower("flow scene() {\n  @KID: Says who?\n}\n");
    assert!(
        e129_count(&diags) >= 1,
        "the compact cue must be reported as not-yet-lowered, got {diags:?}"
    );
}

#[test]
fn per_flow_header_tags_are_reported_on_a_knot() {
    // §8b.4's `flow x #tag { … }` — no `Knot` field receives these, so
    // each tag is reported rather than dropped (#474 owns the API).
    let diags = lower("flow market #act1 #tense {\n  Hi.\n}\n");
    assert_eq!(
        e129_count(&diags),
        2,
        "one diagnostic per unlowered header tag, got {diags:?}"
    );
}

#[test]
fn per_flow_header_tags_are_reported_on_a_stitch_too() {
    // The structurally parallel sibling: a nested `flow` is a `Stitch`,
    // and `Stitch` has no tags field either.
    let diags = lower("flow market() {\n  flow stall #act1 {\n    Hi.\n  }\n}\n");
    assert_eq!(
        e129_count(&diags),
        1,
        "a stitch header's tag is reported too, got {diags:?}"
    );
}

#[test]
fn an_untagged_flow_still_lowers_clean() {
    // The baseline the reports above must not disturb.
    let diags = lower("flow market() {\n  Hi.\n}\n");
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}
