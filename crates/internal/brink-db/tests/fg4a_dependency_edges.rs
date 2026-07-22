//! Targeted dependency-edge regression tests for issue #791 (FG-4a,
//! `docs/fine-grained-salsa-proposal.md` — PR #753's seam finding #3):
//! `has_errors_query`, the boolean projection [`brink_db::ProjectDb::lir_product`]'s
//! error gate now reads instead of the full `Vec<Diagnostic>`, and
//! `lir_lowering_query` (the LIR-lowering half split off `lir_query`, gated
//! on that boolean), following the FG-1/FG-2.1/FG-3 pattern
//! (`fg1_dependency_edges.rs`, `fg2_scc_dependency_edges.rs`,
//! `fg3_dependency_edges.rs`).
//!
//! Both `resolutions_index()` and `lir_product()` use `Arc` pointer identity
//! (not value equality) as the non-re-execution assertion — salsa only
//! returns the *exact same* stored `Arc` allocation when a query's memo is
//! fully validated without re-executing its closure. See those files' module
//! docs for the full rationale.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, SemanticTypeDiagnosticSeverity};
use brink_db::{FileDiagnostics, ProjectDb, partition_diagnostics};
use brink_ir::DiagnosticCode;

/// A two-file fixture with a *persistent* Error-severity diagnostic (`E041`:
/// `b.ink` calls `pay` with a string literal where an `int` is declared) that
/// never goes away, plus a second external (`greet`) whose parameter's
/// semantic type (`actor_id`) is unknown with no host manifest registered —
/// `E040` only fires for it once `semantic_type_check` is raised to `Error`
/// (the #339/#527 default-tolerant path). This lets a single
/// `AnalysisOptions` edit add a *second*, independent error without ever
/// flipping "does this project have any error at all".
fn dominant_error_fixture() -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.ink",
        "/// Pay the piper.\n/// @param amount {int}\nEXTERNAL pay(amount)\n\
         /// @param who {actor_id}\nEXTERNAL greet(who)\n\
         === a_knot ===\nA scene line.\n~ pay(1)\n~ greet(1)\n-> END\n"
            .to_owned(),
    );
    db.set_file(
        "b.ink",
        "=== b_knot ===\n~ pay(\"gold\")\n-> END\n".to_owned(),
    );
    db.set_entry("a.ink");
    db
}

/// Issue #791: `has_errors()` must agree with the pre-lowering
/// `errors.is_empty()` gate on a clean project — `lir_product().program` is
/// `Some`, `errors` is empty. Behavior-neutral sanity: the new projection
/// can't disagree with the value it replaces at the gate.
#[test]
fn has_errors_matches_error_gate_when_clean() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", "Hello world.\n-> END\n".to_owned());
    db.set_entry("main.ink");

    let product = db.lir_product().expect("entry set");
    assert!(!db.has_errors(), "clean project must have no errors");
    assert!(
        product.errors.is_empty(),
        "has_errors() disagrees with lir_product().errors: {:?}",
        product.errors
    );
    assert!(
        product.program.is_some(),
        "a clean project must still produce a program"
    );
}

/// Issue #791: `has_errors()` must agree with the pre-lowering gate on a
/// project with a genuine analysis error (`E041`, external argument type
/// mismatch) — `lir_product().program` is `None`, matching the old
/// `errors.is_empty()`-gated behavior exactly.
#[test]
fn has_errors_matches_error_gate_when_dirty() {
    let db = dominant_error_fixture();

    let product = db.lir_product().expect("entry set");
    assert!(db.has_errors(), "fixture must have at least one error");
    assert!(
        product
            .errors
            .iter()
            .any(|d| d.code == DiagnosticCode::E041),
        "fixture must fire E041, or this test is vacuous: {:?}",
        product.errors
    );
    assert!(
        product.program.is_none(),
        "an Error-severity diagnostic must gate program: None"
    );
}

