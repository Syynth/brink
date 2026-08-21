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
//! Before the fix, the db road's `expected_conventions_module` routed the
//! pointer through `root_relative_key`, which cwd-absolutizes a relative
//! path (correctly so — for REGISTERED FILE KEYS, whose relative spellings
//! are cwd-relative by contract; `crates/brink-compiler/tests/
//! issue_1504_root_content_identity.rs` pins that). A relative
//! `conventions` pointer has the opposite contract — it is written in
//! `brink.toml`, whose directory defines the root, so relative means
//! root-relative by definition — and the shared routing resolved it to
//! `scenes/conventions.brink` instead of `conventions.brink`: a module
//! name (`story::scenes::conventions`) no real file in the project has.
//! `conventions_confinement_diagnostics_query`'s "does not match any file"
//! guard then took over: pre-#2320 that meant confinement was silently
//! skipped (a bare `tracing::warn!`), and post-#2320 it means the
//! pointer-unresolvable `E169` fires instead of the confinement `E169` —
//! either way the wrong outcome for a correctly configured project. The
//! fix gives the pointer its own resolver
//! (`brink_db::modules::conventions_pointer_key`, read at
//! `expected_conventions_module`) that never consults the process cwd,
//! leaving `root_relative_key`'s file-key semantics untouched.
//!
//! `issue_1844_conventions_module_fence.rs`'s
//! `only_the_non_configured_file_is_flagged_among_siblings` is the same
//! scenario shape with no `native_root` (i.e. `discover_native`'s CLI-shaped
//! already-root-relative keys) — this file is its LSP-shaped, `native_root`
//! + nested-cwd counterpart, on both analysis roads.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

const CLAIMING_HANDLER: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
    fn interior(place: content) {\n  return place;\n}\n";

/// Serializes the tests in this file (mirroring
/// `issue_1504_root_content_identity.rs`'s `cwd_lock`): both tests `chdir`
/// into a decoy directory, and `std::env::set_current_dir` is
/// process-global — under a plain threaded `cargo test` (as opposed to
/// `cargo nextest`'s process-per-test) two tests in this binary would
/// otherwise race on the cwd.
fn cwd_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Restores the real process cwd on every exit path (including a
/// mid-assertion panic) so a test can never leave a poisoned cwd behind —
/// dropped (and so restored) before `cwd_lock`'s guard is released, since
/// it is declared after the guard.
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

/// A `ProjectDb` in the LSP's production shape — absolute `native_root`,
/// absolute-keyed files, relative `[project] conventions` pointer — with
/// the real conventions module directly under the root and a claim handler
/// declared in a `scenes/`-nested sibling (the same directory name as the
/// decoy cwd, deliberately: that is exactly the segment the pre-fix bug
/// spuriously prepended to the pointer).
fn lsp_shaped_db(native_root: &std::path::Path) -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_native_root(Some(native_root.to_string_lossy().into_owned()));
    db.set_analysis_options(AnalysisOptions {
        conventions: Some("conventions.brink".to_owned()),
        ..AnalysisOptions::default()
    });
    db.set_file(
        &native_root.join("conventions.brink").to_string_lossy(),
        "flow other() {\n  hi\n}\n".to_owned(),
    );
    db.set_file(
        &native_root.join("scenes/heading.brink").to_string_lossy(),
        format!("{CLAIMING_HANDLER}flow main() {{\n  INT. MARKET SQUARE\n}}\n"),
    );
    db
}

/// The one `E169` the scenario must produce: the *confinement* diagnostic
/// against the out-of-module handler — specifically NOT the
/// pointer-unresolvable `E169` a mis-resolved pointer would produce with
/// the same handler name and pointer string in its message (adversarial
/// review finding F2 on the first attempt: substring assertions alone
/// could not tell the two arms apart, so the test passed even when
/// resolution regressed).
fn assert_confinement_e169(diags: &[Diagnostic]) {
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
    let message = &e169[0].message;
    assert!(message.contains("interior"), "{message:?}");
    assert!(message.contains("conventions.brink"), "{message:?}");
    // The arm-discriminating assertions: the confinement message ("move it
    // there"), never the pointer-unresolvable one — the latter is exactly
    // what fires when the pointer mis-resolves to a nonexistent module.
    assert!(
        message.contains("may only be declared"),
        "expected the confinement E169 (\"may only be declared…\"), not the \
         pointer-unresolvable arm — got {message:?}"
    );
    assert!(
        !message.contains("does not match any file"),
        "the pointer-unresolvable E169 fired, which means the relative \
         pointer failed to resolve against native_root — got {message:?}"
    );
}

