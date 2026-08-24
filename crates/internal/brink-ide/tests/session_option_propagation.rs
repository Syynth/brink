//! `IdeSession`'s registered options must reach its **own** `ProjectDb`, and
//! the db-only module diagnostics must reach the editor (issue #1553).
//!
//! Since option A total (2026-08-24) EVERY session read is a db query —
//! `session.analysis()` included, alongside the direct reads this suite
//! always covered (`db.per_file_diagnostics`, `db.symbol_index`,
//! `db.diagnostics`, `db.effects`) — and every one is gated on the
//! `AnalysisOptions` *input* written into the db. Before #1553 only
//! `IdeSession::compile` ever wrote that input, so an editor that never
//! compiled read every db query under `AnalysisOptions::default()`:
//! the declared dialect, typed-mode policy, `[lints]` table and host manifest
//! were silently absent on exactly the surface the editor renders from.

use brink_analyzer::{Dialect, TypePolicy};
use brink_ide::session::IdeSession;
use brink_ir::DiagnosticCode;

/// A native `.brink` file — the B0.9 strict-only check (`E137`) is the
/// narrowest observable tell that `AnalysisOptions::types` reached the db:
/// `per_file_diagnostics_query` fires it only for an *explicit*
/// `types = gradual`, so the unset default can never produce it.
const NATIVE: &str = "\
flow start() {
  The market is busy.
}
";

/// `db.per_file_diagnostics` must see the session's explicit
/// `types = gradual`, which on a native file is `E137`.
#[test]
fn explicit_gradual_reaches_the_db_native_strict_only_check() {
    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    let file = session.update_and_analyze("main.brink", NATIVE.to_owned());
    session.set_type_policy(TypePolicy::Gradual);

    let diags = session
        .db()
        .per_file_diagnostics(file)
        .expect("file is loaded");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E137),
        "the session's explicit `types = gradual` never reached its own db: {diags:?}"
    );
}

/// The dialect reaches the db too — M-2d cross-module duplicate coexistence
/// (`symbol_index_query`) is `brink`-only, so under the pre-#1553 default
/// (`strict-ink`) the second `greet` was dropped as an `E022` duplicate in
/// the db's index even though the session declared `brink`.
#[test]
fn declared_dialect_reaches_the_db_symbol_index() {
    const A: &str = "#@module(alpha)\n=== greet ===\nHello.\n-> END\n";
    const B: &str = "#@module(beta)\n=== greet ===\nHi.\n-> END\n";

    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    session.update_source("a.ink", A.to_owned());
    session.update_and_analyze("b.ink", B.to_owned());

    let index = session.db().symbol_index();
    let greets = index.by_name.get("greet").map_or(0, Vec::len);
    assert_eq!(
        greets, 2,
        "under `brink` the two declared modules' `greet`s coexist; the db kept \
         {greets} — its index still ran under the default dialect"
    );
    let db_diags = session.db().symbol_index_diagnostics();
    assert!(
        !db_diags.iter().any(|d| d.code == DiagnosticCode::E022),
        "brink-dialect cross-module duplicates must not warn in the db either: {db_diags:?}"
    );
}

/// `E085` (two files landing on the same module name — one declared, one by
/// stem) is produced by `module_map_query`, *not* by the analyzer pass the
/// session runs off-db. It has to be folded back in, or a collision a
/// db-driven compile catches never reaches the editor.
#[test]
fn stem_collision_diagnostics_reach_the_session_analysis() {
    // `alpha.ink` has no `#@module`, so its module is its stem, `alpha` —
    // which `other.ink` declares.
    const ALPHA: &str = "=== greet ===\nHello.\n-> END\n";
    const OTHER: &str = "#@module(alpha)\n=== farewell ===\nBye.\n-> END\n";

    let mut session = IdeSession::new();
    session.set_language_dialect(Dialect::Brink);
    session.update_source("alpha.ink", ALPHA.to_owned());
    session.update_and_analyze("other.ink", OTHER.to_owned());

    let analysis = session.analysis().expect("analysis");
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E085),
        "the stem-collision diagnostic never reached the editor: {:?}",
        analysis.diagnostics
    );
}