/// The core FG-4a proof (PR #753's seam finding #3): a diagnostics-content
/// edit that adds a brand-new Error-severity diagnostic (`E040`, dominated
/// by the fixture's pre-existing `E041`) must NOT flip `has_errors()`'s
/// verdict — it was `true` before (from `E041` alone) and stays `true` after
/// (now `E041` + `E040`). Because `has_errors_query` never reads
/// `resolutions_index_query`, and this is a pure `AnalysisOptions` edit (no
/// file text touched), `resolutions_index()`'s `Arc` survives byte-for-byte —
/// proving the exact data `lir_lowering_query` would read in its success
/// branch is untouched, even though the diagnostics vector genuinely
/// changed. This is the "chunk memos don't ride the full diagnostic vector's
/// Eq" property FG-4b/c/d build on.
#[test]
fn has_errors_stays_true_across_a_genuine_diagnostics_content_change() {
    let mut db = dominant_error_fixture();

    let before_idx = db.resolutions_index();
    let before_has_errors = db.has_errors();
    let before_product = db.lir_product().expect("entry set").clone();
    assert!(before_has_errors, "fixture must start with an error");
    assert!(
        !before_product
            .errors
            .iter()
            .any(|d| d.code == DiagnosticCode::E040),
        "E040 must not fire yet under the default Tolerant policy: {:?}",
        before_product.errors
    );

    // Raise semantic_type_check to Error — adds E040 for `greet`'s unknown
    // `actor_id` semantic type. A pure AnalysisOptions edit: no file text
    // changes, so this can only affect diagnostics, never resolutions/index
    // (matching fg3_dependency_edges.rs's
    // resolutions_index_survives_diagnostics_only_analysis_options_edit).
    db.set_analysis_options(AnalysisOptions {
        semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
        ..AnalysisOptions::default()
    });

    let after_idx = db.resolutions_index();
    let after_has_errors = db.has_errors();
    let after_product = db.lir_product().expect("entry set").clone();

    // Non-vacuous: the diagnostics vector really did change content.
    assert!(
        after_product
            .errors
            .iter()
            .any(|d| d.code == DiagnosticCode::E040),
        "raising semantic_type_check to Error should add E040: {:?}",
        after_product.errors
    );
    assert!(
        after_product
            .errors
            .iter()
            .any(|d| d.code == DiagnosticCode::E041),
        "the original E041 must still be present: {:?}",
        after_product.errors
    );

    // The verdict has_errors_query gates on did not flip.
    assert_eq!(
        before_has_errors, after_has_errors,
        "has_errors() flipped even though the project had an error both \
         before and after (issue #791 FG-4a)"
    );
    assert!(after_has_errors);

    // The seam property: resolutions/index data is untouched.
    assert!(
        Arc::ptr_eq(&before_idx, &after_idx),
        "a diagnostics-only AnalysisOptions edit re-executed \
         resolutions_index_query (issue #791 FG-4a)"
    );
}

/// Composed-equals-monolithic sanity: `has_errors()` must exactly match
/// manually re-running [`partition_diagnostics`] — the same shared function
/// [`brink_db::ProjectDb::lir_product`]'s own gate uses — over the project's
/// own accessors (`suppressions`, `source`, `file_diagnostics`,
/// `analysis().diagnostics`). `has_errors_query` is a pure re-expression of
/// that computation as its own query (see its doc comment), not a new rule,
/// so this must hold on both a clean and a dirty fixture.
#[test]
fn has_errors_matches_manual_partition_diagnostics() {
    for db in [ProjectDb::new(), dominant_error_fixture()] {
        let mut db = db;
        if db.entry().is_none() {
            db.set_file("main.ink", "Hello world.\n-> END\n".to_owned());
            db.set_entry("main.ink");
        }
        let entry = db.entry().expect("entry set");
        let disable_all = db.suppressions(entry).is_some_and(|s| s.disable_all);
        let files: Vec<FileDiagnostics<'_>> = db
            .file_ids()
            .map(|id| FileDiagnostics {
                file: id,
                source: db.source(id).unwrap_or_default(),
                suppressions: db.suppressions(id).expect("every file has suppressions"),
                lowering: db.file_diagnostics(id).unwrap_or_default(),
            })
            .collect();
        let (errors, _warnings) = partition_diagnostics(
            &files,
            &db.analysis().diagnostics,
            disable_all,
            db.analysis_options().type_policy(),
        );
        assert_eq!(
            db.has_errors(),
            !errors.is_empty(),
            "has_errors() disagreed with a manual partition_diagnostics call"
        );
    }
}

/// An error-FREE two-file fixture for the issue #806 probes: the entry file
/// is plain, clean ink, and `b.ink` is a second project file that `a.ink`
/// never INCLUDEs. `lir_product().program` is `Some(Arc<Program>)`, so
/// `lir_lowering_query`'s success branch genuinely runs and its `Arc`
/// pointer identity is observable — the review finding on PR #809: an
/// error-state fixture never even calls `lir_lowering_query` (the
/// `has_errors_query` gate returns `program: None` first), making any
/// "skipped re-execution" claim vacuous.
fn clean_program_fixture() -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.ink",
        "=== a_knot ===\nA scene line.\n-> END\n".to_owned(),
    );
    db.set_file(
        "b.ink",
        "=== b_knot ===\nSide content.\n-> END\n".to_owned(),
    );
    db.set_entry("a.ink");
    db
}

