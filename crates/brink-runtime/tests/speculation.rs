//! F4.1: [`Speculation`] — a sandboxed, side-effect-proof speculative run.
//!
//! These tests exercise the primitive end-to-end against a real compiled
//! program (via `brink-compiler`), proving:
//! - a speculation continuing from some state produces the same lines the
//!   live flow would produce continuing from that same state;
//! - nothing a speculation does (globals, visit counts, rng, turn index)
//!   ever reaches the live `Story` it was forked from;
//! - a caller-supplied `Budget` stops a runaway speculation instead of
//!   hanging or burning the production step/line ceilings;
//! - `eval_function` evaluates a pure ink function without touching the
//!   live story;
//! - `go_to_path` jumps a speculation to an arbitrary knot and runs it.

#![expect(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::Cell;
use std::sync::Arc;

use brink_format::Value;
use brink_runtime::{
    Budget, ExternalFnHandler, ExternalResult, FallbackHandler, FunctionEval, Line, Program,
    RuntimeError, SpeculationStep, Story,
};

/// A single program exercising everything these tests need: a mutated
/// global, an RNG draw, a plain terminal knot (`shrine`), a pure function
/// (`add`), and an infinitely-looping knot (`spin`) with no output — the
/// runaway-probe fixture for the budget tests.
const SOURCE: &str = "\
VAR gold = 0

-> start

=== start ===
Hello.
~ gold = gold + 5
~ temp r = RANDOM(1, 100)
More text.
-> END

=== shrine ===
At the shrine.
-> END

=== spin ===
-> spin

=== function add(x, y) ===
~ return x + y
";

fn compile_and_link(src: &str) -> (Arc<Program>, Vec<Vec<brink_format::LineEntry>>) {
    let out = brink_compiler::compile("t.ink", |p| {
        if p == "t.ink" {
            Ok(src.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such include",
            ))
        }
    })
    .expect("compile");
    let (program, line_tables) = brink_runtime::link(&out.data).expect("link");
    (Arc::new(program), line_tables)
}

/// Drive a `Story` with `continue_single` until a terminal line, collecting
/// every `(text, tags)` pair including the terminal one.
fn drive_live(story: &mut Story<brink_runtime::DotNetRng>) -> Vec<(String, Vec<String>)> {
    let mut lines = Vec::new();
    loop {
        let line = story.continue_single().unwrap();
        let terminal = line.is_terminal();
        lines.push((line.text().to_owned(), line.tags().to_vec()));
        if terminal {
            return lines;
        }
    }
}

/// Drive a `Speculation` with `advance` until a terminal line, collecting
/// every `(text, tags)` pair including the terminal one.
fn drive_speculation(
    spec: &mut brink_runtime::Speculation<brink_runtime::DotNetRng>,
) -> Vec<(String, Vec<String>)> {
    let mut lines = Vec::new();
    loop {
        match spec.advance(Budget::default(), &FallbackHandler).unwrap() {
            SpeculationStep::Line(line) => {
                let terminal = line.is_terminal();
                lines.push((line.text().to_owned(), line.tags().to_vec()));
                if terminal {
                    return lines;
                }
            }
            SpeculationStep::AwaitingExternal => panic!("no externals in this story"),
        }
    }
}

/// A speculation forked mid-story and driven to a terminal line must
/// produce exactly the lines the live flow would have produced continuing
/// from that same point.
#[test]
fn speculation_mirrors_live_continuation() {
    let (program, line_tables) = compile_and_link(SOURCE);

    // Story A: advance one line live, then fork and speculate the rest.
    let mut story_a =
        Story::<brink_runtime::DotNetRng>::new(Arc::clone(&program), line_tables.clone());
    let first_a = story_a.continue_single().unwrap();
    assert!(matches!(first_a, Line::Text { .. }));

    let mut spec = story_a.speculate();
    let spec_rest = drive_speculation(&mut spec);

    // Story B: a completely separate live story, advanced identically.
    let mut story_b = Story::<brink_runtime::DotNetRng>::new(Arc::clone(&program), line_tables);
    let first_b = story_b.continue_single().unwrap();
    assert_eq!(first_a.text(), first_b.text());
    let live_rest = drive_live(&mut story_b);

    assert_eq!(
        spec_rest, live_rest,
        "speculation must mirror the live continuation byte-for-byte"
    );
}

