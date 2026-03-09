//! Walk the test corpus and generate episode files for each test case.
//!
//! Usage:
//!   cargo run -p brink-test-harness --bin gen-episodes [-- path/to/tests]
//!
//! Defaults to `tests/` relative to the workspace root. For each test case
//! that has a `story.ink.json`, runs `explore()` to capture all reachable
//! branches and writes `episodes/e0.episode.json`, `e1.episode.json`, etc.
#![expect(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};

use brink_test_harness::{ExploreConfig, explore};

fn main() {
    let tests_dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("tests"), PathBuf::from);

    if !tests_dir.is_dir() {
        eprintln!("error: {} is not a directory", tests_dir.display());
        std::process::exit(1);
    }

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 100,
    };

    let mut cases = collect_test_cases(&tests_dir);
    cases.sort();

    let total = cases.len();
    let mut ok = 0;
    let mut errors = 0;
    let mut skipped = 0;

    for (i, case_dir) in cases.iter().enumerate() {
        let rel = case_dir
            .strip_prefix(&tests_dir)
            .unwrap_or(case_dir)
            .display();
        match generate_episodes(case_dir, &config) {
            Ok(count) => {
                println!("[{:>3}/{}] {rel}: {count} episode(s)", i + 1, total);
                ok += 1;
            }
            Err(GenError::NoJson) => {
                skipped += 1;
            }
            Err(GenError::Failed(msg)) => {
                eprintln!("[{:>3}/{}] {rel}: ERROR — {msg}", i + 1, total);
                errors += 1;
            }
        }
    }

    println!();
    println!("done: {ok} ok, {errors} errors, {skipped} skipped, {total} total");
    if errors > 0 {
        std::process::exit(1);
    }
}

enum GenError {
    NoJson,
    Failed(String),
}

fn generate_episodes(case_dir: &Path, config: &ExploreConfig) -> Result<usize, GenError> {
    let json_path = case_dir.join("story.ink.json");
    if !json_path.exists() {
        return Err(GenError::NoJson);
    }

    let json_str =
        std::fs::read_to_string(&json_path).map_err(|e| GenError::Failed(e.to_string()))?;

    let ink: brink_json::InkJson =
        serde_json::from_str(&json_str).map_err(|e| GenError::Failed(format!("json: {e}")))?;

    let data =
        brink_converter::convert(&ink).map_err(|e| GenError::Failed(format!("convert: {e}")))?;

    let program = brink_runtime::link(&data).map_err(|e| GenError::Failed(format!("link: {e}")))?;

    let episodes = explore(&program, config);
    if episodes.is_empty() {
        return Err(GenError::Failed("explore produced 0 episodes".into()));
    }

    let episodes_dir = case_dir.join("episodes");
    std::fs::create_dir_all(&episodes_dir).map_err(|e| GenError::Failed(format!("mkdir: {e}")))?;

    // Remove stale episode files before writing new ones.
    if let Ok(entries) = std::fs::read_dir(&episodes_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(".episode.json") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    for (i, ep) in episodes.iter().enumerate() {
        let path = episodes_dir.join(format!("e{i}.episode.json"));
        let content = serde_json::to_string_pretty(ep)
            .map_err(|e| GenError::Failed(format!("serialize: {e}")))?;
        std::fs::write(&path, format!("{content}\n"))
            .map_err(|e| GenError::Failed(format!("write: {e}")))?;
    }

    Ok(episodes.len())
}

/// Recursively find directories containing `story.ink.json`.
fn collect_test_cases(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_recursive(root, &mut result);
    result
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let has_json = dir.join("story.ink.json").exists();
    if has_json {
        out.push(dir.to_path_buf());
    }

    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .filter(|e| e.file_name() != "episodes")
        .map(|e| e.path())
        .collect();
    subdirs.sort();

    for sub in subdirs {
        collect_recursive(&sub, out);
    }
}
