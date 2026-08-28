//! Proof tests for issue #3182 (debugger D4): the runtime's public
//! `(container_idx, offset)` execution-position reporting —
//! [`brink_runtime::DebugPosition`] on [`brink_runtime::DebugFrame`] and
//! [`brink_runtime::DebugSnapshot`].
//!
//! Every position assertion here is cross-checked against
//! [`Program::resolve_address`] (already public under the `testing`
//! feature) or against an independent decode of the container's own
//! bytecode via `Opcode::decode` — never against the new accessor alone —
//! per the issue's proof requirement. Covers both source surfaces (`.ink`
//! and `.brink`), a nested knot→function call, a tunnel, and a thread.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use brink_format::Opcode;
use brink_runtime::{FastRng, Program, Story};

type LineTables = Vec<Vec<brink_format::LineEntry>>;

/// Compile an inline `.ink` source and link it.
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

/// Compile an inline `.brink` (native) source and link it.
///
/// Native discovery always walks a real filesystem
/// (`brink_driver::Driver::discover_native`'s `RealFs`, per
/// `brink-compiler/src/driver.rs`'s `prepare_driver` doc — a `.brink` entry
/// ignores any `read_file` closure entirely), so unlike `compile_ink` this
/// writes to a scratch directory rather than compiling from a string
/// directly. Each call gets its own directory (an atomic counter plus the
/// process id) so parallel test threads never collide.
fn compile_native(src: &str) -> (Program, LineTables) {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "brink-debug-position-3182-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let entry = dir.join("main.brink");
    std::fs::write(&entry, src).expect("write scratch fixture");

    let result = brink_compiler::compile_path(&entry);
    // Best-effort: don't leak scratch fixtures into shared /tmp. Not load-
    // bearing for the test itself, so a failure here is ignored rather than
    // masking the real `compile_path` result.
    let _ = std::fs::remove_dir_all(&dir);
    let out = result.expect("compile .brink");
    brink_runtime::link(&out.data).expect("link")
}

/// Step the default flow one VM opcode at a time (via the testing-only
/// `Story::step_once`), calling `on_step` with a fresh `debug_snapshot()`
/// after every step, until it returns `true` or the story finishes/errors.
/// Bounded — this drives real bytecode, and per `CLAUDE.md` ("guard against
/// unbounded growth") any such loop needs a cap independent of the VM's own
/// step limit.
fn step_until(
    story: &mut Story<FastRng>,
    max_steps: usize,
    mut on_step: impl FnMut(&brink_runtime::DebugSnapshot) -> bool,
) -> bool {
    for _ in 0..max_steps {
        match story.step_once() {
            Ok(Some(_)) => {
                if on_step(&story.debug_snapshot()) {
                    return true;
                }
            }
            Ok(None) | Err(_) => return false,
        }
    }
    false
}

// ── 1. Entry position matches `Program::resolve_address` exactly (both surfaces) ──

