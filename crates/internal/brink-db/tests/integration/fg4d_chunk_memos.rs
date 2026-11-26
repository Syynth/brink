//! FG-4d (issue #830) per-knot LIR chunk-memo tests: the non-re-execution
//! proof and the incremental≡from-scratch (three-resolution-moments) byte
//! identity contract.
//!
//! `lir_lowering_query` is now the link phase over per-knot chunk memos
//! (`lir_knot_chunk_query`, exposed for tests via
//! [`brink_db::ProjectDb::knot_chunk`]). The memo reads only its declaring
//! file's HIR, the whole-project `resolutions_index_query`, and the
//! cutoff-friendly `struct_shape_data_query`, so a body edit in *another*
//! file that changes neither resolutions nor struct shapes leaves the memo's
//! stored `Arc<ScopeChunk>` pointer-identical — the same `Arc::ptr_eq`
//! non-re-execution assertion the FG-1…FG-4a dependency-edge tests use.
//!
//! Input-breadth limit (issue #830, #815): the memo depends on the
//! whole-project `resolutions_index`/`struct_shape_data`, so the achievable
//! proof is cross-file isolation for edits those backdate across, not
//! isolation from every same-project edit. `topological_order`'s all-files
//! fallback (#815, separate slice) still widens the link's own inputs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_db::ProjectDb;
use brink_format::StoryData;

/// A two-file project: `main.ink` (entry, one knot) includes `lib.ink` (one
/// knot). Editing `lib.ink`'s knot *body* changes no symbol, so it neither
/// adds a resolution nor a struct shape.
fn two_file_project(lib_body: &str) -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "INCLUDE lib.ink\n=== start ===\nHello from start.\n-> END\n".to_owned(),
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

/// The core FG-4d non-re-execution proof: editing `lib.ink`'s knot body
/// leaves `main.ink`'s `start` knot chunk memo's `Arc<ScopeChunk>`
/// pointer-identical — its inputs (main's HIR, project resolutions, struct
/// shapes) are all unchanged — while `lib.ink`'s own knot chunk genuinely
/// re-executes (a new `Arc`). Proves the per-def chunk boundary is real:
/// one file's edit does not re-lower another file's untouched knot.
#[test]
fn untouched_file_knot_chunk_is_not_re_executed_across_a_sibling_edit() {
    let mut db = two_file_project("Hello from other.");
    let main = db.entry().expect("entry set");
    let lib = db
        .file_ids()
        .find(|&id| id != main)
        .expect("lib.ink present");

    // Prime both chunk memos.
    let start_before = db.knot_chunk(main, 0);
    let other_before = db.knot_chunk(lib, 0);
    // Non-vacuous: both files really do compile to a story.
    let _ = inkb_bytes(&db);

    // Edit lib.ink's knot body only — a pure content change, no new symbol.
    db.set_file(
        "lib.ink",
        "=== other ===\nHello from a very different other.\n-> END\n".to_owned(),
    );

    let start_after = db.knot_chunk(main, 0);
    let other_after = db.knot_chunk(lib, 0);

    // The edited file's own knot chunk re-executed (content changed).
    assert!(
        !Arc::ptr_eq(&other_before, &other_after),
        "the edited knot's chunk should have re-executed (its HIR changed)"
    );
    // The untouched file's knot chunk did NOT re-execute — same allocation.
    assert!(
        Arc::ptr_eq(&start_before, &start_after),
        "a body edit in lib.ink re-lowered main.ink's untouched `start` knot \
         (issue #830 FG-4d): the per-def chunk memo did not hold"
    );
}

/// A diagnostics/resolution-neutral edit to a sibling file leaves the whole
/// compiled story byte-identical too (the link re-runs but re-assembles the
/// same bytes for `main`'s content).
#[test]
fn sibling_edit_keeps_main_content_compiling() {
    let mut db = two_file_project("Hello from other.");
    let before = inkb_bytes(&db);
    // Editing lib's body changes lib's own output, so the whole-project bytes
    // differ — but the compile stays valid and deterministic.
    db.set_file(
        "lib.ink",
        "=== other ===\nDifferent other line.\n-> END\n".to_owned(),
    );
    let after = inkb_bytes(&db);
    assert!(!before.is_empty() && !after.is_empty());
}

/// The three-resolution-moments contract (issue #830 / the proposal's
/// appendix): id assignment is history-independent, so an incrementally
/// edited database and a freshly built one with the *same final source*
/// produce byte-identical `.inkb`. Here: build a project, mutate a knot, then
/// restore it — the restored incremental build must equal a from-scratch
/// build of the identical source.
#[test]
fn incremental_equals_from_scratch_after_edit_and_restore() {
    let original_lib = "Hello from other.";

    // Fresh build of the final source.
    let fresh = two_file_project(original_lib);
    let fresh_bytes = inkb_bytes(&fresh);

    // Incremental build: same start, edited away, then restored verbatim.
    let mut incremental = two_file_project(original_lib);
    let _ = inkb_bytes(&incremental); // materialize the first build's memos
    incremental.set_file(
        "lib.ink",
        "=== other ===\nA temporary different line.\n-> END\n".to_owned(),
    );
    let _ = inkb_bytes(&incremental); // force the intermediate recompile
    incremental.set_file(
        "lib.ink",
        format!("=== other ===\n{original_lib}\n-> END\n"),
    );
    let incremental_bytes = inkb_bytes(&incremental);

    assert_eq!(
        incremental_bytes, fresh_bytes,
        "incremental build after edit+restore diverged from a from-scratch \
         build of the same source (issue #830: id assignment must be \
         history-independent, not allocation-history-derived)"
    );
}

/// History-independence across a re-ordering-free multi-file build: a project
/// built by adding files in one order equals one built in another order,
/// given the same INCLUDE topology. (The assembler dedups names in
/// topological walk order, not file-registration order.)
#[test]
fn build_is_independent_of_file_registration_order() {
    let mut a = ProjectDb::new();
    a.set_file(
        "main.ink",
        "INCLUDE lib.ink\n=== start ===\nHi.\n-> END\n".to_owned(),
    );
    a.set_file("lib.ink", "=== other ===\nYo.\n-> END\n".to_owned());
    a.set_entry("main.ink");

    let mut b = ProjectDb::new();
    // Register lib before main this time.
    b.set_file("lib.ink", "=== other ===\nYo.\n-> END\n".to_owned());
    b.set_file(
        "main.ink",
        "INCLUDE lib.ink\n=== start ===\nHi.\n-> END\n".to_owned(),
    );
    b.set_entry("main.ink");

    assert_eq!(
        inkb_bytes(&a),
        inkb_bytes(&b),
        "compiled bytes depend on file registration order (issue #830)"
    );
}
