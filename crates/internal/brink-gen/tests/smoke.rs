//! Smoke properties for the structure tier (`docs/program-generator-spec.md`
//! §7, the per-PR lane): every generated story validates, compiles clean
//! through BOTH compile roads with identical bytes, and explores to
//! termination under the harness's default budget.
//!
//! `PROPTEST_CASES` overrides the case count for a deeper local run.

// Integration-test convention across the workspace (see the sibling
// `crates/brink-compiler/tests/*.rs`): helpers outside `#[test]` fns are not
// covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use brink_compiler::Severity;
use brink_gen::{arb_story, print_ink};
use brink_test_harness::{ExploreConfig, Outcome, explore};
use proptest::prelude::*;

const CASES: u32 = 48;

/// `ProptestConfig::with_cases` ignores the `PROPTEST_CASES` environment
/// variable (only `default()` reads it), so the override is applied by hand.
fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CASES);
    ProptestConfig {
        cases,
        // A failing story that is expensive to check (an exhaustive
        // exploration near the episode budget runs about a second) must not
        // turn shrinking into a quarter-hour stall: cap the shrink phase.
        max_shrink_time: 60_000,
        ..ProptestConfig::default()
    }
}

/// A private temp directory per test process so the two-roads check can hand
/// the Environment road a real path (it discovers a source root from it).
fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brink-gen-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Compile one in-memory story, rendering the diagnostics themselves on
/// failure (`CompileError`'s Display only counts them).
fn compile(src: &str) -> Result<brink_compiler::CompileOutput, TestCaseError> {
    brink_compiler::compile("gen.ink", |_| Ok(src.to_owned())).map_err(|e| {
        let detail = match &e {
            brink_compiler::CompileError::Diagnostics(ds) => ds
                .iter()
                .map(|d| {
                    format!(
                        "  [{}] {}..{}: {}",
                        d.code.as_str(),
                        u32::from(d.range.start()),
                        u32::from(d.range.end()),
                        d.message
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            other => other.to_string(),
        };
        TestCaseError::fail(format!("compile failed:\n{detail}\n--- source ---\n{src}"))
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn validates_and_compiles_clean(story in arb_story()) {
        prop_assert_eq!(brink_gen::model::validate(&story), Ok(()));
        let src = print_ink(&story);
        let out = compile(&src)?;
        // Info/Hint-tier lints (E157's anonymous once-only choice, a
        // precision lint ruled "off or info by default") are advisory, not
        // a generator defect; anything Warning or above is.
        let loud: Vec<_> = out
            .warnings
            .iter()
            .filter(|w| !matches!(w.severity, Severity::Info | Severity::Hint))
            .collect();
        prop_assert!(
            loud.is_empty(),
            "generated story compiled with warnings: {loud:?}\n--- source ---\n{src}"
        );
    }

    #[test]
    fn both_roads_agree(story in arb_story()) {
        let src = print_ink(&story);
        let path = scratch_dir().join("gen.ink");
        std::fs::write(&path, &src).expect("write scratch story");
        // The two roads embed different file-path strings (absolute vs
        // root-relative), so bytes legitimately differ; what must agree is
        // behavior — the same explored episodes — exactly as the corpus's
        // `environment_parallel_gate` compares them.
        let config = ExploreConfig { max_depth: 64, max_episodes: 512 };
        let (_, via_path) = brink_test_harness::corpus::compile_and_explore_from_ink(&path, &config)
            .map_err(|e| TestCaseError::fail(format!("compile_path road: {e}\n{src}")))?;
        let (_, via_env) = brink_test_harness::corpus::compile_and_explore_via_environment(&path, &config)
            .map_err(|e| TestCaseError::fail(format!("environment road: {e}\n{src}")))?;
        prop_assert_eq!(via_path.len(), via_env.len(), "episode counts differ on:\n{}", src);
        for (a, b) in via_path.iter().zip(&via_env) {
            let d = brink_test_harness::diff(a, b);
            prop_assert!(d.matches, "compile roads disagree on:\n{}\n{:?}", src, d);
        }
    }

    #[test]
    fn explores_to_termination(story in arb_story()) {
        let src = print_ink(&story);
        let out = compile(&src)?;
        let (program, line_tables) = brink_runtime::link(&out.data)
            .map_err(|e| TestCaseError::fail(format!("link failed: {e}\n{src}")))?;
        // The explorer returns silently when it hits `max_episodes` (the
        // depth cap is loud — it records `InputsExhausted`); a saturated
        // exploration would let this property pass on a truncated DFS, so
        // saturation is itself a failure — a story the default profile
        // cannot exhaust is a profile-size problem to surface, not hide.
        let config = ExploreConfig { max_depth: 64, max_episodes: 4096 };
        let episodes = explore(Arc::new(program), line_tables, &config);
        prop_assert!(!episodes.is_empty(), "no episodes explored for:\n{src}");
        prop_assert!(
            episodes.len() < config.max_episodes,
            "exploration saturated at {} episodes — the profile is too large to explore exhaustively:\n{src}",
            config.max_episodes
        );
        for ep in &episodes {
            prop_assert!(
                matches!(ep.outcome, Outcome::Ended | Outcome::Done),
                "episode did not terminate cleanly ({:?}) after choices {:?} on:\n{src}",
                ep.outcome,
                ep.choice_path
            );
        }
    }
}
