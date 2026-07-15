//! Algorithms corpus — sorting/searching + graphs + DP lanes (issue #822,
//! epic #397/#822).
//!
//! Sibling of `tier1_brink.rs`'s flat `tests/tier1-brink/<name>/` corpus,
//! scoped to `tests/tier1-brink/algorithms/<name>/`: classic algorithms
//! ported to idiomatic brink, each a `story.ink` + hand-verified
//! `expected.txt` golden transcript with no C# oracle behind it (same
//! rationale as `tier1_brink.rs`'s header — brink-dialect syntax has no
//! inklecate equivalent to generate one from). Kept in its own test file
//! (not added to `tier1_brink.rs`) per the epic's "your own new files"
//! scope discipline — `tier1_brink.rs`'s `every_case_directory_has_a_test`
//! invariant skips this nested `algorithms/` directory (see that file);
//! this file owns the equivalent invariant for its own subtree.
//!
//! Each `story.ink` states its own `types` policy and ships an
//! ERGONOMICS-FINDINGS header per the epic's rules — read the source files
//! themselves for the findings; this harness only proves the programs
//! compile, run to completion within the VM's default step limit, and
//! produce byte-identical, deterministic output.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, Line, Story};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-brink")
        .join("algorithms")
}

/// Run `story.ink` (brink dialect, gradual types) to completion and
/// return the concatenated output text — mirrors `tier1_brink.rs::run_case`
/// exactly (deterministic `DotNetRng`, choice-free straight-line programs
/// only).
fn run_case(dir: &Path) -> String {
    run_case_with_types(dir, TypePolicy::Gradual)
}

/// Same as [`run_case`], but lets a case opt into `types = strict` — used
/// by `dijkstra-grid`, whose header documents the strict-mode experiment.
fn run_case_with_types(dir: &Path, types: TypePolicy) -> String {
    let ink_path = dir.join("story.ink");
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types,
        ..AnalysisOptions::default()
    };
    let compile_msg = format!("compile {}", ink_path.display());
    let output = brink_compiler::compile_path_with_options(&ink_path, options).expect(&compile_msg);
    let link_msg = format!("link {}", ink_path.display());
    let (program, line_tables) = brink_runtime::link(&output.data).expect(&link_msg);
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    let step_msg = format!("runtime error in {}", ink_path.display());
    let mut out = String::new();
    let mut hit_choices = false;
    loop {
        match story.continue_single().expect(&step_msg) {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => {
                hit_choices = true;
                break;
            }
        }
    }
    assert!(
        !hit_choices,
        "{} presented choices — algorithms-corpus cases must be choice-free straight-line programs",
        ink_path.display()
    );
    out
}

fn assert_case(name: &str) {
    assert_case_with_types(name, TypePolicy::Gradual);
}

