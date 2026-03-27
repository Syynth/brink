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

use brink_test_harness::corpus::{collect_oracle_cases, compile_and_explore_from_ink};
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

fn is_compile_error_case(case_dir: &std::path::Path) -> bool {
    let meta_path = case_dir.join("metadata.toml");
    std::fs::read_to_string(meta_path).ok().is_some_and(|s| {
        s.lines()
            .any(|line| line.trim() == r#"mode = "compile_error""#)
    })
}

fn has_empty_source(case_dir: &std::path::Path) -> bool {
    let ink_path = case_dir.join("story.ink");
    std::fs::read_to_string(ink_path)
        .ok()
        .is_some_and(|s| s.trim().is_empty())
}

fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

/// Ratchet: minimum number of oracle episodes that must pass.
/// Bump this as compiler coverage improves.
const RATCHET_EPISODE_COUNT: usize = 4684;

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
