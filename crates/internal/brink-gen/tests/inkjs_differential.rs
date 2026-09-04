//! The inkjs differential (issue #3379, `docs/program-generator-spec.md`
//! §6): for generated `plain_ink` stories, brink's explored episodes must
//! match the reference's — `trace(brink(P)) == trace(inkjs(P))` over every
//! choice path, compared by `brink_test_harness::diff_oracle`, the same
//! comparison the corpus ratchet applies to a C# golden.
//!
//! The reference is `tools/inkjs-oracle` (a port of the C# crawler onto
//! inkjs with the .NET `System.Random` generator installed), which
//! `crates/internal/brink-test-harness/tests/inkjs_sanction.rs` proves
//! reproduces every checked-in C# golden before it is allowed to judge
//! anything here. Where brink and inkjs disagree, the C# runtime is the
//! tie-breaker (maintainer-local, `dotnet`): minimise the story and add it
//! to the corpus as an oracle case, per the 2026-09-02 directive.
//!
//! Opt-in: `BRINK_INKJS_ORACLE=1`; `PROPTEST_CASES` overrides the count.
//!
//! ```sh
//! BRINK_INKJS_ORACLE=1 cargo test -p brink-gen --test inkjs_differential -- --nocapture
//! ```

// Integration-test convention across the workspace: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use brink_gen::{Profile, arb_story_with, print_ink};
use brink_test_harness::{ExploreConfig, inkjs};
use proptest::prelude::*;

/// Default case count; the nightly lane raises it through `PROPTEST_CASES`
/// (§7). Each case spawns one `node` (~100ms) on top of brink's own
/// compile-and-explore.
const CASES: u32 = 32;

/// Divergences brink is KNOWN to have from the reference, each keyed on its
/// issue and recognised by a predicate over the printed `.ink` source. A
/// story that fails the differential AND matches a predicate is counted,
/// reported at the end, and does not fail the run; a story that fails and
/// matches none is a new finding and fails with the shrunk source. This is
/// `metadata.toml`'s `expected_mismatch` (#3402) applied to generated
/// stories: the shapes keep running through brink, and the entry is removed
/// (the run then finds nothing to count) once the issue is fixed.
///
/// The cost is stated plainly: a story that matches a predicate could fail
/// for a DIFFERENT reason and be counted here — so keep predicates narrow,
/// and read the per-run tally as a signal, not as noise.
const KNOWN_DIVERGENCES: &[(&str, SourcePredicate)] = &[(
    // `* [a  0]`: ink presents `a  0`, brink `a 0`.
    "#3508 whitespace runs in choice text are collapsed",
    |src| {
        src.lines().any(|l| {
            let t = l.trim_start();
            (t.starts_with('*') || t.starts_with('+')) && t.contains("  ")
        })
    },
)];

/// Recognises a known-divergent shape in a printed `.ink` source.
type SourcePredicate = fn(&str) -> bool;

static KNOWN_HITS: AtomicUsize = AtomicUsize::new(0);

/// `ProptestConfig::with_cases` ignores the `PROPTEST_CASES` environment
/// variable (only `default()` reads it), so the override is applied by hand.
fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CASES);
    ProptestConfig {
        cases,
        ..ProptestConfig::default()
    }
}

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brink-gen-inkjs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The C# crawler's defaults, which `tools/inkjs-oracle` reproduces; brink's
/// explorer is given the same budget so a depth-capped `InputsExhausted`
/// lands at the same step on both sides.
const EXPLORE: ExploreConfig = ExploreConfig {
    max_depth: 20,
    max_episodes: 1000,
};

fn render(story: &brink_gen::Story) -> Result<(), TestCaseError> {
    let src = print_ink(story);
    match compare(&src) {
        Ok(()) => Ok(()),
        Err(e) => {
            let known: Vec<&str> = KNOWN_DIVERGENCES
                .iter()
                .filter(|(_, matches)| matches(&src))
                .map(|(issue, _)| *issue)
                .collect();
            if known.is_empty() {
                Err(e)
            } else {
                KNOWN_HITS.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "inkjs_differential: known divergence ({}) on:\n{src}",
                    known.join("; ")
                );
                Ok(())
            }
        }
    }
}

fn compare(src: &str) -> Result<(), TestCaseError> {
    let scratch = scratch_dir();
    let ink_path = scratch.join("gen.ink");
    std::fs::write(&ink_path, src).expect("write generated story");

    let (_, mut brink_eps) =
        brink_test_harness::corpus::compile_and_explore_from_ink(&ink_path, &EXPLORE)
            .map_err(|e| TestCaseError::fail(format!("brink: {e}\n--- source ---\n{src}")))?;
    for ep in &mut brink_eps {
        inkjs::normalize_brink_episode(ep);
    }

    let mut inkjs_eps = inkjs::explore_file(&ink_path, &scratch.join("inkjs"))
        .map_err(|e| TestCaseError::fail(format!("inkjs: {e}\n--- source ---\n{src}")))?;
    for ep in &mut inkjs_eps {
        inkjs::normalize_oracle_episode(ep);
    }

    prop_assert_eq!(
        inkjs_eps.len(),
        brink_eps.len(),
        "episode counts differ (inkjs vs brink) on:\n{}",
        src
    );
    let brink_by_path: HashMap<&[usize], _> = brink_eps
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect();
    for oracle_ep in &inkjs_eps {
        let Some(brink_ep) = brink_by_path.get(oracle_ep.choice_path.as_slice()) else {
            return Err(TestCaseError::fail(format!(
                "brink has no episode for choice path {:?} on:\n{src}",
                oracle_ep.choice_path
            )));
        };
        let diff = brink_test_harness::diff_oracle(oracle_ep, brink_ep);
        prop_assert!(
            diff.matches,
            "brink diverges from inkjs on choice path {:?}:\n{}\n--- source ---\n{}",
            oracle_ep.choice_path,
            diff,
            src
        );
    }
    Ok(())
}

#[test]
fn brink_matches_inkjs_on_generated_plain_ink() {
    if !inkjs::enabled() {
        eprintln!(
            "inkjs_differential: skipped — set {}=1 (needs node + `npm ci` in tools/inkjs-oracle)",
            inkjs::ENABLE_ENV
        );
        return;
    }
    inkjs::ensure_installed().unwrap_or_else(|e| panic!("{e}"));

    let mut runner = proptest::test_runner::TestRunner::new(config());
    let outcome = runner.run(&arb_story_with(Profile::PLAIN_INK), |story| render(&story));
    eprintln!(
        "inkjs_differential: {} known-divergence hit(s) (see KNOWN_DIVERGENCES)",
        KNOWN_HITS.load(Ordering::Relaxed)
    );
    if let Err(e) = outcome {
        panic!("{e}");
    }
}
