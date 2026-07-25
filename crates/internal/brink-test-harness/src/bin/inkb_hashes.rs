//! Corpus byte-identity harness (a permanent tool — written ad hoc three
//! times before landing: phase-0 slice C, FG-4c, and next FG-4d).
//!
//! Compiles every corpus `story.ink` case through the brink compiler,
//! serializes the resulting `StoryData` to `.inkb` bytes, and prints
//! `relative/path<TAB>hex-hash` lines (sorted). Run on `origin/main` to
//! capture a baseline, then on the FG-4c branch; the two outputs must be
//! byte-for-byte identical.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    clippy::unwrap_used
)]

use std::path::{Path, PathBuf};

fn hash_bytes(bytes: &[u8]) -> u64 {
    // FNV-1a 64-bit — stable, dependency-free.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Recursively find directories containing `story.ink`, via the shared
/// [`brink_source_tree::Walk`] (issue #1433) — deterministic order, and the
/// ignored-directory policy applied by construction. The caller sorts.
fn collect_cases(root: &Path, out: &mut Vec<PathBuf>) {
    if root.join("story.ink").exists() {
        out.push(root.to_path_buf());
    }
    for entry in brink_source_tree::Walk::new(root).flatten() {
        if entry.is_dir() && entry.path().join("story.ink").exists() {
            out.push(entry.into_path());
        }
    }
}

fn main() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // workspace root is manifest/../../..
    let ws_root = Path::new(manifest)
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf();
    let tests_root = ws_root.join("tests");

    let mut cases = Vec::new();
    collect_cases(&tests_root, &mut cases);
    cases.sort();

    let mut lines: Vec<String> = Vec::new();
    for case in &cases {
        let ink = case.join("story.ink");
        let rel = case.strip_prefix(&ws_root).unwrap_or(case);
        match brink_compiler::compile_path(&ink) {
            Ok(output) => {
                let mut buf = Vec::new();
                brink_format::write_inkb(&output.data, &mut buf);
                lines.push(format!("{}\t{:016x}", rel.display(), hash_bytes(&buf)));
            }
            Err(e) => {
                lines.push(format!("{}\tCOMPILE_ERR:{}", rel.display(), e));
            }
        }
    }
    lines.sort();
    for l in &lines {
        println!("{l}");
    }
    eprintln!("{} cases hashed", lines.len());
}
