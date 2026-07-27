//! Issue #460: the per-knot LIR chunk memos now read their knot-invariant
//! lowering environment (resolution lookup, struct-shape tables, file paths,
//! type mode) from one shared `chunk_lowering_ctx_query` instead of rebuilding
//! it inside every chunk memo.
//!
//! The claim that change rests on is *byte identity*: sharing one
//! `ChunkLoweringCtx` across every knot must produce exactly the artifact the
//! per-knot rebuild produced, cold and warm. These tests pin that on a
//! multi-file project whose context is non-trivial in every component the
//! hoist moved — cross-file diverts and function calls (so the resolution
//! lookup is populated), a `STRUCT` with a typed global (so the shape table
//! and the global-shape map are both non-empty), and three files (so the
//! `FileId`→path map has more than one entry).
//!
//! `incremental_fuzz.rs` is the broad randomized version of the same
//! contract over corpus projects; these are the focused, always-run cases for
//! this specific seam — including the access pattern `compile_bench`'s new
//! `projectdb_*` stage profile introduced, which pulls the intermediate
//! queries (`knot_chunk`, `lir_product`) *before* `story_data()`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect};
use brink_db::ProjectDb;
use brink_format::StoryData;

/// `STRUCT`/`#{…}` are brink-extension constructs: under the default
/// `Dialect::StrictInk` they are `E051` errors and the project would never
/// reach LIR lowering at all, so the struct half of the fixture would prove
/// nothing. `Dialect::Brink` is what makes the shape table real.
fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

const MAIN: &str = "\
INCLUDE lib.ink
INCLUDE more.ink

STRUCT Beacon = #{
    level: int,
    label: string,
}

VAR beacon = Beacon#{level: 3, label: \"north\"}
VAR counter = 0

=== start ===
The story opens.
~ counter = counter + bump(2)
-> other
";

const LIB: &str = "\
=== other ===
A line from lib.
-> tail
";

const MORE: &str = "\
=== function bump(n) ===
~ return n + 1

=== tail ===
The last stop, counter is {counter}.
-> END
";

/// A three-file project with the entry's own edited knot last, so an edit to
/// `lib.ink` is genuinely a *sibling* edit for `main.ink`'s chunks.
fn project(lib: &str) -> Vec<(&'static str, String)> {
    vec![
        ("main.ink", MAIN.to_owned()),
        ("lib.ink", lib.to_owned()),
        ("more.ink", MORE.to_owned()),
    ]
}

fn open(files: &[(&'static str, String)]) -> ProjectDb {
    let mut db = ProjectDb::new();
    db.set_analysis_options(brink_opts());
    for (path, source) in files {
        db.set_file(path, source.clone());
    }
    db.set_entry("main.ink").expect("entry set");
    db
}

fn inkb_bytes(db: &ProjectDb) -> Vec<u8> {
    let product = db
        .story_data()
        .expect("entry set, so story_data is available");
    assert!(
        product.errors.is_empty(),
        "fixture must compile clean: {:?}",
        product.errors
    );
    let story: &StoryData = product.story.as_ref().expect("project compiles to a story");
    let mut buf = Vec::new();
    brink_format::write_inkb(story, &mut buf);
    buf
}

/// Cold vs warm: a long-lived db driven through a sequence of edits produces
/// byte-identical `.inkb` to a brand-new db compiling the same final sources.
/// This is the determinism bar an incremental compile path has to clear — a
/// stale or wrongly-shared chunk-lowering context would diverge here.
#[test]
fn warm_recompile_is_byte_identical_to_a_cold_compile() {
    let revisions = [
        "=== other ===\nA line from lib.\n-> tail\n",
        "=== other ===\nA rather longer line from lib, shifting every range after it.\n-> tail\n",
        "=== other ===\nShort again.\n~ counter = counter + 1\n-> tail\n",
    ];

    let mut warm = open(&project(revisions[0]));
    // Prime every memo, including the chunk memos, before the first edit.
    let _ = inkb_bytes(&warm);

    for lib in &revisions[1..] {
        warm.update_file("lib.ink", (*lib).to_owned());
        let warm_bytes = inkb_bytes(&warm);

        let cold = open(&project(lib));
        let cold_bytes = inkb_bytes(&cold);

        assert_eq!(
            warm_bytes, cold_bytes,
            "warm recompile after editing lib.ink diverged from a cold compile of the same \
             sources (issue #460: the shared chunk-lowering context must be rebuilt whenever \
             resolutions, struct shapes, file paths or the type mode change)"
        );
    }
}

/// Restoring the original text must restore the original bytes exactly — the
/// edit-and-restore round trip that catches a context cached one revision too
/// long.
#[test]
fn restoring_the_original_source_restores_the_original_bytes() {
    let mut db = open(&project(LIB));
    let original = inkb_bytes(&db);

    db.update_file(
        "lib.ink",
        "=== other ===\nA completely different line.\n-> tail\n".to_owned(),
    );
    let edited = inkb_bytes(&db);
    assert_ne!(
        original, edited,
        "the fixture edit must actually change the artifact, or the restore below is vacuous"
    );

    db.update_file("lib.ink", LIB.to_owned());
    assert_eq!(
        original,
        inkb_bytes(&db),
        "restoring lib.ink's original text did not restore the original artifact"
    );
}

/// `compile_bench`'s `projectdb_*` stage profile (issue #460) pulls
/// `knot_chunk`/`lir_product` *before* `story_data()` to attribute cost per
/// query-graph layer. Query results must not depend on pull order: the
/// artifact a profiled db produces has to equal the one a db that only ever
/// pulls `story_data()` produces, cold and after an edit.
#[test]
fn pulling_intermediate_queries_first_does_not_change_the_artifact() {
    let plain = open(&project(LIB));
    let expected_cold = inkb_bytes(&plain);

    let mut profiled = open(&project(LIB));
    prime_like_the_profiler(&profiled);
    assert_eq!(
        expected_cold,
        inkb_bytes(&profiled),
        "pulling intermediate queries before story_data() changed the cold artifact"
    );

    let edited_lib = "=== other ===\nAn edited line from lib.\n-> tail\n";
    profiled.update_file("lib.ink", edited_lib.to_owned());
    prime_like_the_profiler(&profiled);

    let cold = open(&project(edited_lib));
    assert_eq!(
        inkb_bytes(&cold),
        inkb_bytes(&profiled),
        "pulling intermediate queries before story_data() changed the warm artifact"
    );
}

/// The `compile_bench` pull sequence: every file's per-knot chunks, then the
/// whole-project LIR product, before anyone asks for `story_data()`.
fn prime_like_the_profiler(db: &ProjectDb) {
    for id in db.file_ids().collect::<Vec<_>>() {
        let knots = db.hir(id).map_or(0, |h| h.knots.len());
        for k in 0..knots {
            let _ = db.knot_chunk(id, u32::try_from(k).unwrap());
        }
    }
    let _ = db.lir_product();
}
