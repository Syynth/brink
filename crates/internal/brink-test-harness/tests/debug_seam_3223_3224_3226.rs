//! Proof tests for the runtime debugger trio (#3223, #3224, #3226): the
//! multi-flow debug seam, external-call resolution in the debug loops, and
//! the multi-hit watchpoint drain.
//!
//! Compiled only with `brink-runtime`'s `debug-hooks` feature on (this
//! crate's own `debug-hooks` feature, `required-features` in Cargo.toml) —
//! same relationship as `debug_control_3186.rs`, whose fixture-compile
//! helpers and cross-checking-against-`resolve_address` discipline this
//! file follows: it is the direct sequel, closing the three gaps that
//! review of D8 (#3186) filed.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::sync::Arc;

use brink_runtime::{
    BreakpointSet, DEFAULT_DEBUG_BUDGET, DebugStopReason, FastRng, Program, RuntimeError, StepMode,
    Story,
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

/// A story whose default flow sits in `main_loop` while a spawnable `side`
/// knot exists for a named flow — the two-flows fixture #3223's proof
/// asks for.
fn two_knot_story() -> (Arc<Program>, Story<FastRng>) {
    let (program, tables) = compile_ink(
        "-> main_loop\n\
         === main_loop ===\n\
         Main line one.\n\
         Main line two.\n\
         -> DONE\n\
         === side ===\n\
         Side line one.\n\
         Side line two.\n\
         -> DONE\n",
    );
    let program = Arc::new(program);
    let story = Story::<FastRng>::new(Arc::clone(&program), tables);
    (program, story)
}

// ── #3223: the seam drives named flows ──────────────────────────────

/// A breakpoint inside a named (isolated) flow halts THAT flow at the
/// right position via `debug_run_flow`, while the default flow's position
/// is untouched — the exact proof #3223 names.
#[test]
fn breakpoint_in_named_flow_halts_it_and_leaves_the_default_flow_alone() {
    let (program, mut story) = two_knot_story();
    let side_id = program
        .definition_id_for_path("side")
        .expect("side resolves");
    let (side_idx, side_off) = program.resolve_address(side_id).expect("resolve_address");

    story.spawn_flow("worker", side_id).expect("spawn_flow");

    // The spawned flow STARTS at `side`'s entry, and the resume rule
    // deliberately skips a breakpoint at the position already stopped at
    // (`debug_run`'s "resume is impossible" fix) — so arm the breakpoint
    // one decoded instruction past the entry, cross-checked against the
    // bytecode exactly as `debug_control_3186.rs` does.
    let bytecode = program.container_bytecode(side_idx);
    let mut next_off = side_off;
    brink_format::Opcode::decode(bytecode, &mut next_off).expect("entry instruction decodes");

    let default_pos_before = story.debug_position();
    let mut breakpoints = BreakpointSet::new();
    let bp_id = breakpoints.insert(side_idx, next_off, "side-entry");

    let outcome = story
        .debug_run_flow(
            Some("worker"),
            &brink_runtime::FallbackHandler,
            &breakpoints,
            DEFAULT_DEBUG_BUDGET,
        )
        .expect("debug_run_flow");
    assert_eq!(
        outcome.reason,
        DebugStopReason::Breakpoint {
            id: bp_id,
            name: "side-entry".to_owned(),
        }
    );
    let pos = outcome.position.expect("breakpoint stop has a position");
    assert_eq!((pos.container_idx, pos.offset), (side_idx, next_off));

    // The getters agree, and the default flow never moved.
    assert_eq!(
        story
            .debug_position_flow(Some("worker"))
            .expect("known flow"),
        outcome.position,
    );
    assert_eq!(
        story.debug_position(),
        default_pos_before,
        "debugging a named flow must not move the default flow"
    );
    assert_eq!(
        story.debug_position_flow(None).expect("default selectable"),
        default_pos_before,
        "None selects the default flow"
    );
}

/// The same seam drives a SHARED flow (`spawn_flow_shared`), whose writes
/// land in the default context — `debug_step_flow` advances it while the
/// default flow's own position stays put.
#[test]
fn debug_step_flow_advances_a_shared_flow_independently() {
    let (program, mut story) = two_knot_story();
    let side_id = program
        .definition_id_for_path("side")
        .expect("side resolves");
    let (side_idx, _) = program.resolve_address(side_id).expect("resolve_address");

    story
        .spawn_flow_shared("narrator", Some(side_idx))
        .expect("spawn_flow_shared");

    let default_pos_before = story.debug_position();
    let outcome = story
        .debug_step_flow(
            Some("narrator"),
            &brink_runtime::FallbackHandler,
            StepMode::Into,
            &BreakpointSet::new(),
            DEFAULT_DEBUG_BUDGET,
        )
        .expect("debug_step_flow");
    assert_eq!(outcome.reason, DebugStopReason::Step);
    assert_eq!(
        story
            .debug_call_stack_depth_flow(Some("narrator"))
            .expect("known flow"),
        outcome.depth,
    );
    assert_eq!(
        story.debug_position(),
        default_pos_before,
        "stepping a shared flow must not move the default flow"
    );
}

/// Every flow-taking verb rejects an unknown name with
/// `RuntimeError::UnknownFlow` — the same error the production
/// `continue_flow*` methods raise — rather than silently driving the
/// default flow.
#[test]
fn unknown_flow_name_is_an_error_on_every_verb() {
    let (_, mut story) = two_knot_story();
    let bp = BreakpointSet::new();

    assert!(matches!(
        story.debug_position_flow(Some("ghost")),
        Err(RuntimeError::UnknownFlow(n)) if n == "ghost"
    ));
    assert!(matches!(
        story.debug_call_stack_depth_flow(Some("ghost")),
        Err(RuntimeError::UnknownFlow(_))
    ));
    assert!(matches!(
        story.debug_run_flow(
            Some("ghost"),
            &brink_runtime::FallbackHandler,
            &bp,
            DEFAULT_DEBUG_BUDGET
        ),
        Err(RuntimeError::UnknownFlow(_))
    ));
    let mut watch = brink_runtime::WatchpointObserver::new(Vec::new());
    assert!(matches!(
        story.debug_run_watching_flow(
            Some("ghost"),
            &brink_runtime::FallbackHandler,
            &bp,
            &mut watch,
            DEFAULT_DEBUG_BUDGET
        ),
        Err(RuntimeError::UnknownFlow(_))
    ));
    assert!(matches!(
        story.debug_step_flow(
            Some("ghost"),
            &brink_runtime::FallbackHandler,
            StepMode::Into,
            &bp,
            DEFAULT_DEBUG_BUDGET
        ),
        Err(RuntimeError::UnknownFlow(_))
    ));
    assert!(matches!(
        story.debug_step_line_flow(
            Some("ghost"),
            &brink_runtime::FallbackHandler,
            StepMode::Into,
            &bp,
            DEFAULT_DEBUG_BUDGET
        ),
        Err(RuntimeError::UnknownFlow(_))
    ));
}

// ── #3224: external-call frames are debuggable ──────────────────────

use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult};

