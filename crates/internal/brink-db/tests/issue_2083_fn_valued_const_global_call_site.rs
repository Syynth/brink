//! Issue #2083, db-direct road: calling a fn-valued global `const` from a
//! call site other than its own declaration must resolve cleanly through
//! the production `db.diagnostics(file)` seam — the same seam
//! `brink-compiler`'s `compile_path`/`compile_with_options` and the LSP's
//! `Driver::diagnostics` both read.
//!
//! RCA (see `crates/internal/brink-analyzer/src/resolve.rs`,
//! `resolve_function`'s "try variables" arm): the bug was never in
//! `brink-db`'s incremental machinery at all — a direct, `brink-db`-free
//! call to `brink_analyzer::resolve`/`analyze()` reproduced the identical
//! `E025` for this exact source, and `var twice = double` (as opposed to
//! `const twice = double`) already resolved cleanly before this fix. The
//! call-site lookup in `resolve_function` searched only
//! `SymbolKind::Variable`, never `SymbolKind::Constant` — a one-sided
//! omission relative to `resolve_variable`'s own bare-read lookup, which
//! already searches `[Variable, Constant]` together. Fixed at the
//! `brink-analyzer` layer, so both this db-direct road and the off-db
//! `IdeSnapshot::analyze` road (`brink-ide`'s
//! `issue_2083_fn_valued_const_global_call_site.rs`) share the one fix by
//! construction — there is no db-specific code to test here beyond proving
//! the production seam actually reaches it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

const SRC: &str = "\
fn double(n: int): int {
  return n * 2;
}

const twice = double

flow main() {
  Result: {twice(21)} -> END
}
";

/// The exact repro from issue #2083's report, driven through `ProjectDb`
/// directly (no `brink-compiler`/`Driver` wrapper) — the innermost seam the
/// issue's own RCA request pointed at.
#[test]
fn const_bare_name_fn_value_call_site_resolves_via_db_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", SRC.to_owned());
    db.set_entry("main.brink");

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E025),
        "a fn-valued CONST global's call site must resolve, got: {diags:?}"
    );
}

/// The lambda-literal sibling (#1774's decl-default form, rather than
/// #1862's bare-name form) — issue #2083 named both as reproducing
/// identically.
#[test]
fn const_lambda_literal_fn_value_call_site_resolves_via_db_diagnostics() {
    let mut db = ProjectDb::new();
    let file = db.set_file(
        "main.brink",
        "const twice = |x| x * 2\n\nflow main() {\n  Result: {twice(21)} -> END\n}\n".to_owned(),
    );
    db.set_entry("main.brink");

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E025),
        "a lambda-literal-valued CONST global's call site must resolve, got: {diags:?}"
    );
}

/// Incrementality (CLAUDE.md "beware incrementality" — a fix that resolves
/// on a full compile but breaks on an incremental re-analyze is half a fix):
/// load a clean file first, force `resolve_query`/`resolutions_index_query`
/// to memoize, edit the file's *unrelated* text (appending a never-called
/// `fn unrelated` that leaves `twice`'s declaration and call site byte-
/// identical while still re-running the lowering/indexing queries), and
/// re-read diagnostics — the second read must still be E025-free, not just
/// the first cold one.
#[test]
fn const_fn_value_call_site_stays_resolved_across_an_incremental_edit() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", SRC.to_owned());
    db.set_entry("main.brink");

    let first = db.diagnostics(file).expect("file diagnostics").to_vec();
    assert!(
        !first.iter().any(|d| d.code == DiagnosticCode::E025),
        "cold compile must resolve, got: {first:?}"
    );

    // An edit unrelated to `twice`/`double` — adds a second, never-called
    // function so `lowered_query`/`symbol_index_query` actually re-run for
    // this file, without touching the declaration or call site under test.
    let edited = format!("{SRC}\nfn unrelated(n: int): int {{\n  return n;\n}}\n");
    db.set_file("main.brink", edited);

    let second = db.diagnostics(file).expect("file diagnostics").to_vec();
    assert!(
        !second.iter().any(|d| d.code == DiagnosticCode::E025),
        "an incremental re-analyze after an unrelated edit must still resolve \
         `twice`'s call site, got: {second:?}"
    );
}
