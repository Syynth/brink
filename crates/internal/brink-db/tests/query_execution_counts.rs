//! Execution-count pins: after one edit of a given shape, exactly which
//! queries execute, and how many times (#3585).
//!
//! Why this exists. A query with a too-COARSE dependency is a correct
//! query: it returns the right value, every correctness test passes, and the
//! only thing it changes is how often it re-executes. A too-NARROW
//! dependency is a stale-result bug and tests catch it at once. So the
//! incentive gradient pushes every author toward the safe mistake, and
//! nothing in the suite pushed back — until the per-def inference family
//! was found re-executing for every knot in a file on every keystroke
//! (quadratic in knots-per-file; 505 ms at 480 knots). The first fix for
//! that moved NOTHING, because a `Vec` index made every def depend on every
//! segment identity in the file — and no reading of the code found it. A
//! per-query execution counter (`brink_db::count_executions`) found it in
//! one screen. These tests make that counter a gate: widen a dependency
//! and the number here moves.
//!
//! Every count is exact. The retention test (`db_memo_retention`) pins
//! memo counts the same way; a range would let the next coarse edge in.
//! The counts are what the code does today, including the costs the design
//! accepts on purpose (see `inserting_a_knot_reexecutes_the_knots_after_it`).
//!
//! Counts are per thread (salsa executes on the calling thread, tests run
//! on parallel threads), so each test owns its own db and its own map.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use brink_db::{ProjectDb, count_executions};

/// The queries whose counts decide whether the per-knot firewall holds.
const WATCHED: &[&str] = &[
    "call_edges_query",
    "def_body_query",
    "referenced_globals_query",
    "solve_scc_query",
    "segment_lowered_query",
    "file_segment_at_query",
    "file_def_segments_query",
    "call_graph_query",
    "scc_membership_query",
    "type_inference_query",
    // The effects family (T2-1) and the per-knot LIR chunks, pulled through
    // `story_data()` — the compile the host debounces behind every keystroke.
    "def_effect_atoms_query",
    "effects_scc_query",
    "effects_query",
    "lir_knot_chunk_query",
    // Per-def declared signatures, pulled for every knot/stitch the way the
    // inlay-hint and hover collectors do.
    "signature_query",
];

fn knot(name: &str, next: Option<&str>, value: u32) -> String {
    let tail = next.map_or_else(|| "-> DONE".to_owned(), |n| format!("-> {n}"));
    format!("=== {name} ===\n~ temp local = {value}\nThe value is {{local + gold}}.\n{tail}\n")
}

/// `VAR gold`, then `alpha -> beta -> gamma -> DONE`: real call edges, a
/// global every body reads, a temp per body.
fn three_knots(beta_value: u32) -> String {
    format!(
        "VAR gold = 10\n{}{}{}",
        knot("alpha", Some("beta"), 1),
        knot("beta", Some("gamma"), beta_value),
        knot("gamma", None, 3)
    )
}

fn warm(db: &mut ProjectDb, text: &str) {
    db.set_file("main.ink", text.to_owned());
    db.set_entry("main.ink").expect("entry");
    pull(db);
}

/// The two pulls a keystroke ends in: the editor's inference (hints,
/// widgets) and the debounced compile (`story_data`, which pulls every
/// inferable def's effect row and every knot's LIR chunk).
fn pull(db: &ProjectDb) {
    let _ = db.type_inference();
    let _ = db.story_data();
    let index = db.symbol_index();
    for (id, info) in &index.symbols {
        if matches!(
            info.kind,
            brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
        ) {
            let _ = db.signature(*id);
        }
    }
}

/// Apply `edit`, pull, and return what executed.
fn executions(db: &mut ProjectDb, edit: impl FnOnce(&mut ProjectDb)) -> BTreeMap<String, u64> {
    let ((), counts) = count_executions(|| {
        edit(db);
        pull(db);
    });
    counts
}

fn watched(counts: &BTreeMap<String, u64>) -> Vec<(String, u64)> {
    WATCHED
        .iter()
        .map(|&q| (q.to_owned(), counts.get(q).copied().unwrap_or(0)))
        .collect()
}

