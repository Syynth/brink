//! Integration tests for `brink fix` (auto-fix M6, `docs/autofix-spec.md`
//! §8, issue #3421).
//!
//! Drives the real `brink` binary over the `tests/fix/E025` fixture — the
//! same fixture `brink_test_harness::fix::assert_safe_fix` and
//! `brink_ide::fix`'s own registry test use — through a real project
//! (`brink.toml` discovered, `INCLUDE` followed), never a hand-built AST.
//! `ImportFixer` (`E025`) is `Applicability::Suggested`, so these tests
//! double as the CLI's proof that the default run leaves a Suggested-only
//! project untouched and `--suggested` is what turns it on.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

/// A fresh copy of `tests/fix/E025/` under the OS temp dir, so each test
/// mutates its own files. Returns the project directory.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn e025_project(tag: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fix/E025")
        .canonicalize()
        .unwrap();
    let dir = std::env::temp_dir().join(format!("brink-fix-cli-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for name in ["before.ink", "quest.ink", "brink.toml"] {
        fs::copy(src.join(name), dir.join(name)).unwrap();
    }
    dir
}

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn expected_source() -> String {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fix/E025/expected.ink")
        .canonicalize()
        .unwrap();
    fs::read_to_string(src).unwrap()
}

/// Same fixture as [`e025_project`], but with a `[fix]` table appended to
/// the temp copy's `brink.toml` — never to the checked-in
/// `tests/fix/E025/brink.toml` itself, which is a sibling issue's fixture
/// this wave (`tests/fix/E025/**` is owned by #3448 per the file-ownership
/// table). `fix_table` is the raw contents of the `[fix]` section body
/// (e.g. `"E025 = \"auto\""`).
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn e025_project_with_fix_table(tag: &str, fix_table: &str) -> PathBuf {
    let dir = e025_project(tag);
    let toml_path = dir.join("brink.toml");
    let mut contents = fs::read_to_string(&toml_path).unwrap();
    contents.push_str("\n[fix]\n");
    contents.push_str(fix_table);
    contents.push('\n');
    fs::write(&toml_path, contents).unwrap();
    dir
}

#[expect(clippy::expect_used, reason = "test fixture setup")]
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git is available for the test");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── default policy: Suggested stays off ──────────────────────────────

#[test]
fn default_run_leaves_a_suggested_only_project_unchanged() {
    let dir = e025_project("default-noop");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();

    let out = brink()
        .arg("fix")
        .arg("before.ink")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("0 fix(es) applied"),
        "E025 is Suggested-max; the default (no --suggested) policy must not \
         batch it: {stdout}"
    );

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, before, "default run must not touch the file");

    fs::remove_dir_all(&dir).ok();
}

// ── --suggested promotes and writes ──────────────────────────────────

