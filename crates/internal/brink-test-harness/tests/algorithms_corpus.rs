//! Algorithms corpus — sorting/searching + graphs + DP + procgen +
//! AI-decision + spatial lanes (issue #822, epic #397/#822).
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
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
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
    let case_label = format!("algorithms/{name}");
    let expected =
        brink_test_harness::corpus::load_golden_transcript(&dir.join("expected.txt"), &case_label)
            .expect("golden transcript must be present and non-vacuous");
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
fn drunkards_walk_carves_a_bounded_cave() {
    assert_case("drunkards-walk");
    assert_case_is_deterministic_across_runs("drunkards-walk");
}

#[test]
fn bsp_dungeon_partitions_the_root_rect_exactly() {
    assert_case("bsp-dungeon");
    assert_case_is_deterministic_across_runs("bsp-dungeon");
}

#[test]
fn cellular_automata_cave_smooths_over_fixed_generations() {
    assert_case("cellular-automata-cave");
    assert_case_is_deterministic_across_runs("cellular-automata-cave");
}

#[test]
fn value_noise_field_interpolates_a_hashed_lattice() {
    assert_case("value-noise-field");
    assert_case_is_deterministic_across_runs("value-noise-field");
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

#[test]
fn behavior_tree_composes_sequence_selector_and_invert_over_a_blackboard() {
    assert_case("behavior-tree");
}

#[test]
fn utility_ai_scores_actions_by_weighted_considerations() {
    assert_case("utility-ai");
}

#[test]
fn minimax_tictactoe_finds_wins_and_blocks_under_strict_types() {
    assert_case_with_types("minimax-tictactoe", TypePolicy::Strict);
}

#[test]
fn npc_fsm_dispatches_dialogue_state_through_a_map_of_fn_handlers() {
    assert_case("npc-fsm");
}

#[test]
fn pcg_rng_streams_deterministic_draws_from_an_explicit_state_value() {
    assert_case("pcg-rng");
    assert_case_is_deterministic_across_runs("pcg-rng");
}

#[test]
fn weighted_loot_table_scans_a_hand_rolled_cumulative_array() {
    assert_case("weighted-loot-table");
    assert_case_is_deterministic_across_runs("weighted-loot-table");
}

#[test]
fn alias_method_draws_in_constant_time_via_voses_table() {
    assert_case("alias-method");
    assert_case_is_deterministic_across_runs("alias-method");
}

#[test]
fn shuffle_bag_refills_and_reshuffles_on_empty() {
    assert_case("shuffle-bag");
    assert_case_is_deterministic_across_runs("shuffle-bag");
}

#[test]
fn reservoir_sampling_keeps_a_bounded_uniform_sample_of_the_stream() {
    assert_case("reservoir-sampling");
    assert_case_is_deterministic_across_runs("reservoir-sampling");
}

#[test]
fn goap_plans_and_executes_the_cheapest_action_sequence_to_the_goal() {
    assert_case("goap");
    assert_case_is_deterministic_across_runs("goap");
}

#[test]
fn mcts_lite_explores_tree_via_ucb1_selection() {
    assert_case("mcts-lite");
    assert_case_is_deterministic_across_runs("mcts-lite");
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
        "drunkards-walk",
        "bsp-dungeon",
        "cellular-automata-cave",
        "value-noise-field",
        "memoized-fibonacci",
        "knapsack-01",
        "longest-common-subsequence",
        "edit-distance",
        "behavior-tree",
        "utility-ai",
        "minimax-tictactoe",
        "npc-fsm",
        "pcg-rng",
        "weighted-loot-table",
        "alias-method",
        "shuffle-bag",
        "reservoir-sampling",
        "bresenham-line",
        "spatial-hash-grid",
        "quadtree",
        "shadowcasting-fov",
        "goap",
        "mcts-lite",
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

#[test]
fn bresenham_line_rasterizes_all_octants_and_reports_direction_asymmetry() {
    assert_case("bresenham-line");
    assert_case_is_deterministic_across_runs("bresenham-line");
}

#[test]
fn spatial_hash_grid_query_matches_brute_force_scan() {
    assert_case("spatial-hash-grid");
    assert_case_is_deterministic_across_runs("spatial-hash-grid");
}

#[test]
fn quadtree_insert_and_range_query_match_brute_force_scan() {
    assert_case("quadtree");
    assert_case_is_deterministic_across_runs("quadtree");
}

#[test]
fn shadowcasting_fov_casts_symmetric_shadows_from_symmetric_walls() {
    assert_case("shadowcasting-fov");
    assert_case_is_deterministic_across_runs("shadowcasting-fov");
}