/// Exact-count assertion over the watched set, with the whole map in the
/// failure message so a regression names what else moved.
fn assert_watched(counts: &BTreeMap<String, u64>, expected: &[(&str, u64)]) {
    let got = watched(counts);
    let want: Vec<(String, u64)> = expected.iter().map(|&(q, n)| (q.to_owned(), n)).collect();
    assert_eq!(
        got, want,
        "execution counts moved — full map of what executed:\n{counts:#?}"
    );
}

// ── negative control ──────────────────────────────────────────────────────

/// A cold build executes each per-def query once per knot. If this ever
/// reads 0, the counter is dead and every 0 below is meaningless.
/// (`file_segment_at_query` counts defs LOOKED UP, not segments: three
/// knots, three lookups; the header segment holds no def.)
#[test]
fn the_counter_is_live_a_cold_build_executes_each_per_def_query_per_knot() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", three_knots(2));
    db.set_entry("main.ink").expect("entry");
    let ((), counts) = count_executions(|| {
        pull(&db);
    });
    assert_watched(
        &counts,
        &[
            ("call_edges_query", 3),
            ("def_body_query", 3),
            ("referenced_globals_query", 3),
            ("solve_scc_query", 3),
            ("segment_lowered_query", 4),
            ("file_segment_at_query", 3),
            ("file_def_segments_query", 1),
            ("call_graph_query", 1),
            ("scc_membership_query", 1),
            ("type_inference_query", 1),
            ("def_effect_atoms_query", 3),
            ("effects_scc_query", 3),
            ("effects_query", 3),
            ("lir_knot_chunk_query", 3),
            ("signature_query", 4),
        ],
    );
}

// ── the firewall, edge by edge ────────────────────────────────────────────

/// `signature_query` reads 2 here, not 1: the edited knot (re-executes,
/// backdates — `Sig` is declaration-only) plus `VAR gold`, which is a
/// root-content def and still on the whole-file road (`def_segment` maps
/// knots and stitches only). Cheap and `Eq`-cut, but counted honestly.
#[test]
fn editing_inside_one_knot_reexecutes_that_knots_defs_only() {
    let mut db = ProjectDb::new();
    warm(&mut db, &three_knots(2));
    let counts = executions(&mut db, |db| {
        db.update_file("main.ink", three_knots(99));
    });
    assert_watched(
        &counts,
        &[
            ("call_edges_query", 1),
            ("def_body_query", 1),
            ("referenced_globals_query", 1),
            ("solve_scc_query", 1),
            ("segment_lowered_query", 1),
            ("file_segment_at_query", 3),
            ("file_def_segments_query", 1),
            ("call_graph_query", 0),
            ("scc_membership_query", 0),
            ("type_inference_query", 1),
            ("def_effect_atoms_query", 1),
            ("effects_scc_query", 0),
            ("effects_query", 0),
            ("lir_knot_chunk_query", 3),
            ("signature_query", 2),
        ],
    );
}

#[test]
fn appending_at_end_of_file_reexecutes_the_last_knots_defs_only() {
    let mut db = ProjectDb::new();
    warm(&mut db, &three_knots(2));
    let counts = executions(&mut db, |db| {
        db.update_file("main.ink", format!("{}// trailing note\n", three_knots(2)));
    });
    assert_watched(
        &counts,
        &[
            ("call_edges_query", 1),
            ("def_body_query", 1),
            ("referenced_globals_query", 1),
            ("solve_scc_query", 1),
            ("segment_lowered_query", 1),
            ("file_segment_at_query", 3),
            ("file_def_segments_query", 1),
            ("call_graph_query", 0),
            ("scc_membership_query", 0),
            ("type_inference_query", 1),
            ("def_effect_atoms_query", 1),
            ("effects_scc_query", 0),
            ("effects_query", 0),
            ("lir_knot_chunk_query", 3),
            ("signature_query", 2),
        ],
    );
}

