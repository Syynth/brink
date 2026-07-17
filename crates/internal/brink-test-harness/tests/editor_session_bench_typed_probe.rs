//! `editor_session_bench`'s `BENCH_TYPED=1` post-edit typed probe (#819).
//!
//! #537's data-gathering pass found that the stock (untyped) bench never
//! reads `type_inference()` or any per-def query, so every per-def family
//! (`signature`/`solve_scc`/`infer_body`/`inferred_signature`/…) reports a
//! memo-count of 0 at every checkpoint, at every project scale — the bench
//! shape misses the typed substrate entirely, and any future capacity/memory
//! tuning of those families would be measuring nothing.
//!
//! This drives the *actual compiled binary* (not its internal functions) via
//! `CARGO_BIN_EXE_editor_session_bench`, once with `BENCH_TYPED` unset and
//! once with `BENCH_TYPED=1`, and asserts on the printed growth-table rows:
//!
//! - Without the probe: every inference-only per-def family's final `count`
//!   is `0` (the T2/FS effects layer legitimately reaches `call_edges_query`
//!   from the stock diagnostics pull — see [`STOCK_REACHABLE_FAMILIES`]).
//! - With the probe: every per-def family's final `count` is `> 0` — the
//!   probe actually reaches `signature`/`infer_body`/`inferred_signature`
//!   for defs in the touched file, which is `type_inference`'s only
//!   documented path in (see `brink_db::ProjectDb::type_inference`'s doc).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use regex::Regex;

/// Small and fast: enough base files/edits to exercise every base file at
/// least once (so every per-def family gets a chance to populate), while
/// keeping the test near-instant.
const EDITS: &str = "80";
const BASE_FILES: &str = "4";

/// The per-def query families named in the decision log ruling (matches
/// `fg5_memory_ceilings.rs`'s `PER_DEF_FAMILIES`) — every one of these reads
/// through the typed substrate and is therefore invisible to the stock bench.
const PER_DEF_FAMILIES: &[&str] = &[
    "signature_query",
    "def_body_query",
    "referenced_globals_query",
    "call_edges_query",
    "solve_scc_query",
    "inferred_signature_query",
    "infer_body_query",
];

fn run_bench(typed: bool) -> String {
    let bin = env!("CARGO_BIN_EXE_editor_session_bench");
    let mut cmd = Command::new(bin);
    cmd.args(["--edits", EDITS, "--base-files", BASE_FILES]);
    if typed {
        cmd.env("BENCH_TYPED", "1");
    } else {
        cmd.env_remove("BENCH_TYPED");
    }
    let output = cmd.output().expect("editor_session_bench should run");
    assert!(
        output.status.success(),
        "editor_session_bench (typed={typed}) exited non-zero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("bench stdout should be valid utf-8")
}

/// Parse a growth-table row's final (post-edits) `count` for the named
/// query, e.g. `count      0 ->     32 (Δ+32)` -> `32`. Returns `None` if the
/// family never appears in the growth table at all (never invoked).
fn final_count(stdout: &str, family: &str) -> Option<u64> {
    let pattern =
        Regex::new(r"count\s+\d+\s*->\s*(\d+)").expect("count-delta pattern is a valid regex");
    stdout
        .lines()
        .find(|line| line.contains("| growth |") && line.contains(family))
        .and_then(|line| pattern.captures(line))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u64>().ok())
}

/// Families the stock bench's own `diagnostics` pull now reaches without the
/// probe: the T2/FS effects layer (`effects_assertion_diagnostics_query`,
/// `await_purity_diagnostics_query`) runs in the diagnostics path and drives
/// the call graph, which executes `call_edges_query` per def. Still asserted
/// in the probe tests below (the probe must read strictly *more*), just not
/// expected to be zero without it.
const STOCK_REACHABLE_FAMILIES: &[&str] = &["call_edges_query"];

#[test]
fn stock_bench_reads_zero_memos_for_every_inference_only_family() {
    let stdout = run_bench(false);
    for &family in PER_DEF_FAMILIES {
        if STOCK_REACHABLE_FAMILIES.contains(&family) {
            continue;
        }
        let count = final_count(&stdout, family).unwrap_or(0);
        assert_eq!(
            count, 0,
            "{family}: stock (untyped) bench should read memo-count 0 (the bench shape \
             never touches the typed substrate) but got {count}"
        );
    }
}

#[test]
fn typed_probe_populates_every_per_def_family() {
    let stdout = run_bench(true);
    for &family in PER_DEF_FAMILIES {
        let count = final_count(&stdout, family);
        assert!(
            count.is_some_and(|c| c > 0),
            "{family}: BENCH_TYPED=1 should populate this family's memo table, but the \
             growth table reports {count:?} (probe did not reach it)"
        );
    }
}

#[test]
fn typed_probe_beats_stock_bench_on_every_per_def_family() {
    let untyped = run_bench(false);
    let typed = run_bench(true);
    for &family in PER_DEF_FAMILIES {
        let before = final_count(&untyped, family).unwrap_or(0);
        let after = final_count(&typed, family).unwrap_or(0);
        assert!(
            after > before,
            "{family}: typed probe should read strictly more memos than the stock bench \
             (before={before}, after={after})"
        );
    }
}
