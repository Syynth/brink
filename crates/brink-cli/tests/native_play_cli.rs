//! Integration tests for issue #1949: `brink play`/`convert` accepting a
//! `.brink` entry directly, routed through the same `compile_entry` path
//! `brink compile` uses (`load_story_data` in `crates/brink-cli/src/main.rs`)
//! rather than requiring a `brink compile … -o … && brink play …` two-step.
//!
//! The trap the issue calls out: a `.brink` entry resolves its source root
//! *and project universe* from `brink.toml` discovery — every `.brink` file
//! under the discovered root is part of the compile (tree-is-universe),
//! unlike `.ink`'s `INCLUDE`-reachable set. Treating the entry path as a
//! standalone file (e.g. reading + parsing just that one file) would compile
//! successfully even with an unparseable sibling `.brink` file sitting right
//! next to it — `play_fails_on_an_unparseable_sibling_brink_file` is the
//! regression guard for that: it fails identically to
//! `directory_prune_escape_hatch_cli.rs`'s own tripwire tests if reverted to
//! standalone-file compilation.

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
        "brink-native-play-cli-{}-{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn write(dir: &std::path::Path, rel: &str, content: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

/// A minimal, choice-free native entry, so batch mode with an empty input
/// file runs it start-to-finish with no interaction required.
const GREETING_ENTRY: &str = "flow main() {\n  Hello, native world. -> END\n}\n";

/// Deliberately unparseable `.brink` source — the same tripwire
/// `directory_prune_escape_hatch_cli.rs` uses: if discovery (correctly)
/// admits this file as part of the project universe, the compile fails.
const UNPARSEABLE: &str = "flow ( this is not valid brink syntax at all {{{\n";

/// Regression for issue #1949: `brink play scene.brink` must compile and run
/// directly — no `brink compile … -o … && brink play …` two-step. An empty
/// `--input` choices file drives batch mode without needing a live terminal,
/// so this exercises `load_story_data`'s `.brink` branch end-to-end through
/// the real binary.
#[test]
fn play_runs_a_native_brink_entry_directly() {
    let dir = project_dir("play-direct");
    let entry = write(&dir, "scene.brink", GREETING_ENTRY);
    let choices = write(&dir, "choices.txt", "");

    let out = brink()
        .arg("play")
        .arg(&entry)
        .args(["--input"])
        .arg(&choices)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "brink play must accept a .brink entry directly: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Hello, native world."),
        "expected the story's text on stdout, got: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// The tree-is-universe trap: a `.brink` entry's compile must discover +
/// include every `.brink` file under the project root, not just parse the
/// entry path standalone. An unparseable sibling file must fail the play
/// (proving it was part of the compile), not be silently ignored (which
/// would happen if `play` read + parsed only the single entry file, the bug
/// this issue's own author flagged losing a debugging round to).
#[test]
fn play_fails_on_an_unparseable_sibling_brink_file() {
    let dir = project_dir("play-tree-universe");
    let entry = write(&dir, "scene.brink", GREETING_ENTRY);
    write(&dir, "sibling.brink", UNPARSEABLE);

    let out = brink().arg("play").arg(&entry).output().unwrap();

    assert!(
        !out.status.success(),
        "an unparseable sibling .brink file under the same root must fail \
         the play, proving the whole tree was compiled — not just the \
         standalone entry file: stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

/// `brink convert` shares the same `load_story_data` loader as `play` — this
/// pins that the `.brink` branch reaches every mount through that shared
/// function, not just `play` specifically.
#[test]
fn convert_accepts_a_native_brink_entry() {
    let dir = project_dir("convert-direct");
    let entry = write(&dir, "scene.brink", GREETING_ENTRY);

    let out = brink().arg("convert").arg(&entry).output().unwrap();

    assert!(
        out.status.success(),
        "brink convert must accept a .brink entry directly: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

/// Unchanged-behavior guard: a genuinely unsupported extension must still be
/// rejected with the same descriptive error, now naming `.brink` alongside
/// `.ink`/`.inkb`/`.inkt` as an accepted format.
#[test]
fn play_rejects_an_unsupported_extension_naming_brink_as_accepted() {
    let dir = project_dir("play-unsupported-ext");
    let entry = write(&dir, "scene.txt", GREETING_ENTRY);

    let out = brink().arg("play").arg(&entry).output().unwrap();

    assert!(
        !out.status.success(),
        "an unsupported extension must still be rejected"
    );
    // `main.rs` reports command errors via `tracing::error!`, and
    // `tracing_subscriber::fmt()`'s default writer is stdout, not stderr —
    // confirmed by `project_config_cli.rs`'s own direct-invocation notes.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(".brink"),
        "the rejection message should name .brink as an accepted format now: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}
