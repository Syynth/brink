//! Corpus status report: per-category breakdown of pass/fail rates.
//!
//! Compares brink compiler output against C# ink runtime oracle episodes.
//!
//! Run with:
//!   cargo test -p brink-test-harness --test `corpus_report` -- --nocapture

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use brink_test_harness::corpus::{collect_oracle_cases, compile_and_explore_from_ink};
use brink_test_harness::oracle;
use brink_test_harness::{Episode, ExploreConfig};

/// Returns true if the case's metadata.toml has `mode = "compile_error"`.
fn is_compile_error_case(case_dir: &std::path::Path) -> bool {
    let meta_path = case_dir.join("metadata.toml");
    std::fs::read_to_string(meta_path).ok().is_some_and(|s| {
        s.lines()
            .any(|line| line.trim() == r#"mode = "compile_error""#)
    })
}

fn has_empty_source(case_dir: &std::path::Path) -> bool {
    let ink_path = case_dir.join("story.ink");
    std::fs::read_to_string(ink_path)
        .ok()
        .is_some_and(|s| s.trim().is_empty())
}

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
}

fn index_by_choice_path(episodes: &[Episode]) -> std::collections::HashMap<&[usize], &Episode> {
    episodes
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect()
}

#[derive(Default)]
struct CategoryStats {
    cases_pass: usize,
    cases_fail: usize,
    cases_compile_error: usize,
    cases_link_error: usize,
    cases_skip: usize,
    episodes_pass: usize,
    episodes_fail: usize,
    episodes_missing: usize,
}

impl CategoryStats {
    fn testable_cases(&self) -> usize {
        self.cases_pass + self.cases_fail
    }

    fn total_episodes(&self) -> usize {
        self.episodes_pass + self.episodes_fail + self.episodes_missing
    }
}

fn progress_bar(pass: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return "░".repeat(width);
    }
    let filled = (width * pass) / total;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