/// Same as [`assert_case`], but lets a case opt into `types = strict`.
fn assert_case_with_types(name: &str, types: TypePolicy) {
    let dir = corpus_dir().join(name);
    let expected_msg = format!("read expected.txt for algorithms/{name}");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect(&expected_msg);
    let actual = run_case_with_types(&dir, types);
    assert_eq!(
        actual, expected,
        "case algorithms/{name}: output mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

/// Re-running a case must reproduce byte-identical output — the
/// determinism the epic's "Gates" section requires isn't just "matches a
/// golden file once", it's "matches on every run", which matters most for
/// `fisher-yates-shuffle`'s seeded `RANDOM` draws.
fn assert_case_is_deterministic_across_runs(name: &str) {
    let dir = corpus_dir().join(name);
    let first = run_case(&dir);
    let second = run_case(&dir);
    assert_eq!(
        first, second,
        "case algorithms/{name}: two runs of the same story diverged — not deterministic"
    );
}

#[test]
fn quicksort_partitions_and_recombines_functionally() {
    assert_case("quicksort");
}

#[test]
fn mergesort_splits_and_merges() {
    assert_case("mergesort");
}

#[test]
fn insertion_sort_shifts_in_place() {
    assert_case("insertion-sort");
}

#[test]
fn binary_search_finds_present_and_absent_targets() {
    assert_case("binary-search");
}

#[test]
fn fisher_yates_shuffle_is_seeded_and_deterministic() {
    assert_case("fisher-yates-shuffle");
    assert_case_is_deterministic_across_runs("fisher-yates-shuffle");
}

#[test]
fn bfs_grid_path_finds_shortest_route() {
    assert_case("bfs-grid-path");
}

#[test]
fn dfs_grid_path_finds_a_route_not_necessarily_shortest() {
    assert_case("dfs-grid-path");
}

/// Same grid, same start/goal as `dfs_grid_path_...` — BFS's level-order
/// search and DFS's fixed-priority stack search must land on different
/// answers on purpose (see both files' headers). If a future change to
/// either port ever made them agree, that would mean one of them stopped
/// demonstrating what it's there to demonstrate.
#[test]
fn bfs_and_dfs_diverge_on_the_shared_grid() {
    let bfs = run_case(&corpus_dir().join("bfs-grid-path"));
    let dfs = run_case(&corpus_dir().join("dfs-grid-path"));
    assert_ne!(
        bfs, dfs,
        "bfs-grid-path and dfs-grid-path produced identical output — the shared grid's \
         entire point is that BFS finds the 4-cell shortest path and DFS finds the 14-cell \
         loop; if these ever match, the grid fixture no longer demonstrates the divergence"
    );
}

#[test]
fn dijkstra_grid_finds_cheapest_route_under_strict_types() {
    assert_case_with_types("dijkstra-grid", TypePolicy::Strict);
}

#[test]
fn astar_grid_finds_same_cost_as_dijkstra_with_fewer_nodes_visited() {
    assert_case("astar-grid");
}

/// A* must match Dijkstra's optimal cost on the shared weighted grid (the
/// Manhattan heuristic is admissible here — see `astar-grid`'s header) —
/// this is the "living documentation" payoff made mechanically true, not
/// just asserted in a comment.
#[test]
fn astar_matches_dijkstra_cost_on_the_shared_weighted_grid() {
    let dijkstra = run_case_with_types(&corpus_dir().join("dijkstra-grid"), TypePolicy::Strict);
    let astar = run_case(&corpus_dir().join("astar-grid"));
    let dijkstra_cost = dijkstra
        .lines()
        .find(|l| l.starts_with("Total cost:"))
        .expect("dijkstra-grid prints a Total cost line");
    let astar_cost = astar
        .lines()
        .find(|l| l.starts_with("Total cost:"))
        .expect("astar-grid prints a Total cost line");
    assert_eq!(
        dijkstra_cost, astar_cost,
        "dijkstra-grid and astar-grid must find the same optimal cost on the shared grid"
    );
}

#[test]
fn memoized_fibonacci_reuses_subproblems_via_a_local_memo_map() {
    assert_case("memoized-fibonacci");
}

#[test]
fn knapsack_01_maximizes_value_under_capacity_via_composite_key_memo() {
    assert_case("knapsack-01");
}

#[test]
fn longest_common_subsequence_recovers_the_subsequence_via_map_of_maps_memo() {
    assert_case("longest-common-subsequence");
}

#[test]
fn edit_distance_computes_levenshtein_distance_via_bottom_up_table() {
    assert_case("edit-distance");
}

/// Every `tests/tier1-brink/algorithms/` case directory is exercised by a
/// `#[test]` above — a directory with no matching test would silently
/// never run (same invariant `tier1_brink.rs` enforces for its own flat
/// corpus).
#[test]
fn every_algorithms_case_directory_has_a_test() {
    let known = [
        "quicksort",
        "mergesort",
        "insertion-sort",
        "binary-search",
        "fisher-yates-shuffle",
        "bfs-grid-path",
        "dfs-grid-path",
        "dijkstra-grid",
        "astar-grid",
        "memoized-fibonacci",
        "knapsack-01",
        "longest-common-subsequence",
        "edit-distance",
    ];
    let mut found: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-brink/algorithms")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected: Vec<String> = known.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(found, expected, "add a #[test] for every case directory");
}
