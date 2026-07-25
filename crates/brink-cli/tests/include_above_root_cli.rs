//! Regression test for issue #1356: an ink `INCLUDE` that escapes the
//! resolved project root must still compile.
//!
//! Before the #1306 `Environment` producer, the CLI read includes through a
//! direct-filesystem closure, so `INCLUDE ../shared.ink` resolved fine. The
//! producer mount drains the project root into an in-memory tree, which by
//! construction cannot contain a file *above* that root — so the same story
//! regressed to a compile failure. `DrainedRoot` restores the old behavior by
//! reading through to disk on a key miss (see `drain_project_tree`).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

/// A fresh, uniquely-named wrapper directory under the OS temp dir.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn wrapper_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "brink-include-above-root-{}-{tag}",
        std::process::id()
    ));
    // Start clean so a rerun can't observe a previous run's fixture.
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `wrap/shared.ink` sits ABOVE `wrap/proj/` — the resolved root, pinned
/// there by its `brink.toml`. `proj/main.ink` reaches up to include it.
#[test]
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn compile_resolves_an_include_above_the_project_root() {
    let wrap = wrapper_dir("compile");
    let proj = wrap.join("proj");
    fs::create_dir_all(&proj).unwrap();

    fs::write(
        wrap.join("shared.ink"),
        "=== shared_knot ===\nShared content.\n-> END\n",
    )
    .unwrap();
    // The `brink.toml` is what makes `proj/` the resolved source root, so
    // `../shared.ink` genuinely escapes it.
    fs::write(proj.join("brink.toml"), "[project]\n").unwrap();
    let story = proj.join("main.ink");
    fs::write(&story, "INCLUDE ../shared.ink\nHello.\n-> shared_knot\n").unwrap();

    let out = brink().arg("compile").arg(&story).output().unwrap();

    assert!(
        out.status.success(),
        "an INCLUDE above the project root must still compile (#1356)\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
