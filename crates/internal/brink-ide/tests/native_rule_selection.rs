//! Which diagnostic rules an `IdeSession` runs over **native `.brink`
//! source** (issue #1358).
//!
//! `IdeSession`'s editor analysis runs off the db — `snapshot()` clones the
//! inputs and `IdeSnapshot::analyze` calls
//! `brink_analyzer::analyze_with_modules` — which is what backs
//! `@brink-lang/web`'s live squiggles. That pure path has no file paths and
//! so cannot classify a file as native itself; the session has to tell it.
//! Before #1358 it never did, so a `.brink` file open in the editor was
//! judged by the *ink* rule set:
//!
//! - the ink-only T1b dialect gate (`E051`) rejected ordinary native syntax
//!   as "a brink extension",
//! - the ink-only `types = strict` config error (`E064`) fired on a native
//!   project that dialed strict (the only policy native even has), and
//! - the B0.9 native strict-only gate (`E137`) — explicit `types = gradual`
//!   is not a policy native source can compile under — could not fire at
//!   all, because the pure path had no way to express it.
//!
//! Every test whose assertion is an *absence* (no `E051`/`E064`/…) pairs it
//! with the *ink arm* of the same inputs, run through the same analyzer
//! entry point with `is_native = false`. That guard is what makes those
//! tests non-vacuous: it proves the fixture really does provoke the rule
//! under the ink arm, so a passing assertion is evidence the native arm was
//! selected, not evidence the fixture happens to be clean. The
//! `analyze_overlay`/`analyze_projection` tests run the ink arm off the
//! throwaway `ProjectDb` the gate itself returns, since that db owns the
//! `FileId`s the gate's result is keyed against. The same-named-flow test
//! doesn't need a guard: its assertion (`greets == 2`) is inherently
//! non-vacuous — the ink arm would collapse it to `1`, not silently pass.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisResult, TypePolicy};
use brink_ide::session::IdeSession;
use brink_ir::DiagnosticCode;

/// `main.brink` — native source whose `struct` declaration and construction
/// literal the ink-only T1b gate flags as brink extensions under the
/// `StrictInk` dialect a native mount defaults to.
const MAIN: &str = "\
struct Guest {
  name: string
}

