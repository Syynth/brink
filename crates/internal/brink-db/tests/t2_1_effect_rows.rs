//! T2-1 effect-row inference substrate, exercised end-to-end through
//! `ProjectDb`'s salsa query layer (docs/effects-spec.md §2/§4, issue #860 —
//! tracked from #859).
//!
//! `crates/internal/brink-analyzer/src/infer/{effects,mod}.rs`'s own test
//! modules carry the pure-function soundness gate
//! (`conservative_total_no_under_report_over_mutual_recursion`) and the
//! lattice/fixpoint unit tests; these tests prove the *reachability* of the
//! new `effects(def)` public API and that the per-SCC `effects_scc_query`
//! fixpoint threads predecessor rows correctly across the salsa boundary,
//! not just in the pure function.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;

fn def_named(db: &ProjectDb, name: &str) -> brink_format::DefinitionId {
    let index = db.symbol_index();
    let ids = index.by_name.get(name).expect("def should be indexed");
    *ids.first().expect("indexed name has at least one def")
}

#[test]
fn effects_query_is_reachable_and_collects_read_write_call_atoms() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR gold = 0\nVAR hp = 10\nEXTERNAL play_sfx(x)\n\
         === function spend(cost) ===\n~ gold = gold - cost\n\
         ~ temp before = hp\n~ play_sfx(cost)\n~ return gold\n"
            .to_owned(),
    );
    let spend = def_named(&db, "spend");
    let gold = def_named(&db, "gold");
    let hp = def_named(&db, "hp");

    let row = db
        .effects(spend)
        .expect("spend has an inferable effect row");
    assert!(row.reads.contains(&gold), "reads gold");
    assert!(row.reads.contains(&hp), "reads hp");
    assert!(row.writes.contains(&gold), "writes gold");
    assert!(!row.writes.contains(&hp), "never writes hp");
    assert!(row.calls.contains("play_sfx"), "calls the external kind");
    assert!(!row.opaque, "a fully-visible body is not pessimal");
}

#[test]
fn effects_query_threads_a_callee_row_across_the_salsa_boundary() {
    // caller -> callee (two SCCs, a real condensation edge): the per-SCC
    // effects_scc_query must read the callee SCC's finalized row as
    // known_rows and fold it into the caller's row.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR gold = 0\nEXTERNAL play_sfx(x)\n\
         === function callee() ===\n~ gold = gold + 1\n~ play_sfx(gold)\n~ return gold\n\
         === caller ===\n~ temp x = callee()\n-> DONE\n"
            .to_owned(),
    );
    let caller = def_named(&db, "caller");
    let gold = def_named(&db, "gold");

    let row = db.effects(caller).expect("caller has an effect row");
    assert!(
        row.writes.contains(&gold),
        "caller transitively writes gold through callee"
    );
    assert!(
        row.calls.contains("play_sfx"),
        "caller transitively calls the external kind through callee"
    );
}

#[test]
fn effects_query_is_pessimal_for_a_call_through_a_function_value() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "=== function apply(cb) ===\n~ return cb(1)\n".to_owned(),
    );
    let apply = def_named(&db, "apply");
    let row = db.effects(apply).expect("apply has an effect row");
    assert!(
        row.opaque,
        "a call through a function value must be pessimal (spec §3/§4)"
    );
}

#[test]
fn effects_query_is_none_for_a_non_callable_def() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", "VAR gold = 10\n-> DONE\n".to_owned());
    let index = db.symbol_index();
    let gold = *index
        .by_name
        .get("gold")
        .expect("gold indexed")
        .first()
        .expect("one id");
    assert!(
        db.effects(gold).is_none(),
        "a VAR has no inferable body, so no effect row"
    );
}
