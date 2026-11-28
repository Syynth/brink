//! Runaway-guard ceilings hold under an editor-session-shaped project
//! (issue #647, decision log "FG-5 memory bounding").
//!
//! #537's measurement is the source of the ~4,096-per-file / ~16,384-per-def
//! ceiling values (large synthetic projects, up to 128 files / ~1,549 defs,
//! 2,000-edit sessions) — running that scale here would be far too slow for
//! a unit test. This test instead proves the two things a fast test *can*
//! prove at a much smaller, `#537`-shaped scale (many files, a typed probe
//! that reaches every per-def family, like #537's `BENCH_TYPED` variant —
//! full session-scale reproduction is #819's bench probe mode, not this
//! test):
//!
//! 1. Every per-file/per-def family's live memo `count` stays **under** its
//!    configured `lru` ceiling at this project's scale — i.e. the ceiling
//!    doesn't accidentally start evicting live working-set entries on an
//!    ordinarily-sized project (the exact failure #537's ruling warns a
//!    *tight* capacity would cause).
//! 2. The families the harness's stock (untyped) probe can't see at all —
//!    every per-def family — actually get populated once a typed pull (this
//!    test's `pull_every_def`) runs, so the ceiling has something real to
//!    guard, and the five `heap_size`-estimator families (#538) report
//!    `Some(_)` once populated, not the honest-`None` every family reported
//!    before this pass.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use brink_db::{IngredientKind, IngredientMemory, ProjectDb};

/// Small next to #537's up-to-128-file/1,549-def scale — enough to give
/// every family in [`PER_FILE_FAMILIES`]/[`PER_DEF_FAMILIES`] a nonzero,
/// genuinely project-size-driven count, while staying fast.
const FILES: usize = 24;
const KNOTS_PER_FILE: usize = 8;

const PER_FILE_LRU_CEILING: usize = 4096;
const PER_DEF_LRU_CEILING: usize = 16384;

/// The per-file query families named in the decision log ruling — keyed by
/// `SourceFile`, so their live count is bounded by files ever seen.
const PER_FILE_FAMILIES: &[&str] = &[
    "parse_query",
    "lowered_query",
    "suppressions_query",
    "resolve_query",
    "per_file_diagnostics_query",
    "value_meta_query",
    "call_site_diagnostics_query",
    "diagnostics_query",
];

/// The per-def query families named in the decision log ruling — keyed by
/// `DefKey`, so their live count is bounded by live project defs.
const PER_DEF_FAMILIES: &[&str] = &[
    "signature_query",
    "def_body_query",
    "referenced_globals_query",
    "call_edges_query",
    "solve_scc_query",
    "inferred_signature_query",
    "infer_body_query",
];

/// The five #538 `heap_size` estimator families (issue #647 scope item 2).
const HEAP_SIZE_FAMILIES: &[&str] = &[
    "signature_query",
    "def_body_query",
    "solve_scc_query",
    "infer_body_query",
    "lowered_query",
];

fn file_path(idx: usize) -> String {
    format!("f{idx:03}.ink")
}

fn generate_file(idx: usize) -> String {
    let mut s = format!("VAR var_{idx:03} = {idx}\n");
    for k in 0..KNOTS_PER_FILE {
        let _ = writeln!(s, "== k{idx:03}_{k:02} ==");
        let _ = writeln!(s, "Line {k} of file {idx}, reading {{var_{idx:03}}}.");
        s.push_str("-> DONE\n");
    }
    s
}

fn generate_main() -> String {
    let mut s = String::new();
    for f in 0..FILES {
        let _ = writeln!(s, "INCLUDE {}", file_path(f));
    }
    s.push_str("Opening line.\n-> k000_00\n");
    s
}

/// The #537 "typed probe" this test's own doc comment describes: pull
/// `signature`/`infer_body`/`inferred_signature` for every def in the
/// index, the same IDE-style hover/inlay-hint pull #537 added to make the
/// stock (untyped) bench see the per-def families at all.
fn pull_every_def(db: &ProjectDb) {
    let index = db.symbol_index();
    for def in index.symbols.keys().copied() {
        let _ = db.signature(def);
        let _ = db.infer_body(def);
        let _ = db.inferred_signature(def);
    }
}

fn row<'a>(rows: &'a [IngredientMemory], name: &str) -> Option<&'a IngredientMemory> {
    rows.iter()
        .find(|r| r.kind == IngredientKind::Query && r.name == name)
}

#[test]
fn per_family_lru_ceilings_hold_at_editor_session_scale() {
    let mut db = ProjectDb::new();
    for f in 0..FILES {
        db.set_file(&file_path(f), generate_file(f));
    }
    db.set_file("main.ink", generate_main());
    db.set_entry("main.ink").expect("entry");

    for f in 0..FILES {
        let id = db.file_id(&file_path(f)).expect("file id");
        let _ = db.diagnostics(id);
        let _ = db.per_file_diagnostics(id);
    }
    assert!(db.story_data().is_some(), "project must compile clean");
    pull_every_def(&db);

    let rows = db.memory_snapshot();

    for &name in PER_FILE_FAMILIES {
        let Some(r) = row(&rows, name) else {
            continue;
        };
        assert!(
            r.count <= PER_FILE_LRU_CEILING,
            "{name}: count={} exceeds the {PER_FILE_LRU_CEILING} per-file ceiling \
             at {FILES}-file scale — the ceiling is starting to evict live \
             working-set entries",
            r.count
        );
    }

    for &name in PER_DEF_FAMILIES {
        let Some(r) = row(&rows, name) else {
            panic!(
                "{name}: no memo row at all after `pull_every_def` — the typed \
                 probe didn't reach this family"
            );
        };
        assert!(
            r.count > 0,
            "{name}: count=0 after `pull_every_def` — the probe didn't \
             populate this per-def family, so the ceiling assertion below is \
             vacuous"
        );
        assert!(
            r.count <= PER_DEF_LRU_CEILING,
            "{name}: count={} exceeds the {PER_DEF_LRU_CEILING} per-def ceiling \
             at {FILES}x{KNOTS_PER_FILE}-def scale — the ceiling is starting to \
             evict live working-set entries",
            r.count
        );
    }

    for &name in HEAP_SIZE_FAMILIES {
        let r = row(&rows, name)
            .unwrap_or_else(|| panic!("{name}: no memo row after `pull_every_def`"));
        assert!(
            r.count > 0,
            "{name}: count=0 — the probe didn't populate this heap_size family"
        );
        assert!(
            r.heap_bytes.is_some(),
            "{name}: heap_bytes is None with count={} — the #538 heap_size \
             estimator isn't wired up (or isn't firing) for this query",
            r.count
        );
    }
}