fn make(): Guest {
  return Guest { name: \"ada\" };
}

flow start() {
  The market is busy.
  -> END
}
";

/// A single-file native project, analyzed under the session defaults — no
/// `set_language_dialect` call, i.e. `Dialect::StrictInk`, which is what a
/// native mount actually runs under (native carries no dialect opinion).
fn native_session() -> IdeSession {
    let mut session = IdeSession::new();
    session.update_and_analyze("main.brink", MAIN.to_owned());
    session
}

/// The same inputs the session just analyzed, re-run through the same
/// analyzer entry point with `is_native = false` — the ink arm. The
/// non-vacuity guard described in this module's doc.
fn ink_arm(session: &IdeSession) -> AnalysisResult {
    let db = session.db();
    let inputs = db.analysis_inputs();
    let refs: Vec<_> = inputs.iter().map(|(id, hir, m)| (*id, hir, m)).collect();
    brink_analyzer::analyze_with_modules(&refs, db.module_map(), &session.analysis_options(), false)
}

fn has(result: &AnalysisResult, code: DiagnosticCode) -> bool {
    result.diagnostics.iter().any(|d| d.code == code)
}

/// The same non-vacuity guard as `ink_arm`, but for a gate's throwaway db
/// (`analyze_overlay`/`analyze_projection`): that db reassigns `FileId`s, so
/// the ink arm has to run off the same db the gate did, not the session's.
fn ink_arm_from_db(session: &IdeSession, db: &brink_db::ProjectDb) -> AnalysisResult {
    let inputs = db.analysis_inputs();
    let refs: Vec<_> = inputs.iter().map(|(id, hir, m)| (*id, hir, m)).collect();
    brink_analyzer::analyze_with_modules(&refs, db.module_map(), &session.analysis_options(), false)
}

#[test]
fn native_session_never_runs_the_ink_only_dialect_gate() {
    let session = native_session();
    let analysis = session.analysis().expect("analysis");

    assert!(
        has(&ink_arm(&session), DiagnosticCode::E051),
        "guard: the fixture must provoke `E051` under the ink arm, or this \
         test proves nothing"
    );
    assert!(
        !has(analysis, DiagnosticCode::E051),
        "a `.brink` file open in the editor must not be judged by the \
         ink-only T1b gate: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn native_session_never_runs_the_ink_only_strict_config_error() {
    let mut session = native_session();
    // `types = strict` is the *only* policy native source has; `E064`
    // rejects it under a non-`brink` **dialect**, an axis native doesn't
    // carry at all.
    session.set_type_policy(TypePolicy::Strict);
    let analysis = session.analysis().expect("analysis");

    assert!(
        has(&ink_arm(&session), DiagnosticCode::E064),
        "guard: the fixture must provoke `E064` under the ink arm, or this \
         test proves nothing"
    );
    assert!(
        !has(analysis, DiagnosticCode::E064),
        "native has no dialect to be wrong about: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn native_session_reports_the_strict_only_gate_for_explicit_gradual() {
    let mut session = native_session();
    session.set_type_policy(TypePolicy::Gradual);
    let analysis = session.analysis().expect("analysis");

    assert!(
        !has(&ink_arm(&session), DiagnosticCode::E137),
        "guard: `E137` is native-only — the ink arm must never produce it, \
         or this test would pass for the wrong reason"
    );
    assert!(
        has(analysis, DiagnosticCode::E137),
        "explicit `types = gradual` is not a policy native source can \
         compile under (B0.9 strict-only ruling): {:?}",
        analysis.diagnostics
    );
}

/// The safe-rename / refactor gate (`analyze_overlay`) analyzes off-db too,
/// and reports the diagnostics an edit *would* introduce. Judged by the ink
/// rule set it condemned every native rename as introducing `E051`.
#[test]
fn the_overlay_gate_uses_the_native_rule_set_too() {
    let session = native_session();
    let overlay = std::collections::BTreeMap::from([("main.brink".to_owned(), MAIN.to_owned())]);
    let (result, db) = session.analyze_overlay(&overlay);

    assert!(
        has(&ink_arm_from_db(&session, &db), DiagnosticCode::E051),
        "guard: the fixture must provoke `E051` under the ink arm, or this \
         test proves nothing"
    );
    assert!(
        !has(&result, DiagnosticCode::E051),
        "the overlay gate must judge native source by the native rules: {:?}",
        result.diagnostics
    );
}

/// The directory rename/move gate (`analyze_projection`), same argument.
#[test]
fn the_projection_gate_uses_the_native_rule_set_too() {
    let session = native_session();
    let projection =
        std::collections::BTreeMap::from([("moved/main.brink".to_owned(), MAIN.to_owned())]);
    let (result, db) = session.analyze_projection(&projection);

    assert!(
        has(&ink_arm_from_db(&session, &db), DiagnosticCode::E051),
        "guard: the fixture must provoke `E051` under the ink arm, or this \
         test proves nothing"
    );
    assert!(
        !has(&result, DiagnosticCode::E051),
        "the projection gate must judge native source by the native rules: {:?}",
        result.diagnostics
    );
}

/// The same wiring also reaches the symbol index's M-2d arm (the one
/// `is_native` originally existed for, issue #1562): a native file's module
/// is its path and is always *declared*, so two native modules declaring a
/// same-named flow are a cross-declared-module pair. Under the ink arm that
/// is a duplicate — the later definition is dropped from the index and every
/// `analysis()`-backed IDE feature (hover, go-to-definition, completion, the
/// story graph) misses it.
#[test]
fn native_session_lets_two_modules_coexist_with_a_same_named_flow() {
    const FLOW: &str = "flow greet() {\n  Hello.\n}\n";
    let mut session = IdeSession::new();
    session.update_source("market/barter.brink", FLOW.to_owned());
    session.update_and_analyze("tavern/chat.brink", FLOW.to_owned());
    let analysis = session.analysis().expect("analysis");

    let greets = analysis
        .index
        .symbols
        .values()
        .filter(|s| s.name == "greet")
        .count();
    assert_eq!(
        greets, 2,
        "both native modules' `greet` must coexist in the index: {:?}",
        analysis.diagnostics
    );
}

/// The ink path is untouched: an ink session still gets the ink arm, whole.
#[test]
fn an_ink_session_still_gets_the_ink_rule_set() {
    let mut session = IdeSession::new();
    session.update_and_analyze("story.ink", "~ x = a[0]\n".to_owned());
    let analysis = session.analysis().expect("analysis");

    assert!(
        has(analysis, DiagnosticCode::E051),
        "postfix indexing is still a brink extension in ink: {:?}",
        analysis.diagnostics
    );
}

/// A **mixed** file set analyzes as ink: the flag is whole-project, and the
/// analyzer has no per-file classification of its own, so applying the
/// native arm to a set holding an ink file would judge that ink file by
/// rules it isn't written under.
#[test]
fn a_mixed_session_falls_back_to_the_ink_rule_set() {
    let mut session = IdeSession::new();
    session.update_source("main.brink", MAIN.to_owned());
    session.update_and_analyze("story.ink", "~ x = a[0]\n".to_owned());
    let analysis = session.analysis().expect("analysis");

    assert!(
        has(analysis, DiagnosticCode::E051),
        "a mixed set must stay on the ink arm: {:?}",
        analysis.diagnostics
    );
}
