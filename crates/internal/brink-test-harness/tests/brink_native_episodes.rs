//! Brink-native episode test: compile `.ink` with the brink compiler,
//! explore, and diff against golden episodes (generated from ink.json).
//!
//! This reveals compiler correctness gaps — any mismatch between brink-compiled
//! and ink.json-derived episodes indicates a compiler bug.
//!
//! Set `BRINK_CASE` to a substring to filter to a single test case, e.g.:
//!   `BRINK_CASE=I002 cargo test -p brink-test-harness --test brink_native_episodes -- --nocapture`

use std::collections::HashMap;
use std::path::PathBuf;

use brink_test_harness::corpus::{
    collect_test_cases, compile_and_explore_from_ink, convert_ink_json, load_golden_episodes,
};
use brink_test_harness::{Episode, ExploreConfig, diff};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Returns true if the case's metadata.toml has `mode = "compile_error"`.
fn is_compile_error_case(case_dir: &std::path::Path) -> bool {
    let meta_path = case_dir.join("metadata.toml");
    std::fs::read_to_string(meta_path).ok().is_some_and(|s| {
        s.lines()
            .any(|line| line.trim() == r#"mode = "compile_error""#)
    })
}

/// Ratchet: minimum number of episodes (not cases) that must pass.
/// Bump this as compiler coverage improves.
const RATCHET_EPISODE_COUNT: usize = 925;

/// Index episodes by their `choice_path` for order-independent matching.
fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

/// Build the diagnostic dump shown on first mismatch.
///
/// Includes the `.ink` source, the compiler's `.inkt` output, and the
/// converter's `.inkt` output (the "expected" bytecode).
fn build_dump(case_dir: &std::path::Path, compiler_data: &brink_format::StoryData) -> String {
    let mut dump = String::new();

    // .ink source
    let ink_path = case_dir.join("story.ink");
    if let Ok(source) = std::fs::read_to_string(&ink_path) {
        dump.push_str("=== .ink source ===\n");
        dump.push_str(&source);
        dump.push('\n');
    }

    // Compiler .inkt
    dump.push_str("=== compiler .inkt ===\n");
    dump.push_str(&compiler_data.to_string());
    dump.push('\n');

    // Converter .inkt (the "expected" bytecode from ink.json)
    let json_path = case_dir.join("story.ink.json");
    if let Ok(converter_data) = convert_ink_json(&json_path) {
        dump.push_str("=== converter .inkt ===\n");
        dump.push_str(&converter_data.to_string());
        dump.push('\n');
    }

    dump
}

#[test]
#[expect(clippy::too_many_lines)]
fn brink_native_episodes() {
    let root = tests_dir();
    let cases = collect_test_cases(&root);
    let case_filter = std::env::var("BRINK_CASE").ok();

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 100,
    };

    let mut cases_pass = 0;
    let mut cases_mismatch = 0;
    let mut compile_error = 0;
    let mut link_error = 0;
    let mut skip = 0;
    let mut expected_compile_error_pass = 0;
    let mut expected_compile_error_fail = 0;

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

        let ink_path = case_dir.join("story.ink");
        if !ink_path.exists() {
            skip += 1;
            continue;
        }

        let expect_compile_error = is_compile_error_case(case_dir);

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
        let (story_data, actual) = match compile_and_explore_from_ink(&ink_path, &config) {
            Ok(pair) => {
                if expect_compile_error {
                    // Compilation succeeded but should have failed.
                    expected_compile_error_fail += 1;
                    failing_cases.push(format!("{rel} (expected compile error, got success)"));
                    if first_mismatch.is_none() {
                        first_mismatch = Some(format!(
                            "{rel}: expected compile error but compilation succeeded"
                        ));
                    }
                    continue;
                }
                pair
            }
            Err(_) if expect_compile_error => {
                // Compilation failed as expected.
                expected_compile_error_pass += 1;
                continue;
            }
            Err(e) if e.starts_with("compile:") => {
                compile_error += 1;
                continue;
            }
            Err(e) if e.starts_with("link:") => {
                link_error += 1;
                failing_cases.push(format!("{rel} (link error)"));
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

        // Debug: print actual choice paths when BRINK_CASE is set.
        if case_filter.is_some() {
            for (i, ep) in actual.iter().enumerate() {
                println!(
                    "  actual[{i}]: path={:?} outcome={:?}",
                    ep.choice_path, ep.outcome
                );
            }
        }

        for (i, exp) in golden.iter().enumerate() {
            let Some(act) = actual_index.get(exp.choice_path.as_slice()) else {
                case_ok = false;
                episodes_mismatch += 1;
                if first_mismatch.is_none() {
                    let dump = build_dump(case_dir, &story_data);
                    first_mismatch = Some(format!(
                        "{rel}: golden episode {i} choice_path {:?} not found in actual \
                         (golden has {}, actual has {} episodes)\n\n{dump}",
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
                    let dump = build_dump(case_dir, &story_data);
                    first_mismatch = Some(format!("{rel}: episode {i}:\n{d}\n\n{dump}"));
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
        "BRINK-NATIVE CASES: {cases_pass} pass / {cases_mismatch} mismatch / \
         {compile_error} compile_error / {link_error} link_error / \
         {skip} skip (of {total_cases})"
    );
    if expected_compile_error_pass > 0 || expected_compile_error_fail > 0 {
        println!(
            "COMPILE-ERROR CASES: {expected_compile_error_pass} correctly rejected / \
             {expected_compile_error_fail} should-have-failed"
        );
    }
    println!(
        "BRINK-NATIVE EPISODES: {episodes_pass} pass / {episodes_mismatch} mismatch \
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

    if case_filter.is_some() {
        // When filtering to specific cases, all episodes must pass.
        assert!(
            episodes_mismatch == 0,
            "{episodes_mismatch} episode(s) still mismatched"
        );
    } else {
        assert!(
            episodes_pass >= RATCHET_EPISODE_COUNT,
            "ratchet regression: {episodes_pass} episodes < {RATCHET_EPISODE_COUNT}"
        );
    }
}
