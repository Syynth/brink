//! Benchmark-fixture compile guard (issue #2777, "the #2138 class").
//!
//! `cargo bench -p brink-runtime --features bench-counters`
//! (`crates/brink-runtime/benches/runtime.rs`) panicked from 2026-08-03
//! until PR #2770, undetected for two weeks, because
//! `struct-field-access-10k/story.ink`'s `VAR p: Point = 0` started
//! failing `E063` once #2085 tightened initializer-type checking — and
//! `.github/workflows/benchmarks.yml` is weekly/manual with a
//! `pull_request` path filter that only gates the `gen-input`/`brink-loop`
//! satellite crates, never `cargo bench -p brink-runtime` itself. Nothing
//! in the normal PR gate ever compiled the fixture the bench actually
//! reads, so the break was invisible until a human happened to run the
//! bench by hand.
//!
//! This test is the cheap half of that gap: it does NOT run any bench (that
//! stays slow, and stays `benchmarks.yml`'s job) — it just compiles every
//! `benchmarks/stories/*/story.ink` fixture under the exact
//! dialect/`TypePolicy` combination its corresponding `runtime.rs` scenario
//! actually exercises (mirroring `compile_story`/`compile_story_brink`/
//! `compile_story_brink_typed` there), so the next diagnostic tightening
//! that breaks a fixture fails `cargo test -p brink-runtime` — already in
//! the required PR gate — instead of hiding for weeks.
//!
//! `FIXTURES` below is deliberately exhaustive against the directory
//! listing (`all_benchmarks_stories_fixtures_are_covered`): a new
//! `benchmarks/stories/<name>/` directory added without a matching entry
//! here fails loudly, rather than silently going untested the way
//! `struct-field-access-10k` did before this guard existed.
//!
//! ## What this guard does NOT catch
//!
//! A fixture that still compiles clean but has stopped exercising the code
//! path its bench comment claims (e.g. an edit that accidentally makes
//! `loop-append-10k` no longer hit the amortized-COW fast path) rots
//! silently through this guard — "compiles" is not "still measures the
//! right thing." That failure mode needs `--features bench-counters`'s
//! `print_bench_counters` assertions (or a human re-reading the bench
//! output), not a compile-only check, and is out of scope here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};

/// The dialect/`TypePolicy` combination a fixture's owning bench scenario
/// compiles it under (mirrors `runtime.rs`'s `compile_story`/
/// `compile_story_brink`/`compile_story_brink_typed` helpers exactly).
#[derive(Clone, Copy)]
enum FixturePolicy {
    /// `compile_story` — default `AnalysisOptions` (dialect `StrictInk`,
    /// `types: None`, which `resolve_type_policy` keys to `Gradual` for
    /// `StrictInk`). Used by the `hanoi-10`/`crucible-8` `scenarios()`
    /// entries.
    Default,
    /// `compile_story_brink` — `Dialect::Brink` + `TypePolicy::Gradual`.
    /// Used by every brink-dialect-only standalone bench.
    BrinkGradual,
    /// `compile_story_brink_typed(.., TypePolicy::Strict)` — used only by
    /// `struct_field_access_bench`'s strict half.
    BrinkStrict,
}

