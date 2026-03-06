#![allow(clippy::unwrap_used)]

//! Corpus test: compile every .ink file in the test corpus to ink.json and
//! compare structurally against the reference .ink.json produced by inklecate.
//!
//! Run with: `cargo test -p brink-compiler --test json_corpus`
//!
//! The test emits a tier-by-tier summary at the end and prints a detailed
//! diff for the first failure encountered.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

// ─── Discovery ──────────────────────────────────────────────────────

struct TestCase {
    /// e.g. "tier1/basics/I001-minimal-story"
    rel_path: String,
    ink_path: PathBuf,
    json_path: PathBuf,
    tier: String,
    category: String,
}

fn discover_corpus() -> Vec<TestCase> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut cases = Vec::new();

    for tier in &["tier1", "tier2", "tier3"] {
        let tier_dir = root.join(tier);
        if !tier_dir.exists() {
            continue;
        }
        for entry in walkdir(&tier_dir) {
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
                    tier: (*tier).to_string(),
                    category,
                });
            }
        }
    }

    cases.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    cases
}

/// Recursively find directories containing story.ink files.
fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.join("story.ink").exists() {
                    results.push(path.clone());
                }
                results.extend(walkdir(&path));
            }
        }
    }
    results
}

// ─── Comparison ─────────────────────────────────────────────────────

#[derive(Debug)]
enum CompareResult {
    Pass,
    CompileError(String),
    JsonMismatch { diff: String },
}

fn compare_one(case: &TestCase) -> CompareResult {
    let our_json =
        match brink_compiler::compile_to_json(case.ink_path.to_string_lossy().as_ref(), |p| {
            std::fs::read_to_string(p)
                .map_err(|e| std::io::Error::new(e.kind(), format!("{p}: {e}")))
        }) {
            Ok(j) => j,
            Err(e) => return CompareResult::CompileError(format!("{e}")),
        };

    let our_value: Value = serde_json::to_value(&our_json).unwrap();

    let ref_text = std::fs::read_to_string(&case.json_path).unwrap();
    let ref_value: Value = serde_json::from_str(&ref_text).unwrap();

    if our_value == ref_value {
        CompareResult::Pass
    } else {
        let diff = structural_diff(&ref_value, &our_value, "");
        CompareResult::JsonMismatch { diff }
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
        format!("{}...", &s[..117])
    } else {
        s
    }
}

// ─── Test ───────────────────────────────────────────────────────────

#[test]
fn json_corpus() {
    let cases = discover_corpus();
    assert!(!cases.is_empty(), "no test cases found");

    let mut tier_pass: BTreeMap<String, usize> = BTreeMap::new();
    let mut tier_fail: BTreeMap<String, usize> = BTreeMap::new();
    let mut tier_error: BTreeMap<String, usize> = BTreeMap::new();
    let mut cat_pass: BTreeMap<String, usize> = BTreeMap::new();
    let mut cat_fail: BTreeMap<String, usize> = BTreeMap::new();

    let mut first_failure: Option<(String, String)> = None;
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let result = compare_one(case);
        let tier_key = case.tier.clone();
        let cat_key = format!("{}/{}", case.tier, case.category);

        match &result {
            CompareResult::Pass => {
                *tier_pass.entry(tier_key).or_default() += 1;
                *cat_pass.entry(cat_key).or_default() += 1;
            }
            CompareResult::CompileError(msg) => {
                *tier_error.entry(tier_key).or_default() += 1;
                *cat_fail.entry(cat_key).or_default() += 1;
                failures.push(format!("  COMPILE ERROR: {}: {msg}", case.rel_path));
                if first_failure.is_none() {
                    first_failure = Some((case.rel_path.clone(), format!("Compile error: {msg}")));
                }
            }
            CompareResult::JsonMismatch { diff } => {
                *tier_fail.entry(tier_key).or_default() += 1;
                *cat_fail.entry(cat_key).or_default() += 1;
                failures.push(format!("  MISMATCH: {}", case.rel_path));
                if first_failure.is_none() {
                    first_failure = Some((case.rel_path.clone(), diff.clone()));
                }
            }
        }
    }

    // ── Summary ─────────────────────────────────────────────────────
    let total = cases.len();
    let total_pass: usize = tier_pass.values().sum();
    let total_fail: usize = tier_fail.values().sum();
    let total_error: usize = tier_error.values().sum();

    let mut summary = String::new();
    summary.push_str("\n╔══════════════════════════════════════════════════════════╗\n");
    let _ = writeln!(
        summary,
        "║  JSON CORPUS: {total_pass} pass / {total_fail} mismatch / {total_error} error  (of {total})"
    );
    summary.push_str("╠══════════════════════════════════════════════════════════╣\n");

    for tier in &["tier1", "tier2", "tier3"] {
        let t = (*tier).to_string();
        let p = tier_pass.get(&t).copied().unwrap_or(0);
        let f = tier_fail.get(&t).copied().unwrap_or(0);
        let e = tier_error.get(&t).copied().unwrap_or(0);
        let tot = p + f + e;
        if tot > 0 {
            let _ = writeln!(
                summary,
                "║  {tier:6}  {p:>4} pass  {f:>4} fail  {e:>4} err   ({tot} total)"
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
        let _ = writeln!(summary, "║  {marker} {cat:<30}  {p:>3}/{:>3}", p + f);
    }

    summary.push_str("╚══════════════════════════════════════════════════════════╝\n");

    eprintln!("{summary}");

    if let Some((path, diff)) = &first_failure {
        eprintln!("─── First failure: {path} ───");
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
}
