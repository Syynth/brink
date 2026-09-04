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

/// `tests/tier4-generated/` — the capture tier (issue #3380,
/// `docs/program-generator-spec.md` §5): shrunk generated stories with a
/// golden the inkjs oracle produced (or the C# oracle re-blessed). Its
/// cases carry `oracle/*.oracle.json` exactly like a curated case, so the
/// shared walk below prunes the directory by name: the tier is
/// self-contained with its own must-pass target (`tier4_generated.rs`) and
/// must never leak into `RATCHET_EPISODE_COUNT`, the sanction, or the
/// respell sweep. [`collect_generated_cases`] is the one way in.
pub const GENERATED_TIER_DIR: &str = "tier4-generated";

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

/// True for a case whose fixture is *deliberately* a compile-error probe
/// (`metadata.toml` sets `mode = "compile_error"`). Such a case is expected
/// to disagree with "success", not with a golden or with another compile
/// road's diagnostics — `oracle_snapshots.rs` and the #2223 parallel gate
/// (`environment_parallel_gate.rs`) both skip it before comparing, via this
/// one definition, so the skip rule cannot drift between the two.
pub fn is_compile_error_case(case_dir: &Path) -> bool {
    let meta_path = case_dir.join("metadata.toml");
    std::fs::read_to_string(meta_path).ok().is_some_and(|s| {
        s.lines()
            .any(|line| line.trim() == r#"mode = "compile_error""#)
    })
}

/// The GitHub issue number (as written in `metadata.toml`, e.g. `"#3395"`)
/// backing this case's known, permanent divergence from the C# oracle —
/// `[source] expected_mismatch = "#NNNN"`.
///
/// Before issue #3402, a case deliberately added to lock in a documented
/// mismatch (e.g. the two `#3395` lift-order cases) was excluded from
/// `oracle_snapshots.rs`'s `RATCHET_EPISODE_COUNT` only by a prose doc
/// comment enumerating which cases those were — nothing
/// asserted *which* cases were the expected failures, so a case silently
/// swapping places (one fixed, an unrelated one regressing) could leave the
/// totals looking flat. `oracle_snapshots.rs` and `corpus_report.rs` both
/// read this field instead: paired with [`mismatch_flag_verdict`], a flagged
/// case whose episodes all now match the oracle is reported as
/// unexpectedly fixed (remove the flag, raise the ratchet) rather than
/// silently absorbed.
///
/// Returns `None` when `metadata.toml` is missing, unreadable, not valid
/// TOML, has no `[source]` table, or that table has no `expected_mismatch`
/// key.
///
/// A parsed `metadata.toml` carrying `expected_mismatch` anywhere other than
/// `[source]` as a string — a top-level key, a key under a different table
/// (e.g. an append-to-end landing in `[classification]`), or a non-string
/// value like a bare integer — fails loudly via `assert!` rather than
/// silently returning `None`. Reverting to "unflagged" on a misplaced or
/// mistyped flag is exactly the silent drift issue #3402 exists to kill.
pub fn expected_mismatch_issue(case_dir: &Path) -> Option<String> {
    expected_mismatch_issue_in(&case_dir.join("metadata.toml"))
}

/// [`expected_mismatch_issue`] over an explicit metadata file — the capture
/// tier's `case.toml` (issue #3380) carries the same `[source]
/// expected_mismatch` field with the same rules.
pub fn expected_mismatch_issue_in(meta_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(meta_path).ok()?;
    let doc: toml::Value = toml::from_str(&text).ok()?;
    let table = doc.as_table()?;

    assert!(
        !table.contains_key("expected_mismatch"),
        "{}: `expected_mismatch` must live in `[source]`, not at the top level",
        meta_path.display(),
    );
    for (table_name, value) in table {
        if table_name == "source" {
            continue;
        }
        let misplaced = value
            .as_table()
            .is_some_and(|sub| sub.contains_key("expected_mismatch"));
        assert!(
            !misplaced,
            "{}: `expected_mismatch` must live in `[source]`, not `[{table_name}]`",
            meta_path.display(),
        );
    }

    let source = table.get("source").and_then(toml::Value::as_table)?;
    let value = source.get("expected_mismatch")?;
    let issue = value.as_str();
    assert!(
        issue.is_some(),
        "{}: `[source] expected_mismatch` must be a quoted issue string like \"#3395\", not {value:?}",
        meta_path.display(),
    );
    issue.map(str::to_string)
}

/// The oracle-conformance verdict for one case, once its
/// [`expected_mismatch_issue`] flag (if any) is folded in — the single
/// definition shared by `oracle_snapshots.rs`'s ratchet-arithmetic assertion
/// and `corpus_report.rs`'s expected-mismatch backlog listing, so
/// "unexpectedly fixed" cannot drift into two different meanings between
/// the two (issue #3402).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchFlagVerdict {
    /// No `expected_mismatch` flag on this case — ordinary pass/fail
    /// accounting applies, unchanged from before #3402.
    Unflagged,
    /// Flagged, and still mismatching or missing episodes against the
    /// oracle — the expected, steady state for a case pinning a known,
    /// unfixed divergence.
    ExpectedAndStillMismatching,
    /// Flagged, but every one of its episodes now matches the oracle: the
    /// underlying bug is fixed. The flag must be removed from
    /// `metadata.toml` and `RATCHET_EPISODE_COUNT` raised in the same
    /// change — this case's episodes are not yet counted toward the floor.
    UnexpectedlyFixed,
}