/// Everything a speculation does — global writes, visit counts, rng draws,
/// turn index — must leave the live `Story` it was forked from completely
/// unchanged, observable only through the public API (no back-door into
/// the sandboxed `FlowLocal`/`World`).
#[test]
fn speculation_writes_never_reach_the_live_story() {
    let (program, line_tables) = compile_and_link(SOURCE);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);

    let gold_before = story.variable("gold").unwrap().clone();
    let snapshot_before = story.debug_snapshot();

    let mut spec = story.speculate();
    let lines = drive_speculation(&mut spec);
    // Sanity: the speculation actually did something (wrote `gold`, drew
    // rng, produced output) — otherwise this test would trivially pass.
    assert!(lines.iter().any(|(text, _)| text.contains("Hello")));

    let gold_after = story.variable("gold").unwrap().clone();
    assert_eq!(gold_after, gold_before, "gold must be unchanged");

    let snapshot_after = story.debug_snapshot();
    assert_eq!(
        snapshot_after.status, snapshot_before.status,
        "the live story was never advanced — status must be untouched"
    );
    assert_eq!(snapshot_after.turn_index, snapshot_before.turn_index);
    assert_eq!(snapshot_after.rng.seed, snapshot_before.rng.seed);
    assert_eq!(snapshot_after.rng.previous, snapshot_before.rng.previous);

    let visits_before: Vec<(String, u32)> = snapshot_before
        .visit_counts
        .iter()
        .map(|v| (v.path.clone(), v.count))
        .collect();
    let visits_after: Vec<(String, u32)> = snapshot_after
        .visit_counts
        .iter()
        .map(|v| (v.path.clone(), v.count))
        .collect();
    assert_eq!(
        visits_before, visits_after,
        "the speculation's `start` visit must not leak into the live story"
    );
}

/// A tiny step budget on a runaway (infinitely-looping, output-free) knot
/// must stop the speculation quickly with a step-limit error rather than
/// hanging or burning the production 1,000,000-step ceiling.
#[test]
fn speculation_budget_caps_a_runaway_step_loop() {
    let (program, line_tables) = compile_and_link(SOURCE);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);

    let mut spec = story.speculate();
    spec.go_to_path("spin").unwrap();

    let tiny = Budget {
        steps: 64,
        lines: 1_000,
    };
    let err = spec.advance(tiny, &FallbackHandler).unwrap_err();
    assert!(
        matches!(err, RuntimeError::StepLimitExceeded(64)),
        "expected a step-limit error at the tiny budget, got: {err:?}"
    );
}

/// A tiny line budget stops further `advance` calls once the speculation
/// has already produced that many lines, independent of the step budget.
#[test]
fn speculation_budget_caps_total_lines_produced() {
    let (program, line_tables) = compile_and_link(SOURCE);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);

    let mut spec = story.speculate();
    let one_line = Budget {
        steps: Budget::default().steps,
        lines: 1,
    };

    // First line succeeds — under the (lines: 1) budget.
    match spec.advance(one_line, &FallbackHandler).unwrap() {
        SpeculationStep::Line(line) => assert!(!line.is_terminal()),
        SpeculationStep::AwaitingExternal => panic!("no externals in this story"),
    }

    // A second call immediately hits the line budget — no further VM
    // stepping happens at all.
    let err = spec.advance(one_line, &FallbackHandler).unwrap_err();
    assert!(
        matches!(err, RuntimeError::LineLimitExceeded(1)),
        "expected a line-limit error at the tiny budget, got: {err:?}"
    );
}

/// `eval_function` evaluates a pure ink function on the speculation and
/// returns its value; the live story is untouched.
#[test]
fn speculation_eval_function_returns_pure_value() {
    let (program, line_tables) = compile_and_link(SOURCE);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);
    let gold_before = story.variable("gold").unwrap().clone();

    let mut spec = story.speculate();
    let result = spec
        .eval_function("add", &[Value::Int(3), Value::Int(4)], &FallbackHandler)
        .unwrap();
    match result {
        FunctionEval::Returned(value) => assert_eq!(value, Value::Int(7)),
        FunctionEval::AwaitingExternal => panic!("`add` has no externals"),
    }

    assert_eq!(*story.variable("gold").unwrap(), gold_before);
}

