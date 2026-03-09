//! Brink-native episode test: compile `.ink` with the brink compiler,
//! explore, and diff against golden episodes (generated from ink.json).
//!
//! This reveals compiler correctness gaps — any mismatch between brink-compiled
//! and ink.json-derived episodes indicates a compiler bug.

use std::collections::HashMap;
use std::path::PathBuf;

use brink_test_harness::corpus::{collect_test_cases, explore_from_ink, load_golden_episodes};
use brink_test_harness::{Episode, ExploreConfig, diff};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Ratchet: minimum number of episodes (not cases) that must pass.
/// Bump this as compiler coverage improves.
const RATCHET_EPISODE_COUNT: usize = 129;

/// Index episodes by their `choice_path` for order-independent matching.
fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

#[test]
#[expect(clippy::too_many_lines)]
fn brink_native_episodes() {
    let root = tests_dir();
    let cases = collect_test_cases(&root);

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 100,
    };

    let mut cases_pass = 0;
    let mut cases_mismatch = 0;
    let mut compile_error = 0;
    let mut link_error = 0;
    let mut skip = 0;

    let mut episodes_pass = 0;
    let mut episodes_mismatch = 0;
    let mut episodes_total = 0;

    let mut first_mismatch: Option<String> = None;

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .display()
            .to_string();

        let ink_path = case_dir.join("story.ink");
        if !ink_path.exists() {
            skip += 1;
            continue;
        }

        // Load golden episodes.
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

        episodes_total += golden.len();

        // Try compiling with brink.
        let actual = match explore_from_ink(&ink_path, &config) {
            Ok(eps) => eps,
            Err(e) if e.starts_with("compile:") => {
                compile_error += 1;
                continue;
            }
            Err(e) if e.starts_with("link:") => {
                link_error += 1;
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("{rel}: {e}"));
                }
                continue;
            }
            Err(e) => {
                compile_error += 1;
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("{rel}: {e}"));
                }
                continue;
            }
        };

        // Match episodes by choice_path (order-independent).
        let actual_index = index_by_choice_path(&actual);
        let mut case_ok = true;

        for (i, exp) in golden.iter().enumerate() {
            let Some(act) = actual_index.get(exp.choice_path.as_slice()) else {
                case_ok = false;
                episodes_mismatch += 1;
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!(
                        "{rel}: golden episode {i} choice_path {:?} not found in actual \
                         (golden has {}, actual has {} episodes)",
                        exp.choice_path,
                        golden.len(),
                        actual.len()
                    ));
                }
                continue;
            };
            let d = diff(exp, act);
            if d.matches {
                episodes_pass += 1;
            } else {
                episodes_mismatch += 1;
                case_ok = false;
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("{rel}: episode {i}:\n{d}"));
                }
            }
        }

        // Count any extra actual episodes not in golden as mismatches.
        if actual.len() > golden.len() {
            episodes_mismatch += actual.len() - golden.len();
            case_ok = false;
        }

        if case_ok {
            cases_pass += 1;
        } else {
            cases_mismatch += 1;
        }
    }

    let total_cases = cases.len();
    println!();
    println!(
        "BRINK-NATIVE CASES: {cases_pass} pass / {cases_mismatch} mismatch / \
         {compile_error} compile_error / {link_error} link_error / \
         {skip} skip (of {total_cases})"
    );
    println!(
        "BRINK-NATIVE EPISODES: {episodes_pass} pass / {episodes_mismatch} mismatch \
         (of {episodes_total} golden)"
    );

    if let Some(ref msg) = first_mismatch {
        println!();
        println!("First mismatch:\n{msg}");
    }

    assert!(
        episodes_pass >= RATCHET_EPISODE_COUNT,
        "ratchet regression: {episodes_pass} episodes < {RATCHET_EPISODE_COUNT}"
    );
}
