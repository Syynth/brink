//! Characterization of the live-typing vs. db diagnostic divergence
//! (issue #1347, `needs-design`) — **resolved by #1358.**
//!
//! `IdeSession`'s editor-facing analysis runs off the db —
//! `snapshot().analyze()` → `brink_analyzer::analyze_with_modules(…,
//! is_native)` — while a compile and every direct db query run
//! `per_file_diagnostics_query`, which knows each file's *path* and
//! therefore its `Language`. Before #1358, the pure path always hardcoded
//! `is_native = false`, so the two disagreed on native `.brink` files in
//! both directions (missing `E084`/`E106`/`E137`/`E138`, and a false-positive
//! `E051`). #1358 threads the db's real `is_native` answer through
//! `IdeSnapshot`/`analyze_with_modules` (design doc §4 option B), which
//! closes every divergence this suite measured — see
//! `docs/live-typing-diagnostics-divergence.md` for the resolution note and
//! the original measurement.
//!
//! These tests now pin *agreement*: for every fixture below, live typing and
//! the db must report the same diagnostic codes. Keeping the original
//! fixtures (rather than deleting the file) keeps the regression coverage —
//! if a future change reintroduces the divergence, one of these fails with a
//! code-set mismatch naming #1347.

use brink_analyzer::{Dialect, TypePolicy};
use brink_ide::session::IdeSession;
use brink_ir::{Diagnostic, DiagnosticCode};

/// A native file whose only content is ordinary native syntax: a `struct`
/// declaration and a construction literal with a repeated field (`E084`).
/// Nothing here is "brink extension syntax bolted onto ink" — it is the
/// native grammar, which is exactly why the T1b dialect gate must not judge
/// it (#1348).
const NATIVE_DUP_FIELD: &str = "\
struct Point {
  x: int,
  y: int
}

fn f() {
  let p = Point { x: 3, x: 4 };
  return p.x;
}

flow main() {
  Sum: {f()} -> END
}
";

/// A plain ink file — the control. The divergence #1347 measured was
/// native-only, and this is what proves it.
const INK_PLAIN: &str = "=== start ===\nHello.\n-> END\n";

/// A native file with a map literal that trips both of the other two checks
/// wired at the same `dialect == Brink || is_native` gate as `E084`
/// (`crates/internal/brink-analyzer/src/lib.rs:634-651`): `3.5` is outside
/// the int/string/bool key domain (`E106`), and `"a"` repeats (`E138`).
const NATIVE_MAP_KEY_ISSUES: &str = "\
fn f() {
  let m = Map { \"a\": 1, \"a\": 2, 3.5: 4 };
  return m;
}

flow main() {
  Sum: {f()} -> END
}
";

/// Load `source` into a session under `dialect` with an explicit
/// `types = gradual`, and return `(live typing diagnostics, db diagnostics)`.
///
/// `None` when the session never produced a cached analysis — surfaced as a
/// return value rather than an `expect`, because `panic`/`expect_used` are
/// denied outside `#[test]` fns even in an integration test (the same reason
/// `dialect_conformance.rs`'s own helper returns `Result`). Callers unwrap it
/// in the test body, where the lint exemption applies.
///
/// The trailing `set_type_policy` is not redundant: option setters funnel
/// through `IdeSession::reanalyze`, which is what pushes the session's
/// options into its own db (#1553). `update_and_analyze` does not, so
/// without a setter call *after* the file lands, the db would read the file
/// under whatever options were last synced.
fn both_surfaces(
    path: &str,
    source: &str,
    dialect: Dialect,
) -> Option<(Vec<Diagnostic>, Vec<Diagnostic>)> {
    let mut session = IdeSession::new();
    session.set_language_dialect(dialect);
    session.update_and_analyze(path, source.to_owned());
    session.set_type_policy(TypePolicy::Gradual);

    let live = session.analysis()?.diagnostics.clone();
    let db = session.db().analysis().diagnostics.clone();
    Some((live, db))
}

fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
    let mut v: Vec<DiagnosticCode> = diags.iter().map(|d| d.code).collect();
    v.sort_by_key(|c| format!("{c:?}"));
    v.dedup();
    v
}

