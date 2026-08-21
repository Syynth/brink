//! Issue #2320 — end-to-end regression at the exact shape `brink-lsp`'s
//! persistent `analysis_loop` uses in production: a `ProjectDb` with an
//! explicit, absolute `native_root` (LSP always keys `ProjectDb` by
//! absolute OS path — see `brink_db::modules::root_relative_key`'s own
//! doc), absolute-keyed files, and a *relative* `[project] conventions`
//! pointer (`"conventions.brink"`) — resolved while the process's own cwd
//! is a **different, nested** directory (`native_root/scenes`), mirroring
//! the issue's own repro: `native_root=/project` launched from cwd
//! `/project/scenes`.
//!
//! Before the fix, `root_relative_key` absolutized the relative pointer
//! against the process's cwd rather than `native_root`, so it resolved to
//! `scenes/conventions.brink` instead of `conventions.brink` — a module
//! name (`story::scenes::conventions`) no real file in the project has.
//! `conventions_confinement_diagnostics_query`'s "does not match any file"
//! guard then took over and silently skipped confinement entirely (a bare
//! `tracing::warn!` — see that query's own doc, and `crates/internal/
//! brink-analyzer/src/conventions_confinement.rs`'s module doc for why
//! this is a `brink-db`-owned resolution problem, not an analyzer one): a
//! claim handler declared OUTSIDE the real conventions module went
//! completely unflagged, exactly the silent-confinement-skip the issue
//! reports for the LSP's persistent session.
//!
//! `is_1844_conventions_module_fence.rs`'s
//! `only_the_non_configured_file_is_flagged_among_siblings` is the same
//! scenario shape with no `native_root` (i.e. `discover_native`'s CLI-shaped
//! already-root-relative keys) — this file is its LSP-shaped, `native_root`
//! + nested-cwd counterpart.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;
use std::path::PathBuf;

const CLAIMING_HANDLER: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
    fn interior(place: content) {\n  return place;\n}\n";

/// Restores the real process cwd on every exit path (including a
/// mid-assertion panic) so this test can never leave a poisoned cwd for
/// whichever test `cargo nextest` schedules next in this process — the gate
/// this repo runs (`CLAUDE.md`) executes each test in its own process, so
/// this chdir cannot pollute a sibling test either way, but nothing here
/// should depend on that isolation to be correct.
struct RestoreCwd(PathBuf);
impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = format!(
        "{}-{}-{}",
        std::process::id(),
        label,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    std::env::temp_dir().join(format!("brink-issue-2320-{unique}"))
}

#[test]
fn conventions_confinement_survives_a_relative_pointer_with_native_root_and_a_nested_lsp_cwd() {
    let original_cwd = std::env::current_dir().expect("process must have a cwd");
    let _restore = RestoreCwd(original_cwd);

    // native_root = <tmp>/project
    // process cwd = <tmp>/project/scenes  (a SUBDIRECTORY of native_root —
    // exactly the issue's own "launched from cwd /project/scenes" repro)
    let native_root = unique_temp_dir("native-root");
    let nested_cwd = native_root.join("scenes");
    std::fs::create_dir_all(&nested_cwd).expect("create the nested LSP-launch-cwd dir");
    std::env::set_current_dir(&nested_cwd).expect("chdir into the nested LSP-launch cwd");

    let mut db = ProjectDb::new();
    db.set_native_root(Some(native_root.to_string_lossy().into_owned()));
    db.set_analysis_options(AnalysisOptions {
        conventions: Some("conventions.brink".to_owned()),
        ..AnalysisOptions::default()
    });

    // The REAL conventions module — absolute-keyed, directly under
    // `native_root`, exactly as the LSP registers every file.
    db.set_file(
        &native_root
            .join("conventions.brink")
            .to_string_lossy(),
        "flow other() {\n  hi\n}\n".to_owned(),
    );
    // A claim handler declared OUTSIDE the conventions module — this must
    // be flagged E169. Also absolute-keyed, nested under `scenes/` (the
    // same directory name as the decoy cwd, deliberately, since that's
    // exactly the string the pre-fix bug spuriously prepended to the
    // pointer).
    db.set_file(
        &native_root
            .join("scenes/heading.brink")
            .to_string_lossy(),
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );

    let diags = db.analysis().diagnostics.clone();

    let e169: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::E169)
        .collect();
    assert_eq!(
        e169.len(),
        1,
        "a relative `[project] conventions` pointer must resolve against \
         `native_root`, not the process's (nested) cwd — a claim handler \
         outside the real conventions module must still be flagged E169. \
         Got diagnostics: {diags:?}"
    );
    assert!(
        e169[0].message.contains("interior"),
        "{:?}",
        e169[0].message
    );
    assert!(
        e169[0].message.contains("conventions.brink"),
        "{:?}",
        e169[0].message
    );

    let _ = std::fs::remove_dir_all(&native_root);
}
