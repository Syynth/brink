//! Bounded call-stack growth for `<- thread`-as-choice game loops
//! (issue #3561).
//!
//! The standard ink idiom — a gather that spawns one thread per option and
//! is looped back into from each choice body — used to retain one call
//! frame per turn, forever. `<-` pushed a `CallFrameType::Thread` boundary
//! frame; a choice raised inside the thread captured that frame in its
//! fork; selecting the choice installed the fork wholesale, so the boundary
//! rode into the main call stack and was never released. Depth grew with
//! the turn count, and since every subsequent fork copies the stack, the
//! per-turn cost grew with it: `benchmarks/stories/hanoi-10` reached depth
//! 2,501 over 5,000 turns, spending roughly half of its 17.7s playthrough
//! copying call frames.
//!
//! ink holds the same loops at a constant depth — it pushes no frame for a
//! thread divert at all (inklecate compiles `<- opt(1)` to a bare
//! `{"->": ...}`, and the target's parameters bind as temps in the frame
//! that is already current). Measured with `tools/inkjs-oracle`'s inkjs
//! 2.4.0 — the sanctioned stand-in for the C# runtime,
//! `docs/program-generator-spec.md` §6 — over 300 turns of each fixture
//! below, `callStack.elements.length` never leaves 1, 1 and 2 respectively,
//! which is what the third column of [`LOOP_FIXTURES`] pins.
//!
//! **Why this tier and not `tests/tier4-generated/`.** The delivered output
//! already matched the oracle before the fix, so there is no trace
//! divergence for a golden episode to hold: promoting these with
//! `--expected-mismatch` would fail, because the tier checks expectations
//! both ways and the cases genuinely match. What needs guarding is a
//! resource invariant, so this is the bounded-growth assertion `CLAUDE.md`'s
//! "Guard against unbounded growth" principle asks for, applied to the call
//! stack: play the same loop at two very different scales and require the
//! depth — and the net frame balance — to come out identical.
//!
//! [`a_looping_knots_own_temps_survive_a_thread_choice`] additionally pins
//! the observable half the boundary frame was corrupting, which is what
//! makes this more than a resource bug: a temp declared in the looping knot
//! was read through the retained `Thread` frame's slot space instead of its
//! own, so a counter ink walks up from 1 read `1` on every turn.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::sync::Arc;

use brink_runtime::{FastRng, Program, Step, Story};

type LineTables = Vec<Vec<brink_format::LineEntry>>;

/// The minimal repro from the issue: one gather, two threaded options,
/// each choice body looping back to the gather.
const SINGLE_THREAD: &str = r"-> gameloop

=== gameloop
- (top)
    <- opt(1)
    <- opt(2)
    -> DONE

= opt(n)
    +   [ Pick {n} ]
        You picked {n}.
    -> top
";

/// A thread spawned from inside another thread — the shape that leaked at
/// the same 2 frames/turn as the flat one.
const NESTED_THREADS: &str = r"-> gameloop

=== gameloop
- (top)
    <- outer(1)
    -> DONE

= outer(n)
    <- inner(n)
    -> DONE

= inner(m)
    +   [ Pick {m} ]
        You picked {m}.
    -> top
";

/// A thread spawned inside a tunnel, whose choice body returns through it
/// with `->->`. This one leaked 1 frame/turn rather than 2: `TunnelReturn`
/// used to strip trailing `Thread` frames lazily, which reclaimed some of
/// them and never kept up. That strip is gone — nothing pushes a frame for
/// it to strip — so the tunnel frame this fixture legitimately holds is the
/// whole of its (constant) depth 2.
const THREAD_IN_TUNNEL: &str = r"-> gameloop

=== gameloop
- (top)
    -> menu ->
    -> top

=== menu
    <- opt(1)
    -> DONE

=== opt(n)
    +   [ Pick {n} ]
        You picked {n}.
    ->->
";

/// `(name, source, the depth ink holds this loop at, the steady-state
/// turn's text)`.
const LOOP_FIXTURES: &[(&str, &str, usize, &str)] = &[
    ("single thread", SINGLE_THREAD, 1, "You picked 1.\n"),
    ("nested threads", NESTED_THREADS, 1, "You picked 1.\n"),
    ("thread in tunnel", THREAD_IN_TUNNEL, 2, "You picked 1.\n"),
];

/// Compile an inline ink source and link it into a `Program` + line tables.
fn compile(src: &str) -> (Program, LineTables) {
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
    brink_runtime::link(&out.data).expect("link")
}

