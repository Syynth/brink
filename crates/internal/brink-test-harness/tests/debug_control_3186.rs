//! Proof tests for issue #3186 (debugger D8): the VM debug-hooks seam —
//! breakpoints, pause/resume, and step in/over/out, plus watchpoints via
//! the existing `WriteObserver` seam.
//!
//! Compiled only with `brink-runtime`'s `debug-hooks` feature on (this
//! crate's own `debug-hooks` feature, `required-features` below) —
//! deliberately mirrors `t2_ground_truth_effects.rs`'s relationship to
//! `effect-trace`. The complementary "disabled build is unaffected" proof
//! lives in `debug_hooks_production_path_unaffected_3186.rs`, which
//! carries no `required-features` at all so it runs identically in both
//! gate configurations — see that file's own doc.
//!
//! Follows `debug_position_3182.rs`'s conventions (same fixture-compile
//! helpers, same cross-checking-against-`resolve_address` discipline,
//! same both-`.ink`-and-`.brink` coverage for the core claims) since this
//! is the direct sequel: D4 (#3182) made position *readable*, D8 (#3186)
//! makes it *controllable*.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use brink_format::Opcode;
use brink_runtime::{
    BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugStopReason, FastRng, Program, RuntimeError, StepMode,
    Story, WatchpointObserver,
};

type LineTables = Vec<Vec<brink_format::LineEntry>>;

fn compile_ink(src: &str) -> (Program, LineTables) {
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
    .expect("compile .ink");
    brink_runtime::link(&out.data).expect("link")
}

/// Same scratch-directory native-compile helper as `debug_position_3182.rs`
/// (native discovery always walks a real filesystem).
fn compile_native(src: &str) -> (Program, LineTables) {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "brink-debug-control-3186-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let entry = dir.join("main.brink");
    std::fs::write(&entry, src).expect("write scratch fixture");

    let result = brink_compiler::compile_path(&entry);
    let _ = std::fs::remove_dir_all(&dir);
    let out = result.expect("compile .brink");
    brink_runtime::link(&out.data).expect("link")
}

// ── 1. Breakpoints halt BEFORE the matching instruction (both surfaces) ──

