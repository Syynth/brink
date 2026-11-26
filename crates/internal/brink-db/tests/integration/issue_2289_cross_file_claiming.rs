//! Issue #2289 (2026-08-05 ruling): conventions claiming reaches the WHOLE
//! PROJECT, not just the file that declares the handler — "it's never file
//! local. you configure conventions for a project, that's why they're
//! conventions and not 'local patterns'."
//!
//! Deliberately multi-file. `tests/tier1-native/conventions-screenplay-preset`
//! is a single file by construction (its own header says it inlines the
//! handlers because single-file dispatch is what was landed before this
//! issue) — it structurally cannot express "claimed in one file, not
//! another", which is exactly why the file-local defect survived a green
//! corpus. Every test here uses at least two files: a conventions module
//! plus one or more separate files whose prose it must claim.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode, ElementKind, Severity};

fn opts_with_conventions(pointer: &str) -> AnalysisOptions {
    AnalysisOptions {
        conventions: Some(pointer.to_owned()),
        ..AnalysisOptions::default()
    }
}

/// The project must compile clean — not merely "no `E169`" (that alone lets
/// the cross-module gate's `E087`/`E025` straight through unnoticed, which is
/// exactly the review finding this asserts against: an injected handler in
/// another module used to make the compile itself fail). Every test in this
/// file that lowers a project through [`ProjectDb::analysis`] asserts this
/// instead of a single-code exclusion.
fn assert_no_error_diagnostics(diags: &[Diagnostic]) {
    let errors: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| d.code.severity() == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics: {errors:?}"
    );
}

const CUE_CONVENTIONS_MODULE: &str = "@[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", order = 10)]\n\
     fn cue(name: string) {\n  return name;\n}\n";

/// The exact defect the ruling names: a claiming handler declared in the
/// project's configured conventions module must claim a line in a
/// DIFFERENT file, not just its own.
#[test]
fn a_handler_in_the_conventions_module_claims_a_line_in_another_file() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", CUE_CONVENTIONS_MODULE.to_owned());
    let story_id = db.set_file(
        "story.brink",
        "flow main() {\n  VENDOR\n  You shouldn't be here after dark.\n}\n".to_owned(),
    );

    let hir = db.hir(story_id).expect("story.brink should lower");
    assert_eq!(
        hir.element_matches.len(),
        1,
        "expected VENDOR to be claimed by the conventions module's `cue` \
         handler: {:?}",
        hir.element_matches
    );
    assert_eq!(hir.element_matches[0].handler.text, "cue");
    assert_eq!(hir.element_matches[0].kind, ElementKind::ContentLine);
    // No confinement diagnostic against the CLAIMING file — the handler is
    // declared in the configured module, it just claims elsewhere. And the
    // project must actually COMPILE: the rewritten call resolves into
    // `conventions.brink`, a different module than `story.brink` — asserting
    // only the absence of `E169` lets the M-2 cross-module gate's `E087`
    // (private-by-default) or `E025` (unimported-public) straight through
    // unnoticed.
    let diags = db.analysis().diagnostics.clone();
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E169),
        "{diags:?}"
    );
    assert_no_error_diagnostics(&diags);
}

/// Reach is WHOLE-PROJECT, not scoped to the conventions module's `IMPORT`
/// closure (the #2167 framing the ruling explicitly says not to read as the
/// spec here) — a file with no `use`/`IMPORT` relationship to the
/// conventions module at all must still have its prose claimed.
#[test]
fn reach_is_whole_project_not_just_the_import_closure() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", CUE_CONVENTIONS_MODULE.to_owned());
    // No `use` of the conventions module anywhere — an ordinary sibling file.
    let barter_id = db.set_file(
        "market/barter.brink",
        "flow haggle() {\n  KID\n  How much for the lantern?\n}\n".to_owned(),
    );

    let hir = db.hir(barter_id).expect("market/barter.brink should lower");
    assert_eq!(
        hir.element_matches.len(),
        1,
        "expected KID to be claimed even with no import relationship to the \
         conventions module: {:?}",
        hir.element_matches
    );
    assert_eq!(hir.element_matches[0].handler.text, "cue");
    // No `use`/`IMPORT` anywhere in this project — if the injected call were
    // gated as an ordinary cross-module reference, this would be exactly the
    // unimported-public (`E025`) or private-by-default (`E087`) case.
    assert_no_error_diagnostics(&db.analysis().diagnostics);
}

/// Two sibling files, neither importing the other nor the conventions
/// module — both must be claimed independently. Proves the merged handler
/// set is genuinely per-file-independent project-wide state, not an
/// accidental one-shot side effect of lowering whichever file happens to be
/// read first.
#[test]
fn multiple_independent_sibling_files_are_each_claimed() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", CUE_CONVENTIONS_MODULE.to_owned());
    let a_id = db.set_file(
        "scenes/a.brink",
        "flow a() {\n  VENDOR\n  Hello.\n}\n".to_owned(),
    );
    let b_id = db.set_file("scenes/b.brink", "flow b() {\n  KID\n  Hi.\n}\n".to_owned());

    let hir_a = db.hir(a_id).expect("scenes/a.brink should lower");
    let hir_b = db.hir(b_id).expect("scenes/b.brink should lower");
    assert_eq!(
        hir_a.element_matches.len(),
        1,
        "{:?}",
        hir_a.element_matches
    );
    assert_eq!(
        hir_b.element_matches.len(),
        1,
        "{:?}",
        hir_b.element_matches
    );
    assert_no_error_diagnostics(&db.analysis().diagnostics);
}

/// A line that does not match the conventions module's pattern stays
/// ordinary, unclaimed prose in another file too — cross-file reach must
/// not turn every line into a call.
#[test]
fn a_non_matching_line_in_another_file_stays_unclaimed() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    db.set_file("conventions.brink", CUE_CONVENTIONS_MODULE.to_owned());
    let story_id = db.set_file(
        "story.brink",
        "flow main() {\n  just some lowercase prose\n}\n".to_owned(),
    );

    let hir = db.hir(story_id).expect("story.brink should lower");
    assert!(
        hir.element_matches.is_empty(),
        "lowercase prose should not match the `cue` handler's pattern: {:?}",
        hir.element_matches
    );
    assert_no_error_diagnostics(&db.analysis().diagnostics);
}

/// The conventions module's own file is excluded from its own injected set
/// — it already claims via its local declarations (the pre-#2289
/// behavior), and double-injecting would be redundant, not merely harmless.
#[test]
fn the_conventions_module_still_claims_within_its_own_file() {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts_with_conventions("conventions.brink"));
    let conventions_src = format!("{CUE_CONVENTIONS_MODULE}flow demo() {{\n  VENDOR\n  Hi.\n}}\n");
    let conventions_id = db.set_file("conventions.brink", conventions_src);

    let hir = db
        .hir(conventions_id)
        .expect("conventions.brink should lower");
    assert_eq!(hir.element_matches.len(), 1, "{:?}", hir.element_matches);
    assert_eq!(hir.element_matches[0].handler.text, "cue");
    assert_no_error_diagnostics(&db.analysis().diagnostics);
}
