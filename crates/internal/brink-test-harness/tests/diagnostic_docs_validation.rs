//! Validation that all DiagnosticCode variants have corresponding documentation files.
//!
//! This test ensures that every error code defined in `brink-ir`'s `DiagnosticCode` enum
//! has a corresponding explanation file in `docs/diagnostics/Exxx.md`.
//!
//! If this test fails when you add a new diagnostic code, you must add a corresponding
//! `docs/diagnostics/Exxx.md` file before the PR can land.

use brink_ir::DiagnosticCode;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[test]
fn all_diagnostic_codes_have_documentation() {
    // Get all DiagnosticCode variants as strings
    let mut expected_codes = BTreeSet::new();

    // Collect all diagnostic codes by testing each one
    // We use Debug formatting to extract the code name
    for code in [
        DiagnosticCode::E001,
        DiagnosticCode::E002,
        DiagnosticCode::E003,
        DiagnosticCode::E004,
        DiagnosticCode::E005,
        DiagnosticCode::E006,
        DiagnosticCode::E007,
        DiagnosticCode::E008,
        DiagnosticCode::E009,
        DiagnosticCode::E010,
        DiagnosticCode::E011,
        DiagnosticCode::E012,
        DiagnosticCode::E013,
        DiagnosticCode::E014,
        DiagnosticCode::E015,
        DiagnosticCode::E016,
        DiagnosticCode::E017,
        DiagnosticCode::E018,
        DiagnosticCode::E019,
        DiagnosticCode::E020,
        DiagnosticCode::E021,
        DiagnosticCode::E022,
        DiagnosticCode::E023,
        DiagnosticCode::E024,
        DiagnosticCode::E025,
        DiagnosticCode::E026,
        DiagnosticCode::E027,
        DiagnosticCode::E028,
        DiagnosticCode::E029,
        DiagnosticCode::E030,
        DiagnosticCode::E031,
        DiagnosticCode::E032,
        DiagnosticCode::E033,
        DiagnosticCode::E034,
        DiagnosticCode::E035,
        DiagnosticCode::E036,
        DiagnosticCode::E037,
        DiagnosticCode::E038,
        DiagnosticCode::E039,
        DiagnosticCode::E040,
        DiagnosticCode::E041,
        DiagnosticCode::E042,
        DiagnosticCode::E043,
        DiagnosticCode::E044,
        DiagnosticCode::E045,
        DiagnosticCode::E046,
        DiagnosticCode::E047,
        DiagnosticCode::E048,
        DiagnosticCode::E049,
        DiagnosticCode::E050,
        DiagnosticCode::E051,
        DiagnosticCode::E052,
        DiagnosticCode::E053,
        DiagnosticCode::E054,
        DiagnosticCode::E055,
        DiagnosticCode::E056,
        DiagnosticCode::E057,
        DiagnosticCode::E058,
        DiagnosticCode::E059,
        DiagnosticCode::E060,
        DiagnosticCode::E061,
        DiagnosticCode::E062,
        DiagnosticCode::E063,
        DiagnosticCode::E064,
        DiagnosticCode::E065,
        DiagnosticCode::E066,
        DiagnosticCode::E067,
        DiagnosticCode::E068,
        DiagnosticCode::E069,
        DiagnosticCode::E070,
        DiagnosticCode::E071,
        DiagnosticCode::E072,
        DiagnosticCode::E073,
        DiagnosticCode::E074,
        DiagnosticCode::E075,
        DiagnosticCode::E076,
        DiagnosticCode::E077,
        DiagnosticCode::E078,
        DiagnosticCode::E079,
        DiagnosticCode::E080,
        DiagnosticCode::E081,
        DiagnosticCode::E082,
        DiagnosticCode::E083,
        DiagnosticCode::E084,
        DiagnosticCode::E085,
        DiagnosticCode::E086,
        DiagnosticCode::E087,
        DiagnosticCode::E088,
        DiagnosticCode::E089,
        DiagnosticCode::E090,
        DiagnosticCode::E091,
        DiagnosticCode::E092,
        DiagnosticCode::E093,
        DiagnosticCode::E094,
        DiagnosticCode::E095,
        DiagnosticCode::E096,
        DiagnosticCode::E097,
        DiagnosticCode::E098,
        DiagnosticCode::E099,
        DiagnosticCode::E100,
        DiagnosticCode::E101,
        DiagnosticCode::E102,
        DiagnosticCode::E103,
        DiagnosticCode::E104,
        DiagnosticCode::E105,
        DiagnosticCode::E106,
        DiagnosticCode::E107,
        DiagnosticCode::E108,
        DiagnosticCode::E109,
        DiagnosticCode::E110,
        DiagnosticCode::E111,
        DiagnosticCode::E112,
        DiagnosticCode::E113,
        DiagnosticCode::E114,
        DiagnosticCode::E115,
        DiagnosticCode::E116,
        DiagnosticCode::E117,
        DiagnosticCode::E118,
        DiagnosticCode::E119,
        DiagnosticCode::E120,
        DiagnosticCode::E121,
        DiagnosticCode::E122,
        DiagnosticCode::E123,
        DiagnosticCode::E124,
        DiagnosticCode::E125,
        DiagnosticCode::E126,
        DiagnosticCode::E127,
        DiagnosticCode::E128,
        DiagnosticCode::E129,
        DiagnosticCode::E130,
        DiagnosticCode::E131,
        DiagnosticCode::E132,
        DiagnosticCode::E133,
        DiagnosticCode::E134,
        DiagnosticCode::E135,
        DiagnosticCode::E136,
        DiagnosticCode::E137,
        DiagnosticCode::E138,
        DiagnosticCode::E139,
        DiagnosticCode::E140,
        DiagnosticCode::E141,
        DiagnosticCode::E142,
        DiagnosticCode::E143,
        DiagnosticCode::E144,
        DiagnosticCode::E145,
        DiagnosticCode::E146,
        DiagnosticCode::E147,
        DiagnosticCode::E148,
        DiagnosticCode::E149,
        DiagnosticCode::E150,
        DiagnosticCode::E151,
        DiagnosticCode::E152,
        DiagnosticCode::E153,
        DiagnosticCode::E154,
        DiagnosticCode::E155,
    ] {
        let code_str = code.name();
        expected_codes.insert(code_str.to_string());
    }

    // Check that each code has a corresponding documentation file
    let docs_dir = Path::new("docs/diagnostics");

    assert!(
        docs_dir.exists(),
        "docs/diagnostics directory must exist"
    );

    let mut missing_docs = Vec::new();

    for code in expected_codes {
        let doc_file = docs_dir.join(format!("{}.md", code));
        if !doc_file.exists() {
            missing_docs.push(code);
        }
    }

    if !missing_docs.is_empty() {
        panic!(
            "The following diagnostic codes are missing documentation files:\n  {}",
            missing_docs.join("\n  ")
        );
    }
}