/// `go_to_path` jumps the speculation's play head to a named knot and runs
/// it, without moving the live story's own position at all.
#[test]
fn speculation_go_to_path_jumps_and_runs() {
    let (program, line_tables) = compile_and_link(SOURCE);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);

    let mut spec = story.speculate();
    spec.go_to_path("shrine").unwrap();
    match spec.advance(Budget::default(), &FallbackHandler).unwrap() {
        SpeculationStep::Line(line) => assert!(line.text().contains("At the shrine")),
        SpeculationStep::AwaitingExternal => panic!("no externals in this story"),
    }

    // The live story was never driven at all — still fresh/active, and the
    // speculation's jump has no way to reach back into it.
    let snapshot = story.debug_snapshot();
    assert_eq!(snapshot.status, "active");
    assert_eq!(snapshot.turn_index, 0);
}

// ── F4.2: `Speculation::resume_function_eval` closes the AwaitingExternal gap ──

/// A program with a pure function (`add`) plus a `query` function that
/// calls an `EXTERNAL` — the fixture for exercising
/// `eval_function` -> `AwaitingExternal` -> `resolve_external` ->
/// `resume_function_eval` -> `Returned`.
const SOURCE_WITH_EXTERNAL: &str = "\
EXTERNAL get_value(x)

VAR gold = 0

-> start

=== start ===
Hello.
~ gold = gold + 5
-> END

=== function query(x) ===
~ return get_value(x)
";

/// Defers (`Pending`) its first call, resolves inline thereafter — mirrors
/// the `DeferProbe` pattern used for the flow-level resume gap in
/// `crates/brink-runtime/tests/session.rs`.
struct DeferOnce {
    deferred: Cell<bool>,
}

impl ExternalFnHandler for DeferOnce {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        if name == "get_value" && !self.deferred.get() {
            self.deferred.set(true);
            ExternalResult::Pending
        } else {
            let Some(Value::Int(x)) = args.first() else {
                panic!("expected an int arg, got {args:?}");
            };
            ExternalResult::Resolved(Value::Int(x * 2))
        }
    }
}

/// A speculative `eval_function` of a function that calls a `Query`
/// external returning `Pending` must pause as `AwaitingExternal`;
/// `resolve_external` + `resume_function_eval` must then complete the
/// evaluation with the correct return value, and the live story must be
/// completely untouched throughout.
#[test]
fn speculation_resume_function_eval_completes_after_deferred_external() {
    let (program, line_tables) = compile_and_link(SOURCE_WITH_EXTERNAL);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);
    let gold_before = story.variable("gold").unwrap().clone();
    let snapshot_before = story.debug_snapshot();

    let mut spec = story.speculate();
    let handler = DeferOnce {
        deferred: Cell::new(false),
    };

    let outcome = spec
        .eval_function("query", &[Value::Int(21)], &handler)
        .unwrap();
    assert!(
        matches!(outcome, FunctionEval::AwaitingExternal),
        "expected the deferred external to pause the eval, got {outcome:?}"
    );

    spec.resolve_external(Value::Int(42));
    let outcome = spec.resume_function_eval(&handler).unwrap();
    match outcome {
        FunctionEval::Returned(value) => assert_eq!(value, Value::Int(42)),
        FunctionEval::AwaitingExternal => panic!("only one external call in `query`"),
    }

    // The live story never advanced at all: unaffected by the speculative
    // eval_function/resolve_external/resume_function_eval cycle.
    assert_eq!(*story.variable("gold").unwrap(), gold_before);
    let snapshot_after = story.debug_snapshot();
    assert_eq!(snapshot_after.status, snapshot_before.status);
    assert_eq!(snapshot_after.turn_index, snapshot_before.turn_index);
}

/// Calling `resume_function_eval` with no evaluation in progress is an
/// error, not a silent no-op.
#[test]
fn speculation_resume_function_eval_errors_when_nothing_pending() {
    let (program, line_tables) = compile_and_link(SOURCE_WITH_EXTERNAL);
    let story = Story::<brink_runtime::DotNetRng>::new(program, line_tables);
    let mut spec = story.speculate();

    let err = spec.resume_function_eval(&FallbackHandler).unwrap_err();
    assert!(
        matches!(err, RuntimeError::NotEvaluatingFunction),
        "expected NotEvaluatingFunction, got {err:?}"
    );
}
