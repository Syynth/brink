//! Corpus status report: per-category breakdown of pass/fail rates.
//!
//! Compares brink compiler output against C# ink runtime oracle episodes.
//!
//! Run with:
//!   cargo test -p brink-test-harness --test `corpus_report` -- --nocapture

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use brink_test_harness::corpus::{
    MismatchFlagVerdict, collect_oracle_cases, compile_and_explore_from_ink,
    expected_mismatch_issue, mismatch_flag_verdict,
};
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

/// Status of one `expected_mismatch`-flagged case in the backlog listing
/// (Finding 1, PR #3432 review). Beyond the two ordinary oracle-comparison
/// outcomes, a flagged case can also be skipped or fail to compile/link —
/// those must still get a row, not silently vanish from the backlog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlaggedStatus {
    /// Compared against the oracle and still mismatching or missing
    /// episodes — the expected, steady state for a documented divergence.
    StillMismatching,
    /// Compared against the oracle and every episode now matches — the flag
    /// must be removed and `RATCHET_EPISODE_COUNT` raised.
    UnexpectedlyFixed,
    /// The case was skipped this run (missing/empty source, or the oracle
    /// couldn't be loaded) before any comparison happened.
    Skipped,
    /// The case failed to compile or link this run, before any comparison
    /// happened.
    CompileError,
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

/// Compare one case against the oracle, updating `cat`'s per-category
/// counters and (if the case carries an `expected_mismatch` flag) pushing
/// its outcome onto `flagged`. Extracted from `corpus_report`'s scan loop so
/// the same production logic backs both the real corpus report and
/// `flagged_case_that_fails_to_compile_still_appears_in_the_backlog` below
/// — a regression test must exercise the actual function, not a
/// reimplementation of it.
///
/// The `expected_mismatch` lookup happens *before* any of the early
/// returns below (skip, compile error, link error) so a flagged case that
/// regresses into one of those states still gets a row in the backlog
/// instead of silently disappearing from it (Finding 1, PR #3432 review).
fn classify_case(
    case_dir: &std::path::Path,
    rel: &str,
    config: &ExploreConfig,
    cat: &mut CategoryStats,
    flagged: &mut Vec<(String, String, FlaggedStatus)>,
) {
    let issue = expected_mismatch_issue(case_dir);
    let mut push_flagged = |status: FlaggedStatus| {
        if let Some(issue) = &issue {
            flagged.push((rel.to_string(), issue.clone(), status));
        }
    };

    let ink_path = case_dir.join("story.ink");
    if !ink_path.exists() || has_empty_source(case_dir) || is_compile_error_case(case_dir) {
        cat.cases_skip += 1;
        push_flagged(FlaggedStatus::Skipped);
        return;
    }

    let oracle_eps = match oracle::load_oracle_episodes(case_dir) {
        Ok(eps) if eps.is_empty() => {
            cat.cases_skip += 1;
            push_flagged(FlaggedStatus::Skipped);
            return;
        }
        Ok(eps) => eps,
        Err(_) => {
            cat.cases_skip += 1;
            push_flagged(FlaggedStatus::Skipped);
            return;
        }
    };

    let (_story_data, actual) = match compile_and_explore_from_ink(&ink_path, config) {
        Ok(pair) => pair,
        Err(e) if e.starts_with("compile:") => {
            cat.cases_compile_error += 1;
            push_flagged(FlaggedStatus::CompileError);
            return;
        }
        Err(e) if e.starts_with("link:") => {
            cat.cases_link_error += 1;
            push_flagged(FlaggedStatus::CompileError);
            return;
        }
        Err(_) => {
            cat.cases_compile_error += 1;
            push_flagged(FlaggedStatus::CompileError);
            return;
        }
    };

    let actual_index = index_by_choice_path(&actual);
    let mut case_ok = true;
    let mut case_mismatch = 0usize;
    let mut case_missing = 0usize;

    for oracle_ep in &oracle_eps {
        let Some(brink_ep) = actual_index.get(oracle_ep.choice_path.as_slice()) else {
            cat.episodes_missing += 1;
            case_missing += 1;
            case_ok = false;
            continue;
        };
        let d = oracle::diff_oracle(oracle_ep, brink_ep);
        if d.matches {
            cat.episodes_pass += 1;
        } else {
            cat.episodes_fail += 1;
            case_mismatch += 1;
            case_ok = false;
        }
    }

    if let Some(issue) = issue {
        let verdict = mismatch_flag_verdict(Some(&issue), case_mismatch, case_missing);
        let status = if verdict == MismatchFlagVerdict::UnexpectedlyFixed {
            FlaggedStatus::UnexpectedlyFixed
        } else {
            FlaggedStatus::StillMismatching
        };
        flagged.push((rel.to_string(), issue, status));
    }

    if case_ok {
        cat.cases_pass += 1;
    } else {
        cat.cases_fail += 1;
    }
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

    // Issue #3402: cases carrying a `[source] expected_mismatch` flag in
    // their `metadata.toml`, so the residual documented-divergence backlog
    // is visible in this report rather than only living in a doc comment.
    // `(rel_path, issue, status)`, filled as each case is compared below —
    // looked up *before* any of the `continue` paths so a flagged case that
    // skips or fails to compile still gets a row instead of silently
    // vanishing from the backlog (that silence would read identically to
    // "the flag was removed").
    let mut flagged: Vec<(String, String, FlaggedStatus)> = Vec::new();

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
        classify_case(case_dir, &rel, &config, cat, &mut flagged);
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

    print_expected_mismatch_report(&flagged);

    native_corpus_report();
}