/// Handler binding `beep` to a constant — the synchronous-resolution case.
struct Beep;
impl ExternalFnHandler for Beep {
    fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
        assert_eq!(name, "beep", "only one external in these fixtures");
        ExternalResult::Resolved(Value::Int(7))
    }
}

/// Handler that always defers — the async / world-access case.
struct Defer;
impl ExternalFnHandler for Defer {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Pending
    }
}

/// A story whose default flow crosses an `EXTERNAL` call with an in-story
/// fallback. The fallback makes the `FallbackHandler` path (the bare
/// `debug_run`) meaningful too.
fn external_story() -> (Arc<Program>, Story<FastRng>) {
    let (program, tables) = compile_ink(
        "EXTERNAL beep(x)\n\
         -> main_loop\n\
         === main_loop ===\n\
         Before call.\n\
         The value is {beep(2)}.\n\
         After call.\n\
         -> DONE\n\
         === function beep(x) ===\n\
         ~ return x * 10\n",
    );
    let program = Arc::new(program);
    let story = Story::<FastRng>::new(Arc::clone(&program), tables);
    (program, story)
}

/// `debug_run` on a story with a bound external passes through the call
/// without erroring — the exact failure #3224 names (`debug_run` used to
/// leave the `External` frame unresolved and error
/// `UnresolvedExternalCall` on the next step). Proven with a real
/// handler, and with the bare method's `FallbackHandler` (in-story
/// fallback body).
#[test]
fn debug_run_passes_through_a_bound_external() {
    let (_, mut story) = external_story();
    let outcome = story
        .debug_run_flow(None, &Beep, &BreakpointSet::new(), DEFAULT_DEBUG_BUDGET)
        .expect("a bound external must not error mid-debug-session");
    assert_eq!(outcome.reason, DebugStopReason::Terminal);

    // The bare method (FallbackHandler) runs the in-story fallback body.
    let (_, mut story) = external_story();
    let outcome = story
        .debug_run(&BreakpointSet::new(), DEFAULT_DEBUG_BUDGET)
        .expect("the in-story fallback resolves under the bare method");
    assert_eq!(outcome.reason, DebugStopReason::Terminal);
}

