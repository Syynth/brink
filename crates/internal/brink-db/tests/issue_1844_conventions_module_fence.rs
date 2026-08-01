//! Issue #1844 — the MODULE half of the 2026-07-31 §9.1 confinement ruling
//! (`docs/decision-log.md` "Conventions are annotated handlers", item 4):
//! a pattern-claiming `@[element(claims = "…")]` handler is legal only in
//! the project's configured conventions module (`brink.toml`'s `[project]
//! elements`). Exercised end-to-end through `ProjectDb`'s salsa query layer
//! — the same `db.analysis()` path `brink compile`/`brink check` and
//! `@brink-lang/web` run (the `t2_2_effects_assertions.rs` precedent for
//! why this is the right test level for a project-config-gated diagnostic).
//!
//! `#1838`/`#1847` cover the sibling *placement* half (`E112`): a claiming
//! `fn` must be a top-level declaration in its own file. This suite is
//! deliberately silent about that half — every fixture here already
//! satisfies it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

const CLAIMING_HANDLER: &str = "@[element(claims = \"^INT\\\\. (?<place>.+)$\")]\n\
    fn interior(place: content) {\n  return place;\n}\n";

fn opts_with_elements(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        elements: Some(pointer.to_owned()),
        ..AnalysisOptions::default()
    }
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn claim_handler_in_the_configured_module_is_silent() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_elements("conventions.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}

#[test]
fn claim_handler_outside_the_configured_module_is_e169() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_elements("conventions.brink"));
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E169], "{diags:?}");
    assert!(diags[0].message.contains("interior"), "{diags:?}");
    assert!(diags[0].message.contains("conventions.brink"), "{diags:?}");
}

/// Two files, only one of which is the configured module — the file that
/// *is* `conventions.brink` stays silent, the sibling that isn't gets
/// flagged. Proves the check is genuinely per-file module identity, not
/// "any claim handler anywhere trips it" or "the first file registered is
/// exempt".
#[test]
fn only_the_non_configured_file_is_flagged_among_siblings() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_elements("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "@[element(claims = \"^A$\")]\nfn a() {\n  return \"a\";\n}\n\
         flow main() {\n  hi\n}\n"
            .to_owned(),
    );
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow scene_flow() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E169], "{diags:?}");
}

/// Unset `elements` means no conventions module is configured at all —
/// nothing to confine claiming to, so every project without this key opted
/// in stays exactly as permissive as it was before `E169` existed.
#[test]
fn unset_elements_never_fires() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(AnalysisOptions::default());
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}

/// A bare preset name (`elements = "screenplay"`) points at a built-in
/// `std::conventions::*` module, not a project file — there is no project
/// path to compare a claiming handler's own module against, so this stays
/// silent rather than flagging every claiming handler in the project.
#[test]
fn a_preset_name_pointer_never_fires() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_elements("screenplay"));
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}

/// A nested-path conventions module (`scenes/conventions.brink`) resolves
/// via the same root-relative module identity as any other native file —
/// not just a bare filename at the project root.
#[test]
fn a_nested_conventions_module_path_resolves_correctly() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_elements("scenes/conventions.brink"));
    db.set_file(
        "scenes/conventions.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}

/// A typo'd `elements` pointer (`"typo.brink"`, no such file in the
/// project) must never flag the real `conventions.brink`'s own claiming
/// handler — nor anyone else's. Before the fix, `expected_module` was
/// compared against every file's module WITHOUT ever checking that some
/// file in the project actually resolves to it, so a typo'd/moved/deleted
/// pointer flagged every claiming handler in the project — including the
/// one in the real conventions module — telling the author to move it into
/// a file that does not exist.
#[test]
fn an_unresolvable_elements_pointer_never_fires_even_against_the_real_module() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_elements("typo.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}
