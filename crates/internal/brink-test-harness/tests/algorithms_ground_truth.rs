//! Corpus differential oracle (issue #898) — cross-validates the
//! algorithms corpus (`tests/tier1-brink/algorithms/`, issue #822) against
//! independent Rust ground truth (`brink_test_harness::ground_truth`),
//! not just against a hand-verified golden transcript.
//!
//! **Why this exists, separate from `algorithms_corpus.rs`:**
//! `algorithms_corpus.rs` proves each case compiles, runs to completion,
//! and reproduces its `expected.txt` byte-for-byte on every run — i.e. it
//! proves *determinism*. It does NOT prove *correctness*: a VM bug that
//! perturbs a computed RESULT while keeping the transcript internally
//! stable (e.g. wrong arithmetic, a COW/collection bug that silently
//! drops or duplicates an element) would still pass every
//! `algorithms_corpus.rs` assertion, because a stably-wrong quicksort
//! reproduces its own (wrong) golden output just as reliably as a correct
//! one would reproduce a correct one. This file is the other half: each
//! case's seeded input and computed result, extracted from the compiled
//! program's own output text (or, where the output doesn't carry the
//! input, from the `story.ink` source's own `VAR` literal — see each
//! `extract_*` helper below), cross-checked against a reference
//! implementation that shares zero code with `brink-runtime` or with the
//! ink port under test.
//!
//! **Lane coverage in this PR (issue #898's own "start with the crisp
//! lanes" instruction) — state per lane what is and isn't proven:**
//! - **Sorting** (`quicksort`, `mergesort`, `insertion-sort`): exact
//!   equality against `slice::sort`
//!   (`brink_test_harness::ground_truth::sort`), both this corpus's one
//!   fixed input AND property mode — `sorting_matches_rust_sort_property`
//!   below runs `PROPERTY_ITERATIONS` (bounded, seeded via
//!   `ground_truth::pcg` for CI-reproducible failures) freshly-compiled
//!   randomized-array variants of `quicksort/story.ink` through the real
//!   VM and diffs each against `slice::sort` on the same array.
//! - **DP** (`knapsack-01`, `edit-distance`,
//!   `longest-common-subsequence`, `memoized-fibonacci`): exact equality
//!   against direct (non-memoized) recurrences
//!   (`brink_test_harness::ground_truth::dp`) on each case's one fixed
//!   corpus input. One carve-out mirroring the graphs lane's "cost, not
//!   shape": `longest-common-subsequence`'s fixture has more than one
//!   valid maximum-length common subsequence ("GA" and "AC" both work), so
//!   only the LCS *length* is checked, not the recovered text (see
//!   `ground_truth::dp::lcs`'s doc). Property mode is NOT wired up for
//!   this lane in this PR — templating a `VAR`-based DP program's input
//!   safely (variable table sizes, map-keyed memo shapes) is more involved
//!   than sorting's single-array template and is left as follow-up scope
//!   (see this crate's issue tracker; do not silently assume it's
//!   covered).
//! - **Graphs** (`dijkstra-grid`, `astar-grid`): exact path-COST equality
//!   against a hand-rolled, independently unit-tested Dijkstra
//!   (`brink_test_harness::ground_truth::graph::dijkstra_cost`,
//!   `no new workspace dependency — no petgraph`) on the shared fixed
//!   grid/start/goal both cases use. Does not validate the recovered path
//!   *shape* (more than one minimum-cost path can exist on this grid —
//!   only the COST is a unique, checkable ground truth) and does not
//!   attempt property mode in this PR (grid templating raises the same
//!   "safely varying a `VAR` shape" question as the DP lane; left as the
//!   same follow-up).
//! - **Randomness lane statistical bounds** (chi-squared over
//!   `weighted-loot-table`'s draw distribution) and **procgen invariant
//!   checks** (BSP connectivity, cave open-ratio bounds) are explicitly
//!   OUT of scope for this PR — issue #898 asks for a different kind of
//!   oracle there (bound-satisfaction, not exact equality) and bundling
//!   it into this first slice would blur what's actually been proven.
//!   Tracked as follow-up, not silently dropped.
//!
//! No `story.ink`/`expected.txt` corpus file changes were needed to reach
//! this coverage: every crisp-lane case already prints its seeded input
//! and/or result in a stable, parseable form (`Sorted: [...]`, `Original
//! untouched: [...]`, `Total cost: N`, `Best value for capacity N: V`,
//! `LCS length: N. LCS: text`, `fib(N) = V`), so no additive transcript
//! markers were required — the golden `expected.txt` files this PR
//! touches: NONE. If a future lane's case doesn't already print what this
//! harness needs, add a marker there per issue #898's own instruction
//! ("additive, keep golden files updated") rather than reaching for
//! source-text scraping as a substitute.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, Line, Story};
use brink_test_harness::ground_truth::{dp, graph, ink_literal, pcg, sort};
use regex::Regex;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-brink")
        .join("algorithms")
}

