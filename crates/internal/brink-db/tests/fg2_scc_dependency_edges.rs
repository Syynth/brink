//! Targeted correctness tests for issue #631 (FG-2, the per-def/per-SCC
//! decomposition of TM-1's `infer_project`: `call_edges(def) ->
//! scc_membership() -> solve_scc(SccId) -> inferred_signature(def)`).
//!
//! `crates/internal/brink-analyzer/src/infer/mod.rs`'s own test module
//! already carries the pure-function decomposition-equivalence gate
//! (`composed_per_scc_solve_equals_monolithic_infer_project`); these tests
//! exercise the same boundary end-to-end through `ProjectDb`'s salsa query
//! layer — reachability of the new `inferred_signature(def)` API, and that
//! the per-SCC solve correctly reacts to a call-graph-topology-altering
//! edit (an SCC/condensation-membership change), not just a body edit
//! within an already-fixed topology.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_db::ProjectDb;
use brink_ir::SymbolKind;

fn def_named(db: &ProjectDb, name: &str) -> brink_format::DefinitionId {
    let index = db.symbol_index();
    let ids = index.by_name.get(name).expect("def should be indexed");
    *ids.first().expect("indexed name has at least one def")
}

#[test]
fn inferred_signature_is_reachable_and_matches_infer_body() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "=== heal(hp) ===\n~ temp x = hp + 1\n-> DONE\n".to_owned(),
    );
    let heal = def_named(&db, "heal");

    let sig = db
        .inferred_signature(heal)
        .expect("heal has an inferable signature");
    assert_eq!(sig.params.len(), 1);
    assert_eq!(sig.params[0].display(), "int");

    // Same params/return picture as infer_body's params (InferredSig is the
    // firewall-facing projection of BodyTypes).
    let body = db.infer_body(heal).expect("heal has an inferable body");
    assert_eq!(body.params[0].1, sig.params[0]);
    assert_eq!(body.return_ty, sig.return_ty);
}

#[test]
fn inferred_signature_is_none_for_a_non_callable_def() {
    let mut db = ProjectDb::new();
    db.set_file("main.ink", "VAR gold = 10\n-> DONE\n".to_owned());
    let index = db.symbol_index();
    let gold = index
        .by_name
        .get("gold")
        .and_then(|ids| ids.first())
        .copied()
        .expect("gold indexed");
    assert_eq!(
        index.symbols.get(&gold).map(|i| i.kind),
        Some(SymbolKind::Variable)
    );
    assert_eq!(
        db.inferred_signature(gold),
        None,
        "a VAR has no inferable signature"
    );
}

#[test]
#[expect(
    clippy::similar_names,
    reason = "ping/pong are the clearest names for this pair"
)]
fn mutual_recursion_signature_reachable_through_project_db() {
    // The same fixture `infer/mod.rs`'s
    // `mutual_recursion_return_type_converges_by_fixpoint` test uses,
    // driven through ProjectDb: proves the per-SCC solve_scc_query path
    // (not just the pure infer_project function) runs the multi-round
    // fixpoint and converges to the same answer.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "=== function ping(n) ===\n{n == 0:\n  ~ return 0.0\n}\n~ return pong(n - 1)\n\
         === function pong(n) ===\n~ return ping(n)\n"
            .to_owned(),
    );
    let ping = def_named(&db, "ping");
    let pong = def_named(&db, "pong");

    let ping_sig = db.inferred_signature(ping).expect("ping has a signature");
    let pong_sig = db.inferred_signature(pong).expect("pong has a signature");
    assert_eq!(ping_sig.return_ty.display(), "float");
    assert_eq!(pong_sig.return_ty.display(), "float");
}

#[test]
fn adding_a_call_edge_changes_the_scc_solve_downstream() {
    // Before the edit, `main` calls nothing: its `~ temp v = 1` local is
    // typed purely from the int literal (Int), independent of `use_it`.
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "=== main ===\n~ temp v = 1\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n"
            .to_owned(),
    );
    let main = def_named(&db, "main");
    let before = db.infer_body(main).expect("main has a body");
    assert_eq!(
        before.locals.get("v").map(brink_analyzer::Ty::display),
        Some("int".to_string()),
        "v is only ever assigned an int literal before the edit"
    );

    // After the edit, `main` calls `use_it(v)` — a new call-graph edge that
    // changes `call_graph_query`'s output (and, for a project this shape,
    // `scc_membership_query`'s partition too, since `use_it` now has a
    // caller). `use_it`'s own body pins its param to Float via the `> 2.5`
    // comparison, so the call site should carry that type back onto `v`.
    db.update_file(
        "main.ink",
        "=== main ===\n~ temp v = 1\n~ use_it(v)\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n"
            .to_owned(),
    );
    let after = db.infer_body(main).expect("main still has a body");
    assert_eq!(
        after.locals.get("v").map(brink_analyzer::Ty::display),
        Some("float".to_string()),
        "adding the use_it(v) call edge must flow use_it's Float param back onto v \
         (the call-graph-topology change must be picked up by the decomposed \
         call_graph/scc_membership/solve_scc query chain, not just a stale cache)"
    );
}