/// Compute a case's [`MismatchFlagVerdict`] from its flag state and episode
/// counts. Pure and side-effect-free so both harness tests can call it
/// directly instead of each re-deriving the same three-way branch by hand.
pub fn mismatch_flag_verdict(
    expected_mismatch: Option<&str>,
    episodes_mismatch: usize,
    episodes_missing: usize,
) -> MismatchFlagVerdict {
    match expected_mismatch {
        None => MismatchFlagVerdict::Unflagged,
        Some(_) if episodes_mismatch == 0 && episodes_missing == 0 => {
            MismatchFlagVerdict::UnexpectedlyFixed
        }
        Some(_) => MismatchFlagVerdict::ExpectedAndStillMismatching,
    }
}

/// True when `case_dir`'s `story.ink` is missing or empty/whitespace-only —
/// a case with nothing to compile, skipped for the same reason as
/// [`is_compile_error_case`] and by the same callers.
pub fn has_empty_source(case_dir: &Path) -> bool {
    let ink_path = case_dir.join("story.ink");
    std::fs::read_to_string(ink_path)
        .ok()
        .is_some_and(|s| s.trim().is_empty())
}

/// Every immediate subdirectory of `root` (e.g. `tests/tier1-native/`),
/// sorted — walked rather than listed, so a newly-added corpus case is
/// swept automatically by every caller (`tier1_native_strict.rs`'s strict
/// sweep and the #2223 parallel gate's native sweep) with no `known`-list to
/// drift. Returns an empty `Vec` if `root` cannot be read; callers that
/// expect a nonempty corpus already assert a floor on the result, so an
/// unreadable root fails loudly there instead of panicking here.
pub fn native_case_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
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

    for entry in Walk::new(dir)
        .prune_also(CASE_DATA_DIRS)
        .prune_also([GENERATED_TIER_DIR])
        .flatten()
    {
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
/// Discovers and applies a co-located `brink.toml` (issue #2289), the same
/// way [`compile_via_environment`] already does via `Project::load` —
/// without this, a fixture that needs `[project] conventions` configured
/// (e.g. a conventions module claiming its own file's prose, now that an
/// unconfigured `@[convention]` is `E169`) would diverge between this
/// `compile_path`-based road and the `Environment`-based one
/// `environment_parallel_gate.rs` compares it against: `compile_path`
/// itself bypasses `Environment`/config entirely (its own doc says so),
/// so without this discovery step here, only the `Environment` road would
/// ever see the file. `None`/no config found is byte-identical to the
/// pre-#2289 behavior (`AnalysisOptions::default()`) — every existing
/// fixture with no `brink.toml` is unaffected.
///
/// Returns `Err` on any compile error, link error, or runtime fault, if
/// the program ever reaches [`brink_runtime::Step::Choices`], or if it
/// produces more than [`brink_runtime::FlowInstance::LINE_LIMIT`] lines
/// without reaching a terminal one.
///
/// The line count is capped so a fixture that diverts back to itself while
/// emitting text (e.g. `flow main() { Hi -> main }`) fails loudly instead
/// of hanging `cargo test --workspace` or `corpus_report` — see the repo's
/// "VM tests must not hang" / "guard against unbounded growth" rules. This
/// mirrors [`brink_runtime::FlowInstance::drive_to_terminal`]'s own
/// `LINE_LIMIT` cap; `continue_single` alone only enforces the per-line
/// step limit, not a cap on the number of lines produced.
pub fn run_native_transcript(brink_path: &Path) -> Result<String, String> {
    let options = native_analysis_options(brink_path)?;
    let output = brink_compiler::compile_path_with_options(brink_path, options)
        .map_err(|e| format!("compile {}: {e}", brink_path.display()))?;
    drive_native_transcript(&output.data, &brink_path.display().to_string())
}

/// Discover a `brink.toml` governing `entry`'s project (walking up from it,
/// [`brink_project_config::load_from_entry`]) and fold it into a fresh
/// [`brink_analyzer::AnalysisOptions`] the same way every production entry
/// point does (`dialect`/`types` unset-means-untouched, so passing `false`
/// for both `_overridden` flags here matches "nothing else has claimed
/// these yet"). `Ok(AnalysisOptions::default())`, byte-identical to today,
/// when no `brink.toml` is found — see [`run_native_transcript`]'s own doc
/// for why this discovery step exists at all.
///
/// `pub`: also used directly by `tier1_native.rs`'s compile-level sibling
/// tests, which link a fixture themselves (via `brink_compiler::
/// compile_path_with_options` + `brink_runtime::link`) rather than driving
/// it to a transcript through this module's own [`drive_native_transcript`].
pub fn native_analysis_options(entry: &Path) -> Result<brink_analyzer::AnalysisOptions, String> {
    let (loaded, _discovery_warnings) = brink_project_config::load_from_entry(entry)
        .map_err(|e| format!("load brink.toml governing {}: {e}", entry.display()))?;
    let mut options = brink_analyzer::AnalysisOptions::default();
    if let Some(loaded) = loaded {
        let _config_warnings = options.apply_project_config(&loaded.config, false, false);
    }
    Ok(options)
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

/// Compile an entry (`.ink` or `.brink`) through the **real production
/// path** (issue #2223) — `brink_environment::Project::load` +
/// `brink_environment::compile`, exactly mirroring `brink-cli`'s
/// `compile_entry` (`brink_driver::native_source_root_with_warnings` for
/// the root, `RealFs` rooted there, default `OptionOverrides`). Every other
/// helper in this module compiles through `brink_compiler::compile_path`,
/// which its own doc says plainly bypasses `Environment` entirely — so
/// nothing here has ever exercised the stdlib mount (#2080) or
/// `Environment`'s manifest keying. This is the parallel gate's sole entry
/// point: it lets a corpus sweep run the *same* fixture through the real
/// path alongside the existing `compile_path`-based one, to catch
/// divergence between the two roads (`docs/decision-log.md`, #2223).
///
/// Discovery warnings (e.g. a pruned directory containing `.brink`
/// sources) are discarded here — this is a correctness probe over corpus
/// fixtures with no config quirks to report, not a CLI invocation.
pub fn compile_via_environment(entry_path: &Path) -> Result<brink_format::StoryData, String> {
    let (root, _warnings) = brink_driver::native_source_root_with_warnings(entry_path);
    let tree = brink_driver::RealFs::new(&root);
    let entry_key = brink_driver::relative_key(&root, entry_path);
    let overrides = brink_environment::OptionOverrides::default();
    let env = brink_environment::Project::load(&tree, &entry_key, &overrides)
        .map_err(|e| format!("Environment::load {}: {e}", entry_path.display()))?;
    let output = brink_environment::compile(&env)
        .map_err(|e| format!("compile(&Environment) {}: {e}", entry_path.display()))?;
    Ok(output.data)
}

/// Compile an entry from disk (`.ink` or `.brink`) and return both the
/// compiled story and its `.inkb` bytes — the pair the equivalence oracle
/// (`crate::trace`) takes: bytes for [`crate::trace::trace_diff`], the
/// [`brink_format::StoryData`] for
/// [`crate::trace::line_identity_diff`].
///
/// Goes through [`native_analysis_options`] exactly like
/// [`run_native_transcript`], so a fixture with a co-located `brink.toml`
/// compiles the same way here as everywhere else in this module.
pub fn compile_entry_to_inkb(entry: &Path) -> Result<(brink_format::StoryData, Vec<u8>), String> {
    let options = native_analysis_options(entry)?;
    let output = brink_compiler::compile_path_with_options(entry, options)
        .map_err(|e| format!("compile {}: {e}", entry.display()))?;
    let bytes = crate::trace::to_inkb(&output.data);
    Ok((output.data, bytes))
}

/// Write `source` to a scratch file named `file_name` and compile it with
/// [`compile_entry_to_inkb`].
///
/// A real file on disk, not an in-memory `read_file` callback: native
/// (`.brink`) discovery reads through `RealFs` and bypasses any virtual
/// source entry point (see `tier1_native.rs`'s own scratch-file note), so
/// one helper that works for both surfaces has to go through disk.
///
/// The scratch directory is unique per process and per call, and is removed
/// before returning whether or not the compile succeeded.
pub fn compile_source_to_inkb(
    label: &str,
    file_name: &str,
    source: &str,
) -> Result<(brink_format::StoryData, Vec<u8>), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "brink-trace-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create scratch dir: {e}"))?;
    let path = dir.join(file_name);
    let written = std::fs::write(&path, source).map_err(|e| format!("write scratch source: {e}"));
    let result = written.and_then(|()| compile_entry_to_inkb(&path));
    std::fs::remove_dir_all(&dir).ok();
    result
}

