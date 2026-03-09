//! Brink-native episode test: compile `.ink` with the brink compiler,
//! explore, and diff against golden episodes (generated from ink.json).
//!
//! This reveals compiler correctness gaps — any mismatch between brink-compiled
//! and ink.json-derived episodes indicates a compiler bug.

use std::path::PathBuf;

use brink_test_harness::corpus::{collect_test_cases, explore_from_ink, load_golden_episodes};
use brink_test_harness::{ExploreConfig, diff};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Ratchet: minimum number of test cases that must pass.
/// Bump this as compiler coverage improves.
///
/// Currently `None`: all cases fail at link ("no root container found") because
/// brink-codegen-inkb doesn't yet produce a root container the linker recognizes.
/// Set to `Some(n)` once cases start passing.
const RATCHET_PASS_COUNT: Option<usize> = None;

#[test]
fn brink_native_episodes() {
    let root = tests_dir();
    let cases = collect_test_cases(&root);

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 100,
    };

    let mut pass = 0;
    let mut compile_error = 0;
    let mut link_error = 0;
    let mut mismatch = 0;
    let mut skip = 0;
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

        // Compare episode count.
        if golden.len() != actual.len() {
            mismatch += 1;
            if first_mismatch.is_none() {
                first_mismatch = Some(format!(
                    "{rel}: episode count mismatch: expected {}, got {}",
                    golden.len(),
                    actual.len()
                ));
            }
            continue;
        }

        // Diff each episode pair.
        let mut case_ok = true;
        for (i, (exp, act)) in golden.iter().zip(actual.iter()).enumerate() {
            let d = diff(exp, act);
            if !d.matches {
                case_ok = false;
                if first_mismatch.is_none() {
                    first_mismatch = Some(format!("{rel}: episode {i}:\n{d}"));
                }
                break;
            }
        }

        if case_ok {
            pass += 1;
        } else {
            mismatch += 1;
        }
    }

    let total = cases.len();
    println!();
    println!(
        "BRINK-NATIVE EPISODES: {pass} pass / {mismatch} mismatch / \
         {compile_error} compile_error / {link_error} link_error / \
         {skip} skip (of {total})"
    );

    if let Some(ref msg) = first_mismatch {
        println!();
        println!("First mismatch:\n{msg}");
    }

    if let Some(ratchet) = RATCHET_PASS_COUNT {
        assert!(pass >= ratchet, "ratchet regression: {pass} < {ratchet}");
    }
}
