#![allow(clippy::unwrap_used)]

//! Corpus test: compile every .ink file in the test corpus to ink.json and
//! compare structurally against the reference .ink.json produced by inklecate.
//!
//! Run with: `cargo test -p brink-compiler --test json_corpus`
//!
//! The test emits a tier-by-tier summary at the end and prints a detailed
//! diff for **only the first failure** encountered. This is intentional —
//! fix this one next, then bump the ratchet and repeat.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

// ─── Ratchet ────────────────────────────────────────────────────────
//
// Bump this after each fix. The test fails if the pass count drops
// below this threshold, preventing regressions.
const RATCHET_PASS_COUNT: usize = 202;

// ─── Discovery ──────────────────────────────────────────────────────

struct TestCase {
    /// e.g. "tier1/basics/I001-minimal-story"
    rel_path: String,
    ink_path: PathBuf,
    json_path: PathBuf,
    suite: String,
    category: String,
}

fn discover_corpus() -> Vec<TestCase> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut cases = Vec::new();

    // Tiered suites: tests/tier{1,2,3}/**/story.ink + story.ink.json
    for tier in &["tier1", "tier2", "tier3"] {
        let tier_dir = root.join(tier);
        if !tier_dir.exists() {
            continue;
        }
        for entry in walkdir_story(&tier_dir) {
            let ink = entry.join("story.ink");
            let json = entry.join("story.ink.json");
            if ink.exists() && json.exists() {
                let rel = entry
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let parts: Vec<&str> = rel.split('/').collect();
                let category = parts.get(1).unwrap_or(&"unknown").to_string();
                cases.push(TestCase {
                    rel_path: rel,
                    ink_path: ink,
                    json_path: json,
                    suite: (*tier).to_string(),
                    category,
                });
            }
        }
    }

    // GitHub and patched suites: tests/tests_{github,patched}/**/*.ink + *.ink.json
    for suite in &["tests_github", "tests_patched"] {
        let suite_dir = root.join(suite);
        if !suite_dir.exists() {
            continue;
        }
        for json_path in walkdir_ink_json(&suite_dir) {
            let ink_path =
                PathBuf::from(json_path.to_string_lossy().strip_suffix(".json").unwrap());
            if !ink_path.exists() {
                continue;
            }
            let rel = json_path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .to_string()
                .strip_suffix(".ink.json")
                .unwrap()
                .to_string();
            let parts: Vec<&str> = rel.split('/').collect();
            let category = parts.get(1).unwrap_or(&"unknown").to_string();
            cases.push(TestCase {
                rel_path: rel,
                ink_path,
                json_path,
                suite: (*suite).to_string(),
                category,
            });
        }
    }

    // Sort tiers first (tier1, tier2, tier3), then github/patched suites.
    cases.sort_by(|a, b| {
        fn suite_order(s: &str) -> u8 {
            match s {
                "tier1" => 0,
                "tier2" => 1,
                "tier3" => 2,
                "tests_patched" => 3,
                "tests_github" => 4,
                _ => 5,
            }
        }
        suite_order(&a.suite)
            .cmp(&suite_order(&b.suite))
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    cases
}

/// Recursively find directories containing story.ink files.
fn walkdir_story(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.join("story.ink").exists() {
                    results.push(path.clone());
                }
                results.extend(walkdir_story(&path));
            }
        }
    }
    results
}

/// Recursively find *.ink.json files.
fn walkdir_ink_json(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(walkdir_ink_json(&path));
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".ink.json"))
            {
                results.push(path);
            }
        }
    }
    results
}

// ─── Comparison ─────────────────────────────────────────────────────

#[derive(Debug)]
enum CompareResult {
    Pass,
    CompileError {
        message: String,
        /// Diagnostic codes from `CompileError::Diagnostics`, if any.
        codes: Vec<String>,
    },
    JsonMismatch {
        diff: String,
    },
}

fn compare_one(case: &TestCase) -> CompareResult {
    let our_json =
        match brink_compiler::compile_to_json(case.ink_path.to_string_lossy().as_ref(), |p| {
            std::fs::read_to_string(p)
                .map_err(|e| std::io::Error::new(e.kind(), format!("{p}: {e}")))
        }) {
            Ok(j) => j,
            Err(e) => {
                let codes = extract_error_codes(&e);
                return CompareResult::CompileError {
                    message: format!("{e}"),
                    codes,
                };
            }
        };

    let our_value: Value = serde_json::to_value(&our_json).unwrap();

    let ref_text = std::fs::read_to_string(&case.json_path).unwrap();
    let ref_text = ref_text.strip_prefix('\u{FEFF}').unwrap_or(&ref_text);
    let ref_value: Value = match serde_json::from_str(ref_text) {
        Ok(v) => v,
        Err(e) => {
            return CompareResult::CompileError {
                message: format!("bad reference json: {e}"),
                codes: Vec::new(),
            };
        }
    };

    if our_value == ref_value {
        CompareResult::Pass
    } else {
        let diff = structural_diff(&ref_value, &our_value, "");
        CompareResult::JsonMismatch { diff }
    }
}