/// The [`compile_via_environment`] analogue of [`compile_and_explore_from_ink`]
/// — compiles through the real production path, links, and explores.
pub fn compile_and_explore_via_environment(
    ink_path: &Path,
    config: &ExploreConfig,
) -> Result<(brink_format::StoryData, Vec<Episode>), String> {
    let data = compile_via_environment(ink_path)?;
    let (program, line_tables) = brink_runtime::link(&data).map_err(|e| format!("link: {e}"))?;
    let episodes = crate::explore(std::sync::Arc::new(program), line_tables, config);
    Ok((data, episodes))
}

/// Drive a compiled [`brink_format::StoryData`] to completion, returning the
/// concatenated output text — the shared drive loop behind
/// [`run_native_transcript`] and its [`compile_via_environment`] analogue,
/// [`run_native_transcript_via_environment`]. Enforces the same choice-free
/// and [`brink_runtime::FlowInstance::LINE_LIMIT`] contracts
/// [`run_native_transcript`]'s own doc comment describes.
fn drive_native_transcript(
    data: &brink_format::StoryData,
    case_label: &str,
) -> Result<String, String> {
    let (program, line_tables) =
        brink_runtime::link(data).map_err(|e| format!("link {case_label}: {e}"))?;
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );

    let mut out = String::new();
    let mut line_count = 0usize;
    loop {
        match story
            .continue_single()
            .map_err(|e| format!("runtime error in {case_label}: {e}"))?
        {
            brink_runtime::Step::Line(line) => out.push_str(&line.text),
            brink_runtime::Step::Done
            | brink_runtime::Step::End
            | brink_runtime::Step::Suspended => {
                break;
            }
            brink_runtime::Step::Choices(_) => {
                return Err(format!(
                    "{case_label} presented choices — tier1-native cases must be choice-free \
                     straight-line programs"
                ));
            }
        }
        line_count += 1;
        if line_count >= brink_runtime::FlowInstance::LINE_LIMIT {
            return Err(format!(
                "{case_label} produced {line_count} lines without reaching a terminal step — \
                 exceeded FlowInstance::LINE_LIMIT ({})",
                brink_runtime::FlowInstance::LINE_LIMIT
            ));
        }
    }
    Ok(out)
}

