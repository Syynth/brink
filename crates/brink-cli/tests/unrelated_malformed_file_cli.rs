//! Regression test for issue #1357: an unrelated malformed/non-UTF-8 file
//! anywhere under the resolved project root must not fail an otherwise-valid
//! compile.
//!
//! Before #1357, `brink compile` drained the *whole* project root eagerly —
//! reading every `.ink`/`.brink`/`brink.toml` file under it up front — so a
//! single malformed file anywhere in the tree, even one never reached by the
//! entry's `INCLUDE` graph, failed the compile. The fix replaces the drain
//! with a lazy `RealFs` mount: `list` enumerates keys by stat only, and
//! `read` is only ever called for a key the compile actually needs.

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
        "brink-unrelated-malformed-file-{}-{tag}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A malformed (non-UTF-8) `.ink` file sits next to a valid, self-contained
/// entry story. Nothing in the entry's `INCLUDE` graph reaches the malformed
/// file, so the compile must succeed.
#[test]
fn compile_succeeds_despite_an_unrelated_non_utf8_ink_file() {
    let dir = project_dir("ink-sibling");

    let story = dir.join("main.ink");
    fs::write(&story, "Hello, world.\n-> END\n").expect("write main.ink");

    // Invalid UTF-8 bytes with a `.ink` extension, never INCLUDEd by
    // main.ink.
    fs::write(dir.join("unrelated.ink"), [0xFF, 0xFE, 0xFD]).expect("write unrelated.ink");

    let out = brink().arg("compile").arg(&story).output().unwrap();

    assert!(
        out.status.success(),
        "an unrelated malformed file must not fail an otherwise-valid compile (#1357)\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Same as above, but the malformed file is a `.brink` file and the entry is
/// `.ink` — still not part of the ink `INCLUDE` graph, so it must not be
/// read either.
#[test]
fn compile_succeeds_despite_an_unrelated_non_utf8_brink_file() {
    let dir = project_dir("brink-sibling");

    let story = dir.join("main.ink");
    fs::write(&story, "Hello, world.\n-> END\n").expect("write main.ink");

    fs::write(dir.join("unrelated.brink"), [0xFF, 0xFE, 0xFD]).expect("write unrelated.brink");

    let out = brink().arg("compile").arg(&story).output().unwrap();

    assert!(
        out.status.success(),
        "an unrelated malformed .brink file must not fail an .ink compile (#1357)\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A nested subdirectory containing a malformed file must not affect a
/// compile whose `INCLUDE` graph never descends into it either.
#[test]
fn compile_succeeds_despite_a_malformed_file_in_a_nested_unrelated_directory() {
    let dir = project_dir("nested-sibling");

    let story = dir.join("main.ink");
    fs::write(&story, "Hello, world.\n-> END\n").expect("write main.ink");

    let nested = dir.join("unused");
    fs::create_dir_all(&nested).expect("mkdir unused");
    fs::write(nested.join("bad.ink"), [0xFF, 0xFE, 0xFD]).expect("write nested bad.ink");

    let out = brink().arg("compile").arg(&story).output().unwrap();

    assert!(
        out.status.success(),
        "a malformed file in an unreachable nested directory must not fail the compile (#1357)\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