/// A breakpoint at `side`'s own entry address halts the default flow
/// exactly there — checked before `side`'s first opcode executes, proven
/// by decoding that first opcode independently (`Opcode::decode`, the same
/// technique `debug_position_3182.rs`'s boundary test uses) and asserting
/// a subsequent `debug_step(Into)` advances the position by exactly that
/// opcode's own length, never more.
#[test]
fn ink_breakpoint_halts_before_matching_instruction() {
    let (program, tables) = compile_ink(
        "-> side\n\
         === side ===\n\
         Side content.\n\
         -> DONE\n",
    );
    let def_id = program
        .definition_id_for_path("side")
        .expect("side should resolve");
    let (container_idx, offset) = program.resolve_address(def_id).expect("resolve_address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let mut breakpoints = BreakpointSet::new();
    let bp_id = breakpoints.insert(container_idx, offset, "side-entry");

    let outcome = story
        .debug_run(&breakpoints, DEFAULT_DEBUG_BUDGET)
        .expect("debug_run");
    assert_eq!(
        outcome.reason,
        DebugStopReason::Breakpoint {
            id: bp_id,
            name: "side-entry".to_owned(),
        }
    );
    let pos = outcome.position.expect("breakpoint stop has a position");
    assert_eq!((pos.container_idx, pos.offset), (container_idx, offset));
    assert_eq!(
        story.debug_position(),
        outcome.position,
        "Story::debug_position must agree with the outcome it just returned"
    );

    // Independently decode the not-yet-executed instruction at the halted
    // position, then single-step it and confirm the position advances by
    // exactly its own decoded length — proof the halt landed BEFORE it ran,
    // not after.
    let bytecode = program.container_bytecode(container_idx);
    let mut decode_off = offset;
    Opcode::decode(bytecode, &mut decode_off)
        .expect("the halted-at instruction must decode cleanly");
    let expected_next_offset = decode_off;

    let step = story
        .debug_step(StepMode::Into, DEFAULT_DEBUG_BUDGET)
        .expect("debug_step into");
    assert_eq!(step.reason, DebugStopReason::Step);
    let after = step.position.expect("position after a single Into step");
    assert_eq!(
        (after.container_idx, after.offset),
        (container_idx, expected_next_offset),
        "single-stepping the halted-at instruction must advance by exactly its own decoded length"
    );
}

/// Same claim on the native `.brink` surface — both-surfaces ruling
/// (`docs/debugger-spec.md` §0).
#[test]
fn brink_native_breakpoint_halts_before_matching_instruction() {
    let (program, tables) = compile_native(
        "flow main() {\n\
         \x20\x20-> side\n\
         }\n\
         pub flow side() {\n\
         \x20\x20Side content.\n\
         \x20\x20-> DONE\n\
         }\n",
    );
    let def_id = program
        .definition_id_for_path("side")
        .expect("side should resolve");
    let (container_idx, offset) = program.resolve_address(def_id).expect("resolve_address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let mut breakpoints = BreakpointSet::new();
    let bp_id = breakpoints.insert(container_idx, offset, "side-entry");

    let outcome = story
        .debug_run(&breakpoints, DEFAULT_DEBUG_BUDGET)
        .expect("debug_run");
    assert_eq!(
        outcome.reason,
        DebugStopReason::Breakpoint {
            id: bp_id,
            name: "side-entry".to_owned(),
        }
    );
    let pos = outcome.position.expect("breakpoint stop has a position");
    assert_eq!((pos.container_idx, pos.offset), (container_idx, offset));
}

// ── 2. Step into / over / out across a knot→function call (both surfaces) ─

/// Drives `debug_step(Into)` from the caller's depth until the call-stack
/// depth first increases (i.e. the call instruction itself just executed),
/// bounded per `CLAUDE.md`'s "guard against unbounded growth". Returns the
/// number of `Into` calls that took.
fn intos_until_depth_increases(story: &mut Story<FastRng>, start_depth: usize) -> usize {
    let mut result = None;
    for n in 1..=200 {
        let outcome = story
            .debug_step(StepMode::Into, DEFAULT_DEBUG_BUDGET)
            .expect("debug_step into");
        assert_ne!(
            outcome.reason,
            DebugStopReason::Terminal,
            "hit a terminal VM outcome before the call ever happened"
        );
        if outcome.depth > start_depth {
            result = Some(n);
            break;
        }
    }
    assert!(
        result.is_some(),
        "call-stack depth never increased within 200 Into steps"
    );
    result.expect("just asserted above")
}

const KNOT_INTO_FUNCTION_INK: &str = "-> start\n\
     === start ===\n\
     Value: {double(21)}\n\
     -> DONE\n\
     === function double(x) ===\n\
     ~ return x * 2\n";

/// `debug_step(Into)` descends into the called function (depth 1 -> 2);
/// from there, `debug_step(Out)` returns to the caller (depth back to 1).
/// Separately, replaying the identical steps up to (not including) the
/// call and issuing `debug_step(Over)` instead must run through the whole
/// call without ever stopping inside it, landing directly back at depth 1
/// — `docs/debugger-spec.md` §4's `Function` row.
#[test]
fn ink_step_into_over_out_across_knot_into_function_call() {
    let (program_a, tables_a) = compile_ink(KNOT_INTO_FUNCTION_INK);
    let mut story_a = Story::<FastRng>::new(Arc::new(program_a), tables_a);
    assert_eq!(
        story_a.debug_call_stack_depth(),
        1,
        "starts at the root frame"
    );

    let n = intos_until_depth_increases(&mut story_a, 1);
    assert_eq!(
        story_a.debug_call_stack_depth(),
        2,
        "Into must have descended into the called function's own frame"
    );

    let out = story_a
        .debug_step(StepMode::Out, DEFAULT_DEBUG_BUDGET)
        .expect("debug_step out");
    assert_eq!(out.reason, DebugStopReason::Step);
    assert_eq!(
        out.depth, 1,
        "step-out must return to the caller's depth exactly"
    );

    // Replay: identical Into steps up to (not including) the call, then
    // step-over instead of stepping in.
    let (program_b, tables_b) = compile_ink(KNOT_INTO_FUNCTION_INK);
    let mut story_b = Story::<FastRng>::new(Arc::new(program_b), tables_b);
    for i in 0..n - 1 {
        let step = story_b
            .debug_step(StepMode::Into, DEFAULT_DEBUG_BUDGET)
            .expect("debug_step into (replay)");
        assert_eq!(
            step.depth, 1,
            "step {i} of the replay must still be at the caller's depth"
        );
    }
    assert_eq!(
        story_b.debug_call_stack_depth(),
        1,
        "positioned right before the call instruction, still at the caller's depth"
    );

    let over = story_b
        .debug_step(StepMode::Over, DEFAULT_DEBUG_BUDGET)
        .expect("debug_step over");
    assert_eq!(over.reason, DebugStopReason::Step);
    assert_eq!(
        over.depth, 1,
        "step-over must not stop inside the called function — it must land back at the caller's depth"
    );
}

/// Same claim on the native `.brink` surface.
#[test]
fn brink_native_step_into_over_out_across_knot_into_function_call() {
    let native_src = "flow main() {\n\
         \x20\x20Value: {double(21)}\n\
         \x20\x20-> DONE\n\
         }\n\
         fn double(x) {\n\
         \x20\x20return x * 2;\n\
         }\n";

    let (program_a, tables_a) = compile_native(native_src);
    let mut story_a = Story::<FastRng>::new(Arc::new(program_a), tables_a);
    assert_eq!(story_a.debug_call_stack_depth(), 1);

    let n = intos_until_depth_increases(&mut story_a, 1);
    assert_eq!(story_a.debug_call_stack_depth(), 2);

    let out = story_a
        .debug_step(StepMode::Out, DEFAULT_DEBUG_BUDGET)
        .expect("debug_step out");
    assert_eq!(out.depth, 1);
    assert_eq!(out.reason, DebugStopReason::Step);

    let (program_b, tables_b) = compile_native(native_src);
    let mut story_b = Story::<FastRng>::new(Arc::new(program_b), tables_b);
    for _ in 0..n - 1 {
        let step = story_b
            .debug_step(StepMode::Into, DEFAULT_DEBUG_BUDGET)
            .expect("debug_step into (replay)");
        assert_eq!(step.depth, 1);
    }
    let over = story_b
        .debug_step(StepMode::Over, DEFAULT_DEBUG_BUDGET)
        .expect("debug_step over");
    assert_eq!(over.depth, 1);
    assert_eq!(over.reason, DebugStopReason::Step);
}

// ── 3. Tunnel and thread: transient frames, caught mid-opcode ────────────
//
// Both are transient (they pop before the story yields a `Step` back to a
// `continue_single` caller), so — exactly as `debug_position_3182.rs`'s
// own tunnel/thread tests note — only opcode-level stepping (what this
// seam's `debug_run` gives, bypassing the buffered line-output path) can
// observe them live. `.ink`-only, matching that precedent: the frame
// model (`CallFrameType::Tunnel`/`Thread`) is shared post-HIR-convergence
// (`docs/debugger-spec.md` §0), so this exercises the shared runtime path
// regardless of which frontend produced the bytecode.

#[test]
fn ink_breakpoint_catches_a_live_tunnel_frame() {
    let (program, tables) = compile_ink(
        "Intro.\n\
         -> side ->\n\
         Back.\n\
         -> DONE\n\
         === side ===\n\
         Side content.\n\
         ->->\n",
    );
    let def_id = program
        .definition_id_for_path("side")
        .expect("side should resolve");
    let (container_idx, offset) = program.resolve_address(def_id).expect("resolve_address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let mut breakpoints = BreakpointSet::new();
    breakpoints.insert(container_idx, offset, "side-entry");

    let outcome = story
        .debug_run(&breakpoints, DEFAULT_DEBUG_BUDGET)
        .expect("debug_run");
    assert!(matches!(outcome.reason, DebugStopReason::Breakpoint { .. }));
    assert_eq!(
        outcome.depth, 2,
        "root + tunnel frame must both be live at the breakpoint"
    );

    // Cross-check against the existing D4 snapshot API: the innermost
    // frame really is a `tunnel`, not merely depth 2 for some other
    // reason.
    let snap = story.debug_snapshot();
    assert_eq!(snap.call_stack.first().map(|f| f.kind), Some("tunnel"));
}

#[test]
fn ink_breakpoint_catches_a_live_thread_frame() {
    let (program, tables) = compile_ink(
        "<- choices\n\
         { CHOICE_COUNT() }\n\
         = end\n\
         -> END\n\
         = choices\n\
         * one -> end\n\
         * two -> end\n",
    );
    let def_id = program
        .definition_id_for_path("choices")
        .expect("choices should resolve");
    let (container_idx, offset) = program.resolve_address(def_id).expect("resolve_address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let mut breakpoints = BreakpointSet::new();
    breakpoints.insert(container_idx, offset, "choices-entry");

    let outcome = story
        .debug_run(&breakpoints, DEFAULT_DEBUG_BUDGET)
        .expect("debug_run");
    assert!(matches!(outcome.reason, DebugStopReason::Breakpoint { .. }));

    let snap = story.debug_snapshot();
    assert_eq!(snap.call_stack.first().map(|f| f.kind), Some("thread"));
}

// ── 4. Step-out from the outermost (Root) frame is refused, not run-away ──

#[test]
fn ink_step_out_from_root_frame_is_refused_without_stepping() {
    let (program, tables) = compile_ink("Line one.\nLine two.\n-> DONE\n");
    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    assert_eq!(story.debug_call_stack_depth(), 1, "only the Root frame");
    let before = story.debug_position();

    let outcome = story
        .debug_step(StepMode::Out, DEFAULT_DEBUG_BUDGET)
        .expect("debug_step out");
    assert_eq!(outcome.reason, DebugStopReason::NoStepOutTarget);
    assert_eq!(
        story.debug_position(),
        before,
        "refusing step-out from Root must not execute any VM step"
    );
}

// ── 5. The debug budget is its own distinct, reportable outcome ──────────

/// A `debug_run` that can never reach its (empty) breakpoint set and never
/// naturally terminates within the given ceiling reports
/// `DebugBudgetExceeded` — never `StepLimitExceeded`, which would
/// misattribute the debug-only budget as the production one (2026-08-28
/// step-limit ruling on issue #3186).
#[test]
fn debug_run_reports_its_own_budget_exceeded_error_not_the_production_one() {
    let (program, tables) = compile_ink(
        "VAR i = 0\n\
         -> loop\n\
         === loop ===\n\
         ~ i = i + 1\n\
         -> loop\n",
    );
    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let breakpoints = BreakpointSet::new(); // empty — nothing will ever halt it
    let ceiling = 5;
    let err = story
        .debug_run(&breakpoints, ceiling)
        .expect_err("an unbounded loop must exceed a tiny debug budget");
    let matched = matches!(&err, RuntimeError::DebugBudgetExceeded { .. });
    assert!(matched, "expected DebugBudgetExceeded, got {err:?}");
    let RuntimeError::DebugBudgetExceeded {
        breakpoint,
        ceiling: reported_ceiling,
    } = &err
    else {
        unreachable!("just asserted above")
    };
    assert_eq!(breakpoint, "run");
    assert_eq!(*reported_ceiling, ceiling);
}

// ── 6. Watchpoints reuse WriteObserver — no second observer mechanism ────

#[test]
fn debug_run_watching_stops_on_the_watched_global_write() {
    let (program, tables) = compile_ink(
        "VAR x = 0\n\
         -> start\n\
         === start ===\n\
         ~ x = 5\n\
         Done.\n\
         -> DONE\n",
    );
    let x_idx = (0..program.global_count())
        .find(|&idx| program.global_name(idx) == Some("x"))
        .expect("VAR x must have a global slot");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let breakpoints = BreakpointSet::new();
    let mut watchpoints = WatchpointObserver::new(vec![x_idx]);
    let outcome = story
        .debug_run_watching(&breakpoints, &mut watchpoints, DEFAULT_DEBUG_BUDGET)
        .expect("debug_run_watching");

    assert_eq!(
        outcome.reason,
        DebugStopReason::Watchpoint { global_idx: x_idx }
    );

    // The write already landed by the time the watchpoint fires
    // (`ObservedContext::set_global` notifies *after* writing) — confirm
    // via the existing, unrelated D4 snapshot API.
    let snap = story.debug_snapshot();
    let x = snap
        .globals
        .iter()
        .find(|g| g.name == "x")
        .expect("x must be in the snapshot's globals");
    assert_eq!(x.value, "5");
}