/// The [`compile_via_environment`] analogue of [`run_native_transcript`] —
/// compiles a native `.brink` entry through the real production path
/// (`Project::load` + `brink_environment::compile`, so the stdlib mount
/// (#2080) is present exactly as it is for `brink compile`/`brink play`),
/// links, and runs it to completion. Used by the #2223 parallel gate to
/// compare against [`run_native_transcript`]'s `compile_path`-based
/// transcript for the same fixture.
pub fn run_native_transcript_via_environment(brink_path: &Path) -> Result<String, String> {
    let data = compile_via_environment(brink_path)?;
    drive_native_transcript(&data, &brink_path.display().to_string())
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
/// the brink-only block). `brink_analyzer::analyze_with_modules` expresses
/// exactly this combination since issue #1358 threaded `is_native` through
/// `finish_analysis` into the per-file and whole-project arms — and made the
/// B0.9 strict-only gate (`E137`) reachable from the pure path at all — so
/// this function just calls it with `is_native = true`. It previously
/// composed those passes by hand (`symbol_index` → per-file `resolve` →
/// `per_file_diagnostics` → `native_strict_only_error` →
/// `whole_project_diagnostics`), because the pure path had no way to say
/// "native"; before *that* it used `analyze_with_options` and ran every
/// native e2e fixture through the *ink* brink-dialect diagnostics arm
/// (hardcoded `Dialect::Brink` + `is_native = false`) — a combination no
/// real compilation path ever produces.
///
/// The module-blind `ModuleMap::new()` it passes is right for this harness
/// specifically: it compiles one in-memory source string that has no path,
/// so there is no path-derived `story::…` identity to qualify by (unlike a
/// `ProjectDb`-backed compile, which must pass `db.module_map()`).
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
    compile_and_explore_from_brink_native_at(src, &std::collections::HashMap::new(), config)
}

