//! Test-case discovery and golden episode loading for corpus tests.

use std::path::{Path, PathBuf};

use brink_source_tree::Walk;

use crate::episode::Episode;
use crate::explorer::ExploreConfig;

/// Case-data directories [`collect_recursive`] never descends into: they
/// hold a case's fixtures, never nested cases, so walking them is pure
/// waste. Pruned *on top of* the standing `target/`/`.git/`/`node_modules/`
/// policy [`Walk`] applies by construction (issue #1433).
const CASE_DATA_DIRS: [&str; 2] = ["episodes", "oracle"];

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

/// Push every directory at or below `dir` that satisfies `predicate` onto
/// `out`. The descent is the shared [`Walk`], so it prunes the standing
/// ignored-directory policy as well as [`CASE_DATA_DIRS`]; `dir` itself is
/// never pruned (a [`Walk`] contract), so a corpus root that happens to be
/// named like one of them is still enumerated. Callers sort `out`.
fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>, predicate: impl Fn(&Path) -> bool + Copy) {
    if predicate(dir) {
        out.push(dir.to_path_buf());
    }

    for entry in Walk::new(dir).prune_also(CASE_DATA_DIRS).flatten() {
        if entry.is_dir() && predicate(entry.path()) {
            out.push(entry.into_path());
        }
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

/// Compile a native `.brink` file from disk with the brink compiler, link,
/// and run it to completion, returning the concatenated output text.
///
/// Used by the `tests/tier1-native/` self-referential golden corpus
/// (issue #1529 — see `tier1_native.rs`'s module doc for why this corpus
/// has no oracle to diff against) and by `corpus_report`'s native section,
/// so the two never drift on how a case is run. Every case in that corpus
/// is a straight-line, choice-free program by convention (mirroring
/// `tests/tier1-brink/`'s own convention) — a case that presents choices
/// is a fixture-authoring bug, so this surfaces it as an `Err` rather than
/// guessing which choice to take.
///
/// Returns `Err` on any compile error, link error, or runtime fault, or if
/// the program ever reaches [`brink_runtime::Line::Choices`].
pub fn run_native_transcript(brink_path: &Path) -> Result<String, String> {
    let output = brink_compiler::compile_path(brink_path)
        .map_err(|e| format!("compile {}: {e}", brink_path.display()))?;
    let (program, line_tables) = brink_runtime::link(&output.data)
        .map_err(|e| format!("link {}: {e}", brink_path.display()))?;
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );

    let mut out = String::new();
    loop {
        match story
            .continue_single()
            .map_err(|e| format!("runtime error in {}: {e}", brink_path.display()))?
        {
            brink_runtime::Line::Text { text, .. } => out.push_str(&text),
            brink_runtime::Line::Done { text, .. }
            | brink_runtime::Line::End { text, .. }
            | brink_runtime::Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            brink_runtime::Line::Choices { .. } => {
                return Err(format!(
                    "{} presented choices — tier1-native cases must be choice-free \
                     straight-line programs",
                    brink_path.display()
                ));
            }
        }
    }
    Ok(out)
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
/// `brink_analyzer` (the real native analyzer configuration, issue #1472 —
/// see below) → `brink_ir::lir::lower_to_program` → `brink_codegen_inkb::emit`
/// → `brink_runtime::link` + explore. Deliberately bypasses `brink-db`/salsa
/// (no incremental caching — every call re-runs the whole pipeline) and the
/// INCLUDE-closure machinery (native has none; `.brink` file-extension
/// registration and multi-file project discovery are B0.10's later
/// project-layer wiring, out of scope here). This proves HIR→episode for
/// one file, not the full `brink compile scene.brink` CLI path.
///
/// **Analyzer configuration (issue #1472):** a real native compile —
/// `brink-db`'s `per_file_diagnostics_query`, or the `native_diagnostics`
/// helper in `brink-ir/tests/b5_native_construction.rs` — runs with
/// `dialect` at its default (`StrictInk`; a native project carries no
/// dialect opinion unless it explicitly opts in) and `is_native = true`
/// (the flag that actually selects the native-only analyzer arm: it skips
/// the ink-only T1b dialect gate entirely, regardless of `dialect`, and
/// widens the construction-literal checks — E084/E106/E138 — to run outside
/// the brink-only block). `brink_analyzer::analyze_with_options` /
/// `finish_analysis` cannot express this combination: that pure,
/// non-salsa path has no `Language` classification of its own and always
/// passes `is_native = false` to `per_file_diagnostics` internally (see
/// `finish_analysis`'s own doc comment) — so this function composes the
/// same three passes `finish_analysis` does
/// (`symbol_index` → per-file `resolve` → `per_file_diagnostics` →
/// `whole_project_diagnostics`) by hand, threading `is_native = true`
/// through where `finish_analysis` hardcodes it. Using
/// `analyze_with_options` here previously ran every native e2e fixture
/// through the *ink* brink-dialect diagnostics arm instead (hardcoded
/// `Dialect::Brink` + `is_native = false`) — a combination no real
/// compilation path ever produces.
///
/// Returns `Err` naming the exact stage (parse/lowering/analysis/LIR/
/// codegen/link) on the first diagnostic or error encountered — a fixture
/// with real diagnostics at any stage is not silently treated as
/// "compiles to nothing".
///
/// **Coverage delta, not just a correctness delta (issue #1472 review):**
/// the corrected configuration is strictly *more permissive* than the old
/// hardcode, not merely different. `per_file_diagnostics`' brink-only block
/// (`dialect == Dialect::Brink`) — annotation content checks (E061),
/// `#fn` creation-site checks (E079/E080/E081), `ref` lvalue-path checks
/// (E080/E097), and protocol reserved-name checks (E113) — no longer runs
/// at all, since `dialect` now stays at its real native default
/// (`StrictInk`) instead of the old hardcoded `Brink`. Likewise
/// `whole_project_diagnostics`' brink-only block (effects exceedance,
/// the FS-2 `await`-purity gate E105, the NS-A4 comparator-contract gate
/// E119) stops running. And `resolve_type_policy` — which has no
/// `is_native` input of its own — now resolves `types` to `Gradual`
/// (`StrictInk`'s default) instead of the old accidental `Strict` (a
/// side effect of hardcoding `Dialect::Brink`), so E063/E065/E066 no
/// longer fire either. A fixture that "still passes unchanged" through
/// this arm is therefore not evidence those checks still ran and found
/// nothing — it is evidence they no longer run at all. See issue #1472's
/// tracking comment for the follow-up to give native projects real
/// coverage here.
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

    let files_for_analysis: Vec<(
        brink_ir::FileId,
        &brink_ir::HirFile,
        &brink_ir::SymbolManifest,
    )> = vec![(file_id, &hir, &manifest)];

    // `AnalysisOptions::default()`: `dialect` stays `StrictInk`, `types`
    // stays unset — a native project's real defaults (see the doc comment
    // above).
    let analysis_opts = brink_analyzer::AnalysisOptions::default();

    let (index, mut diagnostics) = brink_analyzer::symbol_index(&[(file_id, &manifest)]);
    let scope =
        brink_analyzer::ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
    let (file_resolutions, resolve_diags) =
        brink_analyzer::resolve(file_id, &manifest, &index, &scope);
    diagnostics.extend(resolve_diags);
    let mut resolutions = brink_analyzer::ResolutionMap::new();
    resolutions.extend(std::sync::Arc::unwrap_or_clone(file_resolutions));

    // `is_native = true` — the fix (issue #1472): this is the flag real
    // native compiles pass and `analyze_with_options` never can.
    diagnostics.extend(brink_analyzer::per_file_diagnostics(
        file_id,
        &hir,
        &resolutions,
        &index,
        analysis_opts.dialect,
        true,
        analysis_opts.host_manifest.as_ref(),
    ));
    // The B0.9 native strict-only gate (`native_strict_only_error`) —
    // `brink-db`'s `per_file_diagnostics_query` runs this alongside
    // `per_file_diagnostics` for every native file; included here for the
    // same reason (a no-op given `analysis_opts.types` is unset).
    diagnostics.extend(brink_analyzer::native_strict_only_error(
        file_id,
        analysis_opts.types,
    ));

    let (whole_diagnostics, _symbol_meta) = brink_analyzer::whole_project_diagnostics(
        &files_for_analysis,
        &index,
        &resolutions,
        &analysis_opts,
        None,
    );
    diagnostics.extend(whole_diagnostics);

    if !diagnostics.is_empty() {
        return Err(format!("analysis diagnostics: {diagnostics:?}"));
    }

    let files_for_lir: Vec<(brink_ir::FileId, &brink_ir::HirFile)> = vec![(file_id, &hir)];

    // Every analyzer side-table LIR lowering needs (B3a UFCS, issue #1506;
    // B1 `or`-coalescing, issue #1471/#1492; and whatever future table
    // joins them), assembled through the one path a caller with no salsa
    // layer of its own must use — `brink_analyzer::assemble_analyzer_tables`
    // (issue #1528). Before this call, this pipeline hand-rolled the same
    // gate-then-translate pattern per table, independently re-running
    // `infer_project` for each one; a future table added to the analyzer
    // side but not remembered here would have silently lowered with an
    // empty table, passing tests with the wrong coverage. See that
    // function's own doc for the full rationale — extending it, not this
    // call site, is where a future table belongs.
    let inline_docs = brink_analyzer::project_inline_docs(&[(file_id, &manifest)]);
    let analyzer_tables = brink_analyzer::assemble_analyzer_tables(
        &files_for_lir,
        &index,
        &resolutions,
        analysis_opts.host_manifest.as_ref(),
        &inline_docs,
    );

    let (program, lir_diags) = brink_ir::lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &index,
        &resolutions,
        &std::collections::HashMap::new(),
        brink_ir::lir::TypeMode::Gradual,
        analyzer_tables.as_tables(),
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
mod native_analyzer_arm_tests {
    //! Pins the fix for issue #1472:
    //! [`compile_and_explore_from_brink_native`] must run the real native
    //! analyzer configuration (`dialect` defaulted to `StrictInk`,
    //! `is_native = true`), not the old hardcoded `Dialect::Brink` +
    //! `is_native = false` combination no real compile ever produces.
    //!
    //! `is_native = true` itself (`per_file_diagnostics`' construction-
    //! literal checks — E084/E106/E138 — gated on `dialect ==
    //! Dialect::Brink || is_native`) is already pinned by the existing
    //! `crates/internal/brink-test-harness/tests/b5_construction_e2e.rs::
    //! a_duplicate_map_key_refuses_the_compile` e2e test: byte-identical
    //! fixture and assertion to what would otherwise duplicate here, via
    //! the same `explore_from_brink_native` entry point. No copy of it
    //! lives in this module.
    //!
    //! - [`the_harness_no_longer_forces_the_brink_dialect`] pins the other
    //!   half — `dialect` staying off `Brink`: `protocols::check_reserved_names`
    //!   (E113) only runs `if dialect == Dialect::Brink`, an ink-only-dialect
    //!   check unrelated to `is_native`. The old hardcode ran it against
    //!   every native fixture and would reject an ordinary native function
    //!   named `next` — a name with no special meaning under the *harness's*
    //!   dialect configuration. If a future change silently restored
    //!   `Dialect::Brink`, this test would fail.
    //!
    //!   **This is a configuration pin, not a normative statement that
    //!   `next` is safe to use as a function name on the native surface**
    //!   (issue #1472 review): `docs/stdlib-spec.md` §9.6 F6 (RULED
    //!   2026-07-19) reserves `display`/`compare`/`next` with no dialect
    //!   qualification — E113 simply does not *reach* the native surface
    //!   today, because `check_reserved_names` is wired brink-only
    //!   (`crates/internal/brink-analyzer/src/lib.rs`'s brink-only block).
    //!   That is the same class of dialect-vs-`is_native` gap #1464/#1470
    //!   fixed for E084/E106/E138 — open here, not fixed by this PR. See
    //!   issue #1472's tracking comment.
    use super::compile_and_explore_from_brink_native;
    use crate::explorer::ExploreConfig;

    fn config() -> ExploreConfig {
        ExploreConfig::default()
    }

    #[test]
    fn the_harness_no_longer_forces_the_brink_dialect() {
        // `next` is a protocol-reserved method name under
        // `docs/stdlib-spec.md` §9.6 F6, with no dialect qualification —
        // but `check_reserved_names` (E113) is wired to run only `if
        // dialect == Dialect::Brink` (open gap, not asserted correct here;
        // see the module doc above). This test pins that the harness no
        // longer hardcodes `Dialect::Brink`, which is what let the old
        // configuration spuriously reject this fixture.
        let src = "\
fn next() {
  return 1;
}

flow main() {
  Value is {next()}.
}
";
        let (_data, episodes) = compile_and_explore_from_brink_native(src, &config()).expect(
            "an ordinary native function named `next` must compile cleanly \
             under the harness's real native dialect default",
        );
        let episode = episodes.first().expect("one episode");
        let text: String = episode.steps.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "Value is 1.\n");
    }
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