/// Walk `story` to the instruction that pushes the `External` frame: step
/// `Into` repeatedly until the NEXT single step would cross the external.
/// Returns the position at that call site.
fn step_to_external_call_site(
    story: &mut Story<FastRng>,
    handler: &dyn ExternalFnHandler,
) -> brink_runtime::DebugPosition {
    let bp = BreakpointSet::new();
    loop {
        let before = story.debug_position().expect("running story has position");
        let depth_before = story.debug_call_stack_depth();
        // Probe with a CLONE so the real story stays put at the call site.
        let mut probe = story.clone();
        let probe_outcome = probe
            .debug_step_flow(None, &Defer, StepMode::Into, &bp, DEFAULT_DEBUG_BUDGET)
            .expect("probe step");
        if probe_outcome.reason == DebugStopReason::AwaitingExternal {
            let _ = depth_before;
            return before;
        }
        let stepped = story
            .debug_step_flow(None, handler, StepMode::Into, &bp, DEFAULT_DEBUG_BUDGET)
            .expect("advance step");
        assert_ne!(
            stepped.reason,
            DebugStopReason::Terminal,
            "walked past the external without finding the call site"
        );
    }
}

/// Spec §4: step-into on a call that pushes an `External` frame behaves
/// like step-over — same resulting position AND depth, proven by driving
/// two clones of the same story from the same call site.
#[test]
fn step_into_on_external_behaves_like_step_over() {
    let (_, mut story) = external_story();
    let call_site = step_to_external_call_site(&mut story, &Beep);
    let _ = call_site;

    let mut into_story = story.clone();
    let mut over_story = story;
    let bp = BreakpointSet::new();

    let into = into_story
        .debug_step_flow(None, &Beep, StepMode::Into, &bp, DEFAULT_DEBUG_BUDGET)
        .expect("Into across external");
    let over = over_story
        .debug_step_flow(None, &Beep, StepMode::Over, &bp, DEFAULT_DEBUG_BUDGET)
        .expect("Over across external");

    assert_eq!(into.reason, over.reason, "same stop reason");
    assert_eq!(into.position, over.position, "same resulting position");
    assert_eq!(into.depth, over.depth, "same resulting depth");
}

/// A deferring handler parks the run with `AwaitingExternal` — frame
/// intact — and `resolve_external` + another `debug_run` resumes to the
/// terminal, on the DEFAULT flow.
#[test]
fn pending_external_parks_and_resumes_on_the_default_flow() {
    let (_, mut story) = external_story();
    let bp = BreakpointSet::new();
    let outcome = story
        .debug_run_flow(None, &Defer, &bp, DEFAULT_DEBUG_BUDGET)
        .expect("deferred external is a clean park, not an error");
    assert_eq!(outcome.reason, DebugStopReason::AwaitingExternal);
    assert!(
        story.has_pending_external(),
        "the External frame must be left intact for out-of-band resolution"
    );

    story.resolve_external(Value::Int(20));
    let outcome = story
        .debug_run_flow(None, &Defer, &bp, DEFAULT_DEBUG_BUDGET)
        .expect("resume after out-of-band resolution");
    assert_eq!(outcome.reason, DebugStopReason::Terminal);
}

/// The same park/resume works on a NAMED flow via
/// `resolve_external_flow` — #3223's seam and #3224's contract compose.
#[test]
fn pending_external_parks_and_resumes_on_a_named_flow() {
    let (program, mut story) = external_story();
    let main_id = program
        .definition_id_for_path("main_loop")
        .expect("main_loop resolves");
    story.spawn_flow("worker", main_id).expect("spawn_flow");

    let bp = BreakpointSet::new();
    let outcome = story
        .debug_run_flow(Some("worker"), &Defer, &bp, DEFAULT_DEBUG_BUDGET)
        .expect("deferred external parks the named flow");
    assert_eq!(outcome.reason, DebugStopReason::AwaitingExternal);

    story
        .resolve_external_flow("worker", Value::Int(20))
        .expect("known flow resolves");
    let outcome = story
        .debug_run_flow(Some("worker"), &Defer, &bp, DEFAULT_DEBUG_BUDGET)
        .expect("resume the named flow");
    assert_eq!(outcome.reason, DebugStopReason::Terminal);

    assert!(matches!(
        story.resolve_external_flow("ghost", Value::Null),
        Err(RuntimeError::UnknownFlow(_))
    ));
}

// ── #3226: multi-hit watchpoint drain ───────────────────────────────