/// [`compile_and_explore_from_brink_native`], but qualifying the single
/// in-memory file's anonymous-container scope path with the caller-supplied
/// `file_paths` (a real `#file:{path}`-style qualifier via
/// [`hir::stamp_container_ids`](brink_ir::hir::stamp_container_ids)) instead
/// of the pathless-harness empty qualifier the plain function above always
/// used before this split existed.
///
/// **Why this exists (issue #2229 harness fix):** `ink_corpus_convert.rs`'s
/// `assert_episode_identical` compares two compiles of "the same story" —
/// `explore_from_ink` (a real, path-registered `compile_path` call) against
/// this module's own `explore_from_brink_native` (an in-memory string with
/// no path at all). Before #2229's per-knot qualifier, that asymmetry was
/// invisible: an unqualified scope path and a qualified-but-uncollided one
/// still hashed to the pre-existing single-file addresses, since qualifying
/// by a real, non-colliding path never changed which container won an
/// otherwise-empty scope. #2229's fix makes every knot-interior anonymous
/// container's address depend on the qualifier, so the two legs' identical
/// story now mints two *different* addresses for the exact same fallback
/// choice — not a story-behavior regression, a pre-existing test-harness
/// asymmetry #2229 was the first change to expose (confirmed by reverting/
/// reapplying #2229's stamp.rs diff against this same fixture: passes
/// unpatched, fails patched, entirely within this harness's own two
/// compile legs). The fix is to give both legs the *same* qualifier, not to
/// remove the qualifier from either — see
/// [`crate::corpus::explore_from_ink`]'s own `compile_path`, which always
/// registers a real path and always did.
#[expect(
    clippy::implicit_hasher,
    reason = "internal test-harness API, no need to generalize"
)]
pub fn compile_and_explore_from_brink_native_at(
    src: &str,
    file_paths: &std::collections::HashMap<brink_ir::FileId, String>,
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

    // `is_native = true` — the flag real native compiles pass, and which
    // (issue #1358) threads through every arm: the ink-only T1b dialect
    // gate is skipped, the construction-literal checks widen, the B0.9
    // strict-only gate (`E137`) runs, and the ink-only `E064` config error
    // is skipped. An empty `ModuleMap` keeps identity module-blind,
    // matching this single-file, path-less harness.
    //
    // Composed from the analyzer's piece functions (option A total,
    // 2026-08-24 — the `analyze_with_modules` monolith is deleted): this
    // harness is the documented deliberately-hand-composed, no-salsa
    // first-light path (#1106), so spelling the index → resolve → finish
    // composition out here IS its design, not a workaround.
    let empty_map = brink_analyzer::ModuleMap::new();
    let (index, mut diagnostics) = brink_analyzer::symbol_index_with_modules(
        &[(file_id, &manifest)],
        &empty_map,
        analysis_opts.dialect,
        true,
    );
    let scope =
        brink_analyzer::ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
    let (file_map, file_diags) = brink_analyzer::resolve(file_id, &manifest, &index, &scope);
    diagnostics.extend(file_diags);
    let mut scopes = std::collections::BTreeMap::new();
    scopes.insert(file_id, scope);
    let brink_analyzer::AnalysisResult {
        index,
        resolutions,
        diagnostics,
        ..
    } = brink_analyzer::finish_analysis(
        &files_for_analysis,
        index,
        std::sync::Arc::unwrap_or_clone(file_map),
        diagnostics,
        &analysis_opts,
        true,
        None,
        &scopes,
    );

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
        file_paths,
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

/// [`explore_from_brink_native`], threading a real `file_paths` qualifier —
/// see [`compile_and_explore_from_brink_native_at`]'s doc for why a caller
/// comparing this leg against a real, path-registered compile (like
/// [`explore_from_ink`]) needs this instead of the pathless default.
#[expect(
    clippy::implicit_hasher,
    reason = "internal test-harness API, no need to generalize"
)]
pub fn explore_from_brink_native_at(
    src: &str,
    file_paths: &std::collections::HashMap<brink_ir::FileId, String>,
    config: &ExploreConfig,
) -> Result<Vec<Episode>, String> {
    compile_and_explore_from_brink_native_at(src, file_paths, config).map(|(_, episodes)| episodes)
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

#[cfg(test)]
mod expected_mismatch_tests {
    //! Pins issue #3402: a case's `[source] expected_mismatch` flag must be
    //! read from real `metadata.toml` files (not reimplemented ad hoc by a
    //! caller), and the pass/flag verdict must treat "flagged but still
    //! mismatching" and "flagged and now fixed" as genuinely different
    //! outcomes.
    use super::{MismatchFlagVerdict, expected_mismatch_issue, mismatch_flag_verdict};
    use std::path::PathBuf;

    /// A unique scratch directory under the system temp dir, holding a
    /// hand-written `metadata.toml` — removed on drop. Mirrors this module's
    /// existing `ScratchFile` pattern (see `golden_transcript_tests` above)
    /// rather than depending on a real corpus fixture for the synthetic
    /// cases below.
    struct ScratchCaseDir(PathBuf);

    impl ScratchCaseDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "brink-test-harness-expected-mismatch-{name}-{}",
                std::process::id(),
            ));
            std::fs::create_dir_all(&path).expect("create scratch case dir");
            Self(path)
        }

        fn write_metadata(&self, content: &str) {
            std::fs::write(self.0.join("metadata.toml"), content)
                .expect("write scratch metadata.toml");
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for ScratchCaseDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_the_field_from_the_source_table() {
        let scratch = ScratchCaseDir::new("flagged");
        scratch.write_metadata(
            "description = \"a flagged case\"\n\
             mode = \"runtime\"\n\
             \n\
             [source]\n\
             origin = \"brink\"\n\
             original_id = \"flagged\"\n\
             expected_mismatch = \"#3395\"\n",
        );
        assert_eq!(
            expected_mismatch_issue(scratch.path()),
            Some("#3395".to_string())
        );
    }

    #[test]
    fn returns_none_when_the_field_is_absent() {
        let scratch = ScratchCaseDir::new("unflagged");
        scratch.write_metadata(
            "description = \"an ordinary case\"\n\
             mode = \"runtime\"\n\
             \n\
             [source]\n\
             origin = \"brink\"\n\
             original_id = \"unflagged\"\n",
        );
        assert_eq!(expected_mismatch_issue(scratch.path()), None);
    }

    #[test]
    fn returns_none_when_metadata_toml_is_missing() {
        let scratch = ScratchCaseDir::new("no-metadata");
        assert_eq!(expected_mismatch_issue(scratch.path()), None);
    }

    #[test]
    fn returns_none_when_metadata_toml_is_malformed() {
        let scratch = ScratchCaseDir::new("malformed");
        scratch.write_metadata("this is not [ valid toml");
        assert_eq!(expected_mismatch_issue(scratch.path()), None);
    }

    #[test]
    #[should_panic(expected = "top level")]
    fn panics_when_the_flag_is_at_top_level() {
        let scratch = ScratchCaseDir::new("top-level-flag");
        scratch.write_metadata(
            "description = \"a case with a misplaced flag\"\n\
             mode = \"runtime\"\n\
             expected_mismatch = \"#1234\"\n\
             \n\
             [source]\n\
             origin = \"brink\"\n\
             original_id = \"top-level-flag\"\n",
        );
        let _ = expected_mismatch_issue(scratch.path());
    }

    #[test]
    #[should_panic(expected = "[classification]")]
    fn panics_when_the_flag_is_appended_into_the_wrong_table() {
        // Mirrors the real failure mode: appending a line to the end of a
        // metadata.toml whose `[source]` table is followed by
        // `[classification]` lands the new key in the wrong table.
        let scratch = ScratchCaseDir::new("wrong-table-flag");
        scratch.write_metadata(
            "description = \"a case with the flag appended to the end\"\n\
             mode = \"runtime\"\n\
             \n\
             [source]\n\
             origin = \"brink\"\n\
             original_id = \"wrong-table-flag\"\n\
             \n\
             [classification]\n\
             tier = 2\n\
             expected_mismatch = \"#1234\"\n",
        );
        let _ = expected_mismatch_issue(scratch.path());
    }

    #[test]
    #[should_panic(expected = "quoted issue string")]
    fn panics_when_the_flag_value_is_not_a_string() {
        let scratch = ScratchCaseDir::new("non-string-flag");
        scratch.write_metadata(
            "description = \"a case with a bare-integer flag\"\n\
             mode = \"runtime\"\n\
             \n\
             [source]\n\
             origin = \"brink\"\n\
             original_id = \"non-string-flag\"\n\
             expected_mismatch = 1234\n",
        );
        let _ = expected_mismatch_issue(scratch.path());
    }

    /// The real migration target (issue #3402) was the two `#3395`
    /// lift-order fixtures, flagged from the day they were added. The #3395
    /// fix (2026-09-04) flipped both and removed their flags in the same
    /// change — the life cycle the flag exists for — so all three lift-order
    /// cases now read as unflagged, and the fixtures still parse (a flag
    /// misplaced or mistyped would panic above, not read as `None`).
    #[test]
    fn the_real_3395_fixtures_are_unflagged_after_the_fix() {
        let tests_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("tests")
            .join("tier2")
            .join("evaluation");

        for case in [
            "lift-order-seq-fn-cond",
            "lift-order-fn-then-cond",
            "lift-order-cond-then-fn",
        ] {
            let dir = tests_root.join(case);
            assert!(
                dir.join("metadata.toml").is_file(),
                "{case}: the fixture must still exist"
            );
            assert_eq!(
                expected_mismatch_issue(&dir),
                None,
                "{case} must NOT carry expected_mismatch — #3395 is fixed and the ratchet counts it"
            );
        }
    }

    #[test]
    fn verdict_is_unflagged_when_there_is_no_flag() {
        assert_eq!(
            mismatch_flag_verdict(None, 0, 0),
            MismatchFlagVerdict::Unflagged
        );
        assert_eq!(
            mismatch_flag_verdict(None, 3, 1),
            MismatchFlagVerdict::Unflagged
        );
    }

    #[test]
    fn verdict_is_expected_when_flagged_and_still_mismatching() {
        assert_eq!(
            mismatch_flag_verdict(Some("#1234"), 1, 0),
            MismatchFlagVerdict::ExpectedAndStillMismatching
        );
        assert_eq!(
            mismatch_flag_verdict(Some("#1234"), 0, 1),
            MismatchFlagVerdict::ExpectedAndStillMismatching
        );
    }

    #[test]
    fn verdict_is_unexpectedly_fixed_when_flagged_but_clean() {
        assert_eq!(
            mismatch_flag_verdict(Some("#1234"), 0, 0),
            MismatchFlagVerdict::UnexpectedlyFixed
        );
    }
}

