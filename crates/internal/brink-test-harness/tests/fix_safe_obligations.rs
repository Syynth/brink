//! The `Safe`-fix obligation sweep (issue #3417, `docs/autofix-spec.md` §3).
//!
//! Two halves, and they are not the same claim:
//!
//! 1. **Enforcement.** Every fixer in `brink_ide::fix::FIXERS` whose
//!    `max_applicability` is `Safe` must have a `tests/fix/<code>/` fixture,
//!    and that fixture is run through
//!    [`brink_test_harness::fix::assert_safe_fix`] here. The registry test in
//!    `brink_ide::fix` demands the fixture *exists*; this is where it is
//!    actually exercised, because the oracle lives in this crate and
//!    `brink-ide` cannot depend on it.
//! 2. **Measurement.** Every fixture on disk is checked and its verdict
//!    compared against a recorded table. That is how the four migrated
//!    `Suggested` fixers (E025 add-import, E080/E081 creation-site, E063
//!    value-call trim) are exercised: each of them discharges an
//!    **error-severity** diagnostic, so its pre-fix source does not compile
//!    and there is no program whose behaviour could be preserved. The
//!    verdict is `NoPreImage`, and pinning it here is what stops that finding
//!    from quietly rotting.
//!
//! The table is a pin, not a ratchet: a verdict that moves means either the
//! fixture changed or the code's severity did, and both want a human to look.

use std::collections::BTreeMap;

use brink_ide::fix::{Applicability, FIXERS};
use brink_test_harness::fix::{
    SafeFixConfig, SafeVerdict, assert_safe_fix, check_safe_fix, fix_fixture_dirs,
    fix_fixtures_root, load_fix_fixture,
};

/// What each fixture directory is expected to prove today.
///
/// `E014` is the positive case — `docs/autofix-spec.md` §9's first-wave
/// `Safe` candidate, deleting a bare `~`. It has no registered fixer yet
/// (that is its own sub-issue of #3374); it is here so the sweep has one pair
/// that genuinely clears the bar, and so a regression in the oracle cannot
/// hide behind a table of negatives.
fn expected_verdicts() -> BTreeMap<&'static str, SafeVerdict> {
    BTreeMap::from([
        ("E014", SafeVerdict::ObservablyEquivalent),
        ("E025", SafeVerdict::NoPreImage),
        ("E063", SafeVerdict::NoPreImage),
        ("E080", SafeVerdict::NoPreImage),
        ("E081", SafeVerdict::NoPreImage),
    ])
}

#[test]
fn every_safe_max_fixer_has_a_fixture_that_clears_the_bar() {
    let config = SafeFixConfig::default();
    for fixer in FIXERS {
        if fixer.max_applicability() != Applicability::Safe {
            continue;
        }
        let code = fixer.code().as_str();
        let dir = fix_fixtures_root().join(code);
        assert!(
            dir.is_dir(),
            "{code} declares max_applicability = Safe but has no tests/fix/{code}/ fixture \
             (docs/autofix-spec.md §3)"
        );
        let fixture = load_fix_fixture(&dir);
        assert!(fixture.is_ok(), "{code}: {:?}", fixture.err());
        let fixture = fixture.expect("just asserted above");
        let _ = assert_safe_fix(&fixture, &config);
    }
}

#[test]
fn every_fixture_records_the_verdict_it_actually_produces() {
    let config = SafeFixConfig::default();
    let dirs = fix_fixture_dirs();
    assert!(dirs.is_ok(), "{:?}", dirs.err());
    let dirs = dirs.expect("just asserted above");
    assert!(
        !dirs.is_empty(),
        "no fixtures under {} — the sweep would pass vacuously",
        fix_fixtures_root().display()
    );

    let expected = expected_verdicts();
    let mut mismatches: Vec<String> = Vec::new();
    let mut equivalent = 0usize;

    for dir in &dirs {
        let name = dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let fixture = load_fix_fixture(dir);
        assert!(fixture.is_ok(), "{name}: {:?}", fixture.err());
        let report = check_safe_fix(&fixture.expect("just asserted above"), &config);
        if report.verdict == SafeVerdict::ObservablyEquivalent {
            equivalent += 1;
        }
        match expected.get(name.as_str()) {
            Some(want) if *want == report.verdict => {}
            Some(want) => mismatches.push(format!(
                "{name}: recorded {:?}, produced {:?}\n{report}",
                want, report.verdict
            )),
            None => mismatches.push(format!(
                "{name}: no recorded verdict — add one to expected_verdicts()\n{report}"
            )),
        }
    }

    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    assert!(
        equivalent > 0,
        "no fixture reached ObservablyEquivalent — the sweep proves nothing about the oracle"
    );
}

/// Every recorded verdict must name a fixture that exists, so the table
/// cannot keep claiming a finding for a directory somebody deleted.
#[test]
fn every_recorded_verdict_names_a_real_fixture() {
    for name in expected_verdicts().keys() {
        let dir = fix_fixtures_root().join(name);
        assert!(
            dir.is_dir(),
            "expected_verdicts() names {name}, but {} does not exist",
            dir.display()
        );
    }
}
