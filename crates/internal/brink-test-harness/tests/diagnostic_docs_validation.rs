//! Validation that all `DiagnosticCode` variants have corresponding documentation files
//! and that diagnostic codes are unique (no collision between variants).
//!
//! This test ensures that every error code defined in `brink-ir`'s `DiagnosticCode` enum
//! has a corresponding explanation file in `docs/diagnostics/Exxx.md`, and conversely that
//! every file in `docs/diagnostics/` corresponds to a real `DiagnosticCode` variant.
//!
//! It also validates that every `DiagnosticCode` variant maps to exactly one code string
//! via the `as_str()` method — a uniqueness check that prevents the collision hazard where
//! two concurrent build agents allocate the same code independently. This check is built
//! from [`DiagnosticCode::ALL`], the exhaustive variant list, rather than probed through
//! [`DiagnosticCode::from_str_code`] — `from_str_code` is `str -> Option<Self>`, so probing
//! it can only ever discover a many-to-one mapping in the wrong direction (many code
//! strings to one variant, which is not the hazard we care about); it can never surface
//! two variants returning the *same* code string from `as_str()`, which is.
//!
//! The known-code list used for the doc-file checks is derived from the enum itself (via
//! [`DiagnosticCode::from_str_code`]) rather than hand-copied, so a newly added variant is
//! picked up automatically instead of silently passing a stale list.
//!
//! If this test fails when you add a new diagnostic code, you must:
//! - Add the new variant to [`DiagnosticCode::ALL`],
//! - Add a corresponding `docs/diagnostics/Exxx.md` file,
//! - Ensure no other variant returns the same code string from `as_str()`, and
//! - Not skip a number or delete a variant from the middle of the range — codes are
//!   never reused once assigned (retire in place with a `RETIRED` doc comment instead),
//!   so the contiguity check enforces that the numeric sequence has no gaps.

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
    // Enumerate the exhaustive variant list and build the inverse map: code string ->
    // every variant whose `as_str()` produces it. Unlike probing `from_str_code` over
    // `E001..E999` (which is `str -> Option<Self>` and can never surface two variants
    // colliding on the same string), this walks `Self -> str` directly, so a genuine
    // collision produces a BTreeMap key with more than one entry.
    let mut code_to_variants: BTreeMap<&'static str, Vec<DiagnosticCode>> = BTreeMap::new();
    for &variant in DiagnosticCode::ALL {
        code_to_variants.entry(variant.as_str()).or_default().push(variant);
    }

    // Round-trip check: as_str() and from_str_code() must stay synchronized, or the
    // variant becomes unreachable via its own code string.
    for &variant in DiagnosticCode::ALL {
        let code_str = variant.as_str();
        assert_eq!(
            DiagnosticCode::from_str_code(code_str),
            Some(variant),
            "DiagnosticCode variant {variant:?} did not round-trip: \
             from_str_code({code_str}) did not produce {variant:?} back. \
             This indicates inconsistency between as_str() and from_str_code()."
        );
    }

    // ALL must agree with the from_str_code-derived known-code list in size, or ALL is
    // stale (a variant was added to the enum but not to ALL).
    let known_codes = all_diagnostic_code_strings();
    assert_eq!(
        DiagnosticCode::ALL.len(),
        known_codes.len(),
        "DiagnosticCode::ALL has {} entries but from_str_code recognizes {} code strings — \
         ALL is out of sync with the enum. Add the missing variant(s) to ALL.",
        DiagnosticCode::ALL.len(),
        known_codes.len()
    );

    // Now verify no code string maps to multiple variants.
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
    // or *after* the max, but not in the middle.) Codes are never reused once
    // assigned — retire a code in place with a `RETIRED` doc comment on its variant
    // rather than deleting it from the enum, or this check will fail.
    if let (Some(first_code), Some(last_code)) = (
        code_to_variants.keys().next(),
        code_to_variants.keys().last(),
    ) {
        let first_num: u32 = first_code[1..]
            .parse()
            .unwrap_or_else(|e| panic!("diagnostic code {first_code} has a non-numeric suffix: {e}"));
        let last_num: u32 = last_code[1..]
            .parse()
            .unwrap_or_else(|e| panic!("diagnostic code {last_code} has a non-numeric suffix: {e}"));

        let mut gaps = Vec::new();
        for num in first_num..=last_num {
            let code_str = format!("E{num:03}");
            if !code_to_variants.contains_key(code_str.as_str()) {
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
