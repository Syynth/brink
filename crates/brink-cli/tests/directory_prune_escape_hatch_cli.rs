//! Integration tests for issue #1407: the directory-prune policy's escape
//! hatch (`[project] unprune-dirs` in `brink.toml`) and its silent-skip
//! diagnostic, exercised end-to-end through `brink compile` on a real native
//! (`.brink`) project tree.
//!
//! `RealFs`'s discovery walk never descends into `target/`, `.git/`, or
//! `node_modules/` by default (issues #1381/#1433) — before #1407 that was
//! absolute: a project keeping real sources under one of those names got no
//! file and no error. These tests prove both halves of the fix: a
//! `.brink` file under a pruned directory name is invisible by default, is
//! admitted once `unprune-dirs` names it, and a pruned directory that
//! shallowly holds a `.brink` file gets a `tracing::warn!` naming it and
//! suggesting the fix — the same `RUST_LOG=warn`-gated channel
//! `project_config_cli.rs` already proved reaches CLI stdout.

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
        "brink-directory-prune-escape-hatch-cli-{}-{tag}",
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

/// A minimal, valid native entry — self-contained, so a compile only fails
/// because of something a fixture deliberately adds elsewhere in the tree.
const VALID_ENTRY: &str = "flow main() {\n  Hello. -> END\n}\n";

/// Deliberately unparseable `.brink` source — used as a tripwire: if
/// discovery admits this file, the compile fails; if discovery prunes it
/// away, the compile succeeds despite it existing on disk.
const UNPARSEABLE: &str = "flow ( this is not valid brink syntax at all {{{\n";

// ── silent skip, by default (control) ───────────────────────────────────

