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

/// Load a golden transcript file (e.g. `expected.txt`) for a corpus case,
/// rejecting vacuous goldens.
///
/// Returns `Err` — naming the case and the exact problem — if the file is
/// missing, OR present but empty/whitespace-only. A missing file already
/// fails loudly via `read_to_string`'s `Err`, but an *empty* golden is a
/// silent trap: it trivially matches empty actual output, so a case whose
/// compiled program silently produces no output (a real bug) passes the
/// comparison anyway. This must be a hard error, never a silent pass — see
/// issue #1079 (the `#901` MCTS-review blocker: `mcts-lite`'s `expected.txt`
/// briefly shipped as a 0-byte file and the golden-file assertion passed).
pub fn load_golden_transcript(path: &Path, case_label: &str) -> Result<String, String> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "missing golden transcript for {case_label}: {}: {e}",
            path.display()
        )
    })?;
    if content.trim().is_empty() {
        return Err(format!(
            "vacuous golden transcript for {case_label}: {} is empty (or whitespace-only). \
             An empty expected.txt trivially matches empty actual output and can mask a real \
             bug (e.g. a case that silently produces no output) — populate it with the case's \
             real expected output instead of leaving it blank.",
            path.display()
        ));
    }
    Ok(content)
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

/// Compile a `.brink` native source string, link, explore, and also return
/// the [`brink_format::StoryData`] — the native analogue of
/// [`compile_and_explore_from_ink`].
///
/// **The honest minimal path** (first light, `docs/b0-sequencing.md`
/// §B0.10, issue #1106): a single self-contained composition of
/// `brink_syntax_native::parse` → `brink_ir::hir::lower_native::lower` →
/// `brink_analyzer` (dialect `Brink`, single file) →
/// `brink_ir::lir::lower_to_program` → `brink_codegen_inkb::emit` →
/// `brink_runtime::link` + explore. Deliberately bypasses `brink-db`/salsa
/// (no incremental caching — every call re-runs the whole pipeline) and the
/// INCLUDE-closure machinery (native has none; `.brink` file-extension
/// registration and multi-file project discovery are B0.10's later
/// project-layer wiring, out of scope here). This proves HIR→episode for
/// one file, not the full `brink compile scene.brink` CLI path.
///
/// Returns `Err` naming the exact stage (parse/lowering/analysis/LIR/
/// codegen/link) on the first diagnostic or error encountered — a fixture
/// with real diagnostics at any stage is not silently treated as
/// "compiles to nothing".
pub fn compile_and_explore_from_brink_native(
    src: &str,
    config: &ExploreConfig,
) -> Result<(brink_format::StoryData, Vec<Episode>), String> {
    let file_id = brink_ir::FileId(0);

    let parsed = brink_syntax_native::parse(src);
    if !parsed.errors().is_empty() {
        return Err(format!("native parse errors: {:?}", parsed.errors()));
    }
    let tree = parsed.tree();

    let (hir, manifest, lower_diags) = brink_ir::hir::lower_native::lower(file_id, &tree);
    if !lower_diags.is_empty() {
        return Err(format!("native HIR lowering diagnostics: {lower_diags:?}"));
    }

    let files_for_analysis: Vec<(brink_ir::FileId, &brink_ir::HirFile, &brink_ir::SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let analysis_opts = brink_analyzer::AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        ..Default::default()
    };
    let analysis = brink_analyzer::analyze_with_options(&files_for_analysis, &analysis_opts);
    if !analysis.diagnostics.is_empty() {
        return Err(format!("analysis diagnostics: {:?}", analysis.diagnostics));
    }

    let files_for_lir: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = vec![(file_id, &hir)];
    let (program, lir_diags) = brink_ir::lir::lower_to_program(
        &files_for_lir,
        &analysis.index,
        &analysis.resolutions,
        &std::collections::HashMap::new(),
    );
    if !lir_diags.is_empty() {
        return Err(format!("LIR lowering diagnostics: {lir_diags:?}"));
    }
    let program =
        program.ok_or_else(|| "LIR lowering produced no program (see diagnostics)".to_string())?;

    let data = brink_codegen_inkb::emit(&program).map_err(|e| format!("codegen: {e}"))?;

    let (linked, line_tables) = brink_runtime::link(&data).map_err(|e| format!("link: {e}"))?;
    let episodes = crate::explore(std::sync::Arc::new(linked), line_tables, config);
    Ok((data, episodes))
}

/// [`compile_and_explore_from_brink_native`], discarding the compiled
/// [`brink_format::StoryData`] — the native analogue of
/// [`explore_from_ink`] for callers that only need episodes.
pub fn explore_from_brink_native(
    src: &str,
    config: &ExploreConfig,
) -> Result<Vec<Episode>, String> {
    compile_and_explore_from_brink_native(src, config).map(|(_, episodes)| episodes)
}

#[cfg(test)]
mod golden_transcript_tests {
    use super::load_golden_transcript;
    use std::path::PathBuf;

    /// A unique scratch file path under the system temp dir, removed on
    /// drop — this module's tests write small fixture files directly
    /// rather than depending on a corpus fixture directory or a new
    /// `tempfile` dependency (not already a workspace dep).
    struct ScratchFile(PathBuf);

    impl ScratchFile {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "brink-test-harness-golden-{name}-{}.txt",
                std::process::id(),
            ));
            Self(path)
        }

        fn write(&self, content: &str) {
            std::fs::write(&self.0, content).expect("write scratch golden file");
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for ScratchFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_golden_file_is_a_clear_error_not_a_panic() {
        let path = std::env::temp_dir().join("brink-test-harness-golden-does-not-exist.txt");
        let err = load_golden_transcript(&path, "some-case")
            .expect_err("missing golden file must be an Err, never a silent pass");
        assert!(
            err.contains("missing golden transcript"),
            "error should clearly name the problem, got: {err}"
        );
        assert!(
            err.contains("some-case"),
            "error should name the case, got: {err}"
        );
    }

    #[test]
    fn empty_golden_file_is_rejected_as_vacuous() {
        let scratch = ScratchFile::new("empty");
        scratch.write("");
        let err = load_golden_transcript(scratch.path(), "empty-case")
            .expect_err("a 0-byte golden must be rejected, never silently treated as a pass");
        assert!(
            err.contains("vacuous golden transcript"),
            "error should clearly name the problem, got: {err}"
        );
        assert!(
            err.contains("empty-case"),
            "error should name the case, got: {err}"
        );
    }

    #[test]
    fn whitespace_only_golden_file_is_rejected_as_vacuous() {
        let scratch = ScratchFile::new("whitespace");
        scratch.write("   \n\n\t \n");
        let err = load_golden_transcript(scratch.path(), "whitespace-case")
            .expect_err("a whitespace-only golden must be rejected as vacuous too");
        assert!(
            err.contains("vacuous golden transcript"),
            "error should clearly name the problem, got: {err}"
        );
    }

    #[test]
    fn non_empty_golden_file_loads_its_exact_content() {
        let scratch = ScratchFile::new("real");
        scratch.write("Total cost: 12\n");
        let content = load_golden_transcript(scratch.path(), "real-case")
            .expect("a genuinely non-empty golden must load successfully");
        assert_eq!(content, "Total cost: 12\n");
    }
}