fn assert_surfaces_agree(live: &[Diagnostic], db: &[Diagnostic], context: &str) {
    assert_eq!(
        codes(live),
        codes(db),
        "#1347 regression: live typing and the db must agree on {context} \
         (resolved by #1358); live {:?}, db {:?}",
        codes(live),
        codes(db)
    );
}

/// `E137` (B0.9 native strict-only, issue #1342) used to have no pure-path
/// call site: `brink_analyzer::finish_analysis` never called
/// `native_strict_only_error`, only `per_file_diagnostics_query` did, so an
/// author typing in a native file under an explicit `types = gradual` never
/// saw it until they compiled. #1358 wired `is_native` through, and
/// `finish_analysis` now reaches the same check live typing's db does.
#[test]
fn e137_reaches_both_the_db_and_live_typing() {
    for dialect in [Dialect::StrictInk, Dialect::Brink] {
        let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, dialect)
            .expect("session produced an analysis");
        assert!(
            has(&db, DiagnosticCode::E137),
            "fixture must provoke E137 under {dialect:?}; db saw {:?}",
            codes(&db)
        );
        assert_surfaces_agree(&live, &db, &format!("E137 under {dialect:?}"));
    }
}

/// `E084` (struct construction-literal duplicate field) is gated on
/// `dialect == Brink || is_native` inside `brink_analyzer::per_file_diagnostics`.
/// Before #1358 the pure path always supplied `is_native = false`, so under
/// the *default* `strict-ink` dialect — what `EditorSession` starts at — a
/// native file's duplicate field was a real compile error live typing never
/// reported. #1358 supplies the db's real `is_native`, so both surfaces now
/// agree under both dialects.
#[test]
fn e084_reaches_both_surfaces_under_every_dialect() {
    for dialect in [Dialect::StrictInk, Dialect::Brink] {
        let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, dialect)
            .expect("session produced an analysis");
        assert!(
            has(&db, DiagnosticCode::E084),
            "fixture must provoke E084 under {dialect:?}; db saw {:?}",
            codes(&db)
        );
        assert_surfaces_agree(&live, &db, &format!("E084 under {dialect:?}"));
    }
}

/// **The sharper half of #1347, and the one the issue did not know about.**
/// The T1b dialect gate is an ink-only axis: a native `.brink` file's own
/// grammar *is* the superset grammar the gate polices, so
/// `per_file_diagnostics_query` skips it via `is_native` (#1348). Before
/// #1358 the pure path could not skip it, so ordinary native syntax drew an
/// `E051` squiggle in the editor that vanished on compile.
///
/// `EditorSession` defaults to `strict-ink`, so this was the default
/// experience for a `.brink` file with no declared dialect — not an edge
/// case. #1358 closes it: live typing no longer emits `E051` here.
#[test]
fn live_typing_no_longer_invents_e051_on_native_syntax() {
    let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, Dialect::StrictInk)
        .expect("session produced an analysis");
    assert!(
        !has(&live, DiagnosticCode::E051),
        "#1347 regression: live typing invented E051 on native syntax under \
         strict-ink again; live saw {:?}",
        codes(&live)
    );
    assert!(
        !has(&db, DiagnosticCode::E051),
        "#1348 regression: the db must never emit E051 on a native file; db saw {:?}",
        codes(&db)
    );
}

/// `E106` (map-literal key domain) and `E138` (map-literal duplicate key)
/// are gated at the same `dialect == Brink || is_native` block as `E084`
/// (`crates/internal/brink-analyzer/src/lib.rs:634-651`), so before #1358
/// both were also missing from live typing on a native file under the
/// default `strict-ink` dialect — the same pure-path `is_native = false`
/// blind spot `e084_reaches_both_surfaces_under_every_dialect` pins for
/// `E084`.
#[test]
fn e106_and_e138_reach_both_surfaces_under_default_dialect() {
    let (live, db) = both_surfaces("main.brink", NATIVE_MAP_KEY_ISSUES, Dialect::StrictInk)
        .expect("session produced an analysis");
    for code in [DiagnosticCode::E106, DiagnosticCode::E138] {
        assert!(
            has(&db, code),
            "fixture must provoke {code:?} under strict-ink; db saw {:?}",
            codes(&db)
        );
    }
    assert_surfaces_agree(&live, &db, "the map-literal key checks under strict-ink");
}

