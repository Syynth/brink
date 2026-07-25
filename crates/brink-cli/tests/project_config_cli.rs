//! Integration tests for the `brink.toml` project settings file (#1005):
//! discovery + application in `brink compile` and `brink ide`, the explicit
//! `--dialect`/`--types` override precedence, and the "no file = unchanged
//! behavior" guarantee.
//!
//! Fixture source uses a brink-extension construct (a `~ { … }` logic block
//! with a `#[…]` array literal) that `strict-ink` (the default, and the
//! only dialect reachable pre-#1005 without a CLI flag) rejects with `E051`
//! — so whether the compile succeeds is a direct, black-box signal of which
//! dialect actually took effect.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// A minimal story using a `~ { … }` block + `#[…]` array literal — valid
/// only under `dialect = brink`.
const EXTENSION_FIXTURE: &str = "\
VAR arr = 0
~ { arr = #[1, 2, 3] }
Done.
-> END
";

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

/// A fresh, uniquely-named project directory under the OS temp dir.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn project_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "brink-project-config-cli-{}-{tag}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn write_story(dir: &std::path::Path, content: &str) -> PathBuf {
    let path = dir.join("story.ink");
    fs::write(&path, content).unwrap();
    path
}

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn write_config(dir: &std::path::Path, content: &str) {
    fs::write(dir.join("brink.toml"), content).unwrap();
}

// ── brink compile ────────────────────────────────────────────────────

