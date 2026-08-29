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
        .debug_run_flow(Some("worker"), &breakpoints, DEFAULT_DEBUG_BUDGET)
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
        story.debug_run_flow(Some("ghost"), &bp, DEFAULT_DEBUG_BUDGET),
        Err(RuntimeError::UnknownFlow(_))
    ));
    let mut watch = brink_runtime::WatchpointObserver::new(Vec::new());
    assert!(matches!(
        story.debug_run_watching_flow(Some("ghost"), &bp, &mut watch, DEFAULT_DEBUG_BUDGET),
        Err(RuntimeError::UnknownFlow(_))
    ));
    assert!(matches!(
        story.debug_step_flow(Some("ghost"), StepMode::Into, &bp, DEFAULT_DEBUG_BUDGET),
        Err(RuntimeError::UnknownFlow(_))
    ));
    assert!(matches!(
        story.debug_step_line_flow(Some("ghost"), StepMode::Into, &bp, DEFAULT_DEBUG_BUDGET),
        Err(RuntimeError::UnknownFlow(_))
    ));
}
