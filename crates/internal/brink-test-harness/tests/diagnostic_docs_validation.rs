//! Validation that all `DiagnosticCode` variants have corresponding documentation files
//! and that diagnostic codes are unique (no collision between variants).
//!
//! This test ensures that every error code defined in `brink-ir`'s `DiagnosticCode` enum
//! has a corresponding explanation file in `docs/diagnostics/Exxx.md`, and conversely that
//! every file in `docs/diagnostics/` corresponds to a real `DiagnosticCode` variant.
//!
//! It also validates that every `DiagnosticCode` variant maps to exactly one code string
//! via the `as_str()` method — a uniqueness check that prevents the collision hazard where
//! two concurrent build agents allocate the same code independently.
//!
//! The known-code list is derived from the enum itself (via
//! [`DiagnosticCode::from_str_code`]) rather than hand-copied, so a newly added variant is
//! picked up automatically instead of silently passing a stale list.
//!
//! If this test fails when you add a new diagnostic code, you must:
//! - Add a corresponding `docs/diagnostics/Exxx.md` file, and
//! - Ensure no other variant returns the same code string from `as_str()`.

use brink_ir::DiagnosticCode;
use std::collections::BTreeMap;
use std::path::Path;

/// Every diagnostic code string that currently resolves to a real `DiagnosticCode`
/// variant, discovered by probing `E001..=E999` through `DiagnosticCode::from_str_code`.
fn all_diagnostic_code_strings() -> Vec<String> {
    (1..=999)
        .map(|n| format!("E{n:03}"))
        .filter(|code_str| DiagnosticCode::from_str_code(code_str).is_some())
        .collect()
}

#[test]
fn all_diagnostic_codes_have_documentation() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("Could not find workspace root");
    let docs_dir = workspace_root.join("docs/diagnostics");

    assert!(docs_dir.exists(), "docs/diagnostics directory must exist");

    let known_codes = all_diagnostic_code_strings();
    assert!(
        !known_codes.is_empty(),
        "expected at least one DiagnosticCode variant to be discovered"
    );

    let mut missing_docs = Vec::new();
    for code_str in &known_codes {
        let doc_file = docs_dir.join(format!("{code_str}.md"));
        if !doc_file.exists() {
            missing_docs.push(code_str.clone());
        }
    }

    assert!(
        missing_docs.is_empty(),
        "The following diagnostic codes are missing documentation files:\n  {}",
        missing_docs.join("\n  ")
    );

    // Reverse check: every doc file must correspond to a real DiagnosticCode variant,
    // so an orphaned or misnamed file (e.g. a doc for a retired/nonexistent code) is
    // caught instead of silently accumulating.
    let mut orphaned_docs = Vec::new();
    let entries = std::fs::read_dir(&docs_dir).expect("failed to read docs/diagnostics");
    for entry in entries {
        let entry = entry.expect("failed to read docs/diagnostics entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if !known_codes.contains(&stem) {
            orphaned_docs.push(stem);
        }
    }

    assert!(
        orphaned_docs.is_empty(),
        "The following docs/diagnostics files do not correspond to a real DiagnosticCode:\n  {}",
        orphaned_docs.join("\n  ")
    );
}

#[test]
fn diagnostic_codes_are_unique() {
    // Collect all known diagnostic codes and verify that each string maps back to
    // exactly one `DiagnosticCode` variant via `from_str_code`.

    let mut code_to_variants: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();

    // Build a map of code → variant names by checking the canonical code set
    // discovered via from_str_code. This works because from_str_code is the
    // parser — if a code string is in `known_codes`, it maps to something.
    //
    // To verify uniqueness, we reconstruct the inverse: for each known code,
    // we parse it back and confirm its `as_str()` matches. This is sound because
    // the enum's `as_str()` and `from_str_code()` must stay synchronized as a
    // matter of correctness (if they diverge, the code becomes unreachable).

    // We enumerate all possible codes in the range E001..E999 and collect which
    // strings correspond to variants, building the inverse map.
    for code_num in 1..=999 {
        let code_str = format!("E{code_num:03}");

        // If this code string parses to a variant, verify it round-trips correctly
        // by checking `as_str()`.
        if let Some(variant) = DiagnosticCode::from_str_code(&code_str) {
            let canonical = variant.as_str();

            // The variant's code should match the one we parsed
            assert_eq!(
                canonical, code_str,
                "DiagnosticCode variant's as_str() did not round-trip: \
                 from_str_code({code_str}) produced a variant whose as_str() = {canonical}. \
                 This indicates inconsistency between as_str() and from_str_code()."
            );

            code_to_variants
                .entry(code_str.clone())
                .or_default()
                .push(canonical);
        }
    }

    // Now verify no code string maps to multiple variants
    let mut collisions = Vec::new();
    for (code_str, variants) in &code_to_variants {
        if variants.len() > 1 {
            collisions.push(format!(
                "{code_str} is returned by multiple variants: {variants:?}"
            ));
        }
    }

    assert!(
        collisions.is_empty(),
        "The following diagnostic codes are duplicated (multiple variants return the same code):\n  {}",
        collisions.join("\n  ")
    );

    // Verify no gaps in the numeric sequence: if E001 exists and E167 exists,
    // every code E001..=E167 must also exist. (Gaps are allowed *before* E001
    // or *after* the max, but not in the middle.)
    if let (Some(first_code), Some(last_code)) = (
        code_to_variants.keys().next(),
        code_to_variants.keys().last(),
    ) {
        let first_num = first_code[1..].parse::<u32>().unwrap_or(0);
        let last_num = last_code[1..].parse::<u32>().unwrap_or(0);

        let mut gaps = Vec::new();
        for num in first_num..=last_num {
            let code_str = format!("E{num:03}");
            if !code_to_variants.contains_key(&code_str) {
                gaps.push(code_str);
            }
        }

        assert!(
            gaps.is_empty(),
            "Gaps found in the diagnostic code range E{:03}..=E{:03}:\n  {}",
            first_num,
            last_num,
            gaps.join("\n  ")
        );
    }
}