#[test]
fn compile_no_config_extension_syntax_fails_unchanged_behavior() {
    let dir = project_dir("compile-no-config");
    let story = write_story(&dir, EXTENSION_FIXTURE);

    let out = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        !out.status.success(),
        "strict-ink (no brink.toml, no flag) must reject brink-extension syntax"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_discovers_and_applies_dialect_from_brink_toml() {
    let dir = project_dir("compile-config-applies");
    let story = write_story(&dir, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        out.status.success(),
        "brink.toml's dialect = \"brink\" should be discovered and applied: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_walks_up_from_entry_to_find_brink_toml() {
    let dir = project_dir("compile-config-nested");
    let nested = dir.join("chapters");
    fs::create_dir_all(&nested).unwrap();
    let story = write_story(&nested, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        out.status.success(),
        "brink.toml one directory above the entry file should still be found: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

/// Regression for issue #1413: `native_source_root`'s walk-up is purely
/// *lexical* (`Path::parent`) — for a relative `entry_dir` that bottoms out
/// at the process's cwd (`""`), with no way to synthesize a `".."` to keep
/// climbing, unlike an absolute `entry_dir`, whose `Path::parent` chain
/// walks all the way to the filesystem root for free. So `brink compile
/// story.ink`, run from a cwd one directory *below* the true project root
/// (`brink.toml` lives in cwd's *parent*, not in cwd itself), used to miss
/// the config and mis-root at cwd — the entry failed to compile (two bogus
/// E051 dialect errors) even though the identical absolute-path entry (or a
/// bare entry with an extra path component reaching the same directory)
/// resolved correctly. `.current_dir(&sub)` + a bare relative entry
/// reproduces that exact scenario end-to-end, mirroring the `brink ide`
/// regression test #1403/PR #1412 added for the sibling bug.
#[test]
fn compile_finds_brink_toml_above_a_bare_relative_entrys_cwd() {
    let dir = project_dir("compile-config-above-cwd");
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    write_story(&sub, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .current_dir(&sub)
        .args(["compile", "story.ink"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a bare relative entry must still discover a brink.toml above the \
         process's cwd, exactly like the absolute-path form: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

/// Companion for the case issue #1413 names literally: `brink.toml` sitting
/// *beside* a bare, single-component relative entry, directly in the
/// process's cwd (not above it). This shape already resolved correctly
/// before the #1413 fix (the fast relative-walk path finds it on its first
/// probe), but had no CLI-level `.current_dir()` coverage — every existing
/// `compile_*` test above passes an absolute entry path, which sidesteps
/// `native_source_root`'s cwd-relative walk entirely. Pins it explicitly so
/// a future regression here is caught the same way #1413 itself was.
#[test]
fn compile_finds_brink_toml_beside_a_bare_relative_entry_in_cwd() {
    let dir = project_dir("compile-config-beside-bare-entry");
    write_story(&dir, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .current_dir(&dir)
        .args(["compile", "story.ink"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a bare relative entry must discover a brink.toml beside it in cwd: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_explicit_flag_overrides_brink_toml() {
    let dir = project_dir("compile-config-override");
    let story = write_story(&dir, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["--dialect", "strict-ink"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "an explicit --dialect flag must override brink.toml's dialect"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_unknown_config_key_is_a_warning_not_a_compile_failure() {
    let dir = project_dir("compile-config-unknown-key");
    // Plain ink (no extension syntax) so a config error/warning is the only
    // thing that could make this fail.
    let story = write_story(&dir, "Hello.\n-> END\n");
    write_config(&dir, "[project]\nfuture_key = \"x\"\n");

    let out = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        out.status.success(),
        "an unrecognized brink.toml key must warn, not fail compilation: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

/// #1369 (house rule 11): a malformed `brink.toml` must fail `brink compile`
/// and the error must name `brink.toml`, not just report a bare parse
/// error — the CLI-visible, black-box proof of `LoadError::Config`'s
/// `path` field actually reaching the user, not just the in-crate unit
/// tests around `brink_environment::Project::load` directly.
#[test]
fn compile_malformed_brink_toml_names_the_file_in_the_error() {
    let dir = project_dir("compile-config-malformed");
    let story = write_story(&dir, "Hello.\n-> END\n");
    write_config(&dir, "[project]\ndialect = \"sideways\"\n");

    let out = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        !out.status.success(),
        "a malformed brink.toml must fail brink compile"
    );
    // `main.rs` reports the error via `tracing::error!`, and
    // `tracing_subscriber::fmt()`'s default writer is stdout (not stderr) —
    // confirmed by direct invocation, not assumed.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("brink.toml"),
        "compile error must name brink.toml, got stdout: {stdout}, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

// ── brink compile: --deny/--warn/--allow / -D warnings (#1373) ────────

/// A logic line with no effect (`~` alone) — `DiagnosticCode::E014`,
/// `Warning` by default. Plain `strict-ink` source, no extension syntax, so
/// only the lint-override tier can make this fail.
const E014_FIXTURE: &str = "Hello.\n~\n-> END\n";

#[test]
fn compile_deny_e014_flag_fails_an_otherwise_clean_compile() {
    let dir = project_dir("compile-deny-e014");
    let story = write_story(&dir, E014_FIXTURE);

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["--deny", "E014"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "--deny E014 must make an ordinarily-Warning diagnostic fail the compile"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_short_deny_flag_fails_an_otherwise_clean_compile() {
    let dir = project_dir("compile-deny-e014-short");
    let story = write_story(&dir, E014_FIXTURE);

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["-D", "E014"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "-D E014 must make an ordinarily-Warning diagnostic fail the compile"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_no_lint_flags_e014_stays_a_warning() {
    let dir = project_dir("compile-no-lint-flags");
    let story = write_story(&dir, E014_FIXTURE);

    let out = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        out.status.success(),
        "with no --deny/-D warnings flag, E014 must stay a Warning: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_deny_warnings_short_flag_fails_an_unconfigured_warning() {
    let dir = project_dir("compile-deny-warnings-short");
    let story = write_story(&dir, E014_FIXTURE);

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["-D", "warnings"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "-D warnings must promote an unconfigured E014 warning to a compile error"
    );

    fs::remove_dir_all(&dir).ok();
}

/// A CLI `--allow` must beat a conflicting `brink.toml` `[lints] E014 =
/// "deny"` entry — #1005/#1373's `CLI/API > file > default` precedence.
#[test]
fn compile_allow_flag_overrides_a_conflicting_brink_toml_deny() {
    let dir = project_dir("compile-allow-overrides-toml-deny");
    let story = write_story(&dir, E014_FIXTURE);
    write_config(&dir, "[lints]\nE014 = \"deny\"\n");

    // Sanity check: the file alone denies E014 and fails the compile.
    let baseline = brink().arg("compile").arg(&story).output().unwrap();
    assert!(
        !baseline.status.success(),
        "sanity check: brink.toml's E014 = \"deny\" alone must fail the compile"
    );

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["--allow", "E014"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "--allow E014 must override brink.toml's conflicting E014 = \"deny\": {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

/// A CLI `--deny` must beat a conflicting `brink.toml` `[lints] E014 =
/// "allow"` entry, the reverse direction of the above.
#[test]
fn compile_deny_flag_overrides_a_conflicting_brink_toml_allow() {
    let dir = project_dir("compile-deny-overrides-toml-allow");
    let story = write_story(&dir, E014_FIXTURE);
    write_config(&dir, "[lints]\nE014 = \"allow\"\n");

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["--deny", "E014"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "--deny E014 must override brink.toml's conflicting E014 = \"allow\": {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn compile_unrecognized_deny_code_is_a_warning_not_a_compile_failure() {
    let dir = project_dir("compile-deny-unknown-code");
    let story = write_story(&dir, "Hello.\n-> END\n");

    let out = brink()
        .arg("compile")
        .arg(&story)
        .args(["--deny", "E9999"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "an unrecognized --deny code must warn, not fail compilation: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

// ── brink convert ────────────────────────────────────────────────────

/// Regression for the review finding on `load_story_data` (the shared loader
/// behind `brink convert`/`play`/`replay`/`export-xliff`): it compiled raw
/// `.ink` via `compile_path` (always `AnalysisOptions::default()`), ignoring
/// any discovered `brink.toml` — contradicting the PR's "every mount that
/// compiles from source reads brink.toml" claim, since these mounts are
/// inside `brink-cli` itself. `brink convert` must discover + apply the same
/// file `brink compile` does.
#[test]
fn convert_discovers_and_applies_dialect_from_brink_toml() {
    let dir = project_dir("convert-config-applies");
    let story = write_story(&dir, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink().arg("convert").arg(&story).output().unwrap();
    assert!(
        out.status.success(),
        "brink convert should discover + apply brink.toml's dialect = \"brink\": {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn convert_no_config_extension_syntax_fails_unchanged_behavior() {
    let dir = project_dir("convert-no-config");
    let story = write_story(&dir, EXTENSION_FIXTURE);

    let out = brink().arg("convert").arg(&story).output().unwrap();
    assert!(
        !out.status.success(),
        "strict-ink (no brink.toml) must still reject brink-extension syntax via convert"
    );

    fs::remove_dir_all(&dir).ok();
}

// ── brink ide ────────────────────────────────────────────────────────

#[test]
fn ide_check_has_no_dialect_diagnostic_without_config() {
    let dir = project_dir("ide-no-config");
    let story = write_story(&dir, EXTENSION_FIXTURE);

    let out = brink()
        .args(["ide", "check", "-e"])
        .arg(&story)
        .output()
        .unwrap();
    // `check`'s exit code is 1 when diagnostics exist (query-false), 0 clean.
    assert_eq!(
        out.status.code(),
        Some(1),
        "strict-ink (no brink.toml) should flag the brink-extension syntax: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn ide_check_discovers_and_applies_dialect_from_brink_toml() {
    let dir = project_dir("ide-config-applies");
    let story = write_story(&dir, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .args(["ide", "check", "-e"])
        .arg(&story)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "brink ide should discover + apply brink.toml's dialect = \"brink\": {}",
        String::from_utf8_lossy(&out.stdout)
    );

    fs::remove_dir_all(&dir).ok();
}

/// A renameable `gold` var alongside the brink-extension construct, so a
/// `brink ide rename` can be exercised on a project that only compiles
/// cleanly under `dialect = brink`.
const EXTENSION_RENAME_FIXTURE: &str = "\
VAR gold = 0
~ { gold = #[1, 2, 3][0] }

-> intro

=== intro ===
You have {gold} gold.
-> END
";

/// The `compile_walks_up_from_entry_to_find_brink_toml` companion for
/// `brink ide` (issue #1403): `resolve_analysis_options` now discovers over
/// the `SourceTree` seam (`brink_project_config::discover_from_entry_in_tree`)
/// rather than the path-based `load_from_entry` — this proves the walk-up
/// behavior survived the swap end-to-end, not just in the unit-level
/// `resolve_analysis_options_source_tree_seam_tests`.
#[test]
fn ide_check_walks_up_from_entry_to_find_brink_toml() {
    let dir = project_dir("ide-config-nested");
    let nested = dir.join("chapters");
    fs::create_dir_all(&nested).unwrap();
    let story = write_story(&nested, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .args(["ide", "check", "-e"])
        .arg(&story)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "brink.toml one directory above the entry file should still be found by brink ide: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    fs::remove_dir_all(&dir).ok();
}

/// Companion for `compile_finds_brink_toml_above_a_bare_relative_entrys_cwd`,
/// covering `brink ide check` instead of `brink compile` (issue #1425 named
/// this specific gap: every existing `ide_check_*` test above either has no
/// config at all or one sitting beside/at cwd — none with `brink.toml`
/// *above* cwd and a bare relative entry). `brink ide check` discovers its
/// config over `resolve_analysis_options`'s `SourceTree` seam
/// (`brink_project_config::discover_from_entry_in_tree`,
/// `find_config_in_tree`), a different discovery path than `brink compile`'s
/// path-based `native_source_root`/`find_config` — so this is not redundant
/// with the `compile` case even though the fixture shape matches it exactly.
#[test]
fn ide_check_finds_brink_toml_above_a_bare_relative_entrys_cwd() {
    let dir = project_dir("ide-config-above-cwd");
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    write_story(&sub, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .current_dir(&sub)
        .args(["ide", "check", "-e", "story.ink"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a bare relative entry must let brink ide discover a brink.toml above the process's \
         cwd, exactly like brink compile does: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    fs::remove_dir_all(&dir).ok();
}

/// Regression for a review finding on #1403/PR #1412:
/// `ide_check_walks_up_from_entry_to_find_brink_toml` above always passes an
/// *absolute* entry path, which sidesteps `native_source_root`'s walk-up
/// entirely and left it structurally blind to a real regression — with
/// `brink.toml` sitting in the process's cwd and a bare cwd-relative
/// multi-component entry (`chapters/story.ink`, no `./` prefix), discovery
/// silently missed the config and mis-rooted at `chapters` instead of `dir`,
/// so the entry failed to compile at all (two bogus E051 dialect errors)
/// even though the identical absolute-path and `./`-prefixed forms worked.
/// `.current_dir(&dir)` + a bare relative entry argument reproduces that
/// exact scenario end-to-end.
#[test]
fn ide_check_finds_brink_toml_with_a_bare_relative_multi_component_entry() {
    let dir = project_dir("ide-config-relative-entry");
    let nested = dir.join("chapters");
    fs::create_dir_all(&nested).unwrap();
    write_story(&nested, EXTENSION_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .current_dir(&dir)
        .args(["ide", "check", "-e", "chapters/story.ink"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a bare cwd-relative multi-component entry must still discover brink.toml \
         above it, exactly like the absolute-path form: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    fs::remove_dir_all(&dir).ok();
}

/// Regression for the review finding on `introduced_diagnostics`'s
/// re-analysis `Driver`: it used to be built with
/// `AnalysisOptions::default()` (always `strict-ink`) regardless of the
/// baseline project's discovered `brink.toml`. On a `dialect = brink`
/// project using extension syntax, that re-analysis driver would emit a
/// spurious E051 on every valid construct, which `emit_mutation` counted as
/// an "introduced diagnostic" and refused the rename outright — even though
/// the rename itself introduces nothing. The re-analysis driver must apply
/// the same `brink.toml` the baseline `Project::load` did.
#[test]
fn ide_rename_write_succeeds_under_brink_dialect_extension_syntax() {
    let dir = project_dir("ide-rename-brink-dialect");
    let story = write_story(&dir, EXTENSION_RENAME_FIXTURE);
    write_config(&dir, "[project]\ndialect = \"brink\"\n");

    let out = brink()
        .args(["ide", "rename", "gold", "--to", "coins", "--write", "-e"])
        .arg(&story)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "rename must not be refused by a re-analysis driver that ignores brink.toml: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let src = fs::read_to_string(&story).unwrap();
    assert!(src.contains("VAR coins"), "declaration renamed: {src}");
    assert!(src.contains("{coins}"), "reference renamed: {src}");

    fs::remove_dir_all(&dir).ok();
}

// `load_git_baseline` (used by `brink ide effects-diff --rev`) has its own
// regression test as a unit test in `crates/brink-cli/src/ide.rs` — its
// `AnalysisOptions` divergence from `Project::load` isn't observable through
// `effects-diff`'s CLI output (dialect/types only gate diagnostic severity,
// never effect-row content — the dialect grammar is a superset that always
// parses, per `brink-analyzer::dialect_gate`), so a black-box CLI assertion
// here would pass identically with or without the fix. The unit test
// compares `analysis_options()` directly instead.
