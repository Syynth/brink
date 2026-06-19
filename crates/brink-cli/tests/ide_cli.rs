//! Integration tests for `brink ide` (epic #289, Phase 1).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const FIXTURE: &str = "\
VAR gold = 0

-> intro

=== intro ===
You have {gold} gold.
~ gold = gold + 5
-> shop

=== shop ===
Welcome. You still have {gold} gold.
-> END
";

/// Write the fixture to a unique temp file and return its path.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn fixture(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("brink-ide-{}-{}.ink", std::process::id(), tag));
    fs::write(&path, FIXTURE).unwrap();
    path
}

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

#[test]
fn def_resolves_a_knot() {
    let f = fixture("def");
    let out = brink()
        .args(["ide", "def", "intro", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("knot "), "got: {stdout}");
    assert!(stdout.contains(":5:"), "knot header is on line 5: {stdout}");
    fs::remove_file(&f).ok();
}

#[test]
fn references_counts_all_uses() {
    let f = fixture("refs");
    let out = brink()
        .args(["ide", "references", "gold", "--count", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "4");
    fs::remove_file(&f).ok();
}

#[test]
fn exists_exit_code() {
    let f = fixture("exists");
    // `intro` is referenced by `-> intro` → exit 0.
    let used = brink()
        .args(["ide", "references", "intro", "--exists", "-e"])
        .arg(&f)
        .status()
        .unwrap();
    assert!(used.success());
    fs::remove_file(&f).ok();
}

#[test]
fn unknown_symbol_is_usage_error() {
    let f = fixture("nope");
    let out = brink()
        .args(["ide", "def", "nope", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    fs::remove_file(&f).ok();
}

#[test]
fn json_output_is_parseable() {
    let f = fixture("json");
    let out = brink()
        .args([
            "ide",
            "references",
            "gold",
            "--include-decl",
            "--format",
            "json",
            "-e",
        ])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["count"], 5); // 4 uses + the declaration
    assert_eq!(v["kind"], "variable");
    fs::remove_file(&f).ok();
}