#[test]
#[expect(clippy::too_many_lines)]
fn corpus_report() {
    // #2054: a `CARGO_TARGET_DIR` shared across worktrees (the autonomous-pump
    // convention) can silently reuse another worktree's stale — or currently
    // different — build of a dependency, producing a confidently wrong
    // number here. This test cannot detect that itself (dep-info in a
    // shared target dir carries no absolute worktree path), so it can only
    // point at the tool that can: run `pnpm check:target-freshness`
    // (`scripts/check-target-freshness.mjs`) before trusting this report's
    // numbers whenever `CARGO_TARGET_DIR` is set and other worktrees may be
    // live.
    if std::env::var_os("CARGO_TARGET_DIR").is_some() {
        println!(
            "[corpus_report] CARGO_TARGET_DIR is set — before trusting these numbers, run \
             `pnpm check:target-freshness` to check for a shared-target-dir collision with \
             another live worktree (issue #2054).\n"
        );
    }

    let root = tests_dir();
    let cases = collect_oracle_cases(&root);

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 1000,
    };

    // Accumulate stats per "tier/category" key.
    let mut stats: BTreeMap<String, CategoryStats> = BTreeMap::new();

    for case_dir in &cases {
        let rel = case_dir
            .strip_prefix(&root)
            .unwrap_or(case_dir)
            .display()
            .to_string();

        // Extract tier/category from path like "tier1/choices/some-test"
        let parts: Vec<&str> = rel.split('/').collect();
        let key = if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            rel.clone()
        };

        let cat = stats.entry(key).or_default();

        let ink_path = case_dir.join("story.ink");
        if !ink_path.exists() || has_empty_source(case_dir) || is_compile_error_case(case_dir) {
            cat.cases_skip += 1;
            continue;
        }

        let oracle_eps = match oracle::load_oracle_episodes(case_dir) {
            Ok(eps) if eps.is_empty() => {
                cat.cases_skip += 1;
                continue;
            }
            Ok(eps) => eps,
            Err(_) => {
                cat.cases_skip += 1;
                continue;
            }
        };

        let (_story_data, actual) = match compile_and_explore_from_ink(&ink_path, &config) {
            Ok(pair) => pair,
            Err(e) if e.starts_with("compile:") => {
                cat.cases_compile_error += 1;
                continue;
            }
            Err(e) if e.starts_with("link:") => {
                cat.cases_link_error += 1;
                continue;
            }
            Err(_) => {
                cat.cases_compile_error += 1;
                continue;
            }
        };

        let actual_index = index_by_choice_path(&actual);
        let mut case_ok = true;

        for oracle_ep in &oracle_eps {
            let Some(brink_ep) = actual_index.get(oracle_ep.choice_path.as_slice()) else {
                cat.episodes_missing += 1;
                case_ok = false;
                continue;
            };
            let d = oracle::diff_oracle(oracle_ep, brink_ep);
            if d.matches {
                cat.episodes_pass += 1;
            } else {
                cat.episodes_fail += 1;
                case_ok = false;
            }
        }

        if case_ok {
            cat.cases_pass += 1;
        } else {
            cat.cases_fail += 1;
        }
    }

    // --- Render report ---

    let tiers = ["tier1", "tier2", "tier3", "tests_github"];
    let bar_width = 30;

    let mut grand_cases_pass = 0usize;
    let mut grand_cases_total = 0usize;
    let mut grand_episodes_pass = 0usize;
    let mut grand_episodes_total = 0usize;

    println!();

    for tier in &tiers {
        let tier_cats: Vec<(&String, &CategoryStats)> =
            stats.iter().filter(|(k, _)| k.starts_with(tier)).collect();

        if tier_cats.is_empty() {
            continue;
        }

        let tier_pass: usize = tier_cats.iter().map(|(_, s)| s.cases_pass).sum();
        let tier_testable: usize = tier_cats.iter().map(|(_, s)| s.testable_cases()).sum();
        let tier_ep_pass: usize = tier_cats.iter().map(|(_, s)| s.episodes_pass).sum();
        let tier_ep_total: usize = tier_cats.iter().map(|(_, s)| s.total_episodes()).sum();
        let tier_pct = (tier_pass * 100).checked_div(tier_testable).unwrap_or(0);

        grand_cases_pass += tier_pass;
        grand_cases_total += tier_testable;
        grand_episodes_pass += tier_ep_pass;
        grand_episodes_total += tier_ep_total;

        println!("============================================================");
        println!(
            "  {} — {}/{} cases passing ({}%),  {}/{} episodes",
            tier.to_uppercase(),
            tier_pass,
            tier_testable,
            tier_pct,
            tier_ep_pass,
            tier_ep_total,
        );
        println!("============================================================");

        for (key, s) in &tier_cats {
            let category = key.split('/').nth(1).unwrap_or(key);
            let testable = s.testable_cases();
            let pct = (s.cases_pass * 100).checked_div(testable).unwrap_or(0);
            let check = if s.cases_fail == 0
                && s.cases_compile_error == 0
                && s.cases_link_error == 0
                && testable > 0
            {
                "✓"
            } else {
                " "
            };
            let bar = progress_bar(s.cases_pass, testable, bar_width);

            let mut extra = String::new();
            if s.cases_compile_error > 0 {
                let _ = write!(extra, "  +{} compile_err", s.cases_compile_error);
            }
            if s.cases_link_error > 0 {
                let _ = write!(extra, "  +{} link_err", s.cases_link_error);
            }
            if s.cases_skip > 0 {
                let _ = write!(extra, "  +{} skip", s.cases_skip);
            }

            println!(
                "  {} {:<20} {} {:>3}/{:<3} ({:>3}%)  ep: {}/{}{}",
                check,
                category,
                bar,
                s.cases_pass,
                testable,
                pct,
                s.episodes_pass,
                s.total_episodes(),
                extra,
            );
        }
        println!();
    }

    let grand_pct = (grand_cases_pass * 100)
        .checked_div(grand_cases_total)
        .unwrap_or(0);
    let ep_pct = (grand_episodes_pass * 100)
        .checked_div(grand_episodes_total)
        .unwrap_or(0);

    println!("============================================================");
    println!("  OVERALL — {grand_cases_pass}/{grand_cases_total} cases passing ({grand_pct}%)");
    println!("  EPISODES — {grand_episodes_pass}/{grand_episodes_total} passing ({ep_pct}%)");
    println!("============================================================");

    native_corpus_report();
}

