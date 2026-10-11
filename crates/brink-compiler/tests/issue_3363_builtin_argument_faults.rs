//! Issues #3363 and #3364: ink built-ins handed an argument the reference
//! runtime rejects now fault, with the reference's wording, where brink
//! used to play on with a made-up value (`TURNS_SINCE(3)` was -1,
//! `RANDOM(5, 2)` was 5, `SEED_RANDOM("x")` seeded 0).
//!
//! A fault that comes after a finished line delivers that line first, as
//! ink does: ink evaluates past a finished line to see whether glue reaches
//! back over its newline, and a fault there is rolled back with the state
//! snapshot taken at the newline, so the line goes out and the fault comes
//! on the next `Continue`.
//!
//! Every expected output and message is inkjs 2.4.0 via
//! `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, RuntimeError, Step, Story};

fn story(source: &str) -> Story<DotNetRng> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    Story::<DotNetRng>::new(Arc::new(program), line_tables)
}

/// Play to the first fault: every line delivered before it, and the fault.
fn play_to_fault(source: &str) -> (Vec<String>, RuntimeError) {
    let mut story = story(source);
    let mut lines = Vec::new();
    for _ in 0..50 {
        match story.continue_single() {
            Ok(Step::Line(l)) => lines.push(l.text.clone()),
            Ok(other) => panic!("expected a fault, got {other:?} after {lines:?}\n{source}"),
            Err(e) => return (lines, e),
        }
    }
    panic!("no fault in 50 steps\n{source}");
}

const DIVERT_TARGET_HINT: &str = "TURNS_SINCE / READ_COUNT expected a divert target (knot, \
                                  stitch, label name), but saw 3. Did you accidentally pass a \
                                  read count ('knot_name') instead of a target ('-> knot_name')?";

#[test]
fn turns_since_and_read_count_reject_an_int() {
    for call in ["TURNS_SINCE(x)", "READ_COUNT(x)"] {
        let (lines, fault) = play_to_fault(&format!("VAR x = 3\nfirst\nsecond {{{call}}}.\n"));
        assert_eq!(lines, ["first\n"], "{call}");
        assert_eq!(fault.to_string(), DIVERT_TARGET_HINT, "{call}");
    }
}

/// Only an int gets the read-count hint: it is what passing `knot` where
/// `-> knot` was meant produces.
#[test]
fn a_string_gets_no_read_count_hint() {
    let (lines, fault) = play_to_fault("VAR s = \"k\"\nT: {TURNS_SINCE(s)} after.\n");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(
        fault.to_string(),
        "TURNS_SINCE / READ_COUNT expected a divert target (knot, stitch, label name), but saw k"
    );
}

#[test]
fn divert_targets_still_count() {
    let mut story = story(
        "Turns: {TURNS_SINCE(-> k)} {READ_COUNT(-> k)}.\n-> k\n=== k\nAgain: {TURNS_SINCE(-> k)} {READ_COUNT(-> k)}.\n-> END\n",
    );
    let lines: Vec<String> = story
        .continue_maximally()
        .expect("plays")
        .into_iter()
        .filter_map(|s| match s {
            Step::Line(l) => Some(l.text),
            _ => None,
        })
        .collect();
    assert_eq!(lines, ["Turns: -1 0.\n", "Again: 0 1.\n"]);
}

#[test]
fn random_rejects_what_ink_rejects() {
    let cases = [
        (
            "VAR lo = 5\nfirst\nsecond {RANDOM(lo, 2)}.\n",
            "RANDOM was called with minimum as 5 and maximum as 2. The maximum must be larger",
        ),
        (
            "VAR f = 1.5\nfirst\nsecond {RANDOM(f, 3)}.\n",
            "Invalid value for minimum parameter of RANDOM(min, max)",
        ),
        (
            "VAR s = \"a\"\nfirst\nsecond {RANDOM(s, 3)}.\n",
            "Invalid value for minimum parameter of RANDOM(min, max)",
        ),
        (
            "VAR s = \"x\"\nfirst\nsecond {RANDOM(1, s)}.\n",
            "Invalid value for maximum parameter of RANDOM(min, max)",
        ),
        // The minimum is checked first, as the reference does.
        (
            "VAR s = \"x\"\nfirst\nsecond {RANDOM(s, s)}.\n",
            "Invalid value for minimum parameter of RANDOM(min, max)",
        ),
        (
            "VAR s = \"x\"\nfirst\n~ SEED_RANDOM(s)\nsecond.\n",
            "Invalid value passed to SEED_RANDOM",
        ),
        (
            "VAR f = 2.5\nfirst\n~ SEED_RANDOM(f)\nsecond.\n",
            "Invalid value passed to SEED_RANDOM",
        ),
    ];
    for (source, message) in cases {
        let (lines, fault) = play_to_fault(source);
        assert_eq!(lines, ["first\n"], "{source}");
        assert_eq!(fault.to_string(), message, "{source}");
    }
}

/// A one-value range is legal and draws that value.
#[test]
fn random_with_equal_bounds_draws_the_bound() {
    let mut story = story("Rand: {RANDOM(4, 4)}.\n-> END\n");
    let steps = story.continue_maximally().expect("plays");
    assert!(
        matches!(&steps[0], Step::Line(l) if l.text == "Rand: 4.\n"),
        "{steps:?}"
    );
}

/// The line before a fault goes out even when nothing visible followed its
/// newline yet: here the fault is the first thing the next line does.
#[test]
fn the_line_before_a_fault_is_delivered_first() {
    for source in [
        "VAR x = 3\nfirst line\n{TURNS_SINCE(x)} second.\n",
        "VAR x = 3\nfirst line\n~ temp t = TURNS_SINCE(x)\nsecond {t}.\n",
    ] {
        let (lines, _) = play_to_fault(source);
        assert_eq!(lines, ["first line\n"], "{source}");
    }
}

