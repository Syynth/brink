//! Algorithms corpus — sorting/searching lane (issue #822, epic #397/#822).
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

use brink_compiler::{AnalysisOptions, Dialect};
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

/// Run `story.ink` (brink dialect) to completion and return the
/// concatenated output text — mirrors `tier1_brink.rs::run_case` exactly
/// (deterministic `DotNetRng`, choice-free straight-line programs only).
fn run_case(dir: &Path) -> String {
    let ink_path = dir.join("story.ink");
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
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
    let dir = corpus_dir().join(name);
    let expected_msg = format!("read expected.txt for algorithms/{name}");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect(&expected_msg);
    let actual = run_case(&dir);
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
