//! Validation that all `DiagnosticCode` variants have corresponding documentation files.
//!
//! This test ensures that every error code defined in `brink-ir`'s `DiagnosticCode` enum
//! has a corresponding explanation file in `docs/diagnostics/Exxx.md`, and conversely that
//! every file in `docs/diagnostics/` corresponds to a real `DiagnosticCode` variant.
//!
//! The known-code list is derived from the enum itself (via
//! [`DiagnosticCode::from_str_code`]) rather than hand-copied, so a newly added variant is
//! picked up automatically instead of silently passing a stale list.
//!
//! If this test fails when you add a new diagnostic code, you must add a corresponding
//! `docs/diagnostics/Exxx.md` file before the PR can land.

use brink_ir::DiagnosticCode;
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
