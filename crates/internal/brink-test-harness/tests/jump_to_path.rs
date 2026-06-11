//! Integration tests for host-directed jumps
//! ([`Story::choose_path_string`] / [`FlowInstance::choose_path_string`]) —
//! the equivalent of ink's `Story.ChoosePathString(path)` with its default
//! `resetCallstack: true`.
//!
//! These compile small ink stories with the brink compiler, link them, and
//! drive a `Story`, exercising: knot and qualified-stitch targets, visit
//! counting (a jump counts exactly like a `-> path` divert), shared
//! variables surviving the jump, the unknown-path and parked-external
//! errors, jumping mid-flow (pending choices cleared, transcript kept),
//! save/load round-trips after a jump, re-entry after `-> END`, and
//! jumping into a tunnel target.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use brink_format::Value;
use brink_runtime::{
    ExternalFnHandler, ExternalResult, FastRng, Line, Program, RuntimeError, StepOutcome, Story,
};

type LineTables = Vec<Vec<brink_format::LineEntry>>;

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

/// Drive the story to its next terminal yield, concatenating all text.
fn run_to_yield(story: &mut Story<'_, FastRng>) -> String {
    let lines = story.continue_maximally().expect("continue");
    lines
        .iter()
        .map(|l| match l {
            Line::Text { text, .. }
            | Line::Done { text, .. }
            | Line::Choices { text, .. }
            | Line::End { text, .. } => text.as_str(),
        })
        .collect()
}

/// Handler that always defers (`Pending`) — simulates an async host binding.
struct PendingHandler;

impl ExternalFnHandler for PendingHandler {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Pending
    }
}

const HARBOR: &str = "-> intro\n\
     === intro ===\n\
     At the gate.\n\
     -> DONE\n\
     === harbor ===\n\
     Harbor, visit {harbor}.\n\
     -> DONE\n\
     = pier\n\
     Pier, visit {harbor.pier}.\n\
     -> DONE\n";

/// Jump to a knot, then to a qualified stitch; each `continue` runs from
/// the new location.
#[test]
fn jump_to_knot_and_stitch() {
    let (program, tables) = compile(HARBOR);
    let mut story = Story::<FastRng>::new(&program, tables);

    assert_eq!(run_to_yield(&mut story), "At the gate.\n");

    story.choose_path_string("harbor").expect("jump to knot");
    assert_eq!(run_to_yield(&mut story), "Harbor, visit 1.\n");

    story
        .choose_path_string("harbor.pier")
        .expect("jump to stitch");
    assert_eq!(run_to_yield(&mut story), "Pier, visit 1.\n");
}

/// The jump counts as a visit, with divert semantics: a story that reads
/// its own visit count observes 1, 2, 3 across repeated jumps — same
/// counts an in-story `-> harbor` would produce.
#[test]
fn jump_counts_as_visit() {
    let (program, tables) = compile(HARBOR);
    let mut story = Story::<FastRng>::new(&program, tables);
    assert_eq!(run_to_yield(&mut story), "At the gate.\n");

    for expected in 1..=3 {
        story.choose_path_string("harbor").expect("jump");
        assert_eq!(
            run_to_yield(&mut story),
            format!("Harbor, visit {expected}.\n"),
            "visit count after jump #{expected}"
        );
    }
}

/// Global variables live on the shared `Context` and survive the jump —
/// both author-set values from earlier content and host-set values.
#[test]
fn variables_survive_jump() {
    let (program, tables) = compile(
        "VAR coins = 0\n\
         ~ coins = 7\n\
         Start.\n\
         -> DONE\n\
         === vault ===\n\
         Coins: {coins}.\n\
         -> DONE\n",
    );
    let mut story = Story::<FastRng>::new(&program, tables);
    assert_eq!(run_to_yield(&mut story), "Start.\n");

    story.choose_path_string("vault").expect("jump");
    assert_eq!(run_to_yield(&mut story), "Coins: 7.\n");

    assert!(story.set_variable("coins", Value::Int(42)));
    story.choose_path_string("vault").expect("jump again");
    assert_eq!(run_to_yield(&mut story), "Coins: 42.\n");
}

/// An unknown path errors with [`RuntimeError::UnknownPath`], naming the
/// path, and leaves the story playable.
#[test]
fn unknown_path_errors_and_names_path() {
    let (program, tables) = compile(HARBOR);
    let mut story = Story::<FastRng>::new(&program, tables);

    let err = story
        .choose_path_string("no.such.place")
        .expect_err("unknown path must error");
    assert!(matches!(err, RuntimeError::UnknownPath(ref p) if p == "no.such.place"));
    assert!(err.to_string().contains("no.such.place"));

    // The failed jump must not have disturbed the story.
    assert_eq!(run_to_yield(&mut story), "At the gate.\n");
}

