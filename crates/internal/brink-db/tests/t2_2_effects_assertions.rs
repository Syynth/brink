//! T2-2 `#@effects(…)` assertion surface + exceedance error, exercised
//! end-to-end through `ProjectDb`'s salsa query layer (docs/effects-spec.md
//! §10, sitting 2 — 2026-07-14; issue #861 — tracked from #859). Builds on
//! T2-1's `effects(def)` substrate (`t2_1_effect_rows.rs`).
//!
//! Per the sitting-2 ruling, EXCEEDANCE (`E103`) is the *only* diagnostic
//! this surface produces — an inferred row narrower than its declared
//! `#@effects(…)` bound is silent; there is no drift policy. These tests
//! cover one exceedance fixture per atom class (reads, writes, calls, and
//! the ref-param-indirect write class PR #866 fixed), the `pure` sugar, and
//! assertion-satisfied silence.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

fn analyze(source: &str) -> Vec<brink_ir::Diagnostic> {
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    db.set_file("main.ink", source.to_owned());
    db.analysis().diagnostics.clone()
}

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn exceedance_on_an_extra_read() {
    // `#@effects(pure)` declares the empty row; `spend` actually reads
    // `gold` — exceedance.
    let diags = analyze(
        "VAR gold = 0\n\
         === function spend() ===\n#@effects(pure)\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("reads gold"), "{diags:?}");
}

#[test]
fn exceedance_on_an_extra_write() {
    // Declares only `reads: gold`; the body also writes it.
    let diags = analyze(
        "VAR gold = 0\n\
         === function spend(cost) ===\n#@effects(reads: gold)\n~ gold = gold - cost\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("writes gold"), "{diags:?}");
}

#[test]
fn exceedance_on_an_extra_call() {
    // Declares `reads: gold` only; the body also calls the external.
    let diags = analyze(
        "VAR gold = 0\nEXTERNAL play_sfx(x)\n\
         === function spend(cost) ===\n#@effects(reads: gold)\n\
         ~ temp before = gold\n~ play_sfx(cost)\n~ return before\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("calls play_sfx"), "{diags:?}");
}

#[test]
fn exceedance_via_ref_param_indirect_write() {
    // The PR #866 bug class: `inc`'s own body assigns a `Param`, never a
    // `Variable` — the write to `val` only becomes visible at `knot`'s own
    // call site, where `val` is passed into `inc`'s `ref x` slot (mirrors
    // `t2_1_effect_rows.rs`'s `effects_query_writes_through_a_ref_param_at_the_call_site`
    // fixture, now checked through the assertion surface). The assertion
    // declares only `reads: val` — the indirect write must still exceed it.
    let diags = analyze(
        "VAR val = 5\n\
         === knot ===\n#@effects(reads: val)\n~ inc(val)\n{val}\n->->\n\
         === function inc(ref x) ===\n~ x = x + 1\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
    assert!(diags[0].message.contains("writes val"), "{diags:?}");
}

#[test]
fn pure_sugar_is_satisfied_by_a_genuinely_pure_body() {
    let diags = analyze("=== function double(x) ===\n#@effects(pure)\n~ return x * 2\n");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn assertion_satisfied_is_silent_even_when_strictly_wider_than_inferred() {
    // Over-declaring (an assertion listing more than the body actually
    // touches) is explicitly NOT diagnosed — "no drift policy" (sitting 2).
    let diags = analyze(
        "VAR gold = 0\nVAR hp = 10\n\
         === function spend(cost) ===\n#@effects(reads: gold, writes: gold, writes: hp)\n\
         ~ gold = gold - cost\n~ return gold\n",
    );
    assert!(
        diags.is_empty(),
        "over-declaring must stay silent: {diags:?}"
    );
}

#[test]
fn unknown_cell_name_in_assertion_is_e102() {
    let diags = analyze(
        "VAR gold = 0\n=== function spend() ===\n#@effects(reads: nonexistent)\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E102], "{diags:?}");
}

#[test]
fn unknown_external_name_in_assertion_is_e102() {
    let diags = analyze(
        "VAR gold = 0\n=== function spend() ===\n#@effects(calls: nonexistent)\n~ return gold\n",
    );
    assert_eq!(codes(&diags), vec![DiagnosticCode::E102], "{diags:?}");
}

#[test]
fn strict_ink_never_runs_the_exceedance_check() {
    // Default dialect is `StrictInk` — `#@effects` is rejected whole
    // (`E051`) and the exceedance check never runs (no double diagnostic).
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR gold = 0\n=== function spend() ===\n#@effects(pure)\n~ return gold\n".to_owned(),
    );
    let diags = db.analysis().diagnostics.clone();
    assert_eq!(codes(&diags), vec![DiagnosticCode::E051], "{diags:?}");
}

#[test]
fn unannotated_project_produces_no_effects_diagnostics() {
    let diags = analyze(
        "VAR gold = 0\nEXTERNAL play_sfx(x)\n\
         === function spend(cost) ===\n~ gold = gold - cost\n~ play_sfx(cost)\n~ return gold\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
}
