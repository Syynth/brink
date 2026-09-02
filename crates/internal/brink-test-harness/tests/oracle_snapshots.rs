//! Snapshot-based oracle comparison test.
//!
//! Produces per-case insta snapshots of brink's actual output, plus a
//! corpus-wide summary snapshot. Any behavioral change to the compiler
//! or runtime shows up as a snapshot diff, making regressions immediately
//! visible via `cargo insta review` or `git diff`.
//!
//! Subsumes `oracle_comparison.rs` — includes the ratchet assertion.
//!
//! Usage:
//!   `cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture`
//!   `BRINK_CASE=I002 cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture`

use std::collections::HashMap;
use std::path::PathBuf;

use brink_test_harness::corpus::{
    collect_oracle_cases, compile_and_explore_from_ink, has_empty_source, is_compile_error_case,
};
use brink_test_harness::oracle;
use brink_test_harness::snapshot_fmt::{CaseResult, CaseStatus};
use brink_test_harness::{Episode, ExploreConfig};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

/// Ratchet: minimum number of oracle episodes that must pass.
/// Bump this as compiler coverage improves.
///
/// Raised 5607 -> 5608 on 2026-08-03. The measured count had been 5608 while
/// this floor still read 5607, so a real conformance gain sat unprotected —
/// any change could have regressed it back to 5607 with the gate still green.
/// Measured on a *freshly cleaned* `CARGO_TARGET_DIR` (issue #2054: worktrees
/// sharing one target can serve stale test binaries, so a corpus number taken
/// without a clean is not trustworthy): CASES 366 pass / 8 fail / 398 total,
/// EPISODES 5608 pass / 1010 mismatch / 2 missing. Which fix earned the extra
/// episode is not attributed — 391 merges landed between this constant last
/// being set (2026-07-26) and the measurement, and bisecting that range was
/// not worth the compute.
///
/// Raised 5608 -> 5619 on 2026-09-02 (maintainer directive: hand-minimized
/// edge cases found by the gen-expressions generator or by reference-
/// differential probing become permanent C#-oracle corpus cases). Eleven new
/// episodes pass and are attributed exactly, one per new case:
///   tier1/gather/nested-gather-divert (#3383)
///   tier1/gather/nested-gather-divert-text (#3383)
///   tier1/gather/nested-gather-authored-done (#3383)
///   tier1/gather/empty-nested-gather-falls-out (#3383)
///   tier2/conditional/three-inline-conditionals (#3386)
///   tier2/conditional/four-inline-conditionals (#3386)
///   tier2/conditional/three-inline-conditionals-in-gather-choice (#3386)
///   tier2/sequences/sequence-shared-across-lifted-branches (#3275)
///   tier2/sequences/sequence-with-divert-branch (#3275 revoked corner)
///   tier1/choices/once-only-fallback-consumed (generator finding)
///   tier2/evaluation/lift-order-cond-then-fn (#3395 control case)
/// Two more new cases (tier2/evaluation/lift-order-fn-then-cond and
/// tier2/evaluation/lift-order-seq-fn-cond, both #3395) were added in the
/// same batch as EXPECTED MISMATCHES against the C# oracle — they document a
/// known lift-order divergence and deliberately do NOT count toward this
/// floor. Measured on the same freshly-cleaned `CARGO_TARGET_DIR` discipline
/// as the prior raise: CASES 377 pass / 10 fail / 411 total, EPISODES 5619
/// pass / 1012 mismatch / 2 missing.
const RATCHET_EPISODE_COUNT: usize = 5619;

