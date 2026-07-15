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

fn collect_cases(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    if root.join("story.ink").exists() {
        out.push(root.to_path_buf());
    }
    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .map(|e| e.path())
        .collect();
    subdirs.sort();
    for sub in subdirs {
        collect_cases(&sub, out);
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