/// Run `story.ink` (brink dialect) to completion and return the
/// concatenated output text — mirrors `algorithms_corpus.rs::run_case_
/// with_types` exactly (this file is its own self-contained test binary
/// per this corpus's established scope-discipline convention; see that
/// file's own header for why the helper isn't shared).
fn run_case_with_types(dir: &Path, types: TypePolicy) -> String {
    let ink_path = dir.join("story.ink");
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(types),
        ..AnalysisOptions::default()
    };
    let compile_msg = format!("compile {}", ink_path.display());
    let output = brink_compiler::compile_path_with_options(&ink_path, options).expect(&compile_msg);
    let link_msg = format!("link {}", ink_path.display());
    let (program, line_tables) = brink_runtime::link(&output.data).expect(&link_msg);
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    let step_msg = format!("runtime error in {}", ink_path.display());
    let mut out = String::new();
    loop {
        match story.continue_single().expect(&step_msg) {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => {
                panic!(
                    "{} presented choices — algorithms-corpus cases must be choice-free",
                    ink_path.display()
                );
            }
        }
    }
    out
}

/// Compile+run an in-memory source string (used only by the sorting
/// lane's property-mode test) under gradual types.
fn run_source(label: &str, source: &str) -> String {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let output = brink_compiler::compile_with_options(label, |_| Ok(source.to_owned()), options)
        .unwrap_or_else(|e| panic!("compile {label}: {e}"));
    let (program, line_tables) =
        brink_runtime::link(&output.data).unwrap_or_else(|e| panic!("link {label}: {e}"));
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut out = String::new();
    loop {
        match story
            .continue_single()
            .unwrap_or_else(|e| panic!("runtime error in {label}: {e}"))
        {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => panic!("{label} presented choices unexpectedly"),
        }
    }
    out
}

// ── Extraction helpers ───────────────────────────────────────────────────
//
// Every helper below extracts data already present in a case's own output
// text or source text — no corpus file was modified to support this.

/// Extract the brink-runtime `Display` form of an int array (`[1, 2, -3]`,
/// no leading `#`) printed after `prefix` on some line of `output`.
fn extract_bracketed_ints(output: &str, prefix: &str) -> Vec<i64> {
    let line = output
        .lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with `{prefix}` in:\n{output}"));
    let rest = &line[prefix.len()..];
    let inner = rest
        .trim_end_matches('.')
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or_else(|| panic!("expected a bracketed int list after `{prefix}`, got: {rest}"));
    inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<i64>()
                .unwrap_or_else(|e| panic!("not an int in `{prefix}` list: `{s}` ({e})"))
        })
        .collect()
}