/// Control case, no `unprune-dirs`: an unparseable `.brink` file sitting
/// directly under `node_modules/` is pruned away entirely — the compile
/// succeeds despite it existing on disk, proving the file was never
/// discovered (the exact silent-skip issue #1407 was filed about).
#[test]
fn no_config_a_brink_file_under_node_modules_is_silently_pruned() {
    let dir = project_dir("control-no-config");
    write(&dir, "brink.toml", "[project]\n");
    let entry = write(&dir, "main.brink", VALID_ENTRY);
    write(&dir, "node_modules/vendor-ink/bad.brink", UNPARSEABLE);

    let out = brink().arg("compile").arg(&entry).output().unwrap();

    assert!(
        out.status.success(),
        "with no unprune-dirs, node_modules/ must still be pruned entirely: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

// ── escape hatch ─────────────────────────────────────────────────────────

/// `[project] unprune-dirs = ["node_modules"]` admits the same otherwise-
/// pruned `.brink` file: the compile now fails on the deliberately
/// unparseable source, proving discovery actually walked into
/// `node_modules/` this time.
#[test]
fn unprune_dirs_admits_a_brink_file_under_node_modules() {
    let dir = project_dir("escape-hatch-admits");
    write(
        &dir,
        "brink.toml",
        "[project]\nunprune-dirs = [\"node_modules\"]\n",
    );
    let entry = write(&dir, "main.brink", VALID_ENTRY);
    write(&dir, "node_modules/vendor-ink/bad.brink", UNPARSEABLE);

    let out = brink().arg("compile").arg(&entry).output().unwrap();

    assert!(
        !out.status.success(),
        "unprune-dirs = [\"node_modules\"] must admit node_modules/, so the \
         unparseable file inside it must now fail the compile"
    );

    fs::remove_dir_all(&dir).ok();
}

/// `unprune-dirs` narrows to exactly the names it lists: `node_modules/` is
/// admitted, but a sibling `target/` (not named) stays pruned — proving this
/// is a per-name override, not a blanket "stop pruning everything" switch.
#[test]
fn unprune_dirs_does_not_widen_an_unnamed_sibling_ignored_dir() {
    let dir = project_dir("escape-hatch-narrow");
    write(
        &dir,
        "brink.toml",
        "[project]\nunprune-dirs = [\"node_modules\"]\n",
    );
    let entry = write(&dir, "main.brink", VALID_ENTRY);
    // Admitted and valid, so it doesn't fail the compile on its own.
    write(&dir, "node_modules/vendor-ink/extra.brink", VALID_ENTRY);
    // Not named by unprune-dirs, so target/ must stay pruned; if it didn't,
    // this unparseable file would fail the compile.
    write(&dir, "target/debug/bad.brink", UNPARSEABLE);

    let out = brink().arg("compile").arg(&entry).output().unwrap();

    assert!(
        out.status.success(),
        "target/ must stay pruned even though node_modules/ was unpruned: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    fs::remove_dir_all(&dir).ok();
}

// ── diagnostic ───────────────────────────────────────────────────────────

/// With no `unprune-dirs`, a `.brink` file sitting directly inside a pruned
/// `node_modules/` still lets the compile succeed (the file is silently
/// invisible, same as the control case above) — but now warns, naming the
/// pruned directory and suggesting the `unprune-dirs` fix, on the same
/// `RUST_LOG=warn`-gated `tracing::warn!` channel
/// `project_config_cli.rs`'s `compile_unknown_config_key_is_a_warning_not_a_
/// compile_failure` already proved reaches CLI stdout.
#[test]
fn pruned_dir_holding_a_brink_file_warns_naming_it_and_the_fix() {
    let dir = project_dir("diagnostic-warns");
    write(&dir, "brink.toml", "[project]\n");
    let entry = write(&dir, "main.brink", VALID_ENTRY);
    // Valid (not the unparseable tripwire) — the point here is the warning,
    // not another compile failure.
    write(&dir, "node_modules/vendor.brink", VALID_ENTRY);

    let out = brink()
        .arg("compile")
        .arg(&entry)
        .env("RUST_LOG", "warn")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "a pruned directory holding a source file must still compile \
         successfully by default, just warn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("node_modules"),
        "warning must name the pruned directory, got stdout: {stdout}"
    );
    assert!(
        stdout.contains("unprune-dirs"),
        "warning must point at the unprune-dirs fix, got stdout: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// The diagnostic must also fire for the `node_modules/<package>/lib.brink`
/// shape — one level below the pruned directory's immediate children —
/// which is how an npm-style dependency tree actually lays out vendored
/// source (issue #1407's review finding: the original shallow,
/// immediate-children-only scan missed exactly this shape, even though this
/// file's own `node_modules/vendor-ink/bad.brink` escape-hatch fixtures
/// above already use it).
#[test]
fn pruned_dir_holding_a_nested_brink_file_warns_naming_the_pruned_directory() {
    let dir = project_dir("diagnostic-warns-nested");
    write(&dir, "brink.toml", "[project]\n");
    let entry = write(&dir, "main.brink", VALID_ENTRY);
    write(&dir, "node_modules/vendor-ink/lib.brink", VALID_ENTRY);

    let out = brink()
        .arg("compile")
        .arg(&entry)
        .env("RUST_LOG", "warn")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "a pruned directory holding a nested source file must still compile \
         successfully by default, just warn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("node_modules"),
        "warning must name the pruned directory even though the file is nested \
         one level deeper, got stdout: {stdout}"
    );
    assert!(
        stdout.contains("unprune-dirs"),
        "warning must point at the unprune-dirs fix, got stdout: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// Once `unprune-dirs` names the directory, the same tree no longer warns —
/// the diagnostic is for an *unaddressed* prune, not a standing nag about a
/// directory the author already made a deliberate decision about.
#[test]
fn unprune_dirs_silences_the_diagnostic_for_the_directory_it_names() {
    let dir = project_dir("diagnostic-silenced");
    write(
        &dir,
        "brink.toml",
        "[project]\nunprune-dirs = [\"node_modules\"]\n",
    );
    let entry = write(&dir, "main.brink", VALID_ENTRY);
    write(&dir, "node_modules/vendor.brink", VALID_ENTRY);

    let out = brink()
        .arg("compile")
        .arg(&entry)
        .env("RUST_LOG", "warn")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("node_modules"),
        "an unpruned (admitted) directory must not also be reported as a \
         silent-skip warning, got stdout: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}
