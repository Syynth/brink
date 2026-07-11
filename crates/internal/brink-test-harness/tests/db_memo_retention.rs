//! Memo-table retention across remove/re-add churn (issue #536).
//!
//! `ProjectDb::remove_file` used to forget only its own path→id map: the
//! salsa `SourceFile` input and every memo keyed on it stayed live forever,
//! and re-adding the same path minted a *new* `FileId` — so each
//! remove/re-add cycle leaked one input plus its whole per-file memo column
//! (parse/lowered/suppressions/resolve/diagnostics), unreachable and
//! unreclaimable (salsa never frees inputs; LRU only trims live memos).
//!
//! With durable path→`FileId` identity, a re-added path reuses its original
//! input, so memo counts must be *flat* across churn cycles: this test runs
//! a warm-up cycle (so every table row exists), snapshots, churns many more
//! cycles, and asserts every ingredient's `count` is unchanged.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::{IngredientMemory, ProjectDb};

const WARMUP_CYCLES: usize = 2;
const CHURN_CYCLES: usize = 10;

fn main_text(scratch_present: bool) -> String {
    let mut s = String::from("VAR hub = 0\n");
    if scratch_present {
        s.push_str("INCLUDE scratch.ink\n");
    }
    s.push_str("Opening line.\n-> DONE\n");
    s
}

fn scratch_text(variant: usize) -> String {
    format!("== scratch_knot ==\nScratch content, variant {variant}.\n-> DONE\n")
}

/// One full churn cycle: add the scratch file (fresh content each time),
/// pull the pipeline, remove it, pull the pipeline again — the same shape
/// `editor_session_bench`'s Churn edits drive, condensed.
fn cycle(db: &mut ProjectDb, variant: usize) {
    let id = db.set_file("scratch.ink", scratch_text(variant));
    db.update_file("main.ink", main_text(true));
    let _ = db.diagnostics(id);
    assert!(db.story_data().is_some());

    db.remove_file("scratch.ink");
    db.update_file("main.ink", main_text(false));
    let main = db.file_id("main.ink").expect("main.ink");
    let _ = db.diagnostics(main);
    assert!(db.story_data().is_some());
}

#[test]
fn remove_readd_churn_keeps_memo_counts_flat() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", main_text(false));
    db.set_entry("main.ink").expect("entry");
    assert!(db.story_data().is_some());

    for v in 0..WARMUP_CYCLES {
        cycle(&mut db, v);
    }
    let before: Vec<IngredientMemory> = db.memory_snapshot();

    for v in 0..CHURN_CYCLES {
        cycle(&mut db, WARMUP_CYCLES + v);
    }
    let after: Vec<IngredientMemory> = db.memory_snapshot();

    // Same ingredient rows, same live counts: churn must not create new
    // inputs, interned keys, or memo entries once the tables are warm.
    let describe = |rows: &[IngredientMemory]| -> Vec<String> {
        rows.iter()
            .map(|m| format!("{:?} {} count={}", m.kind, m.name, m.count))
            .collect()
    };
    assert_eq!(
        describe(&before),
        describe(&after),
        "memo/input counts grew across {CHURN_CYCLES} remove/re-add cycles (issue #536 leak)"
    );

    // And the re-added path keeps one stable identity throughout.
    let id_a = db.set_file("scratch.ink", scratch_text(999));
    db.remove_file("scratch.ink");
    let id_b = db.set_file("scratch.ink", scratch_text(1000));
    assert_eq!(id_a, id_b, "re-added path must reuse its FileId");
    db.remove_file("scratch.ink");
}
