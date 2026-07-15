//! Law: algorithms sitting on top of a `HashMap`/`HashSet` must produce
//! byte-identical output across fresh process-level instances, regardless of
//! that map's per-instance `RandomState` hasher seed (issue #801 — the
//! enforcement follow-up to #795's nondeterministic module-attribution
//! regression, which shipped through an opus build *and* two adversarial
//! reviews before anyone noticed the flake).
//!
//! [`assert_stable_across_fresh_instances`] generalizes the shape of the
//! #795 regression test (`brink_analyzer::modules::tests::
//! single_file_declared_module_self_reference_never_e087`, a 64-iteration
//! loop each building a fresh `SymbolIndex` — i.e. a fresh `HashMap` with a
//! fresh random seed — and asserting the analyzer's output never varies) so
//! future determinism regressions across any crate can reuse it instead of
//! hand-rolling the loop again.
//!
//! A `HashMap`'s iteration order is stable *within* one process for maps
//! built the same way (same `RandomState`, same insertion sequence) but
//! varies *across* fresh instances/processes — which is exactly why a
//! single test run, or even `cargo test` re-run in the same process, can
//! stay green for a HashMap-order bug for a long time before a different
//! process (a different CI shard, a different dev machine, a different
//! build) hits a different seed and flakes. Rebuilding from scratch on every
//! iteration is what actually exercises the order space; running the same
//! built value N times would not.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Debug;

use brink_driver::Driver;

/// Call `build` `iterations` times — each call must construct its subject
/// from scratch, so a `HashMap` inside it picks a fresh `RandomState` seed
/// every time — and assert every result equals the first. `iterations`
/// should be large enough that an order-dependent bug's odds of hiding by
/// chance are negligible; 64 matches the #795 regression's shape.
///
/// # Panics
///
/// Panics (via `assert_eq!`) on the first iteration whose result differs
/// from the first — carrying the iteration index so a flake is reproducible
/// from the failure message alone.
#[track_caller]
pub fn assert_stable_across_fresh_instances<T, F>(iterations: usize, mut build: F)
where
    T: PartialEq + Debug,
    F: FnMut() -> T,
{
    assert!(iterations >= 2, "need at least 2 iterations to compare");
    let first = build();
    for i in 1..iterations {
        let next = build();
        assert_eq!(
            next, first,
            "output diverged on fresh instance {i} of {iterations} — a \
             HashMap/HashSet iteration order is leaking into this result"
        );
    }
}

/// Run [`Driver::discover`] (the production entry point — the same one
/// `brink-compiler`/the CLI call) over an in-memory 3-file circular
/// `INCLUDE` chain (`a -> b -> c -> a`) and return the `CircularInclude`
/// message text, or panic if discovery unexpectedly succeeded or failed a
/// different way — every iteration in a fresh-instance run must hit the
/// cycle.
fn discover_circular_include_message() -> String {
    let mut driver = Driver::new();
    let files = [
        ("a.ink", "INCLUDE b.ink\n== start ==\nHi\n-> DONE\n"),
        ("b.ink", "INCLUDE c.ink\n"),
        ("c.ink", "INCLUDE a.ink\n"),
    ];
    let read_file = |path: &str| {
        files
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, src)| (*src).to_string())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path.to_string()))
    };
    match driver.discover("a.ink", read_file) {
        Err(brink_driver::DiscoverError::CircularInclude(msg)) => msg,
        other => panic!("expected CircularInclude, got {other:?}"),
    }
}

/// Issue #801: `IncludeGraph::find_cycle` used to pick its DFS start node by
/// iterating `self.forward.keys()` — a `HashMap` — directly, so which
/// rotation of the `a -> b -> c -> a` cycle it reported (and therefore the
/// exact text of `DiscoverError::CircularInclude`'s message, which reaches
/// the user byte-for-byte) depended on that `HashMap`'s per-instance
/// `RandomState` seed. Fixed by sorting the start candidates before the DFS
/// (`crates/internal/brink-db/src/include_graph.rs`); this is the
/// fresh-instance repetition regression for that fix.
#[test]
fn circular_include_message_is_stable_across_fresh_project_dbs() {
    assert_stable_across_fresh_instances(64, discover_circular_include_message);
}
