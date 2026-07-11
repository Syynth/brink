//! Incremental == from-scratch fuzz harness (scripting-substrate spec §6.2).
//!
//! Applies a sequence of random-but-*seeded* edits to corpus-derived
//! projects through a long-lived `ProjectDb` and asserts, after every edit,
//! that the incremental `story_data()` is bit-identical to a fresh compile
//! of the same sources in a brand-new db. This is the safety net for
//! early-cutoff and query-purity mistakes: salsa owns dependency tracking,
//! but *we* own query purity.
//!
//! Determinism: a fixed-constant LCG drives every choice — no clock or OS
//! entropy anywhere. Edits never touch `INCLUDE` lines, so the discovered
//! file set stays fixed (a long-lived db intentionally keeps orphaned files;
//! a from-scratch discovery would not — that difference is pre-existing
//! LSP-vs-compiler behavior, not what this harness tests).
//!
//! Remove/re-add churn (#536): some steps on multi-file projects tombstone
//! a non-entry file (`remove_file`), pull `story_data()` with the file
//! absent (exercising missing-INCLUDE resolution against a tombstoned
//! input), then re-add the path with mutated content before the compare —
//! so every comparison still sees the full file set. The re-added path must
//! reuse its original `FileId` (durable path→id identity), which is exactly
//! what keeps the incremental ids aligned with the fresh db's and lets the
//! bit-identical assertion double as the tombstone-purity check: any stale
//! memo surviving the remove/re-add round-trip diverges from the fresh
//! compile.
//!
//! Bounded: `EDITS_PER_PROJECT` edits over a fixed project list; every step
//! is a small-project compile, so the whole test stays well under the
//! workspace's test-time budget and cannot hang (no loops without bounds).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use brink_driver::ProjectDb;

/// Seeded edits applied to each project.
const EDITS_PER_PROJECT: u64 = 16;

/// Corpus-derived projects under `tests/` (workspace root), chosen to cover
/// single-file, flat-include, and nested-include shapes.
const PROJECTS: &[&str] = &[
    "tier1/basics/I002-fogg-comforts-passepartout",
    "tier1/weave/I041-weave-gathers",
    "tier1/knots/I128-knot-stitch-gather-counts",
    "tier2/variabletext/sequence",
    "tier2/functions/using-function-and-increment-together",
    "tier3/misc/I024-includes",
    "tier3/misc/I025-nested-includes",
];

/// Tiny deterministic PRNG (LCG step, fixed constant seed per project) — the
/// same shape the `compile_bench` harness uses. No entropy sources.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap_or(0) % n.max(1)
    }
}

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // brink-test-harness
    p.pop(); // internal
    p.pop(); // crates
    p
}

/// Load every `.ink` file in a case directory as `filename → text`,
/// flat-keyed the way the corpus INCLUDEs reference them.
fn load_project(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "ink"))
        // `.ink.json` has extension "json"; plain `.ink` only.
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        files.insert(name, text);
    }
    assert!(
        files.contains_key("story.ink"),
        "{} has no story.ink",
        dir.display()
    );
    files
}

