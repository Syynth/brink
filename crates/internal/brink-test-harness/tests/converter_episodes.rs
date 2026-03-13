//! Converter fidelity test: run ink.json → convert → link → explore and
//! verify the output matches the golden `.episode.json` files exactly.
//!
//! The golden episodes were generated from the converter pipeline. This test
//! ensures the converter still reproduces them. Without this, changes to
//! the converter or runtime could silently invalidate the golden episodes,
//! giving false confidence to the compiler episodes test.
//!
//! Unlike the compiler test, this has no ratchet — the converter is the
//! reference pipeline and must match all golden episodes exactly.

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
#[expect(clippy::too_many_lines)]
fn converter_reproduces_golden_episodes() {
    let root = tests_dir();
    let cases = collect_test_cases(&root);
    let case_filter = std::env::var("BRINK_CASE").ok();

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 100,
    };

    let mut cases_pass = 0;
    let mut cases_mismatch = 0;
    let mut convert_error = 0;
    let mut skip = 0;

    let mut episodes_pass = 0;
    let mut episodes_mismatch = 0;
    let mut episodes_total = 0;

    let mut first_mismatch: Option<String> = None;
    let mut failing_cases: Vec<String> = Vec::new();

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .display()
            .to_string();

        // Filter by BRINK_CASE env var if set.
        if let Some(ref filter) = case_filter
            && !rel.contains(filter.as_str())
        {
            continue;
        }

        let json_path = case_dir.join("story.ink.json");
        if !json_path.exists() {
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

        // Run the converter pipeline.
        let actual = match explore_from_ink_json(&json_path, &config) {
            Ok(eps) => eps,
            Err(e) => {
                convert_error += 1;
                failing_cases.push(format!("{rel} ({e})"));
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("{rel}: {e}"));
                }
                continue;
            }
        };

        episodes_total += golden.len();

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
            failing_cases.push(rel);
        }
    }

    let total_cases = cases.len();
    println!();
    println!(
        "CONVERTER CASES: {cases_pass} pass / {cases_mismatch} mismatch / \
         {convert_error} convert_error / {skip} skip (of {total_cases})"
    );
    println!(
        "CONVERTER EPISODES: {episodes_pass} pass / {episodes_mismatch} mismatch \
         (of {episodes_total} golden)"
    );

    if !failing_cases.is_empty() {
        println!("\nFailing cases ({}):", failing_cases.len());
        for name in &failing_cases {
            println!("  {name}");
        }
    }

    if let Some(ref msg) = first_mismatch {
        println!();
        println!("First mismatch:\n{msg}");
    }

    // The converter is the reference pipeline. It must match ALL golden episodes.
    assert!(
        episodes_mismatch == 0,
        "converter fidelity regression: {episodes_mismatch} episode(s) diverged from golden files"
    );
}
