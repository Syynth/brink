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
        let tier_pct = if tier_testable > 0 {
            tier_pass * 100 / tier_testable
        } else {
            0
        };

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
            let pct = if testable > 0 {
                s.cases_pass * 100 / testable
            } else {
                0
            };
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

    let grand_pct = if grand_cases_total > 0 {
        grand_cases_pass * 100 / grand_cases_total
    } else {
        0
    };
    let ep_pct = if grand_episodes_total > 0 {
        grand_episodes_pass * 100 / grand_episodes_total
    } else {
        0
    };

    println!("============================================================");
    println!("  OVERALL — {grand_cases_pass}/{grand_cases_total} cases passing ({grand_pct}%)",);
    println!("  EPISODES — {grand_episodes_pass}/{grand_episodes_total} passing ({ep_pct}%)",);
    println!("============================================================");
}
