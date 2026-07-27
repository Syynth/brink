//! Characterization of the live-typing vs. db diagnostic divergence
//! (issue #1347, `needs-design`).
//!
//! `IdeSession`'s editor-facing analysis runs *off* the db —
//! `snapshot().analyze()` → `brink_analyzer::analyze_with_modules(…, false)` —
//! while a compile and every direct db query run
//! `per_file_diagnostics_query`, which knows each file's *path* and therefore
//! its `Language`. The two disagree on native `.brink` files, in both
//! directions.
//!
//! **These tests deliberately pin behavior that is wrong.** They are not a
//! statement that the current split is correct; they exist so #1347 can be
//! ruled on against a measured inventory rather than assumptions, and so the
//! inventory cannot silently rot. Every assertion below carries the direction
//! it will move in once the ruling lands:
//!
//! - a *false negative* assertion (`db` has a code, live typing does not)
//!   flips when live typing gains the check;
//! - the *false positive* assertion (live typing invents `E051` on native
//!   syntax) flips when the T1b dialect gate stops running on native files
//!   off-db.
//!
//! Either way the failure message names #1347, so whoever moves the seam is
//! told which side of it they moved. See
//! `docs/live-typing-diagnostics-divergence.md` for the full analysis.

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

/// A plain ink file — the control. The divergence is native-only, and this
/// is what proves it.
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

fn has(diags: &[Diagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == code)
}

fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
    let mut v: Vec<DiagnosticCode> = diags.iter().map(|d| d.code).collect();
    v.sort_by_key(|c| format!("{c:?}"));
    v.dedup();
    v
}

/// **False negative.** `E137` (B0.9 native strict-only, issue #1342) has no
/// pure-path call site at all: `brink_analyzer::finish_analysis` never calls
/// `native_strict_only_error`, only `per_file_diagnostics_query` does. So an
/// author typing in a native file under an explicit `types = gradual` never
/// sees it until they compile — the exact symptom #1347 was filed for.
///
/// Under #1347's resolution this assertion inverts: live typing gains `E137`.
#[test]
fn e137_reaches_the_db_but_not_live_typing() {
    for dialect in [Dialect::StrictInk, Dialect::Brink] {
        let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, dialect)
            .expect("session produced an analysis");
        assert!(
            has(&db, DiagnosticCode::E137),
            "#1347 inventory drift: the db lost E137 under {dialect:?}; db saw {:?}",
            codes(&db)
        );
        assert!(
            !has(&live, DiagnosticCode::E137),
            "#1347 appears resolved for E137 under {dialect:?} — live typing now sees it. \
             Update docs/live-typing-diagnostics-divergence.md and this test; live saw {:?}",
            codes(&live)
        );
    }
}

/// **False negative.** `E084` (struct construction-literal duplicate field) is
/// gated on `dialect == Brink || is_native` inside
/// `brink_analyzer::per_file_diagnostics`. The pure path always supplies
/// `is_native = false`, so under the *default* `strict-ink` dialect — which is
/// what `EditorSession` starts at — a native file's duplicate field is a real
/// compile error that live typing never reports.
///
/// Declaring `dialect = brink` masks it (the `|| is_native` arm stops
/// mattering), which is why the `Brink` half of this test asserts agreement:
/// it isolates `is_native` as the variable, not the dialect.
#[test]
fn e084_on_a_native_file_needs_is_native_under_the_default_dialect() {
    let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, Dialect::StrictInk)
        .expect("session produced an analysis");
    assert!(
        has(&db, DiagnosticCode::E084),
        "#1347 inventory drift: the db lost E084; db saw {:?}",
        codes(&db)
    );
    assert!(
        !has(&live, DiagnosticCode::E084),
        "#1347 appears resolved for E084 — live typing now sees it under strict-ink. \
         Update docs/live-typing-diagnostics-divergence.md and this test; live saw {:?}",
        codes(&live)
    );

    // Same file under `brink`: both surfaces agree, because the dialect arm
    // of the gate is satisfied without `is_native`.
    let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, Dialect::Brink)
        .expect("session produced an analysis");
    assert!(
        has(&live, DiagnosticCode::E084) && has(&db, DiagnosticCode::E084),
        "under the brink dialect E084 must reach both surfaces; live {:?}, db {:?}",
        codes(&live),
        codes(&db)
    );
}