/// A flow parked on an unresolved external (async host binding) refuses to
/// jump: the pending host call must be resolved, not silently abandoned.
/// Resolving it unblocks the jump.
#[test]
fn jump_while_awaiting_external_errors() {
    let (program, tables) = compile(
        "EXTERNAL fetch()\n\
         ~ temp x = fetch()\n\
         Got {x}.\n\
         -> DONE\n\
         === harbor ===\n\
         Harbor.\n\
         -> DONE\n",
    );
    let mut story = Story::<FastRng>::new(&program, tables);

    let outcome = story.advance_with(&PendingHandler).expect("advance");
    assert!(matches!(outcome, StepOutcome::AwaitingExternal));
    assert_eq!(story.pending_external_name(), Some("fetch"));

    let err = story
        .choose_path_string("harbor")
        .expect_err("jump while parked must error");
    assert!(matches!(
        err,
        RuntimeError::JumpWhileAwaitingExternal { ref path, ref external }
            if path == "harbor" && external == "fetch"
    ));

    // Resolving the external clears the park; the jump now succeeds.
    story.resolve_external(Value::Int(5));
    story
        .choose_path_string("harbor")
        .expect("jump after resolve");
    assert_eq!(run_to_yield(&mut story), "Harbor.\n");
}

/// Jumping while parked on a choice point abandons the pending choices
/// (callstack reset, like C# `ResetCallstack`) but keeps the transcript:
/// the append-only history must not shrink.
#[test]
fn jump_mid_flow_clears_choices_keeps_transcript() {
    let (program, tables) = compile(
        "Crossroads.\n\
         + [left] Went left. -> DONE\n\
         + [right] Went right. -> DONE\n\
         === harbor ===\n\
         Harbor.\n\
         -> DONE\n",
    );
    let mut story = Story::<FastRng>::new(&program, tables);

    let lines = story.continue_maximally().expect("continue");
    assert!(
        matches!(lines.last(), Some(Line::Choices { choices, .. }) if choices.len() == 2),
        "expected 2 pending choices, got {lines:?}"
    );
    let transcript_before = story.transcript_len();

    story.choose_path_string("harbor").expect("jump");

    // Pending choices are gone: choosing must fail.
    let err = story.choose(0).expect_err("choices were abandoned");
    assert!(matches!(err, RuntimeError::NotWaitingForChoice));

    // Content runs from the target; the prior transcript is intact.
    assert_eq!(run_to_yield(&mut story), "Harbor.\n");
    assert!(
        story.transcript_len() >= transcript_before,
        "transcript must be append-only across a jump"
    );
}

/// Visit counts taken by a jump are durable state: they round-trip through
/// save/load, so a fresh story resumes the count where the old one left off.
#[test]
fn save_load_after_jump_preserves_visits() {
    let (program, tables) = compile(HARBOR);

    let save = {
        let mut story = Story::<FastRng>::new(&program, tables.clone());
        assert_eq!(run_to_yield(&mut story), "At the gate.\n");
        story.choose_path_string("harbor").expect("jump");
        assert_eq!(run_to_yield(&mut story), "Harbor, visit 1.\n");
        story.save_state()
    };

    let mut restored = Story::<FastRng>::new(&program, tables);
    let report = restored.load_state(&save);
    assert!(report.unknown_globals.is_empty(), "clean load");

    restored.choose_path_string("harbor").expect("jump");
    assert_eq!(run_to_yield(&mut restored), "Harbor, visit 2.\n");
}

/// A permanently ended story (`-> END`) can be re-entered by jumping —
/// matching C#, where `ChoosePathString` + `Continue` works after the end.
#[test]
fn jump_after_end_reenters_story() {
    let (program, tables) = compile(
        "Finale.\n\
         -> END\n\
         === epilogue ===\n\
         Epilogue.\n\
         -> DONE\n",
    );
    let mut story = Story::<FastRng>::new(&program, tables);
    assert_eq!(run_to_yield(&mut story), "Finale.\n");

    story
        .choose_path_string("epilogue")
        .expect("jump after END");
    assert_eq!(run_to_yield(&mut story), "Epilogue.\n");
}

/// Jumping into a tunnel target is allowed (matching C#, where
/// `ChoosePathString` happily enters a tunnel knot); the content plays,
/// and the `->->` return then has no tunnel frame because the jump reset
/// the callstack.
///
/// Divergence note: at that point the C# runtime raises a story error
/// ("Found tunnel onwards statement (->->), when expected end of flow" —
/// `Story.cs` `PopTunnel` handling, gated on `callStack.canPop`), whereas
/// brink's `TunnelReturn` is currently lenient for *all* frame-less
/// `->->`s — host jump or not — and completes the flow as `Done`. This
/// test pins the existing brink behavior; tightening `TunnelReturn` to
/// error like C# would be a VM-wide change outside the scope of a host
/// jump API.
#[test]
fn jump_into_tunnel_target_completes_on_frameless_return() {
    let (program, tables) = compile(
        "-> side ->\n\
         Back.\n\
         -> DONE\n\
         === side ===\n\
         Side content.\n\
         ->->\n",
    );
    let mut story = Story::<FastRng>::new(&program, tables);
    // Normal play: the tunnel works.
    assert_eq!(run_to_yield(&mut story), "Side content.\nBack.\n");

    // Host jump into the tunnel knot: content plays; the frameless `->->`
    // ends the flow (brink leniency — C# would error here).
    story.choose_path_string("side").expect("jump into tunnel");
    let lines = story.continue_maximally().expect("continue");
    assert!(
        matches!(lines.last(), Some(Line::Done { .. })),
        "frameless ->-> completes the flow, got {lines:?}"
    );
    assert_eq!(
        lines
            .iter()
            .map(|l| match l {
                Line::Text { text, .. }
                | Line::Done { text, .. }
                | Line::Choices { text, .. }
                | Line::End { text, .. } => text.as_str(),
            })
            .collect::<String>(),
        "Side content.\n"
    );
}
