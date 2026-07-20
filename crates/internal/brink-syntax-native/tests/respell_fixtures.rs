//! Parse-clean gate for the hand-respelled `.brink` fixture corpus
//! (`docs/b0-findings.md` NF-5, "the differential method" — recommendation
//! (c), with (b) as the flagship exit gate for B0.7/B0.8).
//!
//! These fixtures live under `tests/tier1-brink-respell/<case>/story.brink`
//! at the repo root (see that directory's `README.md` for what they are and
//! why). This crate is B0.5's grammar skeleton: no HIR lowering exists yet,
//! so **parsing with zero errors is the only claim this test makes** — it
//! is not an episode-identity check (that's B0.7+, per NF-5's rider).
//!
//! Every fixture is walked generically (rather than one `#[test]` per case)
//! so a new fixture directory is picked up automatically without editing
//! this file — the loop still reports which specific fixture failed via
//! the panic message.
//!
//! Helper functions here return `Result` rather than unwrapping/panicking
//! directly (workspace lint: `unwrap_used`/`expect_used`/`panic` are denied;
//! `clippy.toml`'s `allow-unwrap-in-tests`/`allow-expect-in-tests` only
//! exempt code inside `#[test]`-attributed fn bodies, not plain helpers a
//! test happens to call).

use std::fs;
use std::path::{Path, PathBuf};

use brink_syntax_native::parse;

/// `tests/tier1-brink-respell` at the repo root, resolved relative to this
/// crate's manifest dir (`crates/internal/brink-syntax-native`) rather than
/// the process cwd, so `cargo test` works regardless of invocation
/// directory.
fn fixtures_root() -> Result<PathBuf, String> {
    let candidate =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/tier1-brink-respell");
    candidate
        .canonicalize()
        .map_err(|e| format!("tests/tier1-brink-respell must exist at the repo root ({e})"))
}

/// Every `story.brink` found directly under a fixture-root subdirectory,
/// sorted for deterministic test output (the repo's own determinism rule —
/// never depend on directory-read order).
fn fixture_files() -> Result<Vec<PathBuf>, String> {
    let root = fixtures_root()?;
    let entries = fs::read_dir(&root).map_err(|e| format!("reading {}: {e}", root.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.is_dir())
        .map(|dir| dir.join("story.brink"))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    Ok(files)
}

#[test]
fn every_respelled_fixture_parses_with_zero_errors() {
    let files = fixture_files().unwrap();
    assert!(
        !files.is_empty(),
        "expected at least one story.brink fixture under {}",
        fixtures_root().unwrap().display()
    );

    let mut failures = Vec::new();
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        let parsed = parse(&source);

        // Lossless round-trip is the parser's baseline invariant (mirrors
        // `parser::tests::assert_lossless`) — check it here too, since a
        // fixture that silently dropped text would be a much worse bug
        // than a reported parse error.
        if parsed.syntax().text().to_string() != source {
            failures.push(format!(
                "{}: lossy round-trip (CST text != source)",
                path.display()
            ));
            continue;
        }

        if !parsed.errors().is_empty() {
            failures.push(format!(
                "{}: {} parse error(s): {:?}",
                path.display(),
                parsed.errors().len(),
                parsed.errors()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "the following respelled fixtures did not parse clean:\n{}",
        failures.join("\n")
    );
}

/// Every fixture directory must carry its provenance manifest (ink source +
/// oracle case pointer) — a `story.brink` with no `manifest.toml` breaks the
/// pairing this corpus exists to hold (NF-5's rider: "respelled cases live
/// beside their ink twins in-tree so drift is reviewable").
#[test]
fn every_fixture_has_a_manifest() {
    for path in fixture_files().unwrap() {
        let manifest = path.with_file_name("manifest.toml");
        assert!(
            manifest.is_file(),
            "{} has no sibling manifest.toml",
            path.display()
        );
    }
}