use brink_runtime::{WatchpointObserver, WriteObserver};

fn global_idx(program: &Program, name: &str) -> u32 {
    (0..program.global_count())
        .find(|&idx| program.global_name(idx) == Some(name))
        .expect("VAR must have a global slot")
}

/// The #3226 premise probe. The VM has exactly two `set_global` sites —
/// `Opcode::SetGlobal`, and `Opcode::SetTemp` writing through a `ref`
/// pointer — and each is a single-write opcode, so ONE `vm::step` can
/// queue at most ONE watchpoint hit. This pins that behaviorally over a
/// fixture exercising BOTH opcode shapes back-to-back: every stop leaves
/// the observer's queue empty (a second same-step hit would still be
/// queued at the first stop), and each stop is correctly attributed —
/// the earlier watched global is written, the later one still isn't.
#[test]
fn one_step_never_queues_a_second_watchpoint_hit() {
    let (program, tables) = compile_ink(
        "VAR a = 0\n\
         VAR b = 0\n\
         -> start\n\
         === start ===\n\
         ~ a = 1\n\
         ~ set_it(b, 2)\n\
         Done.\n\
         -> DONE\n\
         === function set_it(ref target, v) ===\n\
         ~ target = v\n",
    );
    let a_idx = global_idx(&program, "a");
    let b_idx = global_idx(&program, "b");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);
    let bp = BreakpointSet::new();
    let mut watch = WatchpointObserver::new(vec![a_idx, b_idx]);

    // Stop 1: the plain `SetGlobal` write to `a`.
    let outcome = story
        .debug_run_watching(&bp, &mut watch, DEFAULT_DEBUG_BUDGET)
        .expect("first watched write");
    assert_eq!(
        outcome.reason,
        DebugStopReason::Watchpoint { global_idx: a_idx }
    );
    assert!(
        watch.hits().is_empty(),
        "one step queued a second hit — the #3226 premise is reachable \
         after all; the drain contract needs a redesign, not this pin"
    );
    assert_eq!(
        story.variable("a"),
        Some(&Value::Int(1)),
        "stop 1 is after a's write"
    );
    assert_eq!(
        story.variable("b"),
        Some(&Value::Int(0)),
        "stop 1 is BEFORE b's write — the hit is attributed to its own \
         instruction, not a later one"
    );

    // Stop 2: the `ref`-pointer write to `b` (`SetTemp` through a
    // `VariablePointer` — the VM's only other `set_global` site).
    let outcome = story
        .debug_run_watching(&bp, &mut watch, DEFAULT_DEBUG_BUDGET)
        .expect("second watched write");
    assert_eq!(
        outcome.reason,
        DebugStopReason::Watchpoint { global_idx: b_idx }
    );
    assert!(watch.hits().is_empty());
    assert_eq!(story.variable("b"), Some(&Value::Int(2)));

    // And the story still finishes cleanly.
    let outcome = story
        .debug_run_watching(&bp, &mut watch, DEFAULT_DEBUG_BUDGET)
        .expect("run out");
    assert_eq!(outcome.reason, DebugStopReason::Terminal);
}

/// The hardening half (#3226): a hit already queued when
/// `debug_run_watching` is called — the observer doubles as a
/// non-pausing logger on the production path, so leftovers are a real
/// state — reports IMMEDIATELY at the current position, before any
/// stepping, instead of being attributed to whatever instruction the
/// next step executes.
#[test]
fn leftover_watchpoint_hit_reports_before_any_stepping() {
    let (program, tables) = compile_ink(
        "VAR a = 0\n\
         -> start\n\
         === start ===\n\
         Some content first.\n\
         ~ a = 1\n\
         -> DONE\n",
    );
    let a_idx = global_idx(&program, "a");
    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);
    let bp = BreakpointSet::new();

    // Simulate a logging-mode leftover: a hit queued outside any debug run.
    let mut watch = WatchpointObserver::new(vec![a_idx]);
    watch.on_set_global(a_idx, &Value::Int(99));

    let pos_before = story.debug_position();
    let outcome = story
        .debug_run_watching(&bp, &mut watch, DEFAULT_DEBUG_BUDGET)
        .expect("leftover drain");
    assert_eq!(
        outcome.reason,
        DebugStopReason::Watchpoint { global_idx: a_idx }
    );
    assert_eq!(
        outcome.position, pos_before,
        "a leftover hit must report at the position it was found at — \
         zero instructions execute before it drains"
    );
    assert_eq!(
        story.variable("a"),
        Some(&Value::Int(0)),
        "no stepping happened: the story's own write to a has not run"
    );
}
