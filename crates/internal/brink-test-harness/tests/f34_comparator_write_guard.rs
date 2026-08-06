//! F34 (ruled 2026-07-19, tracker #1106): the comparator write-guard,
//! keyed on [`ExecMode`]. A `sort_by`/`sorted_by` comparator that performs
//! a world-write mid-sort (a global assignment — direct or through a `ref`
//! parameter — or an RNG-cell advance: a draw IS a write) faults under DEV
//! as the tracked `ComparatorWroteState` fault; under PROD the check is
//! skipped entirely and the write executes — defined and deterministic,
//! because the stable merge-sort's comparison sequence is fixed. Placement,
//! never fabrication: the prod run's sort result is the same permutation
//! the pure comparator would produce.
//!
//! Exemptions pinned here too: visit-count increments from the comparator's
//! own in-story dispatch are NOT world-writes (the ruled dispatch
//! semantics), and reads stay legal at runtime (E119's static bound owns
//! the read posture — no runtime read-guard).
//!
//! Every comparator below is passed through a temp (opaque), keeping E119
//! quiet — these are exactly the gradual-mode runtime residuals.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, ExecMode, RuntimeError, Step, Story};

/// Build a brink-dialect story under gradual types (opaque-comparator
/// misbehavior is exactly what gradual defers to the runtime) — the
/// `ns_a4_exec_mode` shape.
fn build_gradual(source: &str) -> Story<DotNetRng> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        ..AnalysisOptions::default()
    };
    let files = std::collections::HashMap::from([("main.ink", source)]);
    let output = brink_compiler::compile_with_options(
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
    .expect("source must compile under gradual types (opaque comparators pass E119)");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    Story::new(Arc::new(program), line_tables)
}

/// Drive to completion, collecting text; `Err` propagates.
fn run(story: &mut Story<DotNetRng>) -> Result<String, RuntimeError> {
    let mut out = String::new();
    loop {
        match story.continue_single()? {
            Step::Line(line) => out.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended => return Ok(out),
            Step::Choices(_) => panic!("no choices in these stories"),
        }
    }
}

/// A comparator that tallies its invocations into a global while sorting
/// descending. The write is the F34 violation; the sort result is the
/// mode-independent part.
const GLOBAL_WRITE: &str = "\
VAR tally = 0\n\
~ temp a = #[3, 1, 2]\n\
~ temp c = #fn(count_desc)\n\
~ sort_by(a, c)\n\
sorted: {a}; tally: {tally}.\n\
-> END\n\
\n\
=== function count_desc(x, y) ===\n\
~ tally = tally + 1\n\
~ return y - x\n";

#[test]
fn dev_mode_global_write_comparator_faults() {
    let mut story = build_gradual(GLOBAL_WRITE);
    assert_eq!(story.exec_mode(), ExecMode::Dev, "dev is the default");
    let err = run(&mut story).unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::ComparatorWroteState {
                verb: "sort_by",
                role: "comparator",
                what: "assigned a global variable",
            }
        ),
        "{err:?}"
    );
}

#[test]
fn prod_mode_executes_the_write_and_the_sort_lands() {
    let mut story = build_gradual(GLOBAL_WRITE);
    story.set_exec_mode(ExecMode::Prod);
    let out = run(&mut story).expect("prod mode keeps moving");
    // Placement, not fabrication: the sort result is the permutation the
    // pure comparator would produce, AND the write landed — the bottom-up
    // stable merge over [3, 1, 2] makes exactly 3 comparisons (a fixed
    // sequence, which is why the prod-mode write is defined +
    // deterministic).
    assert_eq!(out, "sorted: [3, 2, 1]; tally: 3.\n");
}

/// The write-through seam: the comparator delegates the write to a helper
/// taking `ref` to a global — the guard fires at the pointer write-back
/// (`SetTemp`'s `VariablePointer` arm), not just at direct `SetGlobal`.
const REF_WRITE: &str = "\
VAR sink = 0\n\
~ temp a = #[2, 1]\n\
~ temp c = #fn(sneaky)\n\
~ sort_by(a, c)\n\
{a}\n\
-> END\n\
\n\
=== function sneaky(x, y) ===\n\
~ bump(sink)\n\
~ return x - y\n\
\n\
=== function bump(ref r) ===\n\
~ r = r + 1\n";

