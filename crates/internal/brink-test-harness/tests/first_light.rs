//! First light (`docs/b0-sequencing.md` §B0.10, issue #1106): the payoff of
//! the `.brink` respell corpus (`tests/tier1-brink-respell/`) — each
//! fixture's native source compiled through the honest minimal native
//! pipeline (`brink_test_harness::corpus::compile_and_explore_from_brink_native`)
//! and diffed against its paired ink oracle episodes.
//!
//! **Honesty is the whole point** (the builder brief for this milestone):
//! this test does not force green. Each fixture is reported
//! episode-identical or diverged, with the exact diff, via `--nocapture`
//! console output; the corpus-wide assertion only requires that every
//! fixture at least *runs* through the pipeline (no compile/link/explore
//! error) — episode identity per fixture is asserted individually below,
//! one `#[test]` per fixture, so a regression in one fixture never masks
//! the others and `cargo test` output names exactly which one broke.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::compile_and_explore_from_brink_native;
use brink_test_harness::episode::Episode;
use brink_test_harness::oracle;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn respell_fixture_dir(case: &str) -> PathBuf {
    repo_root()
        .join("tests")
        .join("tier1-brink-respell")
        .join(case)
}

/// Pull `oracle_case = "..."` out of a fixture's `manifest.toml` without
/// pulling in a TOML parsing dependency — the manifest's shape is a fixed,
/// hand-written set of `key = "value"` / `key = """ ... """` lines (see
/// `tests/tier1-brink-respell/README.md`), not free-form TOML this harness
/// needs to round-trip.
fn read_oracle_case(manifest_path: &Path) -> String {
    let text = std::fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("oracle_case") {
            let rest = rest.trim_start();
            let rest = rest
                .strip_prefix('=')
                .expect("oracle_case line must have '='");
            let rest = rest.trim();
            let rest = rest
                .strip_prefix('"')
                .expect("oracle_case value must be quoted");
            let end = rest.find('"').expect("oracle_case value must be quoted");
            return rest[..end].to_string();
        }
    }
    panic!("no oracle_case key found in {}", manifest_path.display());
}

/// `tier1::diverts::basic-tunnel` -> `tests/tier1/diverts/basic-tunnel`.
fn oracle_case_dir(oracle_case: &str) -> PathBuf {
    let mut dir = repo_root().join("tests");
    for segment in oracle_case.split("::") {
        dir.push(segment);
    }
    dir
}

fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

/// The result of diffing one fixture's native episodes against its ink
/// oracle: episode-identical, or the exact per-episode diffs.
enum FixtureResult {
    Identical { episode_count: usize },
    Diverged { report: String },
    PipelineError { stage: String },
}

/// Run one respell fixture through the native pipeline and diff its
/// episodes against the paired ink oracle. Never panics — returns a
/// [`FixtureResult`] so callers can report honestly instead of the test
/// aborting on the first divergence.
fn check_fixture(case: &str) -> FixtureResult {
    let fixture_dir = respell_fixture_dir(case);
    let src = std::fs::read_to_string(fixture_dir.join("story.brink"))
        .unwrap_or_else(|e| panic!("read {case}/story.brink: {e}"));
    let oracle_case = read_oracle_case(&fixture_dir.join("manifest.toml"));
    let oracle_dir = oracle_case_dir(&oracle_case);

    let oracle_eps = oracle::load_oracle_episodes(&oracle_dir).unwrap_or_else(|e| {
        panic!(
            "load oracle episodes for {case} ({oracle_case} -> {}): {e}",
            oracle_dir.display()
        )
    });
    assert!(
        !oracle_eps.is_empty(),
        "{case}: oracle case {oracle_case} has zero episodes — not a valid comparison target"
    );

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 1000,
    };

    let actual = match compile_and_explore_from_brink_native(&src, &config) {
        Ok((_data, episodes)) => episodes,
        Err(e) => {
            return FixtureResult::PipelineError { stage: e };
        }
    };

    let actual_index = index_by_choice_path(&actual);
    let mut report = String::new();
    let mut mismatch_count = 0usize;
    let mut missing_count = 0usize;

    for oracle_ep in &oracle_eps {
        if let Some(brink_ep) = actual_index.get(oracle_ep.choice_path.as_slice()) {
            let d = oracle::diff_oracle(oracle_ep, brink_ep);
            if !d.matches {
                mismatch_count += 1;
                let _ = write!(
                    report,
                    "  episode choice_path={:?}:\n{}",
                    oracle_ep.choice_path, d
                );
            }
        } else {
            missing_count += 1;
            let _ = writeln!(
                report,
                "  episode choice_path={:?}: no matching native episode (native produced {} episode(s) with choice_paths {:?})",
                oracle_ep.choice_path,
                actual.len(),
                actual.iter().map(|e| &e.choice_path).collect::<Vec<_>>()
            );
        }
    }

    // Extra native episodes the oracle never exercised are also a
    // divergence — a native-only path through the story is not
    // episode-identical to the ink twin even if every oracle-covered path
    // matches.
    let oracle_paths: std::collections::HashSet<&[usize]> = oracle_eps
        .iter()
        .map(|e| e.choice_path.as_slice())
        .collect();
    let mut extra_count = 0usize;
    for ep in &actual {
        if !oracle_paths.contains(ep.choice_path.as_slice()) {
            extra_count += 1;
            let _ = writeln!(
                report,
                "  extra native episode choice_path={:?} with no oracle counterpart",
                ep.choice_path
            );
        }
    }

    if mismatch_count == 0 && missing_count == 0 && extra_count == 0 {
        FixtureResult::Identical {
            episode_count: oracle_eps.len(),
        }
    } else {
        FixtureResult::Diverged { report }
    }
}

