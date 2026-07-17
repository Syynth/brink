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