/// One seeded mutation of one file. Never creates or deletes `INCLUDE`
/// lines, so the project file set is stable across the whole run.
fn mutate(rng: &mut Lcg, original: &str, current: &str, step: u64) -> String {
    let lines: Vec<&str> = current.lines().collect();
    // Line indices that are safe to touch (not INCLUDE).
    let safe: Vec<usize> = (0..lines.len())
        .filter(|&i| !lines[i].trim_start().starts_with("INCLUDE"))
        .collect();
    if safe.is_empty() {
        return current.to_owned();
    }

    let op = rng.pick(6);
    let at = safe[rng.pick(safe.len())];
    let mut out: Vec<String> = lines.iter().map(|&l| l.to_owned()).collect();
    match op {
        // Insert a plain text line.
        0 => out.insert(at, format!("A fuzzed line, step {step}.")),
        // Delete a line (keep at least one line).
        1 => {
            if out.len() > 1 {
                out.remove(at);
            }
        }
        // Duplicate a line.
        2 => {
            let dup = out[at].clone();
            out.insert(at, dup);
        }
        // Whitespace-only churn: trailing spaces shift every later range
        // without changing structure — the resolution_index cutoff exercise.
        3 => out[at] = format!("{}  ", out[at]),
        // Insert a temp declaration (locals-in-index exercise; may be
        // invalid at top level, which exercises the diagnostics path).
        4 => out.insert(at, format!("~ temp fz_{step} = {}", rng.pick(9))),
        // Revert the file to its original content.
        _ => return original.to_owned(),
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Compile `files` in a fresh db (same insertion order as the incremental
/// db, so `FileId`s line up) and return its `CompileProduct`.
fn fresh_compile(files: &BTreeMap<String, String>) -> brink_driver::ProjectDb {
    let mut db = ProjectDb::new();
    for (path, text) in files {
        db.set_file(path, text.clone());
    }
    db.set_entry("story.ink").expect("story.ink present");
    db
}

/// Serialize a compiled story to `.inkb` bytes for the bit-identical check.
fn story_bytes(product: &brink_driver::CompileProduct) -> Option<Vec<u8>> {
    product.story.as_ref().map(|story| {
        let mut buf = Vec::new();
        brink_format::write_inkb(story, &mut buf);
        buf
    })
}

#[test]
fn incremental_story_data_equals_fresh_compile() {
    let root = workspace_root().join("tests");
    let mut churn_steps = 0usize;

    for (p_idx, project) in PROJECTS.iter().enumerate() {
        let dir = root.join(project);
        let originals = load_project(&dir);
        let mut current = originals.clone();
        let paths: Vec<String> = current.keys().cloned().collect();

        // Long-lived incremental db.
        let mut db = ProjectDb::new();
        for (path, text) in &current {
            db.set_file(path, text.clone());
        }
        db.set_entry("story.ink").expect("story.ink present");

        // Non-entry files are eligible for remove/re-add churn (#536); the
        // entry must stay present so `story_data()` is always comparable.
        let churnable: Vec<String> = paths
            .iter()
            .filter(|p| *p != "story.ink")
            .cloned()
            .collect();

        // Baseline: before any edit, incremental == fresh.
        let mut rng = Lcg::new(0x5EED_0501 + p_idx as u64);
        for step in 0..=EDITS_PER_PROJECT {
            if step > 0 {
                let churn = !churnable.is_empty() && rng.pick(4) == 0;
                if churn {
                    churn_steps += 1;
                    // Remove a file, pull the pipeline with it absent, then
                    // re-add it (with fresh content) before the compare.
                    let path = churnable[rng.pick(churnable.len())].clone();
                    let id_before = db.file_id(&path);
                    db.remove_file(&path);
                    assert_eq!(
                        db.file_id(&path),
                        None,
                        "{project}: removed file still visible"
                    );
                    // Exercise the query graph in the removed state — any
                    // INCLUDE of this path must now miss. Output is not
                    // compared here (the file set differs from `current`).
                    let _ = db.story_data().expect("entry still set");
                    let edited = mutate(&mut rng, &originals[&path], &current[&path], step);
                    current.insert(path.clone(), edited.clone());
                    let id_after = db.set_file(&path, edited);
                    assert_eq!(
                        Some(id_after),
                        id_before,
                        "{project}: re-added path must reuse its FileId (step {step})"
                    );
                } else {
                    let path = &paths[rng.pick(paths.len())];
                    let edited = mutate(&mut rng, &originals[path], &current[path], step);
                    current.insert(path.clone(), edited.clone());
                    db.update_file(path, edited);
                }
            }

            let incremental = db.story_data().expect("entry set");
            let fresh_db = fresh_compile(&current);
            let fresh = fresh_db.story_data().expect("entry set");

            assert_eq!(
                incremental, fresh,
                "{project}: incremental != from-scratch after edit {step}"
            );
            assert_eq!(
                story_bytes(incremental),
                story_bytes(fresh),
                "{project}: serialized StoryData differs after edit {step}"
            );
        }
    }

    // The remove/re-add coverage (#536) must actually run: the seeded
    // driver hits the churn arm on the multi-file projects every time, and
    // this assertion keeps that coverage from silently vanishing if the
    // project list or edit mix changes.
    assert!(
        churn_steps > 0,
        "no remove/re-add churn steps executed — #536 coverage lost"
    );
}