#[test]
fn suggested_flag_promotes_e025_and_writes_the_import() {
    let dir = e025_project("suggested-write");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();
    let expected = expected_source();
    // A vacuous fixture (before == expected) would certify nothing.
    assert_ne!(
        before, expected,
        "fixture must actually change under the fix"
    );

    let out = brink()
        .args(["fix", "before.ink", "--suggested"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("1 fix(es) applied"),
        "expected exactly one applied fix: {stdout}"
    );

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(
        after, expected,
        "--suggested must reproduce the fixture's expected.ink"
    );

    fs::remove_dir_all(&dir).ok();
}

/// `--suggested E025` (an explicit code list) must have the same effect as
/// the bare flag for a project with only one Suggested-max fixer live.
#[test]
fn suggested_with_explicit_code_list_promotes_just_that_code() {
    let dir = e025_project("suggested-code-list");
    let expected = expected_source();

    let out = brink()
        .args(["fix", "before.ink", "--suggested", "E025"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, expected);

    fs::remove_dir_all(&dir).ok();
}

// ── the project's own `[fix]` table (no CLI flag at all) ──────────────
//
// Regression for the review finding on PR #3453: deleting `fix.rs`'s
// `if let Some(mode) = FixMode::from_config(config.effective_fix_policy(...))`
// block left every test above still green, because none of them exercises
// `brink.toml`'s `[fix]` table without also passing `--suggested` (which
// promotes independently of it). These do not pass `--suggested` at all.

#[test]
fn project_fix_table_auto_promotes_e025_with_no_flags_at_all() {
    let dir = e025_project_with_fix_table("fix-table-auto", "E025 = \"auto\"");
    let expected = expected_source();

    let out = brink()
        .arg("fix")
        .arg("before.ink")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("1 fix(es) applied"),
        "[fix] E025 = \"auto\" must batch it with no --suggested flag: {stdout}"
    );

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, expected);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn project_fix_table_off_yields_zero_fixes_even_with_code_naming_it() {
    let dir = e025_project_with_fix_table("fix-table-off", "E025 = \"off\"");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();

    let out = brink()
        .args(["fix", "before.ink", "--code", "E025"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("0 fix(es) applied"),
        "[fix] E025 = \"off\" must block it even when --code names it: {stdout}"
    );

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, before, "an off-policy code must not be written");

    fs::remove_dir_all(&dir).ok();
}

// ── bare --suggested must not override an explicit "off" entry ────────
//
// Regression for the review finding on PR #3453: `--suggested`'s bare form
// used to rewrite every Suggested-max fixer's mode to Auto unconditionally,
// including one the project explicitly turned off — contradicting
// `docs/book/src/toolchain/project-config.md` §Fix policy ("off — never
// offer or batch a fixer for this code in this project") and this crate's
// own `docs/book/src/toolchain/cli/fix.md` example comment.

#[test]
fn bare_suggested_does_not_override_an_explicit_off_entry() {
    let dir = e025_project_with_fix_table("off-bare-suggested", "E025 = \"off\"");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();

    let out = brink()
        .args(["fix", "before.ink", "--suggested"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("0 fix(es) applied"),
        "bare --suggested must not re-enable an explicit [fix] E025 = \"off\": {stdout}"
    );

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, before);

    fs::remove_dir_all(&dir).ok();
}

/// The explicit-code form of `--suggested` is the sanctioned widening
/// (`docs/autofix-spec.md` §6.2's own example, `--suggested E033`, names a
/// code) — unlike the bare form, it still wins over an `"off"` entry.
#[test]
fn explicit_suggested_code_still_overrides_an_off_entry() {
    let dir = e025_project_with_fix_table("off-explicit-suggested", "E025 = \"off\"");
    let expected = expected_source();

    let out = brink()
        .args(["fix", "before.ink", "--suggested", "E025"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("1 fix(es) applied"), "got: {stdout}");

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, expected);

    fs::remove_dir_all(&dir).ok();
}

// ── --dry-run writes nothing ──────────────────────────────────────────

#[test]
fn dry_run_prints_the_report_and_writes_nothing() {
    let dir = e025_project("dry-run");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();

    let out = brink()
        .args(["fix", "before.ink", "--suggested", "--dry-run"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("1 fix(es) applied"),
        "the report is computed even though nothing is written: {stdout}"
    );

    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, before, "--dry-run must not touch the file");

    fs::remove_dir_all(&dir).ok();
}

// ── --diff emits a git-apply-able patch, and writes nothing ──────────

#[test]
fn diff_flag_emits_a_git_apply_able_patch_and_writes_nothing() {
    let dir = e025_project("diff");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();
    let expected = expected_source();

    let out = brink()
        .args(["fix", "before.ink", "--suggested", "--diff"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let diff = String::from_utf8(out.stdout).unwrap();
    assert!(
        diff.contains("IMPORT { ambush } FROM quest"),
        "diff should add the import line: {diff}"
    );

    // Nothing was written to disk.
    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, before, "--diff must not touch the file");

    // The diff is `git apply`-able against the untouched working tree, and
    // applying it reproduces the fixture's expected.ink exactly.
    fs::write(dir.join("out.diff"), &diff).unwrap();
    git(&dir, &["init", "-q", "."]);
    git(&dir, &["apply", "--check", "out.diff"]);
    git(&dir, &["apply", "out.diff"]);
    let applied = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(applied, expected);

    fs::remove_dir_all(&dir).ok();
}

// ── --diff composes with --dry-run; a cap_hit run always explains its
// exit code — regression for issue #3463 ("brink fix --diff silently
// ignores --dry-run and prints no report on cap_hit") ────────────────

/// `--diff --dry-run` together: the diff still goes to stdout as a clean
/// `git apply`-able patch, nothing is written to disk (as either flag alone
/// already promises), and the report — which `--diff` alone used to drop on
/// the floor entirely — is printed to stderr instead of being silently
/// discarded.
#[test]
fn diff_and_dry_run_compose_clean_patch_to_stdout_and_report_to_stderr() {
    let dir = e025_project("diff-dry-run");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();
    let expected = expected_source();

    let out = brink()
        .args(["fix", "before.ink", "--suggested", "--diff", "--dry-run"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let diff = String::from_utf8(out.stdout).unwrap();
    assert!(
        diff.contains("IMPORT { ambush } FROM quest"),
        "diff should add the import line: {diff}"
    );
    assert!(
        !diff.contains("fix(es) applied"),
        "stdout must stay a clean patch — the report must not land there: {diff}"
    );

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("1 fix(es) applied"),
        "--diff must not silently swallow --dry-run's report — it goes to \
         stderr instead: {stderr}"
    );

    // Nothing was written to disk — both flags promise this independently.
    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(
        after, before,
        "--diff --dry-run together must still write nothing"
    );

    // stdout is a clean, `git apply`-able patch against the untouched tree.
    fs::write(dir.join("out.diff"), &diff).unwrap();
    git(&dir, &["init", "-q", "."]);
    git(&dir, &["apply", "--check", "out.diff"]);
    git(&dir, &["apply", "out.diff"]);
    let applied = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(applied, expected);

    fs::remove_dir_all(&dir).ok();
}

/// A capped run (`--max-rounds 0` with a fix still admitted) must exit `1`
/// and explain why on stderr, even under `--diff` — before this fix, the
/// `--diff` branch never called `print_report` at all, so a capped run
/// exited `1` with nothing on stdout or stderr explaining it (the bug
/// `--diff` in this issue's title describes).
#[test]
fn diff_with_cap_hit_exits_1_and_explains_why_on_stderr() {
    let dir = e025_project("diff-cap-hit");

    let out = brink()
        .args([
            "fix",
            "before.ink",
            "--suggested",
            "--diff",
            "--max-rounds",
            "0",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "a round cap of 0 with an admitted fix must hit the cap: stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("cap hit"),
        "the exit-1 must be explained on stderr, not silent: {stderr}"
    );
    assert!(
        stderr.contains("E025"),
        "the report should name the diagnostic still admitting a fix: {stderr}"
    );

    fs::remove_dir_all(&dir).ok();
}

// ── --code restricts the selection, and rejects an unknown code ─────

#[test]
fn code_flag_restricts_the_selection() {
    let dir = e025_project("code-restrict");

    // A code that has no diagnostic in this project: nothing is admitted,
    // even with --suggested promoting everything else.
    let out = brink()
        .args(["fix", "before.ink", "--suggested", "--code", "E080"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("0 fix(es) applied"), "got: {stdout}");

    // Restricting to the code that IS present still fixes it.
    let out = brink()
        .args(["fix", "before.ink", "--suggested", "--code", "E025"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("1 fix(es) applied"), "got: {stdout}");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn unknown_code_is_a_hard_error_not_a_silent_no_op() {
    let dir = e025_project("unknown-code");

    let out = brink()
        .args(["fix", "before.ink", "--code", "E9999"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("E9999"),
        "the error should name the bad code: {stderr}"
    );

    fs::remove_dir_all(&dir).ok();
}

// ── --placeholder is informational only ──────────────────────────────

#[test]
fn placeholder_flag_does_not_change_the_write_outcome() {
    let dir = e025_project("placeholder");
    let expected = expected_source();

    let out = brink()
        .args(["fix", "before.ink", "--suggested", "--placeholder"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // No Placeholder-tier diagnostic exists in this fixture, so the flag is a
    // pure addition to the (empty) placeholder listing — the E025 fix still
    // applies exactly as it does without the flag.
    let after = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(after, expected);

    fs::remove_dir_all(&dir).ok();
}

/// Regression for the review finding on PR #3453: `--placeholder`'s listing
/// used to be written to the same stdout handle `--diff` had already written
/// the patch to, so `brink fix story.ink --diff --placeholder | git apply`
/// (a pipeline `docs/book/src/toolchain/cli/fix.md` advertises) would
/// corrupt the patch the moment a `Placeholder`-tier fixer existed. No such
/// fixer is registered yet, so this can't reproduce the corruption
/// end-to-end — but it does pin that stdout carries only the diff even with
/// `--placeholder` set.
#[test]
fn diff_and_placeholder_together_leaves_stdout_as_a_clean_patch() {
    let dir = e025_project("diff-placeholder");
    let expected = expected_source();

    let out = brink()
        .args([
            "fix",
            "before.ink",
            "--suggested",
            "--diff",
            "--placeholder",
        ])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let diff = String::from_utf8(out.stdout).unwrap();

    fs::write(dir.join("out.diff"), &diff).unwrap();
    git(&dir, &["init", "-q", "."]);
    git(&dir, &["apply", "--check", "out.diff"]);
    git(&dir, &["apply", "out.diff"]);
    let applied = fs::read_to_string(dir.join("before.ink")).unwrap();
    assert_eq!(
        applied, expected,
        "--placeholder must not corrupt --diff's patch on stdout"
    );

    fs::remove_dir_all(&dir).ok();
}
