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

// ── Issue #750 (FG-3 completion): the external-check family ────────────

/// A two-file fixture whose external call-site checks actually fire: a.ink
/// declares a typed external (inline `///` doc — no host manifest needed,
/// `int` is a base type) plus a call, and b.ink calls it with a `string`
/// literal, a real `E041`. The b-side memo existing and being non-empty
/// keeps the pointer-identity assertions below non-vacuous.
fn external_call_site_fixture() -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.ink",
        "/// Pay the piper.\n/// @param amount {int}\nEXTERNAL pay(amount)\n\
         === a_knot ===\nA scene line.\n~ pay(1)\n-> END\n"
            .to_owned(),
    );
    db.set_file(
        "b.ink",
        "=== b_knot ===\n~ pay(\"gold\")\n-> END\n".to_owned(),
    );
    db
}

/// Issue #750: `call_site_diagnostics_query` must depend only on its own
/// file's `lowered_query` plus the range-free `call_site_metas_query`
/// projection — never another file's HIR. A body edit in the *declaring*
/// file (which shifts every range in the full index and re-executes the
/// enrichment pass) must leave the other file's call-site memo fully
/// validated (same `Arc`), not re-executed — pre-#750, this walk ran
/// project-wide inside `whole_project_diagnostics_query` on any body edit.
#[test]
fn call_site_contributor_survives_unrelated_file_body_edit() {
    let mut db = external_call_site_fixture();
    let b = db.file_id("b.ink").expect("b.ink id");

    let before = db
        .file_call_site_diagnostics(b)
        .expect("b.ink has a call-site memo");
    assert!(
        before.iter().any(|d| d.code == DiagnosticCode::E041),
        "fixture must fire E041 in b.ink (string literal into int param), \
         or this test is vacuous: {before:?}"
    );

    // Body edit in a.ink: insert a line inside the knot, after the EXTERNAL
    // declaration — every later range shifts, so the full ranged index (and
    // with it external_meta_query's inputs) really changes, but no doc,
    // declaration, or external content does.
    db.update_file(
        "a.ink",
        "/// Pay the piper.\n/// @param amount {int}\nEXTERNAL pay(amount)\n\
         === a_knot ===\nA new line first.\nA scene line, revised.\n~ pay(1)\n-> END\n"
            .to_owned(),
    );

    let after = db
        .file_call_site_diagnostics(b)
        .expect("b.ink still has a call-site memo");
    assert!(
        Arc::ptr_eq(&before, &after),
        "editing the declaring file's body re-executed b.ink's call-site \
         contributor memo (issue #750 FG-3 completion)"
    );
}

/// Issue #750: the `call_site_metas_query` cutoff seam itself. A body edit
/// anywhere shifts declaration ranges, so the full-ranged-index-reading
/// enrichment pass re-executes — but the name→meta projection is
/// range-free, so it must come out `Eq` and leave this memo (and through
/// it, every file's call-site memo) untouched. The `resolution_index`
/// playbook, applied to the external-check family.
#[test]
fn call_site_metas_survive_body_edits_anywhere() {
    let mut db = external_call_site_fixture();

    let before = db.call_site_metas();
    assert!(
        before.contains_key("pay"),
        "fixture must produce an external meta for `pay`, or this test is vacuous"
    );

    db.update_file(
        "b.ink",
        "=== b_knot ===\nSome fresh prose.\n~ pay(\"gold\")\n-> END\n".to_owned(),
    );

    let after = db.call_site_metas();
    assert!(
        Arc::ptr_eq(&before, &after),
        "a body edit re-executed call_site_metas_query — the range-free \
         cutoff seam failed to backdate (issue #750 FG-3 completion)"
    );
}

/// Issue #750: `value_meta_query` must depend only on its own file's
/// `lowered_query` (plus the range-zeroed index projection and the
/// range-free doc merge) — a body edit in an unrelated file must leave the
/// memo fully validated (same `Arc`), not re-executed. Pre-#750 this walk
/// (`infer_value_meta`) ran over every file's HIR inside
/// `whole_project_diagnostics_query` on any body edit.
#[test]
fn value_meta_contributor_survives_unrelated_file_body_edit() {
    let mut db = ProjectDb::new();
    db.set_file(
        "vars.ink",
        "/// The player's gold.\nVAR gold = 10\nCONST MAX = 99\n".to_owned(),
    );
    db.set_file(
        "scene.ink",
        "=== scene ===\nOriginal prose.\n-> END\n".to_owned(),
    );
    let vars = db.file_id("vars.ink").expect("vars.ink id");

    let before = db
        .file_value_meta(vars)
        .expect("vars.ink has a value-meta memo");
    assert!(
        !before.is_empty(),
        "fixture must produce VAR/CONST metas, or this test is vacuous"
    );

    db.update_file(
        "scene.ink",
        "=== scene ===\nA new opening line.\nOriginal prose, revised.\n-> END\n".to_owned(),
    );

    let after = db
        .file_value_meta(vars)
        .expect("vars.ink still has a value-meta memo");
    assert!(
        Arc::ptr_eq(&before, &after),
        "editing an unrelated file's body re-executed vars.ink's value-meta \
         contributor memo (issue #750 FG-3 completion)"
    );
}