#[test]
fn dev_mode_ref_param_write_through_faults() {
    let mut story = build_gradual(REF_WRITE);
    let err = run(&mut story).unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::ComparatorWroteState {
                verb: "sort_by",
                role: "comparator",
                what: "assigned a global through a `ref` parameter",
            }
        ),
        "{err:?}"
    );
}

/// A random comparator: `int(0..2)` draws from the RNG cell — a draw IS a
/// write, and a random comparator is exactly the non-determinism the
/// pure·silent contract bans. Dev faults at the draw.
const RAND_CMP: &str = "\
~ temp a = #[10, 20, 30]\n\
~ temp c = #fn(coin)\n\
~ sort_by(a, c)\n\
sorted: {a}.\n\
-> END\n\
\n\
=== function coin(x, y) ===\n\
~ return int(0..2) - 1\n";

#[test]
fn dev_mode_rand_in_comparator_faults() {
    let mut story = build_gradual(RAND_CMP);
    let err = run(&mut story).unwrap_err();
    assert!(
        matches!(
            err,
            RuntimeError::ComparatorWroteState {
                verb: "sort_by",
                role: "comparator",
                what: "advanced the RNG state (a draw is a write)",
            }
        ),
        "{err:?}"
    );
}

#[test]
fn prod_mode_rand_in_comparator_is_defined_and_preserves_elements() {
    let mut story = build_gradual(RAND_CMP);
    story.set_exec_mode(ExecMode::Prod);
    let out = run(&mut story).expect("prod mode keeps moving");
    // The §4b guarantee floor holds by construction even under a random
    // comparator: SOME permutation of the input, never worse. Each element
    // appears exactly once.
    for element in ["10", "20", "30"] {
        assert_eq!(
            out.matches(element).count(),
            1,
            "element {element} must appear exactly once in {out:?}"
        );
    }
}

/// The exemption leg: visit-count increments from the comparator's own
/// invocation are the ruled in-story dispatch semantics, NOT world-writes.
/// The comparator is a visit-counted knot function (its count is read
/// afterwards, so the VISITS flag is compiled in) — legal in dev, and the
/// count proves the increments actually happened mid-sort.
const VISIT_COUNTED: &str = "\
~ temp a = #[2, 1, 3]\n\
~ temp c = #fn(via_helper)\n\
~ sort_by(a, c)\n\
sorted: {a}; helper visits: {via_helper}.\n\
-> END\n\
\n\
=== function via_helper(x, y) ===\n\
~ return x - y\n";

#[test]
fn dev_mode_visit_counted_comparator_stays_legal() {
    let mut story = build_gradual(VISIT_COUNTED);
    let out = run(&mut story).expect("visit counting is exempt — dev must not fault");
    assert_eq!(out, "sorted: [1, 2, 3]; helper visits: 3.\n");
}

/// The read posture: a comparator that READS a global is legal at runtime
/// in both modes (E119's static bound owns reads; no runtime read-guard).
const READ_ONLY: &str = "\
VAR flip = 1\n\
~ temp a = #[2, 1, 3]\n\
~ temp c = #fn(scaled)\n\
~ sort_by(a, c)\n\
sorted: {a}.\n\
-> END\n\
\n\
=== function scaled(x, y) ===\n\
~ return (x - y) * flip\n";

#[test]
fn read_only_comparator_unaffected_in_both_modes() {
    let mut dev = build_gradual(READ_ONLY);
    assert_eq!(run(&mut dev).expect("dev"), "sorted: [1, 2, 3].\n");

    let mut prod = build_gradual(READ_ONLY);
    prod.set_exec_mode(ExecMode::Prod);
    assert_eq!(run(&mut prod).expect("prod"), "sorted: [1, 2, 3].\n");
}
