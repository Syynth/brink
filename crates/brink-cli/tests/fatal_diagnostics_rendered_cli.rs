//! Regression test for issue #1957: a fatal `CompileError::Diagnostics`
//! must render its resolved codes/messages, not just the bare
//! `"N diagnostic(s) prevented compilation"` count.
//!
//! Before #1957, `crates/brink-cli/src/main.rs`'s `log_diagnostic` renderer
//! was wired only to `CompileOutput::warnings` (the non-fatal set, printed
//! on an otherwise-successful compile) — nothing destructured
//! `CompileError::Diagnostics`, the fatal payload, so a failing compile's
//! `tracing::error!("{e}")` only ever printed `CompileError`'s `Display`:
//! the count and nothing else. These tests exercise both halves of the
//! shared `compile_entry` seam (`main.rs`'s own doc comment: every `brink
//! compile`/`convert`/`play`/`replay`/`export-xliff` invocation flows
//! through it) — the native (`.brink`) path via `compile_entry` directly,
//! and the ink (`.ink`) path via `load_story_data` — and assert the
//! resolved diagnostic code and message both appear in the process's
//! `tracing`-rendered log output. Both runs set `RUST_LOG=error` (`main`'s
//! `tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env())`
//! emits nothing without it, the same convention
//! `directory_prune_escape_hatch_cli.rs` uses for its own `RUST_LOG=warn`
//! assertions) and read `output.stdout` — `tracing_subscriber::fmt()`'s
//! default writer, confirmed by direct run, is stdout, not stderr.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

/// A fresh, uniquely-named project directory under the OS temp dir.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn project_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "brink-fatal-diagnostics-rendered-cli-{}-{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `brink compile` on a native (`.brink`) entry with an unresolved divert
/// target must print the resolved `[E024]` code and message, not only the
/// bare count — exercises `compile_entry`'s own error path directly
/// (`Commands::Compile` -> `run_compile` -> `compile_entry`).
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[test]
fn compile_renders_fatal_diagnostic_codes_for_native_entry() {
    let dir = project_dir("native-compile");
    let entry = dir.join("main.brink");
    fs::write(&entry, "flow main() {\n  -> nonexistent_knot\n}\n").unwrap();

    let output = brink()
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.join("main.inkb"))
        .env("RUST_LOG", "error")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "compile of an entry with an unresolved divert target must fail"
    );
    let logs = String::from_utf8_lossy(&output.stdout);
    assert!(
        logs.contains("[E024]"),
        "expected the resolved E024 code in the log output, got: {logs}"
    );
    assert!(
        logs.contains("nonexistent_knot"),
        "expected the diagnostic's own message (naming the unresolved \
         target) in the log output, not just the bare count, got: {logs}"
    );
    assert!(
        logs.contains("diagnostic(s) prevented compilation"),
        "the summary count line should still print, as a trailer under \
         the individual diagnostics rather than the only output: {logs}"
    );
}

/// `brink convert` on an `.ink` entry with an unresolved divert target must
/// also render the resolved diagnostic — proves the fix covers the shared
/// seam (`load_story_data`), not just the `Commands::Compile` subcommand
/// this issue was filed against.
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[test]
fn convert_renders_fatal_diagnostic_codes_for_ink_entry() {
    let dir = project_dir("ink-convert");
    let entry = dir.join("main.ink");
    fs::write(&entry, "-> nonexistent_knot\n").unwrap();

    let output = brink()
        .arg("convert")
        .arg(&entry)
        .arg("-o")
        .arg(dir.join("main.inkb"))
        .env("RUST_LOG", "error")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "convert of an entry with an unresolved divert target must fail"
    );
    let logs = String::from_utf8_lossy(&output.stdout);
    assert!(
        logs.contains("[E024]"),
        "expected the resolved E024 code in the log output, got: {logs}"
    );
    assert!(
        logs.contains("nonexistent_knot"),
        "expected the diagnostic's own message in the log output, got: {logs}"
    );
}