/// Extract a `VAR name = #[...]` integer-array literal straight out of a
/// case's `story.ink` source text (case's `name`d fixed seeded input —
/// used where the output text doesn't itself carry the input, e.g.
/// `insertion-sort` sorts in place and only ever prints the sorted
/// result).
fn extract_var_int_array(source: &str, name: &str) -> Vec<i64> {
    let re = Regex::new(&format!(r"(?m)^VAR {name} = (#\[.*\])\s*$")).expect("valid regex");
    let literal = re
        .captures(source)
        .unwrap_or_else(|| panic!("no `VAR {name} = #[...]` line found"))
        .get(1)
        .expect("capture group 1")
        .as_str();
    ink_literal::parse_int_array(literal).unwrap_or_else(|e| panic!("`VAR {name}` literal: {e}"))
}

/// Extract a `VAR name = -?\d+` scalar int straight out of source text.
fn extract_var_int_scalar(source: &str, name: &str) -> i64 {
    let re = Regex::new(&format!(r"(?m)^VAR {name} = (-?\d+)\s*$")).expect("valid regex");
    re.captures(source)
        .unwrap_or_else(|| panic!("no `VAR {name} = <int>` line found"))
        .get(1)
        .expect("capture group 1")
        .as_str()
        .parse()
        .expect("valid int")
}

/// Extract a `VAR name = #["a", "b", ...]` quoted-string-array literal
/// (the DP lane's sequence inputs, e.g. `seqA`/`seqB`, `wordA`/`wordB`)
/// as an owned `Vec<String>`.
fn extract_var_string_array(source: &str, name: &str) -> Vec<String> {
    let re = Regex::new(&format!(r"(?m)^VAR {name} = #\[(.*)\]\s*$")).expect("valid regex");
    let inner = re
        .captures(source)
        .unwrap_or_else(|| panic!("no `VAR {name} = #[...]` line found"))
        .get(1)
        .expect("capture group 1")
        .as_str();
    let token_re = Regex::new("\"([^\"]*)\"").expect("valid regex");
    token_re
        .captures_iter(inner)
        .map(|c| c[1].to_string())
        .collect()
}

/// Extract the nested `VAR grid = #[#[...], ...]` int-grid literal from
/// `dijkstra-grid`/`astar-grid`'s shared source shape.
fn extract_var_int_grid(source: &str, name: &str) -> Vec<Vec<i64>> {
    let re = Regex::new(&format!(r"(?m)^VAR {name} = (#\[.*\])\s*$")).expect("valid regex");
    let literal = re
        .captures(source)
        .unwrap_or_else(|| panic!("no `VAR {name} = #[#[...]]` line found"))
        .get(1)
        .expect("capture group 1")
        .as_str();
    ink_literal::parse_int_grid(literal).unwrap_or_else(|e| panic!("`VAR {name}` literal: {e}"))
}

/// Extract a labeled trailing integer, e.g. `"Total cost: 12"` -> `12`,
/// from anywhere in `output` (not anchored to line start — some of these
/// labels appear mid-sentence, e.g. `"...capacity 5: 7. Memo entries:"`).
fn extract_labeled_int(output: &str, label: &str) -> i64 {
    let re = Regex::new(&format!(r"{}\s*(-?\d+)", regex::escape(label))).expect("valid regex");
    re.captures(output)
        .unwrap_or_else(|| panic!("no `{label}<int>` found in:\n{output}"))
        .get(1)
        .expect("capture group 1")
        .as_str()
        .parse()
        .expect("valid int")
}

// ── Sorting lane ─────────────────────────────────────────────────────────

fn assert_sorted_matches_rust_sort(name: &str, input: &[i64], actual_sorted: &[i64]) {
    let expected = sort::reference_sort(input);
    assert_eq!(
        actual_sorted, expected,
        "algorithms/{name}: VM-computed sort diverges from `slice::sort` on the same input \
         {input:?} — a stably-wrong sort would still pass the golden-transcript test, which is \
         exactly the gap issue #898 exists to close"
    );
}