// ---------------------------------------------------------------------------
// The capture tier (issue #3380)
// ---------------------------------------------------------------------------

/// Where a `tests/tier4-generated/` case came from and who blessed its
/// golden — the `[provenance]` table of its `case.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedProvenance {
    /// `"proptest"` (a shrunk counterexample) or `"probe"` (a hand-minimised
    /// story from reference-differential probing).
    pub source: String,
    /// The property that failed (`"inkjs_differential"`, …) or the probe.
    pub property: String,
    /// The proptest seed, when one was recorded.
    pub seed: Option<String>,
    /// `"inkjs"` — the golden came from `tools/inkjs-oracle` — or `"csharp"`
    /// once a maintainer re-blessed it with the C# oracle.
    pub oracle_source: String,
    /// The issue the case reproduces, when it reproduces one.
    pub issue: Option<String>,
}

/// One case of the capture tier: its directory, name and provenance. The
/// expected-mismatch flag is read separately through
/// [`expected_mismatch_issue_in`] on `case.toml`, the same road the curated
/// corpus takes for `metadata.toml`.
#[derive(Debug, Clone)]
pub struct GeneratedCase {
    pub dir: PathBuf,
    pub name: String,
    pub provenance: GeneratedProvenance,
}

/// The case directories directly under `<tests>/tier4-generated/`, sorted —
/// every directory there that carries a `case.toml`.
pub fn collect_generated_cases(tests_root: &Path) -> Vec<PathBuf> {
    let root = tests_root.join(GENERATED_TIER_DIR);
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("case.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// Parse a capture-tier case's `case.toml`.
///
/// # Errors
/// A missing or malformed file, or a `[provenance]` table missing a required
/// key, is an error naming the case — never a silently defaulted case.
pub fn load_generated_case(dir: &Path) -> Result<GeneratedCase, String> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta_path = dir.join("case.toml");
    let text = std::fs::read_to_string(&meta_path)
        .map_err(|e| format!("{name}: read {}: {e}", meta_path.display()))?;
    let doc: toml::Value =
        toml::from_str(&text).map_err(|e| format!("{name}: parse {}: {e}", meta_path.display()))?;
    let prov = doc
        .get("provenance")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{name}: case.toml has no [provenance] table"))?;
    let required = |key: &str| -> Result<String, String> {
        prov.get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("{name}: [provenance] {key} is missing or not a string"))
    };
    let optional = |key: &str| {
        prov.get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    let oracle_source = required("oracle-source")?;
    if oracle_source != "inkjs" && oracle_source != "csharp" {
        return Err(format!(
            "{name}: [provenance] oracle-source must be \"inkjs\" or \"csharp\", not {oracle_source:?}"
        ));
    }
    let provenance = GeneratedProvenance {
        source: required("source")?,
        property: required("property")?,
        seed: optional("seed"),
        oracle_source,
        issue: optional("issue"),
    };
    Ok(GeneratedCase {
        dir: dir.to_path_buf(),
        name,
        provenance,
    })
}