/// Same type, new value: no def's inputs changed. The header segment
/// re-lowers; nothing per-def should.
#[test]
fn editing_a_var_initializer_of_the_same_type_reexecutes_no_def() {
    let mut db = ProjectDb::new();
    warm(&mut db, &three_knots(2));
    let counts = executions(&mut db, |db| {
        db.update_file(
            "main.ink",
            three_knots(2).replacen("VAR gold = 10", "VAR gold = 11", 1),
        );
    });
    assert_watched(
        &counts,
        &[
            ("call_edges_query", 0),
            ("def_body_query", 0),
            ("referenced_globals_query", 0),
            ("solve_scc_query", 0),
            ("segment_lowered_query", 1),
            ("file_segment_at_query", 3),
            ("file_def_segments_query", 1),
            ("call_graph_query", 0),
            ("scc_membership_query", 0),
            ("type_inference_query", 0),
            ("def_effect_atoms_query", 0),
            ("effects_scc_query", 0),
            ("effects_query", 0),
            ("lir_knot_chunk_query", 3),
            ("signature_query", 1),
        ],
    );
}

#[test]
fn editing_file_a_leaves_file_bs_defs_alone() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        format!(
            "INCLUDE b.ink\nVAR gold = 10\n{}",
            knot("alpha", Some("beta"), 1)
        ),
    );
    db.set_file(
        "b.ink",
        format!(
            "{}{}",
            knot("beta", Some("gamma"), 2),
            knot("gamma", None, 3)
        ),
    );
    db.set_entry("main.ink").expect("entry");
    pull(&db);
    let counts = executions(&mut db, |db| {
        db.update_file(
            "b.ink",
            format!(
                "{}{}",
                knot("beta", Some("gamma"), 99),
                knot("gamma", None, 3)
            ),
        );
    });
    assert_watched(
        &counts,
        &[
            ("call_edges_query", 1),
            ("def_body_query", 1),
            ("referenced_globals_query", 1),
            ("solve_scc_query", 1),
            ("segment_lowered_query", 1),
            ("file_segment_at_query", 2),
            ("file_def_segments_query", 1),
            ("call_graph_query", 0),
            ("scc_membership_query", 0),
            ("type_inference_query", 1),
            ("def_effect_atoms_query", 1),
            ("effects_scc_query", 0),
            ("effects_query", 0),
            ("lir_knot_chunk_query", 3),
            ("signature_query", 1),
        ],
    );
}

/// A cost the design accepts on purpose: `file_def_segments_query` maps a
/// def to a segment INDEX, so inserting a knot above others shifts theirs
/// and they re-execute once. Here: `alpha` (its divert text changed),
/// `inserted` (new), and `beta`/`gamma` (shifted) — 4. The call graph
/// gained an edge, so `call_graph_query`/`scc_membership_query` re-execute
/// here and nowhere else in this file: FG-2's `Eq` cutoff holds them at 0
/// for every edit that leaves the edge set alone.
#[test]
fn inserting_a_knot_reexecutes_the_knots_after_it() {
    let mut db = ProjectDb::new();
    warm(&mut db, &three_knots(2));
    let counts = executions(&mut db, |db| {
        db.update_file(
            "main.ink",
            format!(
                "VAR gold = 10\n{}{}{}{}",
                knot("alpha", Some("inserted"), 1),
                knot("inserted", Some("beta"), 7),
                knot("beta", Some("gamma"), 2),
                knot("gamma", None, 3)
            ),
        );
    });
    assert_watched(
        &counts,
        &[
            ("call_edges_query", 4),
            ("def_body_query", 4),
            ("referenced_globals_query", 4),
            ("solve_scc_query", 4),
            ("segment_lowered_query", 2),
            ("file_segment_at_query", 4),
            ("file_def_segments_query", 1),
            ("call_graph_query", 1),
            ("scc_membership_query", 1),
            ("type_inference_query", 1),
            ("def_effect_atoms_query", 4),
            ("effects_scc_query", 4),
            ("effects_query", 4),
            ("lir_knot_chunk_query", 4),
            ("signature_query", 5),
        ],
    );
}