#[test]
#[expect(clippy::too_many_lines)]
fn oracle_snapshots() {
    let root = tests_dir();
    let cases = collect_oracle_cases(&root);
    let case_filter = std::env::var("BRINK_CASE").ok();

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 1000,
    };

    let mut results: Vec<CaseResult> = Vec::new();
    let mut episodes_pass: usize = 0;
    let mut episodes_mismatch: usize = 0;
    let mut episodes_missing: usize = 0;

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .display()
            .to_string();

        if let Some(ref filter) = case_filter
            && !rel.contains(filter.as_str())
        {
            continue;
        }

        let ink_path = case_dir.join("story.ink");
        if !ink_path.exists() || has_empty_source(case_dir) || is_compile_error_case(case_dir) {
            results.push(CaseResult {
                rel_path: rel,
                status: CaseStatus::Skip,
            });
            continue;
        }

        // Load oracle episodes.
        let oracle_eps = match oracle::load_oracle_episodes(case_dir) {
            Ok(eps) if eps.is_empty() => {
                results.push(CaseResult {
                    rel_path: rel,
                    status: CaseStatus::Skip,
                });
                continue;
            }
            Ok(eps) => eps,
            Err(_) => {
                results.push(CaseResult {
                    rel_path: rel,
                    status: CaseStatus::Skip,
                });
                continue;
            }
        };

        // Compile with brink.
        let actual = match compile_and_explore_from_ink(&ink_path, &config) {
            Ok((_data, episodes)) => episodes,
            Err(e) if e.starts_with("compile:") => {
                results.push(CaseResult {
                    rel_path: rel,
                    status: CaseStatus::CompileError(e),
                });
                continue;
            }
            Err(e) if e.starts_with("link:") => {
                results.push(CaseResult {
                    rel_path: rel,
                    status: CaseStatus::LinkError(e),
                });
                continue;
            }
            Err(e) => {
                results.push(CaseResult {
                    rel_path: rel,
                    status: CaseStatus::CompileError(e),
                });
                continue;
            }
        };

        // Compare episodes.
        let actual_index = index_by_choice_path(&actual);
        let mut case_pass = 0;
        let mut case_mismatch = 0;
        let mut case_missing = 0;

        for oracle_ep in &oracle_eps {
            if let Some(brink_ep) = actual_index.get(oracle_ep.choice_path.as_slice()) {
                let d = oracle::diff_oracle(oracle_ep, brink_ep);
                if d.matches {
                    case_pass += 1;
                } else {
                    case_mismatch += 1;
                }
            } else {
                case_missing += 1;
            }
        }

        episodes_pass += case_pass;
        episodes_mismatch += case_mismatch;
        episodes_missing += case_missing;

        let total = oracle_eps.len();
        let status = if case_mismatch == 0 && case_missing == 0 {
            CaseStatus::Pass {
                episodes_pass: case_pass,
                episodes_total: total,
            }
        } else {
            CaseStatus::Fail {
                episodes_pass: case_pass,
                episodes_total: total,
                episodes_mismatch: case_mismatch,
                episodes_missing: case_missing,
            }
        };

        // Generate per-case snapshot.
        let snap_name = rel.replace('/', "__");
        let case_snapshot =
            brink_test_harness::snapshot_fmt::format_case_snapshot(&rel, &oracle_eps, &actual);
        insta::assert_snapshot!(snap_name, case_snapshot);

        results.push(CaseResult {
            rel_path: rel,
            status,
        });
    }

    // Generate corpus summary snapshot (skip when filtering to a single case).
    if case_filter.is_none() {
        results.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        let summary: String = results
            .iter()
            .map(CaseResult::summary_line)
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("corpus_summary", summary);
    }

    // Print stats for console output.
    let cases_pass = results
        .iter()
        .filter(|r| matches!(r.status, CaseStatus::Pass { .. }))
        .count();
    let cases_fail = results
        .iter()
        .filter(|r| matches!(r.status, CaseStatus::Fail { .. }))
        .count();
    println!();
    println!(
        "CASES: {cases_pass} pass / {cases_fail} fail / {} total",
        results.len()
    );
    println!(
        "EPISODES: {episodes_pass} pass / {episodes_mismatch} mismatch / {episodes_missing} missing"
    );

    // Ratchet assertion.
    if case_filter.is_some() {
        assert!(
            episodes_mismatch == 0 && episodes_missing == 0,
            "{episodes_mismatch} episode(s) mismatched, {episodes_missing} missing"
        );
    } else {
        assert!(
            episodes_pass >= RATCHET_EPISODE_COUNT,
            "ratchet regression: {episodes_pass} episodes < {RATCHET_EPISODE_COUNT}"
        );
    }
}
