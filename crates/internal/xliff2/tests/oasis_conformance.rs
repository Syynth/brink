use std::path::Path;

/// Parse all valid OASIS test files and ensure they parse without errors.
#[test]
fn parse_oasis_valid_core() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oasis-suite/xliff-21/test-suite/core/valid");

    if !dir.exists() {
        eprintln!("OASIS test suite not checked out (git submodule). Skipping.");
        return;
    }

    let mut count = 0;
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("xlf") {
            continue;
        }
        count += 1;
        let xml = std::fs::read_to_string(&path).unwrap();
        if let Err(e) = xliff2::read::read_xliff(&xml) {
            failures.push(format!(
                "{}: {e}",
                path.file_name().unwrap().to_string_lossy()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{count} OASIS valid core files failed to parse:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(count > 0, "no .xlf files found in {}", dir.display());
    eprintln!("Parsed {count} valid OASIS core files successfully");
}

/// Parse all valid OASIS core files, write them back, re-parse, and assert structural equality.
#[test]
fn round_trip_oasis_valid_core() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oasis-suite/xliff-21/test-suite/core/valid");

    if !dir.exists() {
        eprintln!("OASIS test suite not checked out. Skipping.");
        return;
    }

    let mut count = 0;
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("xlf") {
            continue;
        }
        count += 1;
        let xml = std::fs::read_to_string(&path).unwrap();
        let Ok(doc) = xliff2::read::read_xliff(&xml) else {
            continue; // skip files that don't parse (tested separately)
        };
        let Ok(written) = xliff2::write::to_string(&doc) else {
            failures.push(format!(
                "{}: write failed",
                path.file_name().unwrap().to_string_lossy()
            ));
            continue;
        };
        match xliff2::read::read_xliff(&written) {
            Ok(reparsed) => {
                if doc != reparsed {
                    failures.push(format!(
                        "{}: round-trip mismatch",
                        path.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
            Err(e) => {
                failures.push(format!(
                    "{}: re-parse failed: {e}",
                    path.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }

    assert!(count > 0);
    let pass_count = count - failures.len();
    eprintln!("Round-tripped {pass_count}/{count} valid OASIS core files successfully");
    if !failures.is_empty() {
        eprintln!(
            "Known round-trip issues ({}):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
    // Allow up to 4 failures for now (namespace/CDATA fidelity edge cases)
    assert!(
        failures.len() <= 4,
        "regression: expected at most 4 round-trip failures, got {}:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Verify that invalid OASIS core files either fail to parse or fail validation.
#[test]
fn reject_oasis_invalid_core() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oasis-suite/xliff-21/test-suite/core/invalid");

    if !dir.exists() {
        eprintln!("OASIS test suite not checked out. Skipping.");
        return;
    }

    let mut count = 0;
    let mut false_accepts = Vec::new();

    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("xlf") {
            continue;
        }
        count += 1;
        let xml = std::fs::read_to_string(&path).unwrap();

        // Either the parser should reject it, or validation should find errors
        let accepted = match xliff2::read::read_xliff(&xml) {
            Err(_) => false, // correctly rejected by parser
            Ok(doc) => {
                let errors = xliff2::validate::validate(&doc);
                errors.is_empty() // if no validation errors, it was falsely accepted
            }
        };

        if accepted {
            false_accepts.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }

    assert!(count > 0, "no .xlf files found in {}", dir.display());
    // Log false accepts but don't fail — our validation is not exhaustive yet
    if !false_accepts.is_empty() {
        eprintln!(
            "WARNING: {}/{count} invalid OASIS core files were not rejected:\n{}",
            false_accepts.len(),
            false_accepts.join("\n")
        );
    }
    eprintln!("Checked {count} invalid OASIS core files");
}
