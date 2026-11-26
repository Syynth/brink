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

use std::sync::Arc;

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

// ─── Lazy per-reference globals + full narrowing (FG-2.1, issue #638) ─────
//
// The assertion the whole FG-2.1 slice exists for (spec §9 gate, Ruling 2b):
// an unrelated SCC's `solve_scc` must not *re-execute* on a body/global edit
// in a different file it neither calls into nor reads a global from — not
// just "backdate to an equal value downstream" (the pre-#638 shape, where
// `inference_inputs`'s all-files `hir_refs` recorded a read-edge on every
// file's `lowered_query` regardless). Pointer/Arc identity, not value
// equality, is the assertion — same reasoning `fg1_dependency_edges.rs`'s
// module doc spells out: a query that re-executes and happens to produce an
// `Eq` result still allocates a fresh `Arc`, so identity breaks even though
// downstream consumers never notice. `infer_body`/`inferred_signature` are
// thin per-def views that themselves only re-execute their own closure if
// `solve_scc_query` (or `scc_membership_query`, upstream of it) reports a
// changed dependency — so their returned `Arc`'s pointer is a faithful proxy
// for "did `solve_scc` for this def's SCC actually re-run its fixpoint."

#[test]
fn unrelated_scc_solve_survives_a_body_edit_in_another_file_no_shared_globals_or_calls() {
    let mut db = ProjectDb::new();
    db.set_file(
        "x.ink",
        "=== helper_x ===\n~ temp v = 1\n-> DONE\n".to_owned(),
    );
    db.set_file(
        "y.ink",
        "=== helper_y(hp) ===\n~ temp x = hp + 1\n-> DONE\n".to_owned(),
    );

    let helper_x = def_named(&db, "helper_x");
    let helper_y = def_named(&db, "helper_y");

    let before_x = db.infer_body(helper_x).expect("helper_x has a body");
    let before_y = db.infer_body(helper_y).expect("helper_y has a body");
    let before_y_ptr = Arc::as_ptr(&before_y);

    // Edit x.ink's body value only (Int -> Float literal, so the inferred
    // *type* itself changes, not just the source text) — no call
    // added/removed, no global declared or referenced anywhere in this
    // project. helper_y neither calls helper_x nor reads any global
    // helper_x might reference, so its own SCC's `solve_scc` must not need
    // to re-run at all.
    db.update_file(
        "x.ink",
        "=== helper_x ===\n~ temp v = 2.5\n-> DONE\n".to_owned(),
    );

    // Sanity: the edit is real — helper_x's own inferred body picks up the
    // Int -> Float change (not a vacuously-true test because nothing
    // changed anywhere).
    let after_x = db.infer_body(helper_x).expect("helper_x still has a body");
    assert_ne!(
        before_x.locals.get("v"),
        after_x.locals.get("v"),
        "helper_x's own body edit should change its own inferred local type"
    );

    let after_y = db.infer_body(helper_y).expect("helper_y still has a body");
    assert!(
        Arc::ptr_eq(&before_y, &after_y),
        "editing an unrelated file re-executed helper_y's SCC solve \
         (over-coarse dependency edge — issue #638 FG-2.1 full narrowing)"
    );
    assert_eq!(
        Arc::as_ptr(&after_y),
        before_y_ptr,
        "the returned Arc must be the exact same allocation, not merely Eq"
    );
}

#[test]
fn unrelated_scc_solve_survives_a_global_edit_it_never_references() {
    let mut db = ProjectDb::new();
    db.set_file("globals.ink", "VAR gold = 10\n-> DONE\n".to_owned());
    db.set_file(
        "reader.ink",
        "VAR silver = 5\n=== spend(cost) ===\n~ silver = silver - cost\n-> DONE\n".to_owned(),
    );

    let spend = def_named(&db, "spend");
    let before = db.infer_body(spend).expect("spend has a body");

    // Edit an unreferenced global's initializer in a different file. `spend`
    // reads `silver`, never `gold` — its own SCC's solve must not re-run.
    db.update_file("globals.ink", "VAR gold = 99\n-> DONE\n".to_owned());

    let after = db.infer_body(spend).expect("spend still has a body");
    assert!(
        Arc::ptr_eq(&before, &after),
        "editing an unreferenced global in another file re-executed spend's \
         SCC solve (issue #638 FG-2.1 Ruling 1 — lazy per-reference globals)"
    );
}
