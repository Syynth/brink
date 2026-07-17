//! Integration tests for `brink ide effects-diff` (T2-4, docs/effects-spec.md
//! §10, issue #863) — the drift-*visibility* tooling the sitting-2 ruling
//! names as the lockfile's replacement ("there is no drift policy... Drift
//! visibility is tooling: a `brink ide` effects-diff subcommand... and IDE
//! hover"). Exercised end-to-end against a real temp git repo: a committed
//! revision plus an uncommitted working-tree edit (the common case — "what
//! did my in-progress change do to the rows?"), and a rev-vs-rev diff (the
//! CI case — "what did this PR change?").

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A fresh temp git repo with `main.ink` committed at HEAD.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn init(tag: &str, initial_ink: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("brink-effects-diff-{}-{tag}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "--quiet"]);
        fs::write(dir.join("main.ink"), initial_ink).unwrap();
        git(&dir, &["add", "main.ink"]);
        git(&dir, &["commit", "--quiet", "-m", "initial"]);
        Self { dir }
    }

    fn head(&self) -> String {
        git(&self.dir, &["rev-parse", "HEAD"]).trim().to_string()
    }

    fn write(&self, content: &str) {
        fs::write(self.dir.join("main.ink"), content).unwrap();
    }

    fn commit(&self, msg: &str) {
        git(&self.dir, &["add", "main.ink"]);
        git(&self.dir, &["commit", "--quiet", "-m", msg]);
    }

    fn entry(&self) -> PathBuf {
        self.dir.join("main.ink")
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

const PURE_SPEND: &str = "\
VAR gold = 0

=== function spend() ===
~ return 1
";

const READING_SPEND: &str = "\
VAR gold = 0

=== function spend() ===
~ return gold
";

#[test]
fn no_drift_is_exit_0_and_empty() {
    let repo = Repo::init("clean", PURE_SPEND);
    let out = brink()
        .args(["ide", "effects-diff", "--base"])
        .arg(repo.head())
        .args(["-e"])
        .arg(repo.entry())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("no effect-row drift"), "{stdout}");
}

#[test]
fn a_new_read_in_the_working_tree_is_exit_1_and_reported() {
    let repo = Repo::init("read-drift", PURE_SPEND);
    let base = repo.head();
    repo.write(READING_SPEND); // uncommitted working-tree edit
    let out = brink()
        .args(["ide", "effects-diff", "--base"])
        .arg(&base)
        .args(["-e"])
        .arg(repo.entry())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("~ spend"), "{stdout}");
    assert!(stdout.contains("+ reads gold"), "{stdout}");
}

#[test]
fn rev_vs_rev_diff_does_not_touch_the_working_tree() {
    let repo = Repo::init("rev-vs-rev", PURE_SPEND);
    let base = repo.head();
    repo.write(READING_SPEND);
    repo.commit("spend now reads gold");
    let head = repo.head();
    // Working tree still matches the head commit (nothing uncommitted) —
    // proves --head reads from git, not the filesystem.
    let out = brink()
        .args(["ide", "effects-diff", "--base"])
        .arg(&base)
        .args(["--head"])
        .arg(&head)
        .args(["-e"])
        .arg(repo.entry())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("~ spend"), "{stdout}");
    assert!(stdout.contains("+ reads gold"), "{stdout}");
}

#[test]
fn json_format_reports_the_changed_atoms() {
    let repo = Repo::init("json", PURE_SPEND);
    let base = repo.head();
    repo.write(READING_SPEND);
    let out = brink()
        .args(["ide", "effects-diff", "--base"])
        .arg(&base)
        .args(["-e"])
        .arg(repo.entry())
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let spend = &json["spend"];
    assert_eq!(spend["status"], "changed");
    assert_eq!(spend["head"]["reads"], serde_json::json!(["gold"]));
    assert_eq!(spend["base"]["reads"], serde_json::json!([]));
}

#[test]
fn a_new_def_only_on_one_side_is_added_or_removed() {
    let repo = Repo::init("added", PURE_SPEND);
    let base = repo.head();
    repo.write(&format!(
        "{PURE_SPEND}\n=== function bonus() ===\n~ return 2\n"
    ));
    let out = brink()
        .args(["ide", "effects-diff", "--base"])
        .arg(&base)
        .args(["-e"])
        .arg(repo.entry())
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["bonus"]["status"], "added");
    assert!(json.get("spend").is_none(), "{json}");
}
