//! The capture tier (issue #3380, `docs/program-generator-spec.md` §5):
//! `tests/tier4-generated/<case>/` — a shrunk generated story (`story.ink`),
//! its golden episodes (`oracle/*.oracle.json`), and a `case.toml` with
//! provenance: which property or probe produced it, the seed, and which
//! oracle blessed the golden (`inkjs` from `tools/inkjs-oracle`, or `csharp`
//! once a maintainer re-blessed it with dotnet).
//!
//! ⚠ **Not part of `RATCHET_EPISODE_COUNT`.** The shared corpus walk prunes
//! this directory by name (`corpus::GENERATED_TIER_DIR`), so nothing here
//! reaches `oracle_snapshots.rs`, the inkjs sanction, or the respell sweep.
//! This file is the tier's own must-pass target: every case must match its
//! golden, and [`GENERATED_CASE_COUNT`] only moves through an explicit
//! promotion (`pnpm promote:generated`, `scripts/promote-generated.mjs`,
//! which bumps it). A case added by hand without the bump, or removed
//! without it, fails here by count.
//!
//! A case that reproduces a KNOWN, open divergence carries `[source]
//! expected_mismatch = "#NNNN"` in its `case.toml` — the same flag, with the
//! same two-way discipline, as the curated corpus (#3402): it must keep
//! mismatching until the fix lands, and the fix must remove the flag.
//!
//! ```sh
//! cargo test -p brink-test-harness --test tier4_generated -- --nocapture
//! BRINK_CASE=glue cargo test -p brink-test-harness --test tier4_generated -- --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr
)]

use std::collections::HashMap;
use std::path::PathBuf;

use brink_test_harness::corpus::{
    collect_generated_cases, compile_and_explore_from_ink, expected_mismatch_issue_in,
    load_generated_case,
};
use brink_test_harness::{Episode, ExploreConfig, diff_oracle, load_oracle_episodes};

/// The number of cases under `tests/tier4-generated/`. Moved ONLY by
/// `scripts/promote-generated.mjs` (or the removal of a case), so a case
/// can never appear or vanish without a diff here.
const GENERATED_CASE_COUNT: usize = 4;

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

fn index_by_choice_path(episodes: &[Episode]) -> HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

/// `Ok(())` when every golden episode matches brink's; otherwise the first
/// difference, rendered.
fn check_case(dir: &std::path::Path) -> Result<(), String> {
    let goldens = load_oracle_episodes(dir)?;
    if goldens.is_empty() {
        return Err("no golden episodes".to_owned());
    }
    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 1000,
    };
    let (_, actual) = compile_and_explore_from_ink(&dir.join("story.ink"), &config)?;
    let by_path = index_by_choice_path(&actual);
    if goldens.len() != actual.len() {
        return Err(format!(
            "episode count: {} golden vs {} brink",
            goldens.len(),
            actual.len()
        ));
    }
    for golden in &goldens {
        let Some(brink) = by_path.get(golden.choice_path.as_slice()) else {
            return Err(format!(
                "brink has no episode for choice path {:?}",
                golden.choice_path
            ));
        };
        let diff = diff_oracle(golden, brink);
        if !diff.matches {
            return Err(format!("choice path {:?}: {diff}", golden.choice_path));
        }
    }
    Ok(())
}

#[test]
fn every_generated_case_matches_its_golden() {
    let root = tests_dir();
    let dirs = collect_generated_cases(&root);
    let filter = std::env::var("BRINK_CASE").ok();

    assert_eq!(
        dirs.len(),
        GENERATED_CASE_COUNT,
        "tests/tier4-generated holds {} case(s) but GENERATED_CASE_COUNT is {}: promote through \
         `pnpm promote:generated` (which bumps the constant) or update it alongside a removal",
        dirs.len(),
        GENERATED_CASE_COUNT
    );

    let mut failures: Vec<String> = Vec::new();
    let mut passed = 0usize;
    let mut expected_mismatches = 0usize;

    for dir in &dirs {
        let case = load_generated_case(dir).unwrap_or_else(|e| panic!("{e}"));
        if let Some(f) = &filter
            && !case.name.contains(f.as_str())
        {
            continue;
        }
        let flag = expected_mismatch_issue_in(&dir.join("case.toml"));
        let verdict = check_case(dir);
        match (verdict, flag) {
            (Ok(()), None) => passed += 1,
            (Err(_), Some(_)) => expected_mismatches += 1,
            (Err(e), None) => failures.push(format!(
                "{} (from {}: {}, golden by {}): {e}",
                case.name,
                case.provenance.source,
                case.provenance.property,
                case.provenance.oracle_source
            )),
            (Ok(()), Some(issue)) => failures.push(format!(
                "{}: flagged expected_mismatch ({issue}) but now MATCHES its golden — remove \
                 the flag from case.toml in the same change as the fix",
                case.name
            )),
        }
    }

    eprintln!(
        "tier4_generated: {passed} passing, {expected_mismatches} expected mismatch(es), {} \
         failure(s) of {} case(s)",
        failures.len(),
        dirs.len()
    );
    assert!(
        failures.is_empty(),
        "tier4-generated failures:\n  {}",
        failures.join("\n  ")
    );
}
