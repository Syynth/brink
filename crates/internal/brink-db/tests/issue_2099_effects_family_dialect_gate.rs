//! Characterization suite for issue #2099 (`docs/effects-dialect-gate-
//! audit.md`) — **NOT a fix, no ruling made**. Pins today's dark-by-default
//! behavior of the effects/comparator diagnostic family
//! (E102/E103/E108/E109 via `effects_assertions`, E105 via `await_purity`,
//! E119 via `comparator_contract`) for a genuinely native `.brink` project
//! compiled under fully-default `AnalysisOptions` — no `dialect` override —
//! exercised through the same `ProjectDb::analysis()` salsa seam
//! `brink compile`/`brink check`/`@brink-lang/web` all read.
//!
//! Every fixture below is proven capable of tripping its diagnostic (the
//! `_with_brink_dialect_forced` sibling of each default-options test), so
//! the "silent under default" result is the dialect gate at
//! `crates/internal/brink-analyzer/src/lib.rs:1255` and its three
//! `brink-db` mirrors (`crates/internal/brink-db/src/queries/analysis.rs:
//! 330,376,599`) suppressing the check outright — not the fixture failing
//! to reach the check for some unrelated reason.
//!
//! Whichever way #2099 is ruled, this file needs a look: if native gets an
//! `is_native` fallback (option (a) in the audit doc), the "silent under
//! default" assertions below flip to "fires under default"; if opt-in-only
//! is ruled correct (option (b)), they stand as permanent regression
//! coverage of the intentional posture.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

fn codes(diags: &[brink_ir::Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

fn analyze(source: &str, opts: AnalysisOptions) -> Vec<brink_ir::Diagnostic> {
    let mut db = ProjectDb::new();
    db.set_analysis_options(opts);
    db.set_file("main.brink", source.to_owned());
    // Reviewer finding (docs/effects-dialect-gate-audit.md §2/§6): the
    // whole-project db path's `is_native` (`project_is_native`,
    // `crates/internal/brink-db/src/queries/mod.rs:576-585`) is
    // entry-derived and `false` whenever `ProjectDb::entry()` is `None` —
    // `ProjectDb::new()` leaves it unset. Without this, these fixtures
    // would stay "dark by default" even after an `is_native` fallback (#2099
    // option (a)) landed, silently voiding the suite's forward-looking
    // purpose. Setting the entry here matches the convention ~15 sibling
    // `brink-db` suites already use (e.g. `issue_1844_conventions_module_
    // fence.rs`).
    db.set_entry("main.brink");
    db.analysis().diagnostics.clone()
}

fn brink_dialect() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

// ── E103 (effects_assertions exceedance) ─────────────────────────────────
//
// Fixture verified against `t2_2_effects_assertions.rs`'s own
// `native_pure_assertion_is_exceeded_by_a_global_read` (that suite's
// `analyze_native` helper forces exactly the same `Dialect::Brink` posture
// on every one of its `.brink` fixtures — its own doc comment names this
// as a requirement, not a stylistic choice).
const E103_FIXTURE: &str = "var gold = 0\n\n@[effects(pure)]\nfn spend() {\n  return gold;\n}\n\nflow main() {\n  Hi. -> END\n}\n";

#[test]
fn e103_fires_when_dialect_brink_is_forced_on_a_native_file() {
    let diags = analyze(E103_FIXTURE, brink_dialect());
    assert_eq!(codes(&diags), vec![DiagnosticCode::E103], "{diags:?}");
}

#[test]
fn e103_is_silent_on_the_same_native_file_under_default_options() {
    let diags = analyze(E103_FIXTURE, AnalysisOptions::default());
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E103),
        "expected E103 to be dark under default AnalysisOptions (the #2099 \
         gap) — got {diags:?}"
    );
}

// ── E105 (await_purity, native `until` spelling) ─────────────────────────
//
// Native's flow-suspension spelling is `until <cond>;`, not `await`
// (decision-log 2026-07-23 item 4, retiring the `await` keyword on the
// native surface; `hir/lower_native/tests.rs`'s
// `logic_line_until_lowers_to_stmt_await` proves `~ until n > 0` lowers to
// the same `Stmt::Await` node the brink-dialect ink spelling produces).
// Condition shape mirrors `fs2_await.rs`'s
// `brink_effectful_condition_writing_global_is_rejected` one grammar
// dialect over (`~ await raise_alarm()` → `~ until raise_alarm()`).
const E105_FIXTURE: &str = "var alarm = false\n\nfn raise_alarm() {\n  alarm = true\n  return true\n}\n\nflow main() {\n  ~ until raise_alarm()\n  Hi. -> END\n}\n";

#[test]
fn e105_fires_when_dialect_brink_is_forced_on_a_native_file() {
    let diags = analyze(E105_FIXTURE, brink_dialect());
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "an effectful `until` condition must trip the purity gate under \
         the forced Brink dialect: {diags:?}"
    );
}

#[test]
fn e105_is_silent_on_the_same_native_file_under_default_options() {
    let diags = analyze(E105_FIXTURE, AnalysisOptions::default());
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E105),
        "expected E105 to be dark under default AnalysisOptions (the #2099 \
         gap) — got {diags:?}"
    );
}

// ── E119 (comparator_contract, native bare-name callback, issue #1887) ───
//
// Fixture verified against `crates/brink-compiler/tests/driver.rs`'s own
// `compile_path_native_comparator_contract_call_in_lambda_decl_default_
// is_e119` — that suite's `compile_native_brink_dialect` helper exists
// solely to force `Dialect::Brink` onto an otherwise-ordinary native
// `.brink` compile so E119 (including the #1887 bare-name-callback arm)
// can fire at all; its own doc comment names the gap this test pins.
const E119_FIXTURE: &str = "var seen = 0\n\nfn spy(n) {\n  seen = seen + n\n  return n\n}\n\nconst doIt = || map([1, 2], spy)\n\nflow main() {\n  Hi. -> END\n}\n";

#[test]
fn e119_fires_when_dialect_brink_is_forced_on_a_native_file() {
    let diags = analyze(E119_FIXTURE, brink_dialect());
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E119),
        "an impure named callback passed to `map` must trip the \
         comparator contract gate under the forced Brink dialect: {diags:?}"
    );
}

#[test]
fn e119_is_silent_on_the_same_native_file_under_default_options() {
    let diags = analyze(E119_FIXTURE, AnalysisOptions::default());
    assert!(
        diags.iter().all(|d| d.code != DiagnosticCode::E119),
        "expected E119 to be dark under default AnalysisOptions (the #2099 \
         gap) — got {diags:?}"
    );
}
