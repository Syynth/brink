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

use std::collections::BTreeSet;

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
        row.is_pessimal(),
        "a call through a function value must be pessimal (spec §3/§4)"
    );
    // §6.1 (issue #1680) changed *how*: the param is a row variable, which is
    // the pessimal floor until a caller instantiates it — but not the
    // intrinsic `opaque` bit any more.
    assert_eq!(row.holes, [0].into_iter().collect::<BTreeSet<u32>>());
}

#[test]
fn effects_query_writes_through_a_ref_param_at_the_call_site() {
    // Review-finding regression (issue #860's PR): `inc`'s own body atoms are
    // empty (`~ x = x + 1` assigns a `Param`, never a `Variable`/`Constant`)
    // — the write only becomes visible at `knot`'s own call site, where
    // `val` is passed into `inc`'s `ref x` slot. Same fixture as
    // tests/tier1/variables/variable-pointer-ref-from-knot/story.ink,
    // exercised end-to-end through `ProjectDb::effects` this time.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR val = 5\n\
         === knot ===\n~ inc(val)\n{val}\n->->\n\
         === function inc(ref x) ===\n~ x = x + 1\n"
            .to_owned(),
    );
    let knot = def_named(&db, "knot");
    let inc = def_named(&db, "inc");
    let val = def_named(&db, "val");

    let knot_row = db.effects(knot).expect("knot has an inferable effect row");
    assert!(
        knot_row.writes.contains(&val),
        "knot's call `inc(val)` writes through inc's `ref x` param"
    );

    let inc_row = db.effects(inc).expect("inc has an inferable effect row");
    assert!(
        !inc_row.writes.contains(&val),
        "inc's own row never names `val` — only the caller's call site does"
    );
}

#[test]
fn effects_query_writes_through_an_indexed_assignment_target() {
    // Issue #880's audit of the #856 map-insert-on-assign path: `record_
    // write` used to only recognize a bare `Expr::Path` target, silently
    // dropping the write for any indexed assignment (`arr[i] = v`,
    // `memo[newKey] = v`) to a global — the same silent-drop class as the
    // #866 ref-param bug, just on the plain-assignment path.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR arr = #[1, 2, 3]\n\
         === function bump(i, v) ===\n~ arr[i] = v\n"
            .to_owned(),
    );
    let bump = def_named(&db, "bump");
    let arr = def_named(&db, "arr");
    let row = db.effects(bump).expect("bump has an inferable effect row");
    assert!(
        row.writes.contains(&arr),
        "bump's `arr[i] = v` writes arr through indexed assignment"
    );
}

#[test]
fn effects_query_writes_through_a_nested_indexed_assignment_target() {
    // The nested-chain shape (`grid[y][x] = v`) — `ref_arg_root` must walk
    // through an arbitrary number of `Expr::Index` layers down to the root
    // path, not just one level.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR grid = #[#[1, 2], #[3, 4]]\n\
         === function set_cell(y, x, v) ===\n~ grid[y][x] = v\n"
            .to_owned(),
    );
    let set_cell = def_named(&db, "set_cell");
    let grid = def_named(&db, "grid");
    let row = db
        .effects(set_cell)
        .expect("set_cell has an inferable effect row");
    assert!(
        row.writes.contains(&grid),
        "set_cell's `grid[y][x] = v` writes grid through a nested index chain"
    );
}

#[test]
fn effects_query_writes_through_the_insert_mutator_at_the_call_site() {
    // Issue #880's core ask: `insert(memo, key, val)` writes back through
    // its lvalue first argument — the same shape the three
    // KNOWN_MUTATOR_WRITE_GAP_CASES corpus fixtures hit
    // (crates/internal/brink-test-harness/tests/t2_ground_truth_effects.rs).
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR memo = #{}\n\
         === function memoize(k, v) ===\n~ insert(memo, k, v)\n"
            .to_owned(),
    );
    let memoize = def_named(&db, "memoize");
    let memo = def_named(&db, "memo");
    let row = db
        .effects(memoize)
        .expect("memoize has an inferable effect row");
    assert!(
        row.writes.contains(&memo),
        "memoize's `insert(memo, k, v)` writes memo through the mutator's lvalue arg"
    );
}

#[test]
fn effects_query_writes_through_the_push_mutator_at_the_call_site() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR items = #[]\n\
         === function add(v) ===\n~ push(items, v)\n"
            .to_owned(),
    );
    let add = def_named(&db, "add");
    let items = def_named(&db, "items");
    let row = db.effects(add).expect("add has an inferable effect row");
    assert!(
        row.writes.contains(&items),
        "add's `push(items, v)` writes items through the mutator's lvalue arg"
    );
}

#[test]
fn effects_query_writes_through_the_remove_mutator_at_the_call_site() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR items = #[1, 2, 3]\n\
         === function drop(k) ===\n~ remove(items, k)\n"
            .to_owned(),
    );
    let drop_fn = def_named(&db, "drop");
    let items = def_named(&db, "items");
    let row = db
        .effects(drop_fn)
        .expect("drop has an inferable effect row");
    assert!(
        row.writes.contains(&items),
        "drop's `remove(items, k)` writes items through the mutator's lvalue arg"
    );
}

#[test]
fn effects_query_char_at_is_pure_no_write_recorded() {
    // Issue #880's audit explicitly names `char_at` as pure (no write) —
    // it must never gain a spurious write atom.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "VAR s = \"hello\"\n\
         === function first(i) ===\n~ temp c = char_at(s, i)\n~ return c\n"
            .to_owned(),
    );
    let first = def_named(&db, "first");
    let s = def_named(&db, "s");
    let row = db
        .effects(first)
        .expect("first has an inferable effect row");
    assert!(
        !row.writes.contains(&s),
        "char_at is pure — it must never record a write to its argument"
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