/// Assert episode-identity for one fixture, printing a clear diagnostic on
/// divergence — one `#[test]` per fixture so a single regression is named
/// precisely, per this milestone's "honesty is the whole point" mandate.
macro_rules! first_light_fixture_test {
    ($fn_name:ident, $case:literal) => {
        #[test]
        fn $fn_name() {
            match check_fixture($case) {
                FixtureResult::Identical { episode_count } => {
                    println!("{}: episode-identical ({episode_count} episodes)", $case);
                }
                FixtureResult::Diverged { report } => {
                    panic!("{}: DIVERGED from oracle:\n{report}", $case);
                }
                FixtureResult::PipelineError { stage } => {
                    panic!("{}: pipeline error: {stage}", $case);
                }
            }
        }
    };
}

first_light_fixture_test!(basic_tunnel, "basic-tunnel");
first_light_fixture_test!(complex_flow_v1, "complex-flow-v1");
first_light_fixture_test!(const_vars, "const-vars");
first_light_fixture_test!(exhibit_fogg_passage, "exhibit-fogg-passage");
first_light_fixture_test!(gather_basic, "gather-basic");
first_light_fixture_test!(manual_stitch_v1, "manual-stitch-v1");
first_light_fixture_test!(simple_glue, "simple-glue");
first_light_fixture_test!(sticky_choice, "sticky-choice");
first_light_fixture_test!(weave_options, "weave-options");

/// A non-failing summary across all 9 fixtures — always green, prints a
/// one-line-per-fixture status table with `--nocapture`. Exists so a CI run
/// (or a human) can see the aggregate first-light picture in one place
/// without wading through 9 separate pass/fail test results. The individual
/// `first_light_fixture_test!` cases above are the actual gate.
#[test]
fn first_light_summary() {
    let cases = [
        "basic-tunnel",
        "complex-flow-v1",
        "const-vars",
        "exhibit-fogg-passage",
        "gather-basic",
        "manual-stitch-v1",
        "simple-glue",
        "sticky-choice",
        "weave-options",
    ];
    println!("\n=== first light: native .brink respell corpus vs ink oracle ===");
    let mut identical = 0usize;
    for case in cases {
        match check_fixture(case) {
            FixtureResult::Identical { episode_count } => {
                identical += 1;
                println!("  PASS  {case} ({episode_count} episodes episode-identical)");
            }
            FixtureResult::Diverged { report } => {
                println!("  DIVERGED  {case}:\n{report}");
            }
            FixtureResult::PipelineError { stage } => {
                println!("  ERROR  {case}: {stage}");
            }
        }
    }
    println!(
        "=== first light: {identical}/{} fixtures episode-identical ===\n",
        cases.len()
    );
}
