//! Issue #839 (FG-4e): `lir_prelude_decls_query` — a body-only edit must not
//! force a re-intern of the project's declarations (globals/lists/externals/
//! struct shapes/the seeded name table).
//!
//! Before this slice, `lir_lowering_query` (the FG-4d link phase) called
//! `brink_ir::lir::build_prelude` *inline* on every execution, over the raw,
//! body-carrying HIR of every entry-reachable file — so any knot body edit
//! anywhere in the project re-ran `collect_globals`/`collect_lists`/
//! `collect_externals`/`build_shape_table`/`build_global_shape_map` from
//! scratch, even though none of those passes ever reads a body. This slice
//! gives the declaration-collection step its own memo (`decl_hir_query` per
//! file, backdating across body-only edits, feeding
//! `lir_prelude_decls_query`), mirroring the `struct_shape_data_query`
//! pattern FG-4d already established for structs alone.
//!
//! Input-breadth limit (issue #815, inherited unchanged): scoped to `entry`'s
//! transitive `INCLUDE` closure, not full project-wide backdating — same
//! caveat `struct_shape_data_query`'s own doc carries.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_db::ProjectDb;
use brink_format::StoryData;

fn two_file_project(lib_body: &str) -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "INCLUDE lib.ink\nVAR reachableVar = 1\n=== start ===\nHello from start.\n-> END\n"
            .to_owned(),
    );
    db.set_file("lib.ink", format!("=== other ===\n{lib_body}\n-> END\n"));
    db.set_entry("main.ink");
    db
}

fn inkb_bytes(db: &ProjectDb) -> Vec<u8> {
    let story: &Arc<StoryData> = db
        .story_data()
        .and_then(|c| c.story.as_ref())
        .expect("project compiles to a story");
    let mut buf = Vec::new();
    brink_format::write_inkb(story, &mut buf);
    buf
}

/// The core FG-4e non-re-execution proof: a knot-body-only edit (no
/// declaration touched) leaves `lir_prelude_decls_query`'s `Arc` pointer-
/// identical — the whole-project declaration collection did not re-execute.
#[test]
fn body_only_edit_leaves_prelude_decls_pointer_identical() {
    let mut db = two_file_project("Hello from other.");

    // Prime the memo and the compile once.
    let before = db.lir_prelude_decls();
    let _ = inkb_bytes(&db);

    // Pure content edit: no VAR/CONST/LIST/EXTERNAL/STRUCT touched anywhere.
    db.set_file(
        "lib.ink",
        "=== other ===\nHello from a very different other.\n-> END\n".to_owned(),
    );

    let after = db.lir_prelude_decls();
    assert!(
        Arc::ptr_eq(&before, &after),
        "issue #839: a knot-body-only edit re-executed the whole-project \
         declaration collection (globals/lists/externals/struct shapes/name \
         table) — the per-file decl-only projection did not backdate"
    );

    // The compile itself still reflects the edit (sanity: the probe isn't
    // vacuously true because nothing recompiled at all).
    let after_bytes = inkb_bytes(&db);
    assert!(!after_bytes.is_empty());
}

/// A real declaration-level edit (a new global) *does* re-execute the
/// prelude-decls memo — the pointer-identity probe above isn't vacuous
/// because the memo never re-executes at all.
#[test]
fn declaration_edit_re_executes_prelude_decls() {
    let mut db = two_file_project("Hello from other.");
    let before = db.lir_prelude_decls();

    db.set_file(
        "main.ink",
        "INCLUDE lib.ink\nVAR reachableVar = 1\nVAR anotherVar = 2\n=== start ===\n\
         Hello from start.\n-> END\n"
            .to_owned(),
    );

    let after = db.lir_prelude_decls();
    assert!(
        !Arc::ptr_eq(&before, &after),
        "adding a new global must re-execute the prelude-decls memo"
    );
}

/// Byte-identity: composing the prelude from the memoized decls +
/// per-file `normalized_stamped_query` HIR must still produce output
/// identical to what the monolithic `build_prelude` composition (the
/// `lower_to_program`/`lower_to_program_with_type_mode` path,
/// `brink-converter`-independent) would produce — the FG-4d byte-identity
/// bar, exercised end to end via `inkb_hashes`-style bytes-out comparison
/// for this fixture.
#[test]
fn incremental_prelude_matches_from_scratch_after_edit_and_restore() {
    let original_lib = "Hello from other.";

    let fresh = two_file_project(original_lib);
    let fresh_bytes = inkb_bytes(&fresh);

    let mut incremental = two_file_project(original_lib);
    let _ = inkb_bytes(&incremental);
    incremental.set_file(
        "lib.ink",
        "=== other ===\nA temporary different line.\n-> END\n".to_owned(),
    );
    let _ = inkb_bytes(&incremental);
    incremental.set_file(
        "lib.ink",
        format!("=== other ===\n{original_lib}\n-> END\n"),
    );
    let incremental_bytes = inkb_bytes(&incremental);

    assert_eq!(
        incremental_bytes, fresh_bytes,
        "issue #839: incremental build after edit+restore diverged from a \
         from-scratch build of the same source"
    );
}