#[test]
fn quicksort_matches_rust_slice_sort_on_the_fixed_corpus_input() {
    let output = run_case_with_types(&corpus_dir().join("quicksort"), TypePolicy::Gradual);
    let input = extract_bracketed_ints(&output, "Original untouched: ");
    let actual = extract_bracketed_ints(&output, "Sorted: ");
    assert_sorted_matches_rust_sort("quicksort", &input, &actual);
}

#[test]
fn mergesort_matches_rust_slice_sort_on_the_fixed_corpus_input() {
    let output = run_case_with_types(&corpus_dir().join("mergesort"), TypePolicy::Gradual);
    let input = extract_bracketed_ints(&output, "Original untouched: ");
    let actual = extract_bracketed_ints(&output, "Sorted: ");
    assert_sorted_matches_rust_sort("mergesort", &input, &actual);
}

#[test]
fn insertion_sort_matches_rust_slice_sort_on_the_fixed_corpus_input() {
    let dir = corpus_dir().join("insertion-sort");
    let source = std::fs::read_to_string(dir.join("story.ink")).expect("read story.ink");
    let input = extract_var_int_array(&source, "arr");
    let output = run_case_with_types(&dir, TypePolicy::Gradual);
    let actual = extract_bracketed_ints(&output, "Sorted: ");
    assert_sorted_matches_rust_sort("insertion-sort", &input, &actual);
}

/// Property mode (issue #898's explicit ask): `PROPERTY_ITERATIONS`
/// freshly-generated random arrays, seeded via `ground_truth::pcg` (the
/// same PCG-style generator `pcg-rng/story.ink` uses — see that module's
/// doc for why: one seed-to-sequence mapping shared between the ink
/// corpus and this Rust-side generator, not two independently-invented
/// RNGs), each compiled into a fresh `quicksort`-shaped program and run
/// through the real VM, diffed against `slice::sort` on the same array.
/// Bounded (not "as many as CI time allows") per this project's own
/// "unbounded growth" house rule.
const PROPERTY_ITERATIONS: u32 = 24;
const PROPERTY_SEED: i32 = 424_242;

/// Build a `quicksort`-shaped program (identical function body to
/// `quicksort/story.ink`'s own `quicksort` function — copied verbatim so
/// this test exercises the SAME compiled logic the corpus case does, just
/// with a templated `VAR arr` literal) printing only the sorted result.
fn quicksort_source_for(values: &[i64]) -> String {
    let literal = format!(
        "#[{}]",
        values
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    format!(
        "VAR arr = {literal}\n\
         VAR sorted = 0\n\
         ~ sorted = quicksort(arr)\n\
         Sorted: {{sorted}}.\n\
         -> END\n\n\
         === function quicksort(xs) ===\n\
         ~ {{\n\
         \x20   if len(xs) <= 1 {{\n\
         \x20       return xs\n\
         \x20   }}\n\
         \x20   temp pivot = xs[0]\n\
         \x20   temp less = #[]\n\
         \x20   temp equal = #[]\n\
         \x20   temp greater = #[]\n\
         \x20   for x in xs {{\n\
         \x20       if x < pivot {{\n\
         \x20           push(less, x)\n\
         \x20       }} else if x == pivot {{\n\
         \x20           push(equal, x)\n\
         \x20       }} else {{\n\
         \x20           push(greater, x)\n\
         \x20       }}\n\
         \x20   }}\n\
         \x20   temp left = quicksort(less)\n\
         \x20   temp right = quicksort(greater)\n\
         \x20   temp out = #[]\n\
         \x20   for x in left {{\n\
         \x20       push(out, x)\n\
         \x20   }}\n\
         \x20   for x in equal {{\n\
         \x20       push(out, x)\n\
         \x20   }}\n\
         \x20   for x in right {{\n\
         \x20       push(out, x)\n\
         \x20   }}\n\
         \x20   return out\n\
         }}\n"
    )
}

#[test]
fn sorting_matches_rust_sort_property_mode_over_pcg_seeded_random_arrays() {
    let mut state = pcg::seed(PROPERTY_SEED);
    for iter in 0..PROPERTY_ITERATIONS {
        // 1..=12 elements, values in [-50, 50) — small enough to keep each
        // compile+run cheap across PROPERTY_ITERATIONS in CI.
        let (len_draw, s1) = pcg::below(state, 12);
        state = s1;
        // `pcg::below` guarantees a result in `[0, bound)`, so this never
        // actually truncates/loses sign — `try_from` + `unwrap_or` just
        // keeps clippy's cast lints satisfied without an `as`.
        let len = usize::try_from(len_draw).unwrap_or(0) + 1;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            let (v, s2) = pcg::below(state, 100);
            state = s2;
            values.push(i64::from(v) - 50);
        }

        let label = format!("quicksort_property_{iter}");
        let output = run_source(&label, &quicksort_source_for(&values));
        let actual = extract_bracketed_ints(&output, "Sorted: ");
        assert_sorted_matches_rust_sort(&label, &values, &actual);
    }
}

