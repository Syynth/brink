//! Test-case discovery and golden episode loading for corpus tests.

use std::path::{Path, PathBuf};

use crate::episode::Episode;
use crate::explorer::ExploreConfig;

/// Recursively find directories containing `story.ink.json`.
pub fn collect_test_cases(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_recursive(root, &mut result);
    result.sort();
    result
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    if dir.join("story.ink.json").exists() {
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

/// Explore a story from its `.ink.json` file via the converter pipeline.
///
/// Pipeline: parse JSON → convert → link → explore.
pub fn explore_from_ink_json(
    json_path: &Path,
    config: &ExploreConfig,
) -> Result<Vec<Episode>, String> {
    let json_str = std::fs::read_to_string(json_path).map_err(|e| format!("read: {e}"))?;
    let ink: brink_json::InkJson =
        serde_json::from_str(&json_str).map_err(|e| format!("json: {e}"))?;
    let data = brink_converter::convert(&ink).map_err(|e| format!("convert: {e}"))?;
    let program = brink_runtime::link(&data).map_err(|e| format!("link: {e}"))?;
    Ok(crate::explore(&program, config))
}

/// Convert a `.ink.json` file and return the [`StoryData`].
pub fn convert_ink_json(json_path: &Path) -> Result<brink_format::StoryData, String> {
    let json_str = std::fs::read_to_string(json_path).map_err(|e| format!("read: {e}"))?;
    let ink: brink_json::InkJson =
        serde_json::from_str(&json_str).map_err(|e| format!("json: {e}"))?;
    brink_converter::convert(&ink).map_err(|e| format!("convert: {e}"))
}

/// Compile a `.ink` file with the brink compiler, link, and explore.
///
/// Returns `Err` if compilation or linking fails.
pub fn explore_from_ink(ink_path: &Path, config: &ExploreConfig) -> Result<Vec<Episode>, String> {
    let data = brink_compiler::compile_path(ink_path).map_err(|e| format!("compile: {e}"))?;
    let program = brink_runtime::link(&data).map_err(|e| format!("link: {e}"))?;
    Ok(crate::explore(&program, config))
}

/// Compile a `.ink` file, link, explore, and also return the [`StoryData`].
///
/// Useful when the caller needs to inspect or dump the compiled data on failure.
pub fn compile_and_explore_from_ink(
    ink_path: &Path,
    config: &ExploreConfig,
) -> Result<(brink_format::StoryData, Vec<Episode>), String> {
    let data = brink_compiler::compile_path(ink_path).map_err(|e| format!("compile: {e}"))?;
    let program = brink_runtime::link(&data).map_err(|e| format!("link: {e}"))?;
    let episodes = crate::explore(&program, config);
    Ok((data, episodes))
}
