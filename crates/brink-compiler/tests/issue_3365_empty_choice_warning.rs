//! Issue #3365, Part 1: `E195` — a `*`/`+` choice with neither
//! display/bracket text nor a divert, matching inklecate's own "Choice is
//! completely empty" warning.
//!
//! `issue_3365_empty_choice_diagnostic.rs` (in `brink-ir`) pins the
//! shape-by-shape firing rule directly against `hir::lower`. This file
//! proves the same diagnostic actually reaches an author through the real
//! `brink_compiler::compile` entry point — the issue's own repro compiles
//! clean (warning, not an error) and is `[lints]`-overridable like its
//! `E164`/`E188`/`E193` neighbours.

// `expect` calls below sit in plain helper functions, not directly inside a
// `#[test]` fn, so clippy's `allow-expect-in-tests` (`clippy.toml`) does not
// cover them — same allowance `issue_3354_temp_dominance.rs` in this
// directory already carries, for the same reason.
#![allow(clippy::expect_used)]

use std::collections::HashMap;

use brink_analyzer::LintLevel;
use brink_compiler::{AnalysisOptions, DiagnosticCode, Dialect};

fn compile(source: &str) -> brink_compiler::CompileOutput {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        },
    )
    .expect("E195 is a warning, not an error — the story still compiles")
}

/// The issue's own reference repro (#3365).
const ISSUE_REPRO: &str = "* []\n    Fallthrough body.\n- -> END\n";

#[test]
fn issue_repro_compiles_with_e195_warning() {
    let out = compile(ISSUE_REPRO);
    let e195: Vec<_> = out
        .warnings
        .iter()
        .filter(|d| d.code == DiagnosticCode::E195)
        .collect();
    assert_eq!(
        e195.len(),
        1,
        "expected exactly one E195 warning for the issue's own repro, got {:?}",
        out.warnings
    );
    assert_eq!(e195[0].severity, brink_ir::Severity::Warning);
}

#[test]
fn e195_is_lints_overridable() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", ISSUE_REPRO)]);
    let mut options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let overrides: std::collections::BTreeMap<String, LintLevel> =
        [("E195".to_owned(), LintLevel::Allow)]
            .into_iter()
            .collect();
    let rejected = options.apply_lint_overrides(&overrides, None);
    assert!(
        rejected.is_empty(),
        "E195 = \"allow\" should be a valid override, got rejections: {rejected:?}"
    );

    let out = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
    .expect("still just a warning-tier code once allowed");

    assert!(
        !out.warnings.iter().any(|d| d.code == DiagnosticCode::E195),
        "E195 should be silenced once allowed, got {:?}",
        out.warnings
    );
}

#[test]
fn e034_and_e195_can_co_occur_without_interference() {
    // A choice set whose only choice is a bare `*` with nothing at all is
    // *both* "the set has only fallback choices" (E034, `brink-analyzer`'s
    // own already-lowered-HIR pass — `is_fallback` is set whenever a
    // choice has no `[bracket]`/start/inner content at all, which a bare
    // `*` satisfies) *and* "this choice has no text and no divert" (E195,
    // raised during lowering) — two independent checks over the same
    // shape, neither suppressing the other. (`* []`'s *explicit* empty
    // bracket does not double as E034's fallback shape — an explicit,
    // zero-width `CHOICE_BRACKET_CONTENT` node still counts as "has a
    // bracket" for `is_fallback`, so only the truly bracket-less `*` form
    // exercises both codes at once.)
    let out = compile("*\n- -> END\n");
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E034),
        "expected E034 for the all-fallback set, got {:?}",
        out.warnings
    );
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E195),
        "expected E195 for the empty choice, got {:?}",
        out.warnings
    );
}

#[test]
fn e195_does_not_fire_on_an_ordinary_choice_set() {
    let out = compile(
        "* Take the lantern.\n    You take it.\n* Leave it.\n    You leave it.\n- -> END\n",
    );
    assert!(
        !out.warnings.iter().any(|d| d.code == DiagnosticCode::E195),
        "an ordinary two-choice set must not warn, got {:?}",
        out.warnings
    );
}