// ── DP lane ───────────────────────────────────────────────────────────────

#[test]
fn edit_distance_matches_a_direct_levenshtein_recurrence() {
    let output = run_case_with_types(&corpus_dir().join("edit-distance"), TypePolicy::Gradual);
    // The output line itself carries both inputs and the result:
    // `Edit distance from "kitten" to "sitting": 3.`
    let re =
        Regex::new(r#"Edit distance from "([^"]*)" to "([^"]*)": (-?\d+)\."#).expect("valid regex");
    let caps = re
        .captures(&output)
        .unwrap_or_else(|| panic!("edit-distance: unexpected output shape:\n{output}"));
    let a: Vec<&str> = caps[1].split("").filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = caps[2].split("").filter(|s| !s.is_empty()).collect();
    let actual: i64 = caps[3].parse().expect("valid int");
    let expected = dp::edit_distance(&a, &b);
    assert_eq!(
        actual, expected,
        "algorithms/edit-distance: VM-computed distance diverges from a direct Levenshtein \
         recurrence on the same inputs {a:?} -> {b:?}"
    );
}

#[test]
fn knapsack_01_matches_a_direct_bottom_up_table() {
    let dir = corpus_dir().join("knapsack-01");
    let source = std::fs::read_to_string(dir.join("story.ink")).expect("read story.ink");
    let weights = extract_var_int_array(&source, "weights");
    let values = extract_var_int_array(&source, "values");
    let capacity = extract_var_int_scalar(&source, "capacity");

    let output = run_case_with_types(&dir, TypePolicy::Gradual);
    let actual = extract_labeled_int(&output, "Best value for capacity 5:");
    let expected = dp::knapsack_01(&weights, &values, capacity);
    assert_eq!(
        actual, expected,
        "algorithms/knapsack-01: VM-computed best value diverges from a direct 0/1 knapsack \
         table on weights={weights:?} values={values:?} capacity={capacity}"
    );
}

#[test]
fn longest_common_subsequence_matches_a_direct_bottom_up_table() {
    let dir = corpus_dir().join("longest-common-subsequence");
    let source = std::fs::read_to_string(dir.join("story.ink")).expect("read story.ink");
    let seq_a = extract_var_string_array(&source, "seqA");
    let seq_b = extract_var_string_array(&source, "seqB");
    let a: Vec<&str> = seq_a.iter().map(String::as_str).collect();
    let b: Vec<&str> = seq_b.iter().map(String::as_str).collect();

    let output = run_case_with_types(&dir, TypePolicy::Gradual);
    let actual_len = extract_labeled_int(&output, "LCS length:");

    // Only `length` is checked — the recovered TEXT is not a unique ground
    // truth when more than one maximum-length common subsequence exists
    // (true of this exact fixture: both "GA" and "AC" are valid length-2
    // LCSes of seqA/seqB), so a backtrack tie-break mismatch between this
    // reference and the ink port is not itself a bug. See
    // `ground_truth::dp::lcs`'s doc and this file's module doc (same
    // "cost not shape" carve-out the graphs lane uses).
    let (expected_len, _expected_text_not_unique) = dp::lcs(&a, &b);
    assert_eq!(
        actual_len, expected_len,
        "algorithms/longest-common-subsequence: VM-computed LCS length diverges from a direct \
         LCS table on seqA={a:?} seqB={b:?}"
    );
}

#[test]
fn memoized_fibonacci_matches_a_direct_iterative_recurrence() {
    let output = run_case_with_types(
        &corpus_dir().join("memoized-fibonacci"),
        TypePolicy::Gradual,
    );
    let re = Regex::new(r"fib\((\d+)\) = (-?\d+)").expect("valid regex");
    let mut checked = 0;
    for caps in re.captures_iter(&output) {
        let n: u32 = caps[1].parse().expect("valid int");
        let actual: i64 = caps[2].parse().expect("valid int");
        let expected = dp::fibonacci(n);
        assert_eq!(
            actual, expected,
            "algorithms/memoized-fibonacci: VM-computed fib({n}) diverges from a direct \
             iterative recurrence"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 3,
        "expected exactly 3 `fib(N) = V` occurrences (fib10/fib20/fib30) in:\n{output}"
    );
}

// ── Graphs lane ────────────────────────────────────────────────────────────

fn assert_grid_cost_matches_dijkstra(name: &str, source: &str, output: &str) {
    let grid = extract_var_int_grid(source, "grid");
    // Both `dijkstra-grid` and `astar-grid` fix `start = Point#{r: 0, c: 0}`
    // and `goal = Point#{r: 5, c: 5}` (see this test's module doc + both
    // `story.ink` headers) — parsed here rather than hardcoded so a future
    // edit to either fixture can't silently drift from what this test
    // checks.
    let point_re = |var: &str| {
        Regex::new(&format!(r"temp {var} = Point#\{{r: (-?\d+), c: (-?\d+)\}}"))
            .expect("valid regex")
    };
    let start_caps = point_re("start")
        .captures(source)
        .unwrap_or_else(|| panic!("{name}: no `temp start = Point#{{...}}` found"));
    let goal_caps = point_re("goal")
        .captures(source)
        .unwrap_or_else(|| panic!("{name}: no `temp goal = Point#{{...}}` found"));
    let parse_coord = |s: &str| {
        usize::try_from(s.parse::<i64>().expect("valid int")).expect("non-negative coordinate")
    };
    let start = (parse_coord(&start_caps[1]), parse_coord(&start_caps[2]));
    let goal = (parse_coord(&goal_caps[1]), parse_coord(&goal_caps[2]));

    let actual_cost = extract_labeled_int(output, "Total cost:");
    let expected_cost = graph::dijkstra_cost(&grid, start, goal)
        .unwrap_or_else(|| panic!("{name}: reference Dijkstra found the goal unreachable"));
    assert_eq!(
        actual_cost, expected_cost,
        "algorithms/{name}: VM-computed path cost diverges from a hand-rolled, independently \
         unit-tested Dijkstra on the same grid/start/goal"
    );
}

#[test]
fn dijkstra_grid_cost_matches_a_reference_dijkstra() {
    let dir = corpus_dir().join("dijkstra-grid");
    let source = std::fs::read_to_string(dir.join("story.ink")).expect("read story.ink");
    let output = run_case_with_types(&dir, TypePolicy::Strict);
    assert_grid_cost_matches_dijkstra("dijkstra-grid", &source, &output);
}

#[test]
fn astar_grid_cost_matches_a_reference_dijkstra() {
    let dir = corpus_dir().join("astar-grid");
    let source = std::fs::read_to_string(dir.join("story.ink")).expect("read story.ink");
    let output = run_case_with_types(&dir, TypePolicy::Gradual);
    assert_grid_cost_matches_dijkstra("astar-grid", &source, &output);
}