/// **False positive — the sharper half of #1347, and the one the issue did
/// not know about.** The T1b dialect gate is an ink-only axis: a native
/// `.brink` file's own grammar *is* the superset grammar the gate polices, so
/// `per_file_diagnostics_query` skips it via `is_native` (#1348). The pure
/// path cannot, so ordinary native syntax draws an `E051` squiggle in the
/// editor that vanishes on compile.
///
/// `EditorSession` defaults to `strict-ink`, so this is the default
/// experience for a `.brink` file with no declared dialect — not an edge case.
///
/// Under #1347's resolution this assertion inverts: live typing stops
/// emitting `E051` here.
#[test]
fn live_typing_invents_e051_on_native_syntax_the_db_accepts() {
    let (live, db) = both_surfaces("main.brink", NATIVE_DUP_FIELD, Dialect::StrictInk)
        .expect("session produced an analysis");
    assert!(
        has(&live, DiagnosticCode::E051),
        "#1347 appears resolved for the E051 false positive — live typing no longer \
         rejects native syntax under strict-ink. Update \
         docs/live-typing-diagnostics-divergence.md and this test; live saw {:?}",
        codes(&live)
    );
    assert!(
        !has(&db, DiagnosticCode::E051),
        "#1347 inventory drift: the db now emits E051 on a native file, which #1348 \
         ruled it must not; db saw {:?}",
        codes(&db)
    );
}

/// **False negatives, not previously pinned by this suite.** `E106`
/// (map-literal key domain) and `E138` (map-literal duplicate key) are gated
/// at the same `dialect == Brink || is_native` block as `E084`
/// (`crates/internal/brink-analyzer/src/lib.rs:634-651`), so both are also
/// missing from live typing on a native file under the default `strict-ink`
/// dialect — the same pure-path `is_native = false` blind spot
/// `e084_on_a_native_file_needs_is_native_under_the_default_dialect` pins for
/// `E084`.
///
/// Under #1347's resolution these assertions invert: live typing gains
/// `E106` and `E138`.
#[test]
fn e106_and_e138_map_literal_checks_missing_from_live_typing_under_default_dialect() {
    let (live, db) = both_surfaces("main.brink", NATIVE_MAP_KEY_ISSUES, Dialect::StrictInk)
        .expect("session produced an analysis");
    for code in [DiagnosticCode::E106, DiagnosticCode::E138] {
        assert!(
            has(&db, code),
            "#1347 inventory drift: the db lost {code:?} under strict-ink; db saw {:?}",
            codes(&db)
        );
        assert!(
            !has(&live, code),
            "#1347 appears resolved for {code:?} — live typing now sees it under \
             strict-ink. Update docs/live-typing-diagnostics-divergence.md and this \
             test; live saw {:?}",
            codes(&live)
        );
    }
}

/// The control: on an ink file the two surfaces agree exactly, under both
/// dialects. This is what scopes #1347 to native files — and what says the
/// ink corpus (and therefore the oracle) is untouched by whichever way the
/// issue is resolved.
#[test]
fn ink_files_agree_on_both_surfaces() {
    for dialect in [Dialect::StrictInk, Dialect::Brink] {
        let (live, db) =
            both_surfaces("main.ink", INK_PLAIN, dialect).expect("session produced an analysis");
        assert_eq!(
            codes(&live),
            codes(&db),
            "an ink file must analyze identically on both surfaces under {dialect:?}"
        );
    }
}
