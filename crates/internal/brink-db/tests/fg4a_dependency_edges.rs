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
            db.analysis_options().types,
        );
        assert_eq!(
            db.has_errors(),
            !errors.is_empty(),
            "has_errors() disagreed with a manual partition_diagnostics call"
        );
    }
}
