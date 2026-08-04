//! Issue #2223: a parallel gate that runs the corpus through
//! `brink_environment::compile(&Environment)` — the real production
//! road — **alongside** the existing `compile_path`-based sweeps
//! (`oracle_snapshots.rs`, `tier1_native.rs`), rather than migrating them.
//!
//! # Why "alongside" (per #2223's own sizing questions)
//!
//! `brink_compiler::compile_path`'s own doc says it "bypasses `Environment`
//! entirely." Every corpus entry point in this crate goes through it, so
//! the 5,608-episode ratchet and the tier1-native goldens are structurally
//! blind to anything only `Environment` does — most concretely the stdlib
//! mount (#2080), whose collision with a project's own bare names hard-
//! failed a shipped preset (`conventions-screenplay-preset`) on the real
//! path while every episode stayed green (#2197, already fixed by #2213).
//! Migrating the whole corpus onto `Environment` would touch every existing
//! sweep's assertions; a parallel gate is additive and catches exactly the
//! divergence #2223 is about, at a fraction of the cost.
//!
//! # Coverage — what subset, and why
//!
//! - **The full oracle-derived corpus** (`tests/tier{1,2,3}/` +
//!   `tests/tests_github/`, discovered the same way `oracle_snapshots.rs`
//!   does via `collect_oracle_cases`): 398 cases at the time of writing.
//!   Not a sample — the measurement below (a few seconds) is cheap enough
//!   that the full set ran, so there was no reason to truncate it.
//! - **The full `tests/tier1-native/` corpus** (26 cases via a directory
//!   sweep, mirroring `tier1_native_strict.rs`'s own `case_names()`): the
//!   exact category #2197 already hit, and the one place stdlib-mount
//!   collisions are actually possible (see below).
//! - **Deliberately excluded**: `tier1-brink`, `tier1-brink-respell`,
//!   `tests_patched`, `dialect_fixtures` — none of these are named by
//!   #2223 or covered by `RATCHET_EPISODE_COUNT`; adding them would be
//!   scope creep beyond what this issue asks sized.
//!
//! # Sizing measurements (2026-08-03, this PR)
//!
//! - **Wall-clock**: both tests below together ran in 5.5s on a warm build
//!   (`cargo test -p brink-test-harness --test environment_parallel_gate`,
//!   398 + 26 cases, each compiled+explored/run through *both* roads).
//!   `compile(&Environment)`'s non-incrementality is a per-call cost of the
//!   same order `compile_path` already pays per case — building an
//!   `Environment` per case does **not** make the suite unacceptably slow.
//! - **No case in either corpus depends on the bypass**: every case that
//!   compiles via `compile_path` also compiles via `Environment` (and vice
//!   versa) in this measurement — no INCLUDE topology here exists on one
//!   road and not the other.
//! - **Two confirmed, real `StoryData`-level divergences from the stdlib
//!   mount (#2080) — reported, not fixed here, per #2223's own instruction
//!   not to reconcile a finding by editing anything:**
//!   - *ink entries*: `name_table` permanently gains an extra, unreferenced
//!     entry (confirmed: a bare `"Hello, world!"` story's `name_table`
//!     grows from `[""]` to `["", "scene_entered"]`) even though the
//!     INCLUDE-closure (`compilation_closure_files` in `brink-db`) never
//!     reaches the mounted `.brink` file at all.
//!   - *native entries*: "tree is universe" makes every mounted `.brink`
//!     module — including the std one — a real compilation-unit member, so
//!     `containers` grows (5 extra containers: `scene_entered`, `heading`,
//!     `transition`, `cue`, `parenthetical`) even for a case that never
//!     calls them — harmless unless the case's own names collide (#2197's
//!     exact failure mode, already fixed).
//! - **A third, unrelated `StoryData`-level divergence — a `compile_path`
//!   artifact, not an `Environment` defect, and not filed as a follow-up**:
//!   every `tests/tier{1,3}/includes/*` (and `tests_github` INCLUDE) case
//!   initially showed `containers`/`line_tables`/`addresses` mismatches
//!   between the two roads. Root cause, traced by hand
//!   (`tier1/includes/root-weave-in-entry-and-included-file`): every call
//!   site here passes `compile_path` an **absolute** entry path (derived
//!   from `CARGO_MANIFEST_DIR`); `compile_path` has no root concept, so it
//!   keys an `INCLUDE`d file by literally joining the entry's absolute
//!   directory with the included filename, baking the checkout's absolute
//!   filesystem path into that file's containers' content-addressed
//!   identity and into `LineEntry::source_location`. `Environment` instead
//!   roots at the case directory and keys every file (entry and included)
//!   root-relative, so its identity/`source_location` are clean and
//!   portable. This is a real quirk of `compile_path` (test-only tooling,
//!   gated behind `test-util`) — not a production-path bug — and it does
//!   not change any executed output (verified: episode-for-episode
//!   equality holds on every one of these cases, see below). Because it
//!   would make a literal `StoryData` field comparison noisy without being
//!   meaningful, the ink comparison below is **episode-level** (see next
//!   section), which this divergence does not touch at all.
//!
//! # What "divergence" means for each corpus
//!
//! **Ink** (`oracle_corpus_agrees_between_compile_path_and_environment`):
//! per-case, both roads must reach the same compile/fail *verdict*, and
//! when both succeed, the [`Episode`] sequence [`crate::explore`] walks
//! must be **exactly equal** — the player-observable contract (text, tags,
//! choices, state mutations, outcome), not raw `StoryData`. A literal
//! `StoryData` comparison was tried first and rejected: it flags the two
//! confirmed-harmless divergences above (`name_table` growth,
//! `compile_path`'s absolute-path keying of `INCLUDE`d files) even though
//! not one of the ~380 cases here produces a different episode.
//!
//! **Native** (`native_corpus_transcripts_agree_between_compile_path_and_environment`):
//! per-case, both roads must reach the same compile/fail verdict, and when
//! both succeed, the **executed transcript** (not raw `StoryData` — the
//! extra std containers make that comparison meaningless) must be
//! byte-identical. This is the exact shape of check that would have caught
//! #2197 before it shipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::{
    collect_oracle_cases, compile_and_explore_from_ink, compile_and_explore_via_environment,
    run_native_transcript, run_native_transcript_via_environment,
};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

