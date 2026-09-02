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
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
fn suggested_flag_promotes_e025_and_writes_the_import() {
    let dir = e025_project("suggested-write");
    let before = fs::read_to_string(dir.join("before.ink")).unwrap();
    let expected = expected_source();
    // A vacuous fixture (before == expected) would certify nothing.
    assert_ne!(before, expected, "fixture must actually change under the fix");

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
    assert_eq!(after, expected, "--suggested must reproduce the fixture's expected.ink");

    fs::remove_dir_all(&dir).ok();
}

/// `--suggested E025` (an explicit code list) must have the same effect as
/// the bare flag for a project with only one Suggested-max fixer live.
#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
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

// ── --dry-run writes nothing ──────────────────────────────────────────

#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
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

// ── --code restricts the selection, and rejects an unknown code ─────

#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