/// The control: on an ink file the two surfaces agree exactly, under both
/// dialects — unaffected by #1358, since native-only classification cannot
/// change an ink file's diagnostics. This is what scoped #1347 to native
/// files, and it stays green throughout.
#[test]
fn ink_files_agree_on_both_surfaces() {
    for dialect in [Dialect::StrictInk, Dialect::Brink] {
        let (live, db) =
            both_surfaces("main.ink", INK_PLAIN, dialect).expect("session produced an analysis");
        assert_surfaces_agree(&live, &db, &format!("an ink file under {dialect:?}"));
    }
}

fn has(diags: &[Diagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == code)
}

/// Review finding on PR #2901 (issue #1944, `E185`): the PR's own
/// integration tests drove only the db-direct road (`brink-db`'s
/// `db.diagnostics`, and `brink-compiler`'s `compile_with_options` ->
/// `brink-driver` -> `ProjectDb`) while the PR body claimed "Both analysis
/// roads ... Proved directly, not just structurally". Both call sites do
/// converge on the same `brink_analyzer::strict_diagnostics` seam (verified
/// by reading both), but this module's whole premise — since #1358 — is
/// that convergence at one shared seam does not by itself guarantee the two
/// *entry points* into it agree; CLAUDE.md's "exercise both roads" rule
/// wants a direct proof through the off-db road too, the same way every
/// other test in this file proves it through `IdeSession::analysis`
/// (`IdeSnapshot::analyze`/`analyze_with_modules` under the hood) rather
/// than only through `session.db()`.
#[test]
fn e185_reaches_both_surfaces_under_strict() {
    let src = "STRUCT Point = #{x: float, y: float}\n\
               VAR p: Point = Point#{x: 0.0, y: 0.0}\n\
               === main ===\n~ p.bogus = 1\n-> DONE\n";
    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    session.update_and_analyze("main.ink", src.to_owned());
    session.set_type_policy(TypePolicy::Strict);

    let live = session
        .analysis()
        .expect("session produced an analysis")
        .diagnostics
        .clone();
    let db = session.db().analysis().diagnostics.clone();

    assert!(
        has(&live, DiagnosticCode::E185),
        "the off-db road (IdeSession::analysis, IdeSnapshot::analyze/analyze_with_modules \
         under the hood) must report E185 directly, not merely by structural argument about a \
         shared seam; live saw {:?}",
        codes(&live)
    );
    assert_surfaces_agree(&live, &db, "E185 under strict types, brink dialect");
}

/// Issue #2906's own both-roads proof, extending
/// `e185_reaches_both_surfaces_under_strict` above the same way PR #2901's
/// review asked: same off-db-road/db-road structure, but `p` is an
/// *unannotated* `~ temp` initialized from a construction literal rather
/// than an annotated `VAR` — the exact recording-site gap #2906 closes.
#[test]
fn e185_on_unannotated_temp_initializer_reaches_both_surfaces_under_strict_issue_2906() {
    let src = "STRUCT Point = #{x: float, y: float}\n\
               === main ===\n~ temp p = Point#{x: 0.0, y: 0.0}\n~ p.bogus = 1\n-> DONE\n";
    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    session.update_and_analyze("main.ink", src.to_owned());
    session.set_type_policy(TypePolicy::Strict);

    let live = session
        .analysis()
        .expect("session produced an analysis")
        .diagnostics
        .clone();
    let db = session.db().analysis().diagnostics.clone();

    assert!(
        has(&live, DiagnosticCode::E185),
        "the off-db road must report E185 for an unannotated-temp-initializer \
         receiver just like it does for an annotated VAR; live saw {:?}",
        codes(&live)
    );
    assert_surfaces_agree(
        &live,
        &db,
        "E185 on an unannotated temp initializer under strict types, brink dialect",
    );
}
