//! Issue #1844 — the MODULE half of the 2026-07-31 §9.1 confinement ruling
//! (`docs/decision-log.md` "Conventions are annotated handlers", item 4):
//! a pattern-claiming `@[convention(claims = "…", order = N)]` handler is legal only in
//! the project's configured conventions module (`brink.toml`'s `[project]
//! conventions`, renamed from `elements` by issue #2180). Exercised
//! end-to-end through `ProjectDb`'s salsa query layer
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

const CLAIMING_HANDLER: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
    fn interior(place: content) {\n  return place;\n}\n";

fn opts_with_conventions(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        conventions: Some(pointer.to_owned()),
        ..AnalysisOptions::default()
    }
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn claim_handler_in_the_configured_module_is_silent() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
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
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    // `conventions.brink` must actually exist in the project — an
    // `conventions` pointer that resolves to no real file is a *different*
    // silent case (see `an_unresolvable_conventions_pointer_never_fires_
    // even_against_the_real_module` below), not this one, which is
    // specifically "the configured module exists, but this handler isn't
    // in it".
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
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
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file(
        "conventions.brink",
        "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\
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

/// Unset `conventions` means no conventions module is configured at all —
/// issue #2289 part 2 (2026-08-05 ruling) corrects this from a silent pass
/// to `E169`: a declared claim handler names no module to belong to, which
/// is a misconfiguration, not an opt-out. (Superseded
/// `unset_conventions_never_fires`, which asserted the pre-#2289 silent
/// behavior this ruling reverses.)
#[test]
fn unset_conventions_is_e169() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(AnalysisOptions::default());
    db.set_file(
        "scenes/heading.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E169], "{diags:?}");
    assert!(diags[0].message.contains("interior"), "{diags:?}");
    assert!(
        diags[0]
            .message
            .contains("no conventions module is configured"),
        "{diags:?}"
    );
}

/// A file mounted under the reserved `std/` peer root is exempt from
/// confinement entirely, even with `conventions` unset (issue #2289, found
/// while implementing `unset_conventions_is_e169` above): `brink-environment`
/// unconditionally mounts `std::conventions::screenplay` into every
/// compiled project's file set (issue #2080), so without this exemption
/// EVERY project with no `conventions` key configured would suddenly fail
/// to compile — the mounted preset's own declared handlers would trip the
/// new unconfigured-`E169` above. This test proves the exemption directly
/// (a `.brink` file keyed under `std/`, without going through the real
/// `brink-environment` mounting machinery) rather than only through the
/// `brink-cli` end-to-end repro that first found the regression.
#[test]
fn a_file_mounted_under_the_std_root_is_exempt_even_when_unconfigured() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(AnalysisOptions::default());
    db.set_file(
        "std/conventions/screenplay.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}

/// The same exemption also covers the pre-existing "outside the configured
/// module" half (`E169`) — a project that DOES configure `conventions` to
/// one of its own files must not also flag the std-mounted preset's own
/// handlers as misplaced.
#[test]
fn a_file_mounted_under_the_std_root_is_exempt_even_when_a_different_module_is_configured() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", "flow other() {\n  hi\n}\n".to_owned());
    db.set_file(
        "std/conventions/screenplay.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
}

/// A bare preset name (`conventions = "screenplay"`) points at a built-in
/// `std::conventions::*` module, not a project file — there is no project
/// path to compare a claiming handler's own module against, so this stays
/// silent rather than flagging every claiming handler in the project.
#[test]
fn a_preset_name_pointer_never_fires() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("screenplay"));
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
    db.set_analysis_options(opts_with_conventions("scenes/conventions.brink"));
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

/// A typo'd `conventions` pointer (`"typo.brink"`, no such file in the
/// project) must never tell the author to move the real `conventions.
/// brink`'s own claiming handler — or anyone else's — into a file that
/// does not exist. Originally (pre-#1844-fix) `expected_module` was
/// compared against every file's module WITHOUT ever checking that some
/// file in the project actually resolves to it, so a typo'd/moved/deleted
/// pointer misused `conventions_module_diagnostics`'s "move it there"
/// message against every claiming handler in the project.
///
/// Superseded `an_unresolvable_conventions_pointer_never_fires_even_
/// against_the_real_module`, which asserted this scenario produced NO
/// diagnostics at all — issue #2320 corrects that: a `tracing::warn!` was
/// the only signal an unresolvable pointer ever produced, invisible to
/// every wasm consumer (`brink-web`'s `EditorSession`) since no `tracing`
/// subscriber exists there. Confinement is still correctly skipped (no
/// file exists to check against), but the misconfiguration itself is now
/// a real, visible `E169` — with a message that blames the pointer, not a
/// nonexistent destination.
#[test]
fn an_unresolvable_conventions_pointer_is_e169_naming_the_pointer_not_a_destination() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("typo.brink"));
    db.set_file(
        "conventions.brink",
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E169], "{diags:?}");
    assert!(
        diags[0].message.contains("does not match any file"),
        "{diags:?}"
    );
    assert!(
        !diags[0].message.contains("may only be declared"),
        "must not use the \"move it there\" message — there is no correct \
         destination to name — got {diags:?}"
    );
}
