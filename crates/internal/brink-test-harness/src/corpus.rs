//! Test-case discovery and golden episode loading for corpus tests.

use std::path::{Path, PathBuf};

use crate::episode::Episode;
use crate::explorer::ExploreConfig;

/// Recursively find directories containing `story.ink`.
pub fn collect_test_cases(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_recursive(root, &mut result, |dir| dir.join("story.ink").exists());
    result.sort();
    result
}

/// Recursively find directories containing an `oracle/` subdirectory with `.oracle.json` files.
pub fn collect_oracle_cases(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_recursive(root, &mut result, |dir| {
        let oracle_dir = dir.join("oracle");
        oracle_dir.is_dir()
            && std::fs::read_dir(&oracle_dir).is_ok_and(|entries| {
                entries
                    .flatten()
                    .any(|e| e.path().to_string_lossy().contains(".oracle.json"))
            })
    });
    result.sort();
    result
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>, predicate: impl Fn(&Path) -> bool + Copy) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    if predicate(dir) {
        out.push(dir.to_path_buf());
    }

    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .filter(|e| {
            let name = e.file_name();
            name != "episodes" && name != "oracle"
        })
        .map(|e| e.path())
        .collect();
    subdirs.sort();

    for sub in subdirs {
        collect_recursive(&sub, out, predicate);
    }
}

/// Load golden episode files from a test case's `episodes/` directory.
///
/// Returns episodes sorted by filename (e0, e1, ...).
pub fn load_golden_episodes(case_dir: &Path) -> Result<Vec<Episode>, String> {
    let episodes_dir = case_dir.join("episodes");
    if !episodes_dir.is_dir() {
        return Err(format!("no episodes/ directory in {}", case_dir.display()));
    }

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&episodes_dir)
        .map_err(|e| format!("read episodes dir: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "json")
                && p.to_string_lossy().contains(".episode.")
        })
        .collect();
    paths.sort();

    let mut episodes = Vec::with_capacity(paths.len());
    for path in &paths {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let ep: Episode =
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {e}", path.display()))?;
        episodes.push(ep);
    }

    Ok(episodes)
}

/// Compile a `.ink` file with the brink compiler, link, and explore.
///
/// Returns `Err` if compilation or linking fails.
pub fn explore_from_ink(ink_path: &Path, config: &ExploreConfig) -> Result<Vec<Episode>, String> {
    let output = brink_compiler::compile_path(ink_path).map_err(|e| format!("compile: {e}"))?;
    let (program, line_tables) =
        brink_runtime::link(&output.data).map_err(|e| format!("link: {e}"))?;
    Ok(crate::explore(
        std::sync::Arc::new(program),
        line_tables,
        config,
    ))
}

/// Compile a `.ink` file, link, explore, and also return the [`StoryData`].
///
/// Useful when the caller needs to inspect or dump the compiled data on failure.
pub fn compile_and_explore_from_ink(
    ink_path: &Path,
    config: &ExploreConfig,
) -> Result<(brink_format::StoryData, Vec<Episode>), String> {
    let output = brink_compiler::compile_path(ink_path).map_err(|e| format!("compile: {e}"))?;
    let (program, line_tables) =
        brink_runtime::link(&output.data).map_err(|e| format!("link: {e}"))?;
    let episodes = crate::explore(std::sync::Arc::new(program), line_tables, config);
    Ok((output.data, episodes))
}