/// Issue #3402: list every case pinning a `[source] expected_mismatch`
/// flag in `metadata.toml`, with the issue it documents and whether it is
/// still in its expected (mismatching) state — making the residual
/// documented-divergence backlog visible here instead of only in a
/// `RATCHET_EPISODE_COUNT` doc comment. Sorted by `rel_path`: `flagged` is
/// built in `collect_oracle_cases`' own sorted order, so this is a stable
/// sort with no `HashMap` involved.
#[expect(
    clippy::print_stdout,
    reason = "this is a diagnostic report, not production output"
)]
fn print_expected_mismatch_report(flagged: &[(String, String, FlaggedStatus)]) {
    if flagged.is_empty() {
        return;
    }

    println!();
    println!("============================================================");
    println!("  EXPECTED-MISMATCH CASES (issue #3402) — documented C#-oracle divergences");
    println!("============================================================");
    for (rel, issue, status) in flagged {
        let marker = match status {
            FlaggedStatus::StillMismatching => "still mismatching, as expected",
            FlaggedStatus::UnexpectedlyFixed => {
                "⚠ NOW MATCHES THE ORACLE — remove the flag and raise RATCHET_EPISODE_COUNT"
            }
            FlaggedStatus::Skipped => "⚠ SKIPPED this run — flag status unverified, investigate",
            FlaggedStatus::CompileError => {
                "⚠ FAILS TO COMPILE/LINK this run — flag status unverified, investigate"
            }
        };
        println!("  {rel}  ({issue})  {marker}");
    }
    println!("============================================================");
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

/// A synthetic, disk-backed corpus case in a scratch directory — isolated
/// from the real `tests/` corpus (so it can never move `RATCHET_EPISODE_COUNT`
/// or the corpus stats) but real enough to drive [`classify_case`] through
/// its actual file-reading production functions (`expected_mismatch_issue`,
/// `oracle::load_oracle_episodes`, `compile_and_explore_from_ink`).
struct ScratchCorpusCase(PathBuf);

#[expect(
    clippy::expect_used,
    reason = "test fixture helper: panic on bad scratch I/O"
)]
impl ScratchCorpusCase {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "brink-test-harness-corpus-report-{name}-{}",
            std::process::id(),
        ));
        std::fs::create_dir_all(path.join("oracle")).expect("create scratch case dir");
        Self(path)
    }

    fn write_story(&self, content: &str) {
        std::fs::write(self.0.join("story.ink"), content).expect("write scratch story.ink");
    }

    fn write_metadata(&self, content: &str) {
        std::fs::write(self.0.join("metadata.toml"), content).expect("write scratch metadata.toml");
    }

    fn write_oracle_episode(&self, content: &str) {
        std::fs::write(self.0.join("oracle").join("e0.oracle.json"), content)
            .expect("write scratch oracle episode");
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ScratchCorpusCase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A minimal, valid `.oracle.json` — a single-step, no-choice episode.
/// Content doesn't matter here: this case is engineered to fail to compile
/// before the oracle episode is ever compared against.
const MINIMAL_ORACLE_EPISODE: &str = r#"{
  "steps": [
    {
      "text": "irrelevant\n",
      "tags": [],
      "outcome": "Ended",
      "variable_changes": {},
      "visit_changes": {},
      "turn_index": 0
    }
  ],
  "outcome": "Ended",
  "choice_path": [],
  "initial_state": {
    "variables": {},
    "turn_index": 0
  }
}
"#;

/// Regression for PR #3432 review Finding 1: `expected_mismatch_issue` used
/// to be looked up only *after* the compile-error `continue`, so a flagged
/// case that starts failing to compile silently vanished from
/// `corpus_report`'s "EXPECTED-MISMATCH CASES" backlog instead of surfacing
/// a row that needs attention. Reverting `classify_case` to look the flag up
/// after the early returns makes this test fail: `flagged` comes back empty
/// instead of carrying the `CompileError` row.
#[test]
fn flagged_case_that_fails_to_compile_still_appears_in_the_backlog() {
    let scratch = ScratchCorpusCase::new("flagged-compile-error");
    // Deliberately invalid ink syntax — an unterminated interpolation brace
    // is a hard parse error, guaranteed to fail `brink_compiler::compile_path`
    // with a "compile: ..." error.
    scratch.write_story("Hello, {unterminated interpolation\n");
    scratch.write_metadata(
        "description = \"synthetic flagged case that fails to compile\"\n\
         mode = \"runtime\"\n\
         \n\
         [source]\n\
         origin = \"brink\"\n\
         original_id = \"flagged-compile-error\"\n\
         expected_mismatch = \"#9999\"\n",
    );
    scratch.write_oracle_episode(MINIMAL_ORACLE_EPISODE);

    let config = ExploreConfig {
        max_depth: 20,
        max_episodes: 1000,
    };
    let mut cat = CategoryStats::default();
    let mut flagged: Vec<(String, String, FlaggedStatus)> = Vec::new();

    classify_case(
        scratch.path(),
        "synthetic/flagged-compile-error",
        &config,
        &mut cat,
        &mut flagged,
    );

    assert_eq!(
        cat.cases_compile_error, 1,
        "the synthetic case's invalid source must actually fail to compile"
    );
    assert_eq!(
        flagged,
        vec![(
            "synthetic/flagged-compile-error".to_string(),
            "#9999".to_string(),
            FlaggedStatus::CompileError,
        )],
        "a flagged case that fails to compile must still appear in the backlog"
    );
}