/// Describe a `native_corpus_report` output mismatch with enough detail to
/// actually locate the regression: the first differing byte offset plus
/// truncated, escaped snippets of both strings. A byte-length-only message
/// (e.g. "expected 12 bytes, got 12") renders identically whenever a
/// one-character regression happens to preserve length — exactly the
/// common case this report exists to surface.
fn describe_output_mismatch(expected: &str, actual: &str) -> String {
    const SNIPPET_CHARS: usize = 80;

    let diff_at = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(e, a)| e != a)
        .unwrap_or_else(|| expected.len().min(actual.len()));

    format!(
        "output mismatch at byte {diff_at} (expected {} bytes, got {} bytes)\n\
         \x20     expected: {}\n\
         \x20     actual:   {}",
        expected.len(),
        actual.len(),
        truncate_escaped(expected, SNIPPET_CHARS),
        truncate_escaped(actual, SNIPPET_CHARS),
    )
}

/// Truncate `s` to at most `max_chars` characters and escape control/
/// non-printable characters (`\n`, `\t`, …) so a mismatch snippet renders
/// on one line instead of visually merging into the report's row layout.
fn truncate_escaped(s: &str, max_chars: usize) -> String {
    let truncated: String = s.chars().take(max_chars).collect();
    let ellipsis = if s.chars().count() > max_chars {
        "…"
    } else {
        ""
    };
    format!("{}{ellipsis}", truncated.escape_debug())
}

/// `tests/tier1-native/` — the native (`.brink`) self-referential golden
/// corpus (issue #1529). Printed as its own clearly-labeled section,
/// *never* folded into the oracle CASES/EPISODES totals above: native
/// source has no C# ink counterpart, so a case here passing or failing
/// says nothing about oracle conformance, and the oracle ratchet
/// (`RATCHET_EPISODE_COUNT` in `oracle_snapshots.rs`) must stay a
/// completely separate number from whatever this prints. See
/// `tier1_native.rs`'s module doc for the full rationale and
/// `brink_test_harness::corpus::run_native_transcript` for how each case
/// is run — this report and that test share the exact same run path so
/// they can never silently disagree.
#[expect(
    clippy::print_stdout,
    reason = "this is a diagnostic report, not production output"
)]
fn native_corpus_report() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native");

    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();

    if cases.is_empty() {
        return;
    }

    let mut pass = 0usize;
    let mut rows: Vec<(String, bool, String)> = Vec::new();

    for case_dir in &cases {
        let name = case_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let expected = match brink_test_harness::corpus::load_golden_transcript(
            &case_dir.join("expected.txt"),
            &name,
        ) {
            Ok(e) => e,
            Err(e) => {
                rows.push((name, false, e));
                continue;
            }
        };

        match brink_test_harness::corpus::run_native_transcript(&case_dir.join("story.brink")) {
            Ok(actual) if actual == expected => {
                pass += 1;
                rows.push((name, true, String::new()));
            }
            Ok(actual) => rows.push((name, false, describe_output_mismatch(&expected, &actual))),
            Err(e) => rows.push((name, false, e)),
        }
    }

    let total = rows.len();

    println!();
    println!("============================================================");
    println!(
        "  TIER1-NATIVE (self-referential, NOT oracle-derived, issue #1529) \
         — {pass}/{total} cases passing"
    );
    println!("============================================================");
    for (name, ok, detail) in &rows {
        let check = if *ok { "✓" } else { " " };
        if *ok {
            println!("  {check} {name}");
        } else {
            println!("  {check} {name}  FAILED: {detail}");
        }
    }
    println!();
    println!(
        "  ⚠ These goldens have no C# oracle counterpart — this section is entirely \
         separate from the OVERALL/EPISODES totals above and from the oracle ratchet."
    );
    println!("============================================================");
}