/// The lir_query-level FG-4a non-re-execution probe (issue #806), the part
/// that is provable today: `AnalysisOptions` edits that don't change any
/// lowering input must NOT re-execute `lir_lowering_query`. Because that
/// query is `no_eq` (a re-execution always allocates and stores a fresh
/// `Arc<Program>`, even if byte-identical), surviving `Arc::ptr_eq` across
/// an edit IS the proof its closure never ran — salsa only returns the same
/// stored allocation when the memo is fully validated without re-execution.
///
/// Two levers, both of which broke pointer identity before the `.types`
/// read was routed through the narrow `type_policy_query` projection (a raw
/// `project.analysis_options(db).types` field read depends on the whole
/// `AnalysisOptions` input field, and a salsa input write always bumps the
/// field revision — even a value-identical one):
///
/// 1. a no-op re-set of the identical default `AnalysisOptions`, and
/// 2. a genuine toggle of an options field `lir_lowering_query` never reads
///    (`semantic_type_check`; the fixture has no semantic types anywhere,
///    so no diagnostic changes and `has_errors()` stays `false`).
#[test]
fn lir_program_arc_survives_options_edits_that_leave_lowering_inputs_untouched() {
    let mut db = clean_program_fixture();

    // Non-vacuity: the program-generating phase really ran — the fixture is
    // error-free and produced a real program Arc to observe.
    assert!(!db.has_errors(), "fixture must be error-free");
    let before = db
        .lir_product()
        .expect("entry set")
        .program
        .clone()
        .expect("an error-free fixture must produce Some(Arc<Program>)");

    // Lever 1: re-set the exact same (default) options value. Salsa bumps
    // the input-field revision regardless of value equality, so only a
    // backdating-capable projection between the field and the lowering memo
    // keeps the memo validated.
    db.set_analysis_options(AnalysisOptions::default());
    let after_noop = db
        .lir_product()
        .expect("entry set")
        .program
        .clone()
        .expect("still error-free after a no-op options re-set");
    assert!(
        Arc::ptr_eq(&before, &after_noop),
        "a value-identical AnalysisOptions re-set re-executed \
         lir_lowering_query (issue #806 FG-4a: the raw `.types` input-field \
         read leak)"
    );

    // Lever 2: toggle a field the lowering never reads. No semantic types
    // exist anywhere in the fixture, so raising `semantic_type_check` to
    // `Error` changes no diagnostic — but it IS a real options-input write.
    db.set_analysis_options(AnalysisOptions {
        semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
        ..AnalysisOptions::default()
    });
    // Non-vacuity: the edit actually took.
    assert_eq!(
        db.analysis_options().semantic_type_check,
        SemanticTypeDiagnosticSeverity::Error,
        "the options toggle must be observable, or this lever is vacuous"
    );
    assert!(!db.has_errors(), "the toggle must not introduce an error");
    let after_toggle = db
        .lir_product()
        .expect("entry set")
        .program
        .clone()
        .expect("still error-free after the unrelated-field toggle");
    assert!(
        Arc::ptr_eq(&before, &after_toggle),
        "toggling an AnalysisOptions field lir_lowering_query never reads \
         (`semantic_type_check`) re-executed it (issue #806 FG-4a: the raw \
         `.types` input-field read leak)"
    );
}

/// The diagnostics-content half of the issue #806 probe, scoped to what is
/// true today: a warning-only text edit (unreachable-after-`-> END` content
/// in `b.ink` → `E033`, Warning severity) keeps `has_errors() == false`, so
/// the `lir_query` error gate stays open and the program stays `Some`.
///
/// Deliberately NOT asserted here: `Arc::ptr_eq` across this edit. `b.ink`
/// is unreachable from the entry (never `INCLUDE`d), but
/// `IncludeGraph::topological_order`'s all-files fallback appends every
/// unreachable project file to the lowering order, so `b.ink`'s
/// `lowered_query` HIR is a *genuine* recorded input of `lir_lowering_query`
/// today — any text edit to any project file legitimately re-executes the
/// memo, warning-only or not. That input over-breadth is issue #815
/// (deliberately out of scope for #806 / PR #809); once #815 narrows
/// lowering inputs to include-reachable files, upgrade this test to assert
/// `Arc::ptr_eq(&before_program, &after.program.unwrap())` across this
/// exact edit.
#[test]
fn warning_only_edit_in_unincluded_file_keeps_error_verdict_and_program() {
    let mut db = clean_program_fixture();

    let before = db.lir_product().expect("entry set").clone();
    let before_program = before
        .program
        .clone()
        .expect("an error-free fixture must produce Some(Arc<Program>)");
    assert!(!db.has_errors(), "fixture must be error-free");
    assert!(
        !before
            .warnings
            .iter()
            .any(|d| d.code == DiagnosticCode::E033),
        "E033 must not fire before the edit, or this test is vacuous: {:?}",
        before.warnings
    );

    // Warning-only diagnostics-content edit: content after `-> END` is
    // unreachable → E033 (Warning severity), no error anywhere.
    db.update_file(
        "b.ink",
        "=== b_knot ===\n-> END\nUnreachable side content.\n".to_owned(),
    );

    let after = db.lir_product().expect("entry set").clone();

    // Non-vacuity: the diagnostics content really changed.
    assert!(
        after
            .warnings
            .iter()
            .any(|d| d.code == DiagnosticCode::E033),
        "the edit must add an E033 warning, or this test is vacuous: {:?}",
        after.warnings
    );

    // The error verdict did not flip, so the gate stayed open and the
    // program is still generated.
    assert!(
        !db.has_errors(),
        "a warning-only edit must not flip has_errors()"
    );
    assert!(
        after.program.is_some(),
        "a warning-only edit must not gate off the program"
    );
    // See the doc comment: pointer identity across this lever is #815's
    // acceptance criterion, not #806's — `before_program` is captured above
    // so the upgrade is a one-line change.
    drop(before_program);
}
