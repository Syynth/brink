//! Regression test: replay golden episode fixtures and verify they match.
//!
//! Loads each `episodes/*.episode.json` from the test corpus, re-runs
//! `explore()` on the corresponding `story.ink.json`, and diffs the results.
//!
//! Episodes are matched by `choice_path` (not position) for robustness.

use std::collections::HashMap;
use std::path::PathBuf;

use brink_test_harness::corpus::{collect_test_cases, explore_from_ink_json, load_golden_episodes};
use brink_test_harness::{Episode, ExploreConfig, diff};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Index episodes by their `choice_path` for order-independent matching.
fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

#[test]
fn episode_regression() {
    let root = tests_dir();
    let cases = collect_test_cases(&root);

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 100,
    };

    let mut pass = 0;
    let mut skip = 0;
    let mut fail = 0;
    let mut first_failure: Option<String> = None;

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .display()
            .to_string();

        // Skip cases without golden episodes.
        let golden = match load_golden_episodes(case_dir) {
            Ok(eps) if eps.is_empty() => {
                skip += 1;
                continue;
            }
            Ok(eps) => eps,
            Err(_) => {
                skip += 1;
                continue;
            }
        };

        let json_path = case_dir.join("story.ink.json");
        let actual = match explore_from_ink_json(&json_path, &config) {
            Ok(eps) => eps,
            Err(e) => {
                fail += 1;
                if first_failure.is_none() {
                    first_failure = Some(format!("{rel}: explore failed: {e}"));
                }
                continue;
            }
        };

        // Compare episode count.
        if golden.len() != actual.len() {
            fail += 1;
            if first_failure.is_none() {
                first_failure = Some(format!(
                    "{rel}: episode count mismatch: expected {}, got {}",
                    golden.len(),
                    actual.len()
                ));
            }
            continue;
        }

        // Match episodes by choice_path (order-independent).
        let actual_index = index_by_choice_path(&actual);
        let mut case_ok = true;

        for (i, exp) in golden.iter().enumerate() {
            let Some(act) = actual_index.get(exp.choice_path.as_slice()) else {
                case_ok = false;
                if first_failure.is_none() {
                    first_failure = Some(format!(
                        "{rel}: golden episode {i} choice_path {:?} not found in actual",
                        exp.choice_path
                    ));
                }
                break;
            };
            let d = diff(exp, act);
            if !d.matches {
                case_ok = false;
                if first_failure.is_none() {
                    first_failure = Some(format!(
                        "{rel}: episode {i} (path {:?}):\n{d}",
                        exp.choice_path
                    ));
                }
                break;
            }
        }

        if case_ok {
            pass += 1;
        } else {
            fail += 1;
        }
    }

    println!();
    println!(
        "EPISODE REGRESSION: {pass} pass / {fail} fail / {skip} skip (of {} total)",
        cases.len()
    );

    if let Some(ref msg) = first_failure {
        println!();
        println!("First failure:\n{msg}");
    }

    assert_eq!(fail, 0, "episode regression failures detected");
}