fn extract_error_codes(err: &brink_compiler::CompileError) -> Vec<String> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags
            .iter()
            .map(|d| format!("{}:{}", d.code.as_str(), d.message))
            .collect(),
        _ => Vec::new(),
    }
}

/// Produce a human-readable structural diff between two JSON values.
fn structural_diff(expected: &Value, actual: &Value, path: &str) -> String {
    let mut diffs = Vec::new();
    collect_diffs(expected, actual, path, &mut diffs);
    if diffs.len() > 40 {
        let total = diffs.len();
        diffs.truncate(40);
        diffs.push(format!("  ... and {} more differences", total - 40));
    }
    diffs.join("\n")
}

fn collect_diffs(expected: &Value, actual: &Value, path: &str, out: &mut Vec<String>) {
    if expected == actual {
        return;
    }

    match (expected, actual) {
        (Value::Object(exp_map), Value::Object(act_map)) => {
            for key in exp_map.keys() {
                let child_path = format!("{path}.{key}");
                if let Some(act_val) = act_map.get(key) {
                    collect_diffs(&exp_map[key], act_val, &child_path, out);
                } else {
                    out.push(format!(
                        "  MISSING {child_path}: expected {}",
                        truncate_json(&exp_map[key])
                    ));
                }
            }
            for key in act_map.keys() {
                if !exp_map.contains_key(key) {
                    let child_path = format!("{path}.{key}");
                    out.push(format!(
                        "  EXTRA   {child_path}: got {}",
                        truncate_json(&act_map[key])
                    ));
                }
            }
        }
        (Value::Array(exp_arr), Value::Array(act_arr)) => {
            let max_len = exp_arr.len().max(act_arr.len());
            if exp_arr.len() != act_arr.len() {
                out.push(format!(
                    "  LENGTH  {path}: expected {} elements, got {}",
                    exp_arr.len(),
                    act_arr.len()
                ));
            }
            for i in 0..max_len {
                let child_path = format!("{path}[{i}]");
                match (exp_arr.get(i), act_arr.get(i)) {
                    (Some(e), Some(a)) => collect_diffs(e, a, &child_path, out),
                    (Some(e), None) => {
                        out.push(format!(
                            "  MISSING {child_path}: expected {}",
                            truncate_json(e)
                        ));
                    }
                    (None, Some(a)) => {
                        out.push(format!("  EXTRA   {child_path}: got {}", truncate_json(a)));
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            out.push(format!(
                "  DIFF    {path}: expected {}, got {}",
                truncate_json(expected),
                truncate_json(actual)
            ));
        }
    }
}

fn truncate_json(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_else(|_| format!("{v:?}"));
    if s.len() > 120 {
        // Find a char boundary at or before byte 117
        let mut end = 117;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    } else {
        s
    }
}

// ─── Test ───────────────────────────────────────────────────────────

#[test]
fn json_corpus() {
    let cases = discover_corpus();
    assert!(!cases.is_empty(), "no test cases found");

    let mut suite_pass: BTreeMap<String, usize> = BTreeMap::new();
    let mut suite_fail: BTreeMap<String, usize> = BTreeMap::new();
    let mut suite_error: BTreeMap<String, usize> = BTreeMap::new();
    let mut cat_pass: BTreeMap<String, usize> = BTreeMap::new();
    let mut cat_fail: BTreeMap<String, usize> = BTreeMap::new();

    // Per-error-code: how many diagnostics total, and how many files hit it.
    let mut code_diag_count: BTreeMap<String, usize> = BTreeMap::new();
    let mut code_file_count: BTreeMap<String, usize> = BTreeMap::new();
    let mut files_with_diag_codes: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();

    // Only the first failure diff is shown — fix this one next.
    let mut first_failure: Option<(String, String)> = None;
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let result = compare_one(case);
        let suite_key = case.suite.clone();
        let cat_key = format!("{}/{}", case.suite, case.category);

        match &result {
            CompareResult::Pass => {
                *suite_pass.entry(suite_key).or_default() += 1;
                *cat_pass.entry(cat_key).or_default() += 1;
            }
            CompareResult::CompileError { message, codes } => {
                *suite_error.entry(suite_key).or_default() += 1;
                *cat_fail.entry(cat_key).or_default() += 1;
                failures.push(format!("  COMPILE ERROR: {}: {message}", case.rel_path));
                for code in codes {
                    failures.push(format!("    {code}"));
                }
                if first_failure.is_none() {
                    first_failure =
                        Some((case.rel_path.clone(), format!("Compile error: {message}")));
                }

                // Track per-code frequencies
                if !codes.is_empty() {
                    files_with_diag_codes.insert(case.rel_path.clone());
                }
                let mut seen_codes = std::collections::BTreeSet::new();
                for code in codes {
                    *code_diag_count.entry(code.clone()).or_default() += 1;
                    seen_codes.insert(code.clone());
                }
                for code in seen_codes {
                    *code_file_count.entry(code).or_default() += 1;
                }
            }
            CompareResult::JsonMismatch { diff } => {
                *suite_fail.entry(suite_key).or_default() += 1;
                *cat_fail.entry(cat_key).or_default() += 1;
                failures.push(format!("  MISMATCH: {}", case.rel_path));
                if first_failure.is_none() {
                    first_failure = Some((case.rel_path.clone(), diff.clone()));
                }
            }
        }
    }

    let total = cases.len();
    let total_pass: usize = suite_pass.values().sum();
    let total_fail: usize = suite_fail.values().sum();
    let total_error: usize = suite_error.values().sum();

    let summary = format_summary(
        total,
        total_pass,
        total_fail,
        total_error,
        &suite_pass,
        &suite_fail,
        &suite_error,
        &cat_pass,
        &cat_fail,
        &code_diag_count,
        &code_file_count,
        files_with_diag_codes.len(),
    );
    eprintln!("{summary}");

    if let Some((path, diff)) = &first_failure {
        eprintln!("─── First failure (fix this one next): {path} ───");
        eprintln!("{diff}");
        eprintln!("────────────────────────────────────────────────────────");
    }

    if !failures.is_empty() {
        eprintln!("\nAll failures ({}):", failures.len());
        for f in &failures {
            eprintln!("{f}");
        }
    }

    assert!(total > 0, "should have found test cases");

    // Ratchet: pass count must not drop below the established baseline.
    assert!(
        total_pass >= RATCHET_PASS_COUNT,
        "REGRESSION: pass count {total_pass} dropped below ratchet {RATCHET_PASS_COUNT}"
    );
}

#[expect(clippy::too_many_arguments)]
fn format_summary(
    total: usize,
    total_pass: usize,
    total_fail: usize,
    total_error: usize,
    suite_pass: &BTreeMap<String, usize>,
    suite_fail: &BTreeMap<String, usize>,
    suite_error: &BTreeMap<String, usize>,
    cat_pass: &BTreeMap<String, usize>,
    cat_fail: &BTreeMap<String, usize>,
    code_diag_count: &BTreeMap<String, usize>,
    code_file_count: &BTreeMap<String, usize>,
    files_with_diag_codes: usize,
) -> String {
    let mut summary = String::new();
    summary.push_str("\n╔══════════════════════════════════════════════════════════╗\n");
    let _ = writeln!(
        summary,
        "║  JSON CORPUS: {total_pass} pass / {total_fail} mismatch / {total_error} error  (of {total})"
    );
    summary.push_str("╠══════════════════════════════════════════════════════════╣\n");

    for suite in &["tier1", "tier2", "tier3", "tests_github", "tests_patched"] {
        let t = (*suite).to_string();
        let p = suite_pass.get(&t).copied().unwrap_or(0);
        let f = suite_fail.get(&t).copied().unwrap_or(0);
        let e = suite_error.get(&t).copied().unwrap_or(0);
        let tot = p + f + e;
        if tot > 0 {
            let _ = writeln!(
                summary,
                "║  {suite:14}  {p:>4} pass  {f:>4} fail  {e:>4} err   ({tot} total)"
            );
        }
    }

    summary.push_str("╠══════════════════════════════════════════════════════════╣\n");

    let mut all_cats: Vec<&String> = cat_pass
        .keys()
        .chain(cat_fail.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    all_cats.sort();

    for cat in &all_cats {
        let p = cat_pass.get(*cat).copied().unwrap_or(0);
        let f = cat_fail.get(*cat).copied().unwrap_or(0);
        let marker = if f > 0 { "✗" } else { "✓" };
        let _ = writeln!(summary, "║  {marker} {cat:<40}  {p:>3}/{:>3}", p + f);
    }

    if total_error > 0 {
        summary.push_str("╠══════════════════════════════════════════════════════════╣\n");
        summary.push_str("║  Error codes (diagnostics / files affected):            ║\n");
        let mut codes: Vec<_> = code_diag_count.iter().collect();
        codes.sort_by(|a, b| b.1.cmp(a.1));
        for (code, diag_n) in &codes {
            let file_n = code_file_count.get(*code).copied().unwrap_or(0);
            let _ = writeln!(summary, "║    {code}  {diag_n:>5} diags  {file_n:>5} files");
        }
        let non_diag_errors = total_error.saturating_sub(files_with_diag_codes);
        if non_diag_errors > 0 {
            let _ = writeln!(
                summary,
                "║    (I/O / bad ref json)         {non_diag_errors:>5} files"
            );
        }
    }

    summary.push_str("╚══════════════════════════════════════════════════════════╝\n");
    summary
}