/// One `benchmarks/stories/<dir>/story.ink` fixture and the policy its
/// bench scenario compiles it under. `struct-field-access-10k` appears
/// twice — `runtime.rs`'s `struct_field_access_bench` compiles the same
/// source under both policies to isolate static-offset vs by-name field
/// dispatch, so this guard proves both compiles stay clean, not just one.
const FIXTURES: &[(&str, FixturePolicy)] = &[
    ("hanoi-10", FixturePolicy::Default),
    ("crucible-8", FixturePolicy::Default),
    ("loop-append-10k", FixturePolicy::BrinkGradual),
    ("loop-append-field-10k", FixturePolicy::BrinkGradual),
    ("share-then-mutate-5k", FixturePolicy::BrinkGradual),
    ("fn-creation-density-10k", FixturePolicy::BrinkGradual),
    ("fn-bind-chain-shallow", FixturePolicy::BrinkGradual),
    ("fn-bind-chain-deep", FixturePolicy::BrinkGradual),
    ("dynamic-dispatch-10k", FixturePolicy::BrinkGradual),
    ("direct-call-10k", FixturePolicy::BrinkGradual),
    ("save-state-small", FixturePolicy::BrinkGradual),
    ("save-state-medium", FixturePolicy::BrinkGradual),
    ("save-state-large", FixturePolicy::BrinkGradual),
    ("snapshot-retention-g10-m10", FixturePolicy::BrinkGradual),
    ("snapshot-retention-g10-m100", FixturePolicy::BrinkGradual),
    ("snapshot-retention-g100-m10", FixturePolicy::BrinkGradual),
    ("snapshot-retention-g100-m100", FixturePolicy::BrinkGradual),
    ("struct-field-access-10k", FixturePolicy::BrinkGradual),
    ("struct-field-access-10k", FixturePolicy::BrinkStrict),
];

/// Repo root's `benchmarks/stories/` directory, two levels up from this
/// crate's manifest dir (`crates/brink-runtime`) — the same
/// `../../benchmarks/stories/...` relative path `benches/runtime.rs`'s own
/// `HANOI_10_INK` etc. constants use, resolved against
/// `CARGO_MANIFEST_DIR` (stable regardless of which target — bin, test,
/// bench — is compiling).
fn benchmarks_stories_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/stories")
}

#[test]
fn every_benchmark_fixture_compiles_under_its_bench_scenarios_policy() {
    for (dir, policy) in FIXTURES {
        let path = benchmarks_stories_dir().join(dir).join("story.ink");
        let result = match policy {
            FixturePolicy::Default => brink_compiler::compile_path(&path),
            FixturePolicy::BrinkGradual => brink_compiler::compile_path_with_options(
                &path,
                AnalysisOptions {
                    dialect: Dialect::Brink,
                    types: Some(TypePolicy::Gradual),
                    ..AnalysisOptions::default()
                },
            ),
            FixturePolicy::BrinkStrict => brink_compiler::compile_path_with_options(
                &path,
                AnalysisOptions {
                    dialect: Dialect::Brink,
                    types: Some(TypePolicy::Strict),
                    ..AnalysisOptions::default()
                },
            ),
        };
        assert!(
            result.is_ok(),
            "benchmarks/stories/{dir}/story.ink failed to compile under its \
             bench scenario's policy — this is the #2138 class (issue \
             #2777): a fixture broke and nothing on the normal PR gate \
             noticed. Compile error: {:?}",
            result.err()
        );
    }
}

/// `FIXTURES` is a hand-maintained list; this pins it against the actual
/// directory listing so a fixture added to `benchmarks/stories/` without a
/// matching `FIXTURES` entry — and therefore never exercised by
/// [`every_benchmark_fixture_compiles_under_its_bench_scenarios_policy`] —
/// fails loudly here instead of silently going untested.
#[test]
fn all_benchmarks_stories_fixtures_are_covered() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(benchmarks_stories_dir())
        .expect("benchmarks/stories/ should exist")
        .filter_map(|entry| {
            let entry = entry.expect("dir entry should be readable");
            let file_type = entry.file_type().expect("file type should be readable");
            file_type
                .is_dir()
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();

    let covered: BTreeSet<String> = FIXTURES.iter().map(|(dir, _)| (*dir).to_owned()).collect();

    assert!(
        on_disk == covered,
        "benchmarks/stories/ directories and FIXTURES in \
         benchmark_fixtures_compile.rs have drifted apart — on disk but \
         not covered: {:?}; covered but not on disk: {:?}. Add/remove the \
         matching FIXTURES entry (see this file's module doc, issue \
         #2777).",
        on_disk.difference(&covered).collect::<Vec<_>>(),
        covered.difference(&on_disk).collect::<Vec<_>>()
    );
}
