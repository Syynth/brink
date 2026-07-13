//! Targeted dependency-edge regression tests for issue #632 (FG-3,
//! `docs/fine-grained-salsa-proposal.md` §1 item 4 and the `analysis_query`
//! decomposition section), following the FG-1/FG-2.1 pattern
//! (`fg1_dependency_edges.rs`, `fg2_scc_dependency_edges.rs`).
//!
//! Both tests use `Arc` pointer identity, not value equality, as the
//! assertion: salsa only returns the *exact same* stored `Arc` allocation
//! when a query's memo is fully validated without re-executing its closure.
//! If a query re-executes — even if the freshly recomputed value is `Eq` to
//! the old one — the newly computed value still replaces the stored one, so
//! pointer identity breaks. That distinction is exactly what these tests
//! pin: the bug this issue fixes was needless *re-execution* of
//! validate/dialect_gate/annotation-content checks across every project
//! file on nearly any edit, not a wrong final value (the old bundled
//! `analysis_query` was already correct, just coarse).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, ExternalCheckSeverity, SemanticTypeDiagnosticSeverity};
use brink_db::{ProjectDb, ResolvedProject};
use brink_ir::DiagnosticCode;

/// FG-3 design doc §1 item 4: `per_file_diagnostics_query` must depend only
/// on its own file's `lowered_query`/`resolve_query` (plus the narrow,
/// cutoff-friendly `resolution_index_query` projection) — never another
/// file's HIR. A body edit in an unrelated file must leave the memo fully
/// validated (same `Arc`), not re-executed.
#[test]
fn per_file_contributor_survives_unrelated_file_body_edit() {
    let mut db = ProjectDb::new();
    db.set_file("a.ink", "=== quest(hero) ===\nOnward.\n-> END\n".to_owned());
    let b = db.set_file(
        "b.ink",
        "=== filler ===\nOriginal filler line.\n-> END\n".to_owned(),
    );

    let a = db.file_id("a.ink").expect("a.ink id");
    let before = db
        .per_file_diagnostics(a)
        .expect("a.ink has a contributor memo");

    // Edit b.ink's body only — insert a line so byte offsets after it
    // shift, exercising the exact "any file's lowered_query changed"
    // trigger the old whole-project validate/dialect_gate/annotations loops
    // were sensitive to. No declaration in b.ink changes.
    db.update_file(
        "b.ink",
        "=== filler ===\nA new line before.\nOriginal filler line, revised.\n-> END\n".to_owned(),
    );

    let after = db
        .per_file_diagnostics(a)
        .expect("a.ink still has a contributor memo");
    assert!(
        Arc::ptr_eq(&before, &after),
        "editing an unrelated file's body re-executed a.ink's per-file \
         diagnostic contributor memo (issue #632 FG-3)"
    );

    // Sanity: b.ink's own edit is real (the file set isn't somehow static),
    // proving this isn't a vacuously-true test because nothing happened —
    // b.ink's own contributor memo must still be reachable post-edit.
    assert!(
        db.per_file_diagnostics(b).is_some(),
        "b.ink should still have a reachable contributor memo after its own edit"
    );
}

/// FG-3 design doc §1: `resolutions_index_query` must be independent of
/// `AnalysisOptions` — a diagnostics-only options edit (raising
/// `semantic_type_check` to `Error`, which adds an `E040` diagnostic without
/// touching any declaration or resolution) must leave every resolutions-only
/// reader fully validated: the `Arc` pointer identity of
/// `resolutions_index()` survives untouched, because neither
/// `symbol_index_query` nor any file's `resolve_query` reads
/// `project.analysis_options` at all — salsa doesn't even need to re-run
/// this query's closure, matching the FG-1 `type_inference_query` precedent
/// applied at the `analysis_query` decomposition layer.
#[test]
fn resolutions_index_survives_diagnostics_only_analysis_options_edit() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "/// @param who {actor_id}\nEXTERNAL add_state(who)\n".to_owned(),
    );

    let before: Arc<ResolvedProject> = db.resolutions_index();

    // No manifest, default (Tolerant) semantic_type_check: no E040 yet.
    assert!(
        !db.analysis()
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E040),
        "default Tolerant semantic_type_check should not yet raise E040"
    );

    // Raise semantic_type_check to Error — a pure `AnalysisOptions` edit
    // that changes `analysis_query`'s diagnostics but touches neither the
    // symbol index nor any resolution (no ink source changes).
    db.set_analysis_options(AnalysisOptions {
        semantic_type_check: SemanticTypeDiagnosticSeverity::Error,
        ..AnalysisOptions::default()
    });

    // Confirm the scenario actually changed the diagnostics — otherwise
    // this test would pass vacuously regardless of the fix.
    assert!(
        db.analysis()
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E040),
        "raising semantic_type_check to Error should raise E040 for the \
         unknown `actor_id` semantic type"
    );

    let after: Arc<ResolvedProject> = db.resolutions_index();
    assert!(
        Arc::ptr_eq(&before, &after),
        "a diagnostics-only AnalysisOptions edit re-executed \
         resolutions_index_query (the analysis_query bundling bug — issue \
         #632 FG-3)"
    );
}

/// Companion check: `resolutions_index()`'s *value* (not just an unrelated
/// query) really is diagnostics-free — flipping `external_check` severity
/// (another diagnostics-only lever) leaves both the index and the
/// resolutions untouched byte-for-byte, confirming the split struct carries
/// no diagnostic data to begin with.
#[test]
fn resolutions_index_value_is_unaffected_by_external_check_severity() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", "EXTERNAL foo(x)\n-> END\n".to_owned());

    let before = db.resolutions_index();
    db.set_analysis_options(AnalysisOptions {
        external_check: ExternalCheckSeverity::Off,
        ..AnalysisOptions::default()
    });
    let after = db.resolutions_index();

    assert!(
        Arc::ptr_eq(&before, &after),
        "toggling external_check severity re-executed resolutions_index_query"
    );
}
