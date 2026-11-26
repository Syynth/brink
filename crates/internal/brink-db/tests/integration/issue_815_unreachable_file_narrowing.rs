//! Issue #815 (FG-4 train): `IncludeGraph::topological_order`'s "append
//! every unreached file" fallback fed every project file's HIR into
//! `lir_lowering_query`'s inputs — not just `entry` and its transitive
//! `INCLUDE` closure.
//!
//! `brink-driver::discover` (the CLI/oracle-corpus path) never produces this
//! shape: it only ever loads `entry` plus files it transitively `INCLUDE`s,
//! so the fallback was always a no-op there (which is why narrowing it
//! leaves the oracle corpus byte-identical). The shape the fallback actually
//! mattered for is `ProjectDb`'s other use as the long-lived studio/LSP
//! model, where files are added independently of any one entry point (open
//! editor tabs, a directory scan) and a wholly unrelated second story can
//! coexist with `entry` in the same database with no `INCLUDE` edge between
//! them at all.
//!
//! Two properties must both hold after narrowing:
//! 1. An unreachable file's declarations (globals, knots, …) must not leak
//!    into `entry`'s compiled `Program`.
//! 2. That unreachable file's own diagnostics must still be reported — they
//!    run as independent per-file passes over the project's full file set
//!    (`analysis_diagnostics_query`/`diagnostics_query`), never through
//!    `topological_order`, so narrowing LIR's inputs must not touch them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;
use brink_ir::DiagnosticCode;

/// Property 1: a global declared only in a file with no `INCLUDE`
/// relationship to `entry` must not appear in `entry`'s compiled `Program`.
#[test]
fn unreachable_file_globals_do_not_leak_into_entrys_program() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR reachableVar = 1\n=== main ===\nHello.\n-> END\n".to_owned(),
    );
    // A second, wholly independent story with no INCLUDE relationship to
    // main.ink whatsoever — exactly the shape a studio/LSP session produces
    // when an unrelated .ink file is open in the same workspace.
    db.set_file(
        "orphan.ink",
        "VAR orphanVar = 2\n=== orphan ===\nUnrelated.\n-> END\n".to_owned(),
    );
    db.set_entry("main.ink");

    let product = db.lir_product().expect("entry set");
    assert!(
        product.errors.is_empty(),
        "fixture must compile cleanly: {:?}",
        product.errors
    );
    let program = product
        .program
        .as_ref()
        .expect("clean compile must produce a program");

    let global_names: Vec<&str> = program
        .globals
        .iter()
        .map(|g| program.name_table[g.name.0 as usize].as_str())
        .collect();
    assert!(
        global_names.contains(&"reachableVar"),
        "entry's own global must still lower: {global_names:?}"
    );
    assert!(
        !global_names.contains(&"orphanVar"),
        "issue #815: an unrelated, unreachable file's global leaked into \
         entry's compiled program: {global_names:?}"
    );
}

/// Property 2: narrowing LIR's inputs must not silence an unreachable
/// file's own diagnostics — they're computed independently of
/// `topological_order`/`lir_lowering_query`.
#[test]
fn unreachable_file_diagnostics_still_fire_after_narrowing() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR reachableVar = 1\n=== main ===\nHello.\n-> END\n".to_owned(),
    );
    let orphan_id = db.set_file(
        "orphan.ink",
        "=== orphan ===\nUnrelated.\n-> nonexistent\n".to_owned(),
    );
    db.set_entry("main.ink");

    let orphan_diags = db
        .diagnostics(orphan_id)
        .expect("orphan.ink is a live file");
    assert!(
        orphan_diags.iter().any(|d| d.code == DiagnosticCode::E024),
        "issue #815: orphan.ink's own unresolved-divert diagnostic must \
         still fire even though it's unreachable from entry: {orphan_diags:?}"
    );
}