/// Same skip rule `oracle_snapshots.rs` applies before comparing to the
/// oracle: a case whose fixture is *deliberately* a compile-error probe, or
/// whose `story.ink` is empty, is not one this gate can usefully compare —
/// both roads are expected to disagree with "success," not with each
/// other's diagnostics.
fn is_compile_error_case(case_dir: &Path) -> bool {
    let meta_path = case_dir.join("metadata.toml");
    std::fs::read_to_string(meta_path).ok().is_some_and(|s| {
        s.lines()
            .any(|line| line.trim() == r#"mode = "compile_error""#)
    })
}

fn has_empty_source(case_dir: &Path) -> bool {
    let ink_path = case_dir.join("story.ink");
    std::fs::read_to_string(ink_path)
        .ok()
        .is_some_and(|s| s.trim().is_empty())
}

#[test]
fn oracle_corpus_agrees_between_compile_path_and_environment() {
    let root = tests_dir();
    let cases = collect_oracle_cases(&root);
    assert!(
        cases.len() > 300,
        "sanity check: expected the full oracle-derived corpus (hundreds of cases), got {} — \
         collect_oracle_cases likely regressed or the corpus moved",
        cases.len()
    );

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 1000,
    };

    let mut divergences: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut skipped = 0usize;

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .display()
            .to_string();
        let ink_path = case_dir.join("story.ink");
        if !ink_path.exists() || has_empty_source(case_dir) || is_compile_error_case(case_dir) {
            skipped += 1;
            continue;
        }

        let via_path = compile_and_explore_from_ink(&ink_path, &config);
        let via_env = compile_and_explore_via_environment(&ink_path, &config);

        match (via_path, via_env) {
            (Ok((_, episodes_path)), Ok((_, episodes_env))) => {
                compared += 1;
                if episodes_path != episodes_env {
                    divergences.push(format!(
                        "{rel}: episodes diverge ({} via compile_path, {} via Environment)",
                        episodes_path.len(),
                        episodes_env.len()
                    ));
                }
            }
            (Err(_), Err(_)) => {
                // Both roads agree the case doesn't compile — fine, even
                // if the diagnostics' exact shape differs (entry-path
                // spelling differs between the two roads).
                compared += 1;
            }
            (Ok(_), Err(env_err)) => {
                divergences.push(format!(
                    "{rel}: compile_path SUCCEEDED but Environment FAILED — exactly the #2197 \
                     failure mode: {env_err}"
                ));
            }
            (Err(path_err), Ok(_)) => {
                divergences.push(format!(
                    "{rel}: Environment SUCCEEDED but compile_path FAILED — the case depends on \
                     the bypass: {path_err}"
                ));
            }
        }
    }

    assert!(
        divergences.is_empty(),
        "found {} divergence(s) between compile_path and brink_environment::compile across {} \
         compared oracle-derived cases ({} skipped as compile-error/empty-source fixtures):\n{}",
        divergences.len(),
        compared,
        skipped,
        divergences.join("\n")
    );
}

fn native_corpus_dir() -> PathBuf {
    tests_dir().join("tier1-native")
}

/// Every case directory under `tests/tier1-native/`, sorted — mirrors
/// `tier1_native_strict.rs`'s own `case_names()` so a newly-added case is
/// swept here automatically.
fn native_case_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(native_corpus_dir())
        .expect("read tests/tier1-native")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn native_corpus_transcripts_agree_between_compile_path_and_environment() {
    let names = native_case_names();
    assert!(
        names.len() > 10,
        "sanity check: expected tests/tier1-native/ to hold more than a handful of cases, got {}",
        names.len()
    );

    let mut divergences: Vec<String> = Vec::new();
    let mut compared = 0usize;

    for name in &names {
        let brink_path = native_corpus_dir().join(name).join("story.brink");
        if !brink_path.exists() {
            continue;
        }

        let via_path = run_native_transcript(&brink_path);
        let via_env = run_native_transcript_via_environment(&brink_path);

        match (via_path, via_env) {
            (Ok(path_out), Ok(env_out)) => {
                compared += 1;
                if path_out != env_out {
                    divergences.push(format!(
                        "{name}: transcript diverges\n--- compile_path ---\n{path_out}\n--- \
                         environment ---\n{env_out}"
                    ));
                }
            }
            (Err(_), Err(_)) => {
                compared += 1;
            }
            (Ok(_), Err(env_err)) => {
                divergences.push(format!(
                    "{name}: compile_path SUCCEEDED but Environment FAILED — exactly the #2197 \
                     failure mode (mounted stdlib collided with the project's own definitions): \
                     {env_err}"
                ));
            }
            (Err(path_err), Ok(_)) => {
                divergences.push(format!(
                    "{name}: Environment SUCCEEDED but compile_path FAILED: {path_err}"
                ));
            }
        }
    }

    assert!(
        divergences.is_empty(),
        "found {} divergence(s) between compile_path and brink_environment::compile across {} \
         compared tier1-native cases:\n{}",
        divergences.len(),
        compared,
        divergences.join("\n")
    );
}