/// Glue that reaches back over the newline means the line was never
/// finished, so ink delivers nothing before the fault.
#[test]
fn glue_before_the_fault_delivers_nothing() {
    let (lines, _) = play_to_fault("VAR x = 3\nfirst line\n<> glued {TURNS_SINCE(x)}.\n");
    assert!(lines.is_empty(), "{lines:?}");
}

/// The text of the interrupted line itself is dropped, as ink drops it.
#[test]
fn the_interrupted_line_is_dropped() {
    let (lines, _) = play_to_fault("VAR x = 3\nfirst line\nsecond ~ {TURNS_SINCE(x)} tail.\n");
    assert_eq!(lines, ["first line\n"]);
}

/// A host jump between the delivered line and the held fault starts a new
/// run, and the fault from the old one does not follow it there.
#[test]
fn a_host_jump_drops_the_held_fault() {
    let mut story = story(
        "VAR x = 3\nfirst line\n{TURNS_SINCE(x)} second.\n-> END\n=== elsewhere\nsafe\n-> END\n",
    );
    let first = story.continue_single().expect("the line before the fault");
    assert!(
        matches!(&first, Step::Line(l) if l.text == "first line\n"),
        "{first:?}"
    );
    story.choose_path_string("elsewhere").expect("jump");
    let next = story.continue_single().expect("the jump target plays");
    assert!(
        matches!(&next, Step::Line(l) if l.text == "safe\n"),
        "{next:?}"
    );
}

// ── E196: what inklecate's compiler refuses, brink refuses too ─────────

/// The `E196` messages a source compiles to (empty when it compiles).
fn e196(source: &str) -> Vec<String> {
    match brink_compiler::compile("story.ink", |_| Ok(source.to_owned())) {
        Ok(_) => Vec::new(),
        Err(brink_compiler::CompileError::Diagnostics(diags)) => diags
            .iter()
            .filter(|d| d.code == brink_compiler::DiagnosticCode::E196)
            .map(|d| d.message.clone())
            .collect(),
        Err(other) => panic!("unexpected compile error {other}\n{source}"),
    }
}

/// Each shape inklecate refuses, with inklecate's wording after the title
/// (checked against `tools/inkjs-oracle`'s compiler).
#[test]
fn e196_refuses_what_inklecate_refuses() {
    let target = "The TURNS_SINCE() function should take one argument: a divert target to the \
                  target knot, stitch, gather or choice you want to check. e.g. \
                  TURNS_SINCE(-> myKnot)";
    let cases: &[(&str, &str)] = &[
        (
            "-> k\n=== k\nR: {READ_COUNT(k)}.\n-> END\n",
            "Should be READ_COUNT(-> k). Usage without the '->' only makes sense for variable \
             targets.",
        ),
        (
            "-> k\n=== k\n= s\nT: {TURNS_SINCE(k.s)}.\n-> END\n",
            "Should be TURNS_SINCE(-> k.s). Usage without the '->' only makes sense for \
             variable targets.",
        ),
        ("T: {TURNS_SINCE(3)}.\n-> END\n", target),
        ("T: {TURNS_SINCE()}.\n-> END\n", target),
        (
            "R: {RANDOM(1, 2.5)}.\n-> END\n",
            "RANDOM's maximum parameter should be an integer",
        ),
        (
            "R: {RANDOM(-1.5, 2)}.\n-> END\n",
            "RANDOM's minimum parameter should be an integer",
        ),
        (
            "R: {RANDOM(true, 2)}.\n-> END\n",
            "RANDOM's minimum parameter should be an integer",
        ),
        (
            "~ SEED_RANDOM(2.0)\nok\n-> END\n",
            "SEED_RANDOM's parameter should be an integer seed",
        ),
        (
            "R: {RANDOM(1)}.\n-> END\n",
            "RANDOM should take 2 parameters: a minimum and a maximum integer",
        ),
        (
            "R: {FLOOR(1, 2)}.\n-> END\n",
            "FLOOR should take 1 parameter",
        ),
        (
            "R: {CHOICE_COUNT(1)}.\n-> END\n",
            "The CHOICE_COUNT() function shouldn't take any arguments",
        ),
    ];
    for (source, detail) in cases {
        let messages = e196(source);
        let expected = format!("{}: {detail}", brink_compiler::DiagnosticCode::E196.title());
        assert_eq!(messages, [expected], "{source}");
    }
}

/// What inklecate compiles, brink compiles: divert targets, variables and
/// parameters holding them, and arguments only the runtime can judge.
#[test]
fn e196_leaves_alone_what_inklecate_compiles() {
    for source in [
        "T: {TURNS_SINCE(-> k)} {READ_COUNT(-> k.s)}\n-> END\n=== k\n= s\n-> END\n",
        "VAR v = -> k\n-> k\n=== k\nT: {TURNS_SINCE(v)}.\n-> END\n",
        "-> k\n=== function f(x)\n~ return TURNS_SINCE(x)\n=== k\nT: {f(-> k)}.\n-> END\n",
        "VAR x = 3\nT: {TURNS_SINCE(x)}.\n-> END\n",
        "R: {RANDOM(\"a\", 3)} {RANDOM(5, 2)} {RANDOM(1, 2 + 0.5)}.\n-> END\n",
        "R: {RANDOM(1, 6)} {FLOOR(1.5)} {TURNS()} {CHOICE_COUNT()}.\n-> END\n",
    ] {
        assert!(e196(source).is_empty(), "{source}");
    }
}
