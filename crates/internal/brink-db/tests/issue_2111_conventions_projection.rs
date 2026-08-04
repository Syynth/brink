//! Issue #2111 (NS-T seam 1/6) — the serialized conventions projection
//! query and its invalidation. Exercised end-to-end through `ProjectDb`'s
//! salsa query layer, the same level `issue_1844_conventions_module_fence.rs`
//! tests the sibling `E169` confinement check at (and this suite reuses that
//! file's resolution fixtures deliberately, to prove the same pointer
//! resolves identically for both consumers).
//!
//! Per `ConventionsProjection`'s own doc (`brink-ir`), there is no
//! comptime-fault / last-good-value case to test here: the mechanism that
//! would have needed one (`fn conventions()` registration, issue #1840) is
//! dissolved (`docs/decision-log.md` 2026-08-03). What remains testable —
//! and is tested below — is the query SHAPE, its KEYING against the
//! `[project] conventions` pointer, and its INVALIDATION footprint (exactly
//! the resolved conventions module file, nothing else).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::ConventionMode;

const CLAIMING_HANDLER: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
    fn interior(place: content) {\n  return place;\n}\n";

const BLOCK_CLAIMING_HANDLER: &str = "struct Cue {\n  speaker: string,\n}\n\
    @[convention(claims = \"^(?<name>[A-Z]+)$\", order = 5, block, attach = Cue)]\n\
    fn cue(name: string, body: content): Cue {\n  return Cue { speaker: name };\n}\n";

fn opts_with_conventions(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        conventions: Some(pointer.to_owned()),
        ..AnalysisOptions::default()
    }
}

#[test]
fn unset_conventions_projects_to_empty() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(AnalysisOptions::default());
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

#[test]
fn a_preset_name_pointer_projects_to_empty_for_now() {
    // Mirrors `a_preset_name_pointer_never_fires` in the E169 confinement
    // suite: nothing resolves a bare preset name to its mounted source yet
    // (`brink_analyzer`'s own `BUILTIN_ELEMENT_PRESETS` doc — needs #1582's
    // pub marker and #2167's closure-scoped confinement, neither built).
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("screenplay"));
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

#[test]
fn an_unresolvable_conventions_pointer_projects_to_empty() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("typo.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

#[test]
fn the_configured_modules_own_handlers_are_projected_in_order() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{BLOCK_CLAIMING_HANDLER}{CLAIMING_HANDLER}"),
    );
    let projection = db.conventions_projection();
    let names: Vec<&str> = projection
        .entries
        .iter()
        .map(|e| e.name.text.as_str())
        .collect();
    // `cue` (order 5) before `interior` (order 10) — ascending order, not
    // declaration position (`cue` is declared second in the source above).
    assert_eq!(names, vec!["cue", "interior"], "{projection:?}");
    assert_eq!(projection.entries[0].mode, ConventionMode::Wrap);
    assert_eq!(projection.entries[0].attach.as_deref(), Some("Cue"));
    assert_eq!(projection.entries[1].mode, ConventionMode::Attach);
    assert_eq!(projection.entries[1].attach, None);
}

/// A claiming handler declared OUTSIDE the configured conventions module
/// must never appear in the projection — the module is the SOLE source of
/// active claiming handlers (`docs/decision-log.md` 2026-08-03 "subtraction"
/// ruling), mirroring `E169` confinement's own "only the configured file's
/// declarations count" posture.
#[test]
fn a_claiming_handler_outside_the_configured_module_is_never_projected() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    assert!(db.conventions_projection().entries.is_empty());
}

/// The invalidation contract (`docs/decision-log.md` 2026-08-01 "Match
/// overlap… the projection is cached on its closure"): editing a file that
/// is NOT the conventions module must never re-execute the projection
/// query's closure. `Arc::ptr_eq`, not value equality, is the assertion —
/// see `fg3_dependency_edges.rs`'s own doc for why pointer identity is what
/// proves a memo was fully validated rather than recomputed.
#[test]
fn editing_an_unrelated_file_never_reexecutes_the_projection_query() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow other() {{\n  hi\n}}\n"),
    );
    db.set_file(
        "scenes/heading.brink",
        "flow main() {\n  A plain narrative line.\n}\n".to_owned(),
    );

    let before = db.conventions_projection();
    db.update_file(
        "scenes/heading.brink",
        "flow main() {\n  A DIFFERENT plain narrative line.\n}\n".to_owned(),
    );
    let after = db.conventions_projection();

    assert!(
        Arc::ptr_eq(&before, &after),
        "editing an unrelated file re-executed the conventions projection query (issue #2111)"
    );
}

/// The other half of the same contract: editing the conventions module
/// ITSELF must re-evaluate the projection — a real content change, not just
/// a re-execution with the same output.
#[test]
fn editing_the_conventions_module_itself_updates_the_projection() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow other() {{\n  hi\n}}\n"),
    );
    assert_eq!(db.conventions_projection().entries.len(), 1);

    db.update_file(
        "conventions.brink",
        format!("{BLOCK_CLAIMING_HANDLER}{CLAIMING_HANDLER}flow other() {{\n  hi\n}}\n"),
    );
    assert_eq!(db.conventions_projection().entries.len(), 2);
}
