//! Dialect conformance corpus runner (#368).
//!
//! Loads the interpreter-agnostic JSON corpus at `tests/dialect_fixtures/
//! at_cue.json` (repo root) and drives it through the real
//! `line_contexts_with_dialect` pipeline — the same path the wasm boundary
//! (`brink-web`) and CLI use. A future TS interpreter is expected to load
//! the identical JSON and assert the same `expect` shapes, so this file
//! must never encode Rust-specific fixture semantics — only the documented
//! `chain_after`/`chain_after_attrs`/`expect` contract.

use std::path::PathBuf;

use brink_ide::line_context::line_contexts_with_dialect;
use brink_ir::{DialogueDialect, FileId, ResolvedDialect, hir};
use serde_json::Value;

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("dialect_fixtures")
        .join("at_cue.json")
}

/// Classify `line` in isolation, or immediately after a synthetic line that
/// itself classifies as `chain_after` (when present), exactly reproducing
/// the real editor pipeline (a knot body with one or two content lines).
/// Returns `Err` for a malformed fixture (unknown `chain_after` kind, or an
/// empty result set) rather than panicking, so the crate's `panic`/
/// `expect_used` lints don't need a helper-function exemption.
fn classify_case(
    dialect: &ResolvedDialect,
    line: &str,
    chain_after: Option<&str>,
) -> Result<Value, String> {
    // No trailing newline after `line` — `line_contexts` emits one extra
    // (blank) entry for a trailing '\n', which would otherwise shift the
    // target line away from `ctx.last()`.
    let source = match chain_after {
        // The chain-context synthetic predecessor must itself be a real
        // matching line for the declared kind so the chain pass sees a
        // genuine `dialect` on the previous `LineContext`. For "character"
        // and "parenthetical" the at-cue preset's own templates are reused;
        // for "dialogue" (chain-only), a plain narrative-after-cue line
        // stands in for "already inside a chained run".
        Some("character") => format!("=== start ===\n@Alice:<>\n{line}"),
        Some("parenthetical") => format!("=== start ===\n(warmly)<>\n{line}"),
        Some("dialogue") => format!("=== start ===\n@Alice:<>\nFirst dialogue line.\n{line}"),
        Some(other) => return Err(format!("fixture uses unknown chain_after kind '{other}'")),
        None => format!("=== start ===\n{line}"),
    };

    let parse = brink_syntax::parse(&source);
    let file_id = FileId(0);
    let ast = parse.tree();
    let (hir, _, _) = hir::lower(file_id, &ast);
    let ctx = line_contexts_with_dialect(&hir, &source, &parse.syntax(), dialect);

    // The target line is always the last content line we appended.
    let Some(target) = ctx.last() else {
        return Err("line_contexts produced no lines".to_owned());
    };
    Ok(match &target.dialect {
        Some(d) => {
            let attrs: serde_json::Map<String, Value> = d
                .attrs
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            serde_json::json!({ "kind": d.kind, "attrs": attrs })
        }
        None => Value::Null,
    })
}

#[test]
fn at_cue_conformance_corpus() {
    let raw = std::fs::read_to_string(fixtures_path()).expect("fixture file readable");
    let corpus: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
    let cases = corpus["cases"].as_array().expect("cases is an array");
    assert!(!cases.is_empty(), "corpus must not be empty");

    let dialect =
        ResolvedDialect::compile(&DialogueDialect::default()).expect("at-cue preset compiles");

    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap_or("<unnamed>");
        let line = case["line"].as_str().expect("case.line is a string");
        let chain_after = case["chain_after"].as_str();
        let expected = &case["expect"];

        match classify_case(&dialect, line, chain_after) {
            Ok(actual) if &actual == expected => {}
            Ok(actual) => failures.push(format!(
                "case '{id}' (line {line:?}, chain_after {chain_after:?}): expected {expected}, got {actual}"
            )),
            Err(e) => failures.push(format!("case '{id}': {e}")),
        }
    }

    assert!(
        failures.is_empty(),
        "dialect conformance corpus failures:\n{}",
        failures.join("\n")
    );
}

/// The corpus must contain at least one positive and one negative fixture —
/// otherwise it isn't exercising the "declared kinds classify, near-misses
/// don't" contract the spec requires.
#[test]
fn corpus_has_positive_and_negative_fixtures() {
    let raw = std::fs::read_to_string(fixtures_path()).expect("fixture file readable");
    let corpus: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
    let cases = corpus["cases"].as_array().expect("cases is an array");

    let positives = cases.iter().filter(|c| !c["expect"].is_null()).count();
    let negatives = cases.iter().filter(|c| c["expect"].is_null()).count();
    assert!(positives > 0, "corpus must include positive fixtures");
    assert!(negatives > 0, "corpus must include negative fixtures");
}