/// Spawning a flow at a named knot puts the reported position at exactly
/// that knot's linked address — cross-checked against
/// [`Program::resolve_address`], independent of the new accessor.
#[test]
fn ink_position_matches_resolve_address_at_flow_entry() {
    let (program, tables) = compile_ink(
        "-> DONE\n\
         === side ===\n\
         Side content.\n\
         -> DONE\n",
    );
    let def_id = program
        .definition_id_for_path("side")
        .expect("side should resolve to a definition id");
    let expected = program
        .resolve_address(def_id)
        .expect("resolve_address should know side's address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);
    story.spawn_flow("side_flow", def_id).expect("spawn_flow");
    let snap = story
        .debug_snapshot_flow("side_flow")
        .expect("debug_snapshot_flow");

    let position = snap.position.expect("freshly spawned flow has a position");
    assert_eq!(
        (position.container_idx, position.offset),
        expected,
        "reported entry position must equal Program::resolve_address(side)"
    );
    // The frame's own position must agree with the snapshot-level one.
    assert_eq!(snap.call_stack[0].position, snap.position);
}

/// Same claim on the native `.brink` surface — the frame model is shared
/// end to end, and both surfaces are a ruled requirement
/// (`docs/debugger-spec.md`).
#[test]
fn brink_native_position_matches_resolve_address_at_flow_entry() {
    let (program, tables) = compile_native(
        "flow main() {\n\
         \x20\x20-> DONE\n\
         }\n\
         pub flow side() {\n\
         \x20\x20Side content.\n\
         \x20\x20-> DONE\n\
         }\n",
    );
    let def_id = program
        .definition_id_for_path("side")
        .expect("side should resolve to a definition id");
    let expected = program
        .resolve_address(def_id)
        .expect("resolve_address should know side's address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);
    story.spawn_flow("side_flow", def_id).expect("spawn_flow");
    let snap = story
        .debug_snapshot_flow("side_flow")
        .expect("debug_snapshot_flow");

    let position = snap.position.expect("freshly spawned flow has a position");
    assert_eq!(
        (position.container_idx, position.offset),
        expected,
        "reported entry position must equal Program::resolve_address(side) on the native surface"
    );
}

// ── 2. Reported offset lands on a real instruction boundary ──────────────

/// After running some content, the reported `offset` is independently
/// verified by decoding the container's own bytecode from `0` with
/// `Opcode::decode` and summing instruction lengths — it must land exactly
/// on a decode boundary, not merely "somewhere in bounds". This is the
/// strongest available proof that the reported offset is the real
/// instruction pointer and not a stale or off-by-one value, without
/// depending on the new accessor to check itself.
#[test]
fn position_offset_is_a_real_decoded_instruction_boundary() {
    let (program, tables) = compile_ink(
        "First line.\n\
         Second line.\n\
         -> DONE\n",
    );
    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    // Run to just after the first line so there's real, non-zero offset.
    let step = story.continue_single().expect("continue_single");
    assert!(matches!(step, brink_runtime::Step::Line(_)), "{step:?}");

    let snap = story.debug_snapshot();
    let position = snap.position.expect("active story has a position");

    let bytecode = program.container_bytecode(position.container_idx);
    assert!(
        position.offset <= bytecode.len(),
        "offset must be within the container's bytecode"
    );

    // Walk decode boundaries from 0 until we reach or pass the reported
    // offset (bounded by the container length itself).
    let mut off = 0usize;
    let mut hit_boundary = off == position.offset;
    while off < bytecode.len() && off < position.offset {
        let decoded = Opcode::decode(bytecode, &mut off);
        assert!(
            decoded.is_ok(),
            "bytecode must decode cleanly up to the reported offset, got {decoded:?} at {off}"
        );
        if off == position.offset {
            hit_boundary = true;
            break;
        }
    }
    assert!(
        hit_boundary,
        "reported offset {} is not a decode boundary in container {}",
        position.offset, position.container_idx
    );
}

// ── 3. Frame-stack shape: nested knot -> function call ────────────────────

/// A knot calling into a function pushes a `Function` frame above the
/// caller's frame; the reported per-frame position for the function frame
/// matches `Program::resolve_address` for the function's own entry.
#[test]
fn ink_frame_stack_shape_for_knot_into_function() {
    let (program, tables) = compile_ink(
        "-> start\n\
         === start ===\n\
         Value: {double(21)}\n\
         -> DONE\n\
         === function double(x) ===\n\
         ~ return x * 2\n",
    );
    let fn_def_id = program
        .definition_id_for_path("double")
        .expect("double should resolve");
    let expected_fn_addr = program
        .resolve_address(fn_def_id)
        .expect("resolve_address(double)");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let found = step_until(&mut story, 500, |snap| {
        snap.call_stack.len() == 2 && snap.call_stack[0].kind == "function"
    });
    assert!(
        found,
        "expected to observe a 2-deep call stack with an innermost `function` frame"
    );

    let snap = story.debug_snapshot();
    assert_eq!(snap.call_stack.len(), 2);
    let inner = &snap.call_stack[0];
    let outer = &snap.call_stack[1];
    assert_eq!(inner.kind, "function");
    assert_eq!(outer.kind, "root");
    assert_eq!(outer.location.as_deref(), Some("start"));

    let inner_pos = inner.position.expect("function frame has a position");
    assert_eq!(
        (inner_pos.container_idx, inner_pos.offset),
        expected_fn_addr,
        "function frame's position must equal double's own resolve_address entry \
         (caught at the first step after the call, before any of its own opcodes run)"
    );

    // The snapshot-level `position` always mirrors the innermost frame.
    assert_eq!(snap.position, inner.position);
}

// ── 4. Frame-stack shape: tunnel ───────────────────────────────────────────

/// `-> side -> ... ->->` pushes a `Tunnel` frame while inside the tunnel
/// knot; `CallFrameType::Tunnel` is a distinct case from `Function` in the
/// frame model (`call_stack.rs`), and the debug snapshot must say so.
///
/// Stepped one opcode at a time (as for the thread test below) rather than
/// via `continue_single`: this fixture's whole turn (tunnel call, its one
/// line, and the `->->` return) completes within a single internal VM run
/// before any `Step` is handed back to the caller, so the tunnel frame is
/// never the *live* top-of-stack frame at a `continue_single` boundary —
/// only mid-opcode, which `step_once` can catch.
#[test]
fn ink_frame_stack_shape_for_tunnel() {
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
    let expected = program.resolve_address(def_id).expect("resolve_address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let found = step_until(&mut story, 500, |snap| {
        snap.call_stack.first().is_some_and(|f| f.kind == "tunnel")
    });
    assert!(found, "expected to observe a live `tunnel` frame");

    let snap = story.debug_snapshot();
    assert_eq!(snap.call_stack.len(), 2, "root + tunnel frame");
    let inner = &snap.call_stack[0];
    let outer = &snap.call_stack[1];
    assert_eq!(inner.kind, "tunnel");
    assert_eq!(outer.kind, "root");
    assert_eq!(inner.location.as_deref(), Some("side"));
    let pos = inner.position.expect("tunnel frame has a position");
    assert_eq!(
        pos.container_idx, expected.0,
        "tunnel frame's container must be side's own container"
    );
    assert!(
        pos.offset <= program.container_bytecode(pos.container_idx).len(),
        "offset should stay within side's bytecode bounds, got {pos:?}"
    );
}

// ── 5. Frame-stack shape: thread ───────────────────────────────────────────

/// `<- choices` forks a `Thread` frame — distinct from `Function`/`Tunnel`
/// (`CallFrameType::Thread`'s own doc: "when this frame exhausts, the
/// thread is done ... inherited frames below it are never unwound into").
/// The thread frame is transient (it pops before the story yields
/// `Step::Choices`), so this steps one opcode at a time to catch it live.
#[test]
fn ink_frame_stack_shape_for_thread() {
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
    let expected = program.resolve_address(def_id).expect("resolve_address");

    let program = Arc::new(program);
    let mut story = Story::<FastRng>::new(Arc::clone(&program), tables);

    let found = step_until(&mut story, 500, |snap| {
        snap.call_stack.first().is_some_and(|f| f.kind == "thread")
    });
    assert!(
        found,
        "expected to observe a live `thread` frame before it unwinds"
    );

    let snap = story.debug_snapshot();
    let thread_frame = &snap.call_stack[0];
    assert_eq!(thread_frame.kind, "thread");
    let pos = thread_frame.position.expect("thread frame has a position");
    assert_eq!(
        pos.container_idx, expected.0,
        "thread frame's container must be the threaded knot's own container"
    );
}
