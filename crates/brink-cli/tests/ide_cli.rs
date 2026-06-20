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

/// A project with an unused constant (`UNUSED_MAX`) and an error-free body.
const UNUSED_FIXTURE: &str = "VAR gold = 0\nCONST UNUSED_MAX = 100\n\n-> intro\n\n=== intro ===\nYou have {gold} gold.\n-> END\n";

/// A project with an unresolved divert (a compile error).
const ERR_FIXTURE: &str = "-> nowhere\n\n=== intro ===\nHello.\n-> END\n";

/// Write `content` to a unique temp file and return its path.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn write(tag: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("brink-ide-{}-{}.ink", std::process::id(), tag));
    fs::write(&path, content).unwrap();
    path
}

/// Write the default multi-symbol fixture and return its path.
fn fixture(tag: &str) -> PathBuf {
    write(tag, FIXTURE)
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

#[test]
fn at_addressing_resolves_cursor_to_definition() {
    let f = fixture("at");
    // Line 7 is `~ gold = gold + 5`; column 3 sits on the first `gold`, a use of
    // the VAR declared on line 1 — def should resolve there.
    let at = format!("{}:7:3", f.display());
    let out = brink()
        .args(["ide", "def", "--at", &at, "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("variable "), "got: {stdout}");
    assert!(
        stdout.contains(":1:"),
        "VAR is declared on line 1: {stdout}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn symbols_outline_lists_knots_and_globals() {
    let f = fixture("symbols");
    let out = brink()
        .args(["ide", "symbols", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("knot intro"), "got: {s}");
    assert!(s.contains("variable gold"), "got: {s}");
    fs::remove_file(&f).ok();
}

#[test]
fn symbols_search_is_a_flat_name_filter() {
    let f = fixture("search");
    let out = brink()
        .args(["ide", "symbols", "--search", "gold", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("gold"), "got: {s}");
    assert!(
        !s.contains("knot"),
        "search is flat, only gold matches: {s}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn unused_reports_dead_symbols_and_exits_nonzero() {
    let f = write("unused", UNUSED_FIXTURE);
    let out = brink()
        .args(["ide", "unused", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("UNUSED_MAX"),
        "the unused constant should be reported"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn check_clean_project_is_silent_and_zero() {
    let f = fixture("check-ok");
    let out = brink()
        .args(["ide", "check", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "clean project emits nothing");
    fs::remove_file(&f).ok();
}

#[test]
fn check_reports_errors_and_exits_nonzero() {
    let f = write("check-err", ERR_FIXTURE);
    let out = brink()
        .args(["ide", "check", "--format", "json", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["severity"], "error");
    assert!(v[0]["code"].as_str().unwrap().starts_with('E'));
    fs::remove_file(&f).ok();
}