/// What one playthrough leaves behind.
struct Run {
    /// Call-stack depth of the current thread when the loop stopped.
    depth: usize,
    /// `frames_pushed - frames_popped`: frames still held at the end.
    net_frames: i64,
    /// Text delivered by the final turn — the loop's steady state.
    last_turn_text: String,
}

/// Play `turns` turns of `src`, always taking choice 0.
fn play(name: &str, src: &str, turns: usize) -> Run {
    let (program, line_tables) = compile(src);
    let mut story = Story::<FastRng>::new(Arc::new(program), line_tables);
    let mut last_turn_text = String::new();

    for turn in 0..turns {
        let steps = story.continue_maximally();
        assert!(steps.is_ok(), "{name}: turn {turn} failed: {steps:?}");
        let steps = steps.expect("just asserted above");
        last_turn_text = steps.iter().map(Step::text).collect();
        assert!(
            matches!(steps.last(), Some(Step::Choices(_))),
            "{name}: turn {turn} ended without choices ({:?}) — the loop \
             fixture must keep offering them for the whole run",
            steps.last()
        );
        let chosen = story.choose(0);
        assert!(
            chosen.is_ok(),
            "{name}: turn {turn} choose failed: {chosen:?}"
        );
    }

    let stats = story.stats();
    Run {
        depth: story.debug_call_stack_depth(),
        net_frames: i64::try_from(stats.frames_pushed).expect("frame count fits in i64")
            - i64::try_from(stats.frames_popped).expect("frame count fits in i64"),
        last_turn_text,
    }
}

/// The regression itself: depth and retained-frame count must come out the
/// same after a short run and after an eight-times-longer one. Before the
/// fix both grew by one frame per turn (two for the flat and nested shapes,
/// one for the tunnel shape), so every one of these equalities failed.
#[test]
fn a_thread_as_choice_loop_holds_a_constant_call_stack() {
    for &(name, src, ink_depth, steady_text) in LOOP_FIXTURES {
        let short = play(name, src, 8);
        let long = play(name, src, 64);

        assert_eq!(
            short.depth, long.depth,
            "{name}: call-stack depth grew with the turn count \
             ({} after 8 turns, {} after 64)",
            short.depth, long.depth
        );
        assert_eq!(
            short.depth, ink_depth,
            "{name}: expected the depth ink holds this loop at"
        );
        assert_eq!(
            short.net_frames, long.net_frames,
            "{name}: frames were retained per turn ({} after 8 turns, \
             {} after 64) — every frame a turn pushes has to be released \
             again by the end of it",
            short.net_frames, long.net_frames
        );
        assert_eq!(
            short.last_turn_text, steady_text,
            "{name}: unexpected steady-state output"
        );
        assert_eq!(
            long.last_turn_text, steady_text,
            "{name}: steady-state output changed over a longer run"
        );
    }
}

/// `<-` must not shadow the looping knot's own temps.
///
/// `count` lives in `gameloop`'s frame. The retained `Thread` frame sat on
/// top of it with its own slot space, so from the first choice onwards
/// `~ count = count + 1` read and wrote the thread's slot instead: brink
/// printed "Count is 1." on every turn where ink counts 1, 2, 3, … . The
/// expected transcript below is inkjs 2.4.0's, verbatim.
#[test]
fn a_looping_knots_own_temps_survive_a_thread_choice() {
    const SRC: &str = r"-> gameloop

=== gameloop
~ temp count = 0
- (top)
    ~ count = count + 1
    Count is {count}.
    <- opt(7)
    -> DONE

= opt(n)
    +   [ Pick {n} ]
        You picked {n}.
    -> top
";

    let (program, line_tables) = compile(SRC);
    let mut story = Story::<FastRng>::new(Arc::new(program), line_tables);
    let mut transcript = Vec::new();
    for turn in 0..4 {
        let steps = story.continue_maximally();
        assert!(steps.is_ok(), "turn {turn} failed: {steps:?}");
        let steps = steps.expect("just asserted above");
        transcript.push(steps.iter().map(Step::text).collect::<String>());
        let chosen = story.choose(0);
        assert!(chosen.is_ok(), "turn {turn} choose failed: {chosen:?}");
    }

    assert_eq!(
        transcript,
        [
            "Count is 1.\n",
            "You picked 7.\nCount is 2.\n",
            "You picked 7.\nCount is 3.\n",
            "You picked 7.\nCount is 4.\n",
        ]
    );
}
