//! The inkjs sanction (issue #3379, `docs/program-generator-spec.md` §6):
//! every checked-in C# oracle golden — tier1–3 plus the four GitHub-corpus
//! cases that carry one — replayed through `tools/inkjs-oracle`, must
//! match.
//!
//! inkjs is not on `CLAUDE.md`'s trust hierarchy. This test is what earns
//! it standing as a stand-in for the rank-2 reference in sessions that have
//! `node` but no `dotnet` — and it is what the generated-story differential
//! (`crates/internal/brink-gen/tests/inkjs_differential.rs`) rests on: a
//! reference that cannot reproduce the goldens has no business judging
//! brink. The comparison is the raw episode JSON after
//! `brink_test_harness::inkjs`'s two normalisations (error-message
//! dressing, float32 printing — see that module's header for the
//! measurement behind each); everything else is exact.
//!
//! Opt-in: `BRINK_INKJS_ORACLE=1` (needs `node` and `npm ci` in
//! `tools/inkjs-oracle`). `BRINK_CASE=<substring>` filters cases, as in
//! `oracle_snapshots.rs`:
//!
//! ```sh
//! BRINK_INKJS_ORACLE=1 cargo test -p brink-test-harness --test inkjs_sanction -- --nocapture
//! ```

// Integration-test convention across the workspace: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use brink_test_harness::corpus::collect_oracle_cases;
use brink_test_harness::inkjs;

/// Cases where inkjs is KNOWN to diverge from the C# golden after
/// normalisation, each with the reason. An entry here is a real, understood
/// difference between the two reference runtimes (never a brink bug — brink
/// is not involved in this test), and is checked both ways: a listed case
/// that matches again fails the test until the entry is removed, exactly
/// like `metadata.toml`'s `expected_mismatch` flag (#3402).
///
/// Empty as of 2026-09-04: 414 of 414 oracle cases match (400 byte for
/// byte, 14 after the two normalisations).
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[];

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

fn scratch_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brink-inkjs-sanction-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear stale scratch");
    }
    std::fs::create_dir_all(&dir).expect("create scratch");
    dir
}

/// Episodes keyed by their `choice_path`, so a comparison never depends on
/// file order (`e10` sorts before `e2` either way).
fn by_choice_path(dir: &Path) -> Result<BTreeMap<String, serde_json::Value>, String> {
    let mut out = BTreeMap::new();
    for mut value in inkjs::load_episode_values(dir)? {
        inkjs::normalize_episode_value(&mut value);
        let key = value
            .get("choice_path")
            .map(ToString::to_string)
            .ok_or_else(|| format!("episode in {} has no choice_path", dir.display()))?;
        if out.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate choice_path {key} in {}", dir.display()));
        }
    }
    Ok(out)
}

/// `None` when the two directories hold the same episodes; otherwise the
/// first difference, rendered.
fn compare_case(golden_dir: &Path, inkjs_dir: &Path) -> Result<Option<String>, String> {
    if !inkjs_dir.is_dir() {
        return Ok(Some(
            "inkjs produced no output for this case (compile or explore failure — see the \
             crawl log above)"
                .to_owned(),
        ));
    }
    let golden = by_choice_path(golden_dir)?;
    let actual = by_choice_path(inkjs_dir)?;
    if golden.len() != actual.len() {
        return Ok(Some(format!(
            "episode count: {} (C#) vs {} (inkjs)",
            golden.len(),
            actual.len()
        )));
    }
    for (path, expected) in &golden {
        let Some(got) = actual.get(path) else {
            return Ok(Some(format!("no inkjs episode with choice_path {path}")));
        };
        if let Some(diff) = inkjs::first_difference(expected, got) {
            return Ok(Some(format!("episode with choice_path {path} {diff}")));
        }
    }
    Ok(None)
}

#[test]
fn inkjs_reproduces_every_oracle_golden() {
    if !inkjs::enabled() {
        eprintln!(
            "inkjs_sanction: skipped — set {}=1 (needs node + `npm ci` in tools/inkjs-oracle)",
            inkjs::ENABLE_ENV
        );
        return;
    }

    let root = tests_dir();
    let scratch = scratch_root();
    let case_filter = std::env::var("BRINK_CASE").ok();
    let cases = collect_oracle_cases(&root);

    // One crawl per top-level corpus directory that holds an oracle case
    // (tier1, tier2, tier3, tests_github — derived, not listed, so a new
    // root with goldens cannot be silently left out), not one process per
    // case: about two seconds per tier.
    let roots: BTreeSet<String> = cases
        .iter()
        .filter_map(|c| c.strip_prefix(&root).ok()?.components().next())
        .map(|first| first.as_os_str().to_string_lossy().into_owned())
        .collect();
    for tier in &roots {
        let log = inkjs::crawl(&root.join(tier), &scratch.join(tier))
            .unwrap_or_else(|e| panic!("inkjs crawl of {tier} failed: {e}"));
        if !log.all_succeeded {
            // Expected: the corpus holds compile-error probes with no golden.
            // Anything with a golden that failed shows up below, per case.
            eprintln!("[inkjs crawl {tier}]\n{}", log.stderr);
        }
    }

    let mut matched = 0usize;
    let mut allowlisted = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(filter) = &case_filter
            && !rel.contains(filter.as_str())
        {
            continue;
        }

        let verdict = compare_case(&case_dir.join("oracle"), &scratch.join(&rel))
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        let known = KNOWN_DIVERGENCES.iter().find(|(case, _)| *case == rel);

        match (verdict, known) {
            (None, None) => matched += 1,
            (Some(_), Some(_)) => allowlisted += 1,
            (Some(diff), None) => failures.push(format!("{rel}: {diff}")),
            (None, Some((_, reason))) => failures.push(format!(
                "{rel}: listed in KNOWN_DIVERGENCES ({reason}) but now MATCHES the golden — \
                 remove the entry"
            )),
        }
    }

    eprintln!(
        "inkjs_sanction: {matched} matched, {allowlisted} known divergences, {} failures",
        failures.len()
    );
    assert!(
        matched + allowlisted > 0,
        "no oracle cases compared — wrong tests dir or filter?"
    );
    assert!(
        failures.is_empty(),
        "inkjs diverges from the C# oracle on {} case(s):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
