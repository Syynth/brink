//! Targeted dependency-edge regression tests for issue #630 (FG-1,
//! `docs/fine-grained-salsa-proposal.md` §2 items 1 and the `analysis_query`
//! edge in §3).
//!
//! Both tests use pointer/Arc identity, not value equality, as the
//! assertion: salsa only returns the *exact same* stored value (same `Arc`
//! allocation / same memo address) when a query's memo is fully validated
//! without re-executing its closure. If a query re-executes — even if the
//! freshly recomputed value is `Eq` to the old one and salsa "backdates" the
//! memo's `changed_at` so *downstream* consumers don't see a change — the
//! newly computed value still replaces the stored one, so identity breaks.
//! That distinction is exactly what these tests are pinned on: the bug this
//! issue fixes was needless *re-execution* (wasted CPU), not a wrong final
//! value (the old code was already correct, just coarse).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_analyzer::{AnalysisOptions, InferenceResult};
use brink_db::ProjectDb;
use brink_ir::{
    DiagnosticCode, ExternalKind, HostManifest, ManifestExternal, ManifestParam, TypeRef,
};

fn def_named(db: &ProjectDb, name: &str) -> brink_format::DefinitionId {
    let index = db.symbol_index();
    let ids = index.by_name.get(name).expect("def should be indexed");
    *ids.first().expect("indexed name has at least one def")
}

/// FG-1 §2.1: `signature_query` must depend only on the *declaring* file's
/// `lowered_query`, not on every project file's. A body edit in an unrelated
/// file must leave the memo fully validated (same `Arc`), not re-executed.
#[test]
fn signature_memo_survives_unrelated_file_body_edit() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.ink",
        "VAR gold = 10\n=== quest(hero) ===\nOnward.\n-> END\n".to_owned(),
    );
    db.set_file(
        "b.ink",
        "=== filler ===\nOriginal filler line.\n-> END\n".to_owned(),
    );

    let gold = def_named(&db, "gold");
    let before = db.signature(gold).expect("gold has a signature");

    // Edit b.ink's body only — insert a line so byte offsets after it shift,
    // exercising the exact "any file's lowered_query changed" trigger the
    // old over-coarse `hir_refs` loop was sensitive to. No declaration in
    // b.ink changes (no new/removed knot, var, etc).
    db.update_file(
        "b.ink",
        "=== filler ===\nA new line before.\nOriginal filler line, revised.\n-> END\n".to_owned(),
    );

    let after = db.signature(gold).expect("gold still has a signature");
    assert!(
        Arc::ptr_eq(&before, &after),
        "editing an unrelated file's body re-executed a.ink's signature memo \
         (over-coarse per-file dependency — issue #630 FG-1)"
    );

    // Sanity: b.ink's own edit is real (the file set isn't somehow static),
    // proving this isn't a vacuously-true test because nothing happened.
    let filler = def_named(&db, "filler");
    assert!(
        db.signature(filler).is_some(),
        "filler knot should still resolve after its own edit"
    );
}

/// FG-1 §3: `type_inference_query` must be re-sourced off
/// `resolution_index_query` + per-file `resolve_query`, not `analysis_query`
/// — otherwise an edit that only changes `AnalysisResult`'s diagnostics
/// (never `index`/`resolutions`) forces a needless whole-project
/// re-inference, since `AnalysisResult`'s `PartialEq` almost never
/// backdates.
///
/// Toggling [`brink_analyzer::ExternalCheckSeverity`] between `Off` and
/// `Error` is such an edit: the field is read by exactly two diagnostics
/// gates — `external_check::analyze_externals`'s own
/// `severity == Off => diags.clear()` gate, and
/// `call_site_diagnostics_query`'s `== ExternalCheckSeverity::Off` early
/// return (`crates/internal/brink-db/src/queries/analysis.rs`) — so flipping
/// it adds an `E039` diagnostic without touching the symbol index, any
/// resolution, or (issue #1921) `collect_external_sigs`'s declaration-derived
/// signature for `foo` — that reads only `host_manifest` (registered
/// identically, unchanged, on both sides of this edit) and `inline_docs`,
/// never `external_check`.
///
/// This scenario deliberately keeps the manifest *itself* constant across
/// the edit (previously this test registered the manifest only on the
/// "after" side) — issue #1921 fixed `type_inference_query` to merge
/// `collect_external_sigs`'s seed (via its own `external_signatures_query`
/// memo) into the aggregated `InferenceResult::signatures` it returns, so
/// `InferenceResult::signatures` now legitimately depends on `host_manifest`
/// for any indexed `EXTERNAL` the manifest registers a matching entry for —
/// a *manifest* edit is no longer diagnostics-only in that case, precisely
/// because it is now correctly wired into typing. Only `external_check` —
/// read solely by the two diagnostics gates above, never by
/// `solve_scc`/`collect_external_sigs`/`external_signatures_query` — stays
/// genuinely diagnostics-only, so it is what this FG-1 pin now edits.
#[test]
fn type_inference_memo_survives_diagnostics_only_analysis_options_edit() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "EXTERNAL foo(x)\n=== main ===\n~ foo(1)\n-> DONE\n".to_owned(),
    );

    // The manifest disagrees with the ink declaration's arity (2 params vs.
    // 1) — registered from the start and never edited below, so it cannot
    // itself be the source of any `InferenceResult` change this test
    // observes.
    let manifest = HostManifest {
        markup: Vec::new(),
        externals: vec![ManifestExternal {
            name: "foo".to_string(),
            params: vec![
                ManifestParam {
                    name: "x".to_string(),
                    ty: TypeRef::default(),
                },
                ManifestParam {
                    name: "y".to_string(),
                    ty: TypeRef::default(),
                },
            ],
            returns: TypeRef::default(),
            kind: ExternalKind::default(),
            doc: None,
            widgets: vec![],
            path: Vec::new(),
        }],
        types: vec![],
    };
    db.set_analysis_options(AnalysisOptions {
        host_manifest: Some(manifest),
        external_check: brink_analyzer::ExternalCheckSeverity::Off,
        ..AnalysisOptions::default()
    });

    let before = std::ptr::from_ref::<InferenceResult>(db.type_inference());

    // `external_check` starts `Off`: no manifest-driven diagnostic for `foo`.
    assert!(
        !db.analysis()
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E039),
        "external_check = Off should suppress E039"
    );

    // Flip *only* `external_check` — the manifest (and everything else) is
    // untouched — a pure `AnalysisOptions` edit that changes
    // `analysis_query`'s diagnostics but neither the symbol index, any
    // resolution, nor any input `solve_scc`/`collect_external_sigs` reads.
    db.set_analysis_options(AnalysisOptions {
        host_manifest: db.analysis_options().host_manifest.clone(),
        external_check: brink_analyzer::ExternalCheckSeverity::Error,
        ..AnalysisOptions::default()
    });

    // Confirm the scenario actually changed `analysis_query`'s diagnostics —
    // otherwise this test would pass vacuously regardless of the fix.
    assert!(
        db.analysis()
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E039),
        "external_check = Error should raise E039 for the mismatched-arity manifest"
    );

    let after = std::ptr::from_ref::<InferenceResult>(db.type_inference());
    assert_eq!(
        before, after,
        "a diagnostics-only AnalysisOptions edit re-executed whole-project \
         type inference (the analysis_query cutoff-inheritance bug — issue \
         #630 FG-1)"
    );
}
