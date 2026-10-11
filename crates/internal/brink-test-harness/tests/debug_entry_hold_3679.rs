//! #3679: a debug run checks the breakpoint at the position it starts from,
//! unless it is resuming from a stop there.
//!
//! `debug_run` used to skip its starting position unconditionally, which is
//! how a hold resumes. But `Story::choose` leaves the flow on the first
//! instruction of the taken choice — where a breakpoint on the choice line
//! binds — so a run after a choice stepped straight past it, against the
//! 2026-10-09 ruling that a breakpoint on a choice line holds when that
//! choice is taken. The flow now remembers where the last debug verb
//! stopped (stamped with the run, which a choice or a jump renews), and
//! only a resume from that exact stop skips the check.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::sync::Arc;

use brink_runtime::{BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugStopReason, FastRng, Story};

const SRC: &str = "-> pick\n\
                   === pick ===\n\
                   Which way?\n\
                   * [Left] You go left.\n  -> END\n\
                   * [Right] You go right.\n  -> END\n";

fn story() -> Story<FastRng> {
    let out = brink_compiler::compile("t.ink", |_| Ok(SRC.to_owned())).expect("compile");
    let (program, tables) = brink_runtime::link(&out.data).expect("link");
    Story::<FastRng>::new(Arc::new(program), tables)
}

/// Run to the choice point, take `index`, and report where that left the
/// flow — the place a breakpoint on that choice's line binds.
fn landing_of(index: usize) -> brink_runtime::DebugPosition {
    let mut s = story();
    let out = s
        .debug_run(&BreakpointSet::new(), DEFAULT_DEBUG_BUDGET)
        .expect("runs");
    assert_eq!(out.reason, DebugStopReason::Choices);
    s.choose(index).expect("choose");
    s.debug_position().expect("a position after choose")
}

#[test]
fn a_breakpoint_where_a_taken_choice_lands_holds_and_resumes() {
    let right = landing_of(1);
    let mut bps = BreakpointSet::new();
    bps.insert(right.container_idx, right.offset, "right");

    let mut s = story();
    let out = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    assert_eq!(
        out.reason,
        DebugStopReason::Choices,
        "offering is not taking"
    );
    s.choose(1).expect("choose");

    let held = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    assert!(
        matches!(held.reason, DebugStopReason::Breakpoint { .. }),
        "taking the marked choice holds before it runs: {held:?}"
    );
    assert_eq!(held.position, Some(right));

    // Resuming from that hold goes on, rather than holding again.
    let resumed = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    assert_eq!(resumed.reason, DebugStopReason::Terminal, "{resumed:?}");
}

#[test]
fn taking_the_other_choice_runs_past_the_breakpoint() {
    let right = landing_of(1);
    let mut bps = BreakpointSet::new();
    bps.insert(right.container_idx, right.offset, "right");

    let mut s = story();
    let _ = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    s.choose(0).expect("choose");
    let out = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    assert_eq!(out.reason, DebugStopReason::Terminal, "{out:?}");
}

/// A fresh start is not a resume either: a breakpoint on the very first
/// instruction holds at Start (before #3679 it could never be hit), and
/// running on from it goes to the choice point.
#[test]
fn a_breakpoint_on_the_first_instruction_holds_at_start() {
    let mut s = story();
    let start = s.debug_position().expect("a starting position");
    let mut bps = BreakpointSet::new();
    bps.insert(start.container_idx, start.offset, "start");

    let held = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    assert!(
        matches!(held.reason, DebugStopReason::Breakpoint { .. }),
        "{held:?}"
    );
    assert_eq!(held.position, Some(start));
    let on = s.debug_run(&bps, DEFAULT_DEBUG_BUDGET).expect("runs");
    assert_eq!(on.reason, DebugStopReason::Choices, "{on:?}");
}