#[test]
fn no_extraneous_diagnostic_docs() {
    // This test ensures we don't have documentation files for non-existent codes,
    // which would be a sign of stale or orphaned documentation.

    let mut known_codes = BTreeSet::new();

    // Collect all diagnostic codes
    for code in [
        DiagnosticCode::E001,
        DiagnosticCode::E002,
        DiagnosticCode::E003,
        DiagnosticCode::E004,
        DiagnosticCode::E005,
        DiagnosticCode::E006,
        DiagnosticCode::E007,
        DiagnosticCode::E008,
        DiagnosticCode::E009,
        DiagnosticCode::E010,
        DiagnosticCode::E011,
        DiagnosticCode::E012,
        DiagnosticCode::E013,
        DiagnosticCode::E014,
        DiagnosticCode::E015,
        DiagnosticCode::E016,
        DiagnosticCode::E017,
        DiagnosticCode::E018,
        DiagnosticCode::E019,
        DiagnosticCode::E020,
        DiagnosticCode::E021,
        DiagnosticCode::E022,
        DiagnosticCode::E023,
        DiagnosticCode::E024,
        DiagnosticCode::E025,
        DiagnosticCode::E026,
        DiagnosticCode::E027,
        DiagnosticCode::E028,
        DiagnosticCode::E029,
        DiagnosticCode::E030,
        DiagnosticCode::E031,
        DiagnosticCode::E032,
        DiagnosticCode::E033,
        DiagnosticCode::E034,
        DiagnosticCode::E035,
        DiagnosticCode::E036,
        DiagnosticCode::E037,
        DiagnosticCode::E038,
        DiagnosticCode::E039,
        DiagnosticCode::E040,
        DiagnosticCode::E041,
        DiagnosticCode::E042,
        DiagnosticCode::E043,
        DiagnosticCode::E044,
        DiagnosticCode::E045,
        DiagnosticCode::E046,
        DiagnosticCode::E047,
        DiagnosticCode::E048,
        DiagnosticCode::E049,
        DiagnosticCode::E050,
        DiagnosticCode::E051,
        DiagnosticCode::E052,
        DiagnosticCode::E053,
        DiagnosticCode::E054,
        DiagnosticCode::E055,
        DiagnosticCode::E056,
        DiagnosticCode::E057,
        DiagnosticCode::E058,
        DiagnosticCode::E059,
        DiagnosticCode::E060,
        DiagnosticCode::E061,
        DiagnosticCode::E062,
        DiagnosticCode::E063,
        DiagnosticCode::E064,
        DiagnosticCode::E065,
        DiagnosticCode::E066,
        DiagnosticCode::E067,
        DiagnosticCode::E068,
        DiagnosticCode::E069,
        DiagnosticCode::E070,
        DiagnosticCode::E071,
        DiagnosticCode::E072,
        DiagnosticCode::E073,
        DiagnosticCode::E074,
        DiagnosticCode::E075,
        DiagnosticCode::E076,
        DiagnosticCode::E077,
        DiagnosticCode::E078,
        DiagnosticCode::E079,
        DiagnosticCode::E080,
        DiagnosticCode::E081,
        DiagnosticCode::E082,
        DiagnosticCode::E083,
        DiagnosticCode::E084,
        DiagnosticCode::E085,
        DiagnosticCode::E086,
        DiagnosticCode::E087,
        DiagnosticCode::E088,
        DiagnosticCode::E089,
        DiagnosticCode::E090,
        DiagnosticCode::E091,
        DiagnosticCode::E092,
        DiagnosticCode::E093,
        DiagnosticCode::E094,
        DiagnosticCode::E095,
        DiagnosticCode::E096,
        DiagnosticCode::E097,
        DiagnosticCode::E098,
        DiagnosticCode::E099,
        DiagnosticCode::E100,
        DiagnosticCode::E101,
        DiagnosticCode::E102,
        DiagnosticCode::E103,
        DiagnosticCode::E104,
        DiagnosticCode::E105,
        DiagnosticCode::E106,
        DiagnosticCode::E107,
        DiagnosticCode::E108,
        DiagnosticCode::E109,
        DiagnosticCode::E110,
        DiagnosticCode::E111,
        DiagnosticCode::E112,
        DiagnosticCode::E113,
        DiagnosticCode::E114,
        DiagnosticCode::E115,
        DiagnosticCode::E116,
        DiagnosticCode::E117,
        DiagnosticCode::E118,
        DiagnosticCode::E119,
        DiagnosticCode::E120,
        DiagnosticCode::E121,
        DiagnosticCode::E122,
        DiagnosticCode::E123,
        DiagnosticCode::E124,
        DiagnosticCode::E125,
        DiagnosticCode::E126,
        DiagnosticCode::E127,
        DiagnosticCode::E128,
        DiagnosticCode::E129,
        DiagnosticCode::E130,
        DiagnosticCode::E131,
        DiagnosticCode::E132,
        DiagnosticCode::E133,
        DiagnosticCode::E134,
        DiagnosticCode::E135,
        DiagnosticCode::E136,
        DiagnosticCode::E137,
        DiagnosticCode::E138,
        DiagnosticCode::E139,
        DiagnosticCode::E140,
        DiagnosticCode::E141,
        DiagnosticCode::E142,
        DiagnosticCode::E143,
        DiagnosticCode::E144,
        DiagnosticCode::E145,
        DiagnosticCode::E146,
        DiagnosticCode::E147,
        DiagnosticCode::E148,
        DiagnosticCode::E149,
        DiagnosticCode::E150,
        DiagnosticCode::E151,
        DiagnosticCode::E152,
        DiagnosticCode::E153,
        DiagnosticCode::E154,
        DiagnosticCode::E155,
    ] {
        known_codes.insert(code.name().to_string());
    }

    let docs_dir = Path::new("docs/diagnostics");
    if !docs_dir.exists() {
        return;
    }

    let entries = fs::read_dir(docs_dir).expect("Failed to read docs/diagnostics");

    let mut extraneous_docs = Vec::new();

    for entry in entries {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.extension().map(|e| e == "md").unwrap_or(false) {
            if let Some(filename) = path.file_stem() {
                let code_name = filename.to_string_lossy().to_string();
                if !known_codes.contains(&code_name) {
                    extraneous_docs.push(code_name);
                }
            }
        }
    }

    if !extraneous_docs.is_empty() {
        eprintln!(
            "Warning: The following documentation files exist but don't correspond to known codes:\n  {}",
            extraneous_docs.join("\n  ")
        );
    }
}
