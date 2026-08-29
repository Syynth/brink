//! Scripted debug-session goldens (issue #3247).
//!
//! Each `tests/debug/<case>/` holds a story (`story.ink` or
//! `story.brink`), a `session.dbg` script of actions a person would take in
//! a debugger, and an insta snapshot of the resulting transcript.
//!
//! **These are self-referential goldens**, like `tests/tier1-native/` —
//! ink has no debugger, so there is no C# oracle to check against. They are
//! NOT part of the oracle ratchet and must never be conflated with it.
//!
//! The transcript is the artifact. A change in stepping semantics shows up
//! as a readable diff rather than a boolean, which is the whole point: the
//! current arrangement defines "what step over does" only in tests written
//! beside the code that does it, so a refactor that changes it updates its
//! own test and passes.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::collections::BTreeMap;
use std::sync::Arc;

use brink_environment::{OptionOverrides, Project};
use brink_source_tree::InMemory;
use brink_test_harness::debug_script::{Session, parse_script, run_script};

/// Compile a fixture with debug info over the production road and build a
/// driveable session.
fn session_for(dir: &std::path::Path) -> Session {
    let entry = ["story.ink", "story.brink"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
        .unwrap_or_else(|| unreachable!("{dir:?} must hold story.ink or story.brink"));
    let name = entry
        .file_name()
        .and_then(|s| s.to_str())
        .expect("fixture file name is utf-8")
        .to_string();
    let text = std::fs::read_to_string(&entry).expect("read fixture story");

    let mut sources = BTreeMap::new();
    sources.insert(name.clone(), text);

    let tree = InMemory::new(sources.clone());
    let overrides = OptionOverrides {
        debug_info: true,
        ..Default::default()
    };
    let env = Project::load(&tree, &name, &overrides).expect("Project::load");
    let out = brink_environment::compile(&env).expect("fixture compiles");
    let (program, line_tables) = brink_runtime::link(&out.data).expect("link");
    Session::new(Arc::new(program), line_tables, sources)
}

fn run_case(case: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/debug")
        .join(case);
    let script_text = std::fs::read_to_string(dir.join("session.dbg")).expect("read session.dbg");
    let script = parse_script(&script_text)
        .unwrap_or_else(|e| unreachable!("{case}: malformed script — {e}"));
    let mut session = session_for(&dir);
    match run_script(&mut session, &script) {
        Ok(transcript) => {
            insta::with_settings!({snapshot_suffix => case, omit_expression => true}, {
                insta::assert_snapshot!(transcript);
            });
        }
        Err(message) => {
            // The message carries the transcript up to the failure, so a
            // broken session reads as a session rather than a bare assert.
            unreachable!("{case}: {message}");
        }
    }
}

#[test]
fn breakpoint_and_locals() {
    run_case("breakpoint_and_locals");
}

#[test]
fn step_over_a_call() {
    run_case("step_over_a_call");
}

#[test]
fn native_flow() {
    run_case("native_flow");
}

/// The #3264 pair: line stepping over the same story `step_over_a_call`
/// walks with `stepi`. Four instruction steps to cross line 7 there, one
/// line step here — read the two transcripts together.
#[test]
fn line_stepping() {
    run_case("line_stepping");
}

// ── Script parsing is its own contract ──────────────────────────────────

#[test]
fn step_and_next_parse_as_line_granularity_and_stepi_as_instruction() {
    use brink_runtime::StepMode;
    use brink_test_harness::debug_script::Command;

    // The two granularities must stay distinguishable at the parser, not
    // just at the executor: aliasing `step` onto `stepi` is precisely how
    // "four presses per line" would become a mystery again.
    assert_eq!(
        parse_script("step over").expect("parses"),
        vec![Command::StepLine(StepMode::Over)]
    );
    assert_eq!(
        parse_script("stepi over").expect("parses"),
        vec![Command::StepInstruction(StepMode::Over)]
    );
    // `next` is GDB's spelling of `step over`, not a third thing.
    assert_eq!(
        parse_script("next").expect("parses"),
        parse_script("step over").expect("parses")
    );
}

#[test]
fn an_unknown_verb_is_an_error_not_a_skip() {
    // A silently-ignored line is a script that appears to test something it
    // does not — the same "unearned coverage" failure a tautological test
    // has, but in the fixture rather than the code.
    let err = parse_script("run\nfrobnicate the thing\n").expect_err("must reject unknown verbs");
    assert_eq!(err.line, 2, "the error must point at the offending line");
    assert!(err.message.contains("frobnicate"), "got: {}", err.message);
}

#[test]
fn line_zero_is_rejected_because_scripts_are_one_based() {
    let err = parse_script("break story.ink:0\n").expect_err("0 is not a line");
    assert!(err.message.contains("1-based"), "got: {}", err.message);
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let script =
        parse_script("# just a note\n\n  run  # trailing comment\n").expect("comments parse");
    assert_eq!(script.len(), 1, "only the `run` should survive");
}