/// The DB-DIRECT road (`ProjectDb::analysis`, what the studio's Problems
/// panel renders): confinement must fire correctly with `native_root` set
/// and the process cwd nested inside it.
#[test]
fn conventions_confinement_survives_a_relative_pointer_with_native_root_and_a_nested_lsp_cwd() {
    let _cwd_guard = cwd_lock();
    let original_cwd = std::env::current_dir().expect("process must have a cwd");
    let _restore = RestoreCwd(original_cwd);

    // native_root = <tmp>/project
    // process cwd = <tmp>/project/scenes  (a SUBDIRECTORY of native_root —
    // exactly the issue's own "launched from cwd /project/scenes" repro)
    let native_root = unique_temp_dir("native-root");
    let nested_cwd = native_root.join("scenes");
    std::fs::create_dir_all(&nested_cwd).expect("create the nested LSP-launch-cwd dir");
    std::env::set_current_dir(&nested_cwd).expect("chdir into the nested LSP-launch cwd");

    let db = lsp_shaped_db(&native_root);
    let diags = db.analysis().diagnostics.clone();
    assert_confinement_e169(&diags);

    let _ = std::fs::remove_dir_all(&native_root);
}

/// The OFF-DB road (issue #2320's wave-145 ask): the same scenario through
/// `brink_analyzer::analyze_with_modules` — the exact composition
/// `IdeSnapshot::analyze` runs (inputs + the db's `module_map()` + the
/// snapshot's raw `conventions` pointer; see
/// `crates/internal/brink-ide/src/session.rs`) and the road `brink-lsp`'s
/// `analysis_loop` takes in production. This road resolves the pointer via
/// the analyzer's own `native_module_path(pointer)` with no cwd in the
/// computation at all, so a relative pointer was never cwd-mangled here —
/// this test pins that behavior AND that the two roads agree on the
/// LSP-shaped scenario, so neither can drift without failing here.
///
/// (Known, pre-existing both-roads drift for an ABSOLUTE pointer — the db
/// road strips it to a root-relative key, the off-db road mints a module
/// from the absolute path verbatim — is out of this test's scope and
/// tracked on issue #2320.)
#[test]
fn off_db_road_agrees_with_native_root_and_a_nested_lsp_cwd() {
    let _cwd_guard = cwd_lock();
    let original_cwd = std::env::current_dir().expect("process must have a cwd");
    let _restore = RestoreCwd(original_cwd);

    let native_root = unique_temp_dir("off-db-road");
    let nested_cwd = native_root.join("scenes");
    std::fs::create_dir_all(&nested_cwd).expect("create the nested LSP-launch-cwd dir");
    std::env::set_current_dir(&nested_cwd).expect("chdir into the nested LSP-launch cwd");

    let db = lsp_shaped_db(&native_root);

    // Mirror `IdeSnapshot::analyze`'s composition off the same db the
    // db-road test uses: cloned inputs, the db's module map, the raw
    // relative pointer.
    let inputs = db.analysis_inputs();
    let refs: Vec<_> = inputs
        .iter()
        .map(|(id, hir, manifest)| (*id, hir, manifest))
        .collect();
    let opts = AnalysisOptions {
        conventions: Some("conventions.brink".to_owned()),
        ..AnalysisOptions::default()
    };
    let modules = db.module_map().clone();
    let off_db = brink_analyzer::analyze_with_modules(&refs, &modules, &opts, true);
    assert_confinement_e169(&off_db.diagnostics);

    // Both roads, same scenario, same verdict.
    let db_road = db.analysis().diagnostics.clone();
    let e169 = |diags: &[Diagnostic]| -> Vec<(brink_ir::FileId, String)> {
        diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E169)
            .map(|d| (d.file, d.message.clone()))
            .collect()
    };
    assert_eq!(
        e169(&db_road),
        e169(&off_db.diagnostics),
        "the db-direct and off-db roads must agree on the LSP-shaped \
         relative-pointer scenario"
    );

    let _ = std::fs::remove_dir_all(&native_root);
}
