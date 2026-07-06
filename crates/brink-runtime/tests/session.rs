//! Integration tests for [`StorySession`]: the journaling, replayable session
//! wrapper (#370 + #371 snapshots).
//!
//! Programs are built from the known-good `.ink.json` converter reference
//! pipeline. Every stepping loop is bounded — VM tests must not hang.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::default_trait_access,
    clippy::items_after_statements,
    reason = "test harness"
)]

use std::path::Path;

use brink_converter::convert;
use brink_format::{ListValue, SaveState, Value};
use brink_json::InkJson;
use brink_runtime::{
    DotNetRng, EventKind, ExternalFnHandler, ExternalReplayMode, ExternalResult, FailReason, Line,
    Program, ReplayOutcome, ReplayWarning, SESSION_JOURNAL_CAP, SessionError, SessionJournal,
    StepOutcome, Story, StorySession, diff,
};

/// Link a program from a tier `.ink.json` fixture path (relative to the repo).
fn link_fixture(rel: &str) -> (Program, Vec<Vec<brink_format::LineEntry>>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let ink: InkJson = serde_json::from_str(&json).unwrap();
    let data = convert(&ink).unwrap();
    brink_runtime::link(&data).unwrap()
}

const CHOICE_STORY: &str = "tests/tier1/choices/I084-sticky-choices-stay-sticky/story.ink.json";
const LIST_STORY: &str = "tests/tier2/lists/I067-list-save-load/story.ink.json";
const EXTERNAL_STORY: &str = "tests/tier3/runtime/external-function-1-arg-v1/story.ink.json";
const FUNCTION_STORY: &str = "tests/tier2/function/func-none/story.ink.json";

/// Bounded run of a session to its first choice set (or terminal). Returns the
/// collected text.
fn run_to_pause(session: &mut StorySession<'_, DotNetRng>) -> Vec<Line> {
    session.continue_to_pause().unwrap()
}

// ── Journal round-trip (serde, tagged values incl. a List) ───────────────────

#[test]
fn journal_roundtrips_with_tagged_list_value() {
    let mut journal = SessionJournal::new(0xDEAD_BEEF, Some(42));
    let list = Value::List(
        ListValue {
            items: vec![brink_format::DefinitionId::new(
                brink_format::DefinitionTag::ListItem,
                7,
            )],
            origins: vec![],
        }
        .into(),
    );
    journal
        .events
        .push(brink_runtime::JournalEvent::new(EventKind::External {
            name: "get_flag".to_owned(),
            args: vec![Value::Int(3)],
            result: list.clone(),
        }));
    journal
        .events
        .push(brink_runtime::JournalEvent::new(EventKind::Choice {
            index: 1,
            label: Some("Choose me".to_owned()),
        }));

    let json = serde_json::to_string(&journal).unwrap();
    // The List variant must survive as a tagged value (not null).
    assert!(
        json.contains("List"),
        "list value must serialize tagged: {json}"
    );
    let back: SessionJournal = serde_json::from_str(&json).unwrap();
    assert_eq!(back, journal);
    // The list result round-trips exactly (no lossy null-ing).
    if let EventKind::External { result, .. } = &back.events[0].kind {
        assert_eq!(result, &list);
    } else {
        panic!("expected External event");
    }
}

// ── Record → replay identical run ────────────────────────────────────────────

#[test]
fn record_then_replay_identical_run() {
    let (program, tables) = link_fixture(CHOICE_STORY);

    // Record: play, choose 0, play again.
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables.clone()), None);
    let _ = run_to_pause(&mut session);
    session.choose(0).unwrap();
    let _ = run_to_pause(&mut session);
    session.choose(2).unwrap(); // "Finish" -> END
    let _ = run_to_pause(&mut session); // drive to END
    let after_record = session.snapshot();
    let journal = session.journal().clone();

    // Replay against a fresh story.
    let (replayed, outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables),
        &journal,
        ExternalReplayMode::Recorded,
        None,
    );
    assert!(
        matches!(outcome, ReplayOutcome::Replayed { ref warnings } if warnings.is_empty()),
        "expected clean replay, got {outcome:?}",
    );
    // Replayed end state matches the recorded run's.
    assert!(diff(&after_record, &replayed.snapshot()).is_empty());
}

// ── Divergence after an edit (choice removed) → Diverged + truncation ────────

#[test]
fn divergence_choice_out_of_range_truncates_and_parks() {
    let (program, tables) = link_fixture(CHOICE_STORY);

    // Record a run that selects choice index 1.
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables.clone()), None);
    let _ = run_to_pause(&mut session);
    session.choose(1).unwrap();
    let mut journal = session.journal().clone();

    // Simulate an edit that removed a choice: rewrite the recorded Choice to an
    // out-of-range index so replay cannot apply it.
    for ev in &mut journal.events {
        if let EventKind::Choice { index, .. } = &mut ev.kind {
            *index = 99;
        }
    }

    let (replayed, outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables),
        &journal,
        ExternalReplayMode::Recorded,
        None,
    );
    match outcome {
        ReplayOutcome::Diverged {
            at_event, found, ..
        } => {
            assert!(matches!(
                found,
                brink_runtime::DivergenceFound::ChoiceIndexOutOfRange { index: 99, .. }
            ));
            // Journal truncated at divergence; parked at reached position.
            assert!(replayed.journal().truncated);
            assert!(replayed.journal().len() <= at_event + 1);
        }
        other => panic!("expected Diverged, got {other:?}"),
    }
}

// ── Recorded vs live external modes ──────────────────────────────────────────

/// Returns a fixed int for `externalFunction`, counting invocations so a test
/// can prove "recorded" replay does NOT re-invoke.
struct CountingExternal {
    value: i32,
    calls: std::cell::Cell<u32>,
}

impl ExternalFnHandler for CountingExternal {
    fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
        if name == "externalFunction" {
            self.calls.set(self.calls.get() + 1);
            ExternalResult::Resolved(Value::Int(self.value))
        } else {
            ExternalResult::Fallback
        }
    }
}

#[test]
fn recorded_replay_does_not_reinvoke_live_does() {
    let (program, tables) = link_fixture(EXTERNAL_STORY);

    // Record: play through, resolving the external live.
    let record_handler = CountingExternal {
        value: 77,
        calls: std::cell::Cell::new(0),
    };
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables.clone()), None);
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(steps < 1000, "step budget");
        match session.advance_with(&record_handler).unwrap() {
            StepOutcome::Line(l) if l.is_terminal() => break,
            StepOutcome::Line(_) => {}
            StepOutcome::AwaitingExternal => panic!("handler resolves inline"),
        }
    }
    assert_eq!(
        record_handler.calls.get(),
        1,
        "recorded exactly one external"
    );
    let journal = session.journal().clone();
    // The external result was journaled.
    assert!(journal.events.iter().any(
        |e| matches!(&e.kind, EventKind::External { name, result, .. }
            if name == "externalFunction" && *result == Value::Int(77))
    ));

    // Replay Recorded: the handler must NOT be re-invoked (its count stays 0).
    let replay_handler = CountingExternal {
        value: 999,
        calls: std::cell::Cell::new(0),
    };
    let (_replayed, outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables.clone()),
        &journal,
        ExternalReplayMode::Recorded,
        Some(&replay_handler),
    );
    assert!(
        matches!(outcome, ReplayOutcome::Replayed { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        replay_handler.calls.get(),
        0,
        "recorded mode must not re-invoke externals",
    );

    // Replay Live: the handler IS re-invoked.
    let live_handler = CountingExternal {
        value: 123,
        calls: std::cell::Cell::new(0),
    };
    let (_replayed, outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables),
        &journal,
        ExternalReplayMode::Live,
        Some(&live_handler),
    );
    assert!(
        matches!(outcome, ReplayOutcome::Replayed { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        live_handler.calls.get(),
        1,
        "live mode re-invokes externals"
    );
}

// ── Label-drift warning ──────────────────────────────────────────────────────

#[test]
fn label_drift_is_a_soft_warning_on_replayed() {
    let (program, tables) = link_fixture(CHOICE_STORY);

    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables.clone()), None);
    let _ = run_to_pause(&mut session);
    session.choose(0).unwrap();
    let mut journal = session.journal().clone();

    // Rewrite the recorded label so it drifts from what the program presents at
    // the same index — a matching index with different text.
    for ev in &mut journal.events {
        if let EventKind::Choice { label, .. } = &mut ev.kind {
            *label = Some("STALE LABEL".to_owned());
        }
    }

    let (_replayed, outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables),
        &journal,
        ExternalReplayMode::Recorded,
        None,
    );
    match outcome {
        ReplayOutcome::Replayed { warnings } => {
            assert!(
                warnings.iter().any(|w| matches!(
                    w,
                    ReplayWarning::ChoiceLabelDrift { recorded, .. } if recorded == "STALE LABEL"
                )),
                "expected label-drift warning, got {warnings:?}",
            );
        }
        other => panic!("label drift must be a soft warning on Replayed, got {other:?}"),
    }
}

// ── callFunction journaled but externals isolated ────────────────────────────

#[test]
fn call_function_is_journaled_but_externals_are_isolated() {
    // func-none defines `function f()` returning 3.8.
    let (program, tables) = link_fixture(FUNCTION_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables), None);

    // Isolated handler: if its externals leaked into the journal we'd see them.
    struct Trap;
    impl ExternalFnHandler for Trap {
        fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
            ExternalResult::Fallback
        }
    }
    let ret = session.call_function("f", &[], &Trap).unwrap();
    assert_eq!(ret, Value::Float(3.8));

    // The journal has exactly one Call event and no External events from it.
    let calls = session
        .journal()
        .events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::Call { name, .. } if name == "f"))
        .count();
    assert_eq!(calls, 1, "call_function journals a Call event");
    let externals = session
        .journal()
        .events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::External { .. }))
        .count();
    assert_eq!(externals, 0, "call_function externals must not journal");
}

// ── Checkpoint fast-restore skips replay ─────────────────────────────────────

#[test]
fn checkpoint_fast_restore_skips_replay() {
    let (program, tables) = link_fixture(CHOICE_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables.clone()), None);
    let _ = run_to_pause(&mut session);
    session.choose(0).unwrap();
    let _ = run_to_pause(&mut session);
    // Export refreshes the checkpoint.
    let journal = session.export_journal();
    assert!(journal.checkpoint.is_some(), "export embeds a checkpoint");
    assert_eq!(journal.program_checksum, program.source_checksum());

    // Restore against the same program: checksum matches → fast path, no replay
    // warnings and no stepping needed.
    let (restored, outcome) =
        StorySession::<DotNetRng>::restore(Story::new(&program, tables), journal).unwrap();
    assert!(
        matches!(outcome, ReplayOutcome::Replayed { ref warnings } if warnings.is_empty()),
        "fast-restore returns clean Replayed, got {outcome:?}",
    );
    // Turn index survived the checkpoint restore.
    let snap = restored.snapshot();
    assert!(snap.turn_index >= 1);
}

#[test]
fn restore_without_checkpoint_on_mismatch_errors() {
    let (program, tables) = link_fixture(CHOICE_STORY);
    // Journal for a *different* program (bad checksum), no checkpoint.
    let journal = SessionJournal::new(program.source_checksum().wrapping_add(1), None);
    match StorySession::<DotNetRng>::restore(Story::new(&program, tables), journal) {
        Err(SessionError::ChecksumMismatch { .. }) => {}
        Err(other) => panic!("expected ChecksumMismatch, got {other:?}"),
        Ok(_) => panic!("expected ChecksumMismatch error"),
    }
}

// ── Cap sets truncated ───────────────────────────────────────────────────────

/// The cap is enforced by the session's internal `push`: journaling past the
/// cap sets `truncated` and drops. Exercised deterministically through the
/// replay re-journaling path with more `SetVar` events than the cap.
#[test]
fn journal_push_honors_cap() {
    let (program, tables) = link_fixture(EXTERNAL_STORY);
    let mut src = SessionJournal::new(program.source_checksum(), None);
    src.events
        .push(brink_runtime::JournalEvent::new(EventKind::Start {
            path: None,
            args: vec![],
        }));
    for i in 0..SESSION_JOURNAL_CAP + 5 {
        src.events
            .push(brink_runtime::JournalEvent::new(EventKind::SetVar {
                name: "x".to_owned(),
                value: Value::Int(i as i32),
            }));
    }
    let (replayed, _outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables),
        &src,
        ExternalReplayMode::Recorded,
        None,
    );
    // Re-journaling more than the cap must set truncated and stop growing.
    assert!(replayed.journal().truncated, "cap must set truncated");
    assert!(replayed.journal().len() <= SESSION_JOURNAL_CAP);
}

// ── Turn-boundary queue/reject behavior ──────────────────────────────────────

#[test]
fn mutation_mid_turn_is_rejected() {
    let (program, tables) = link_fixture(CHOICE_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables), None);
    // Advance exactly one step: the story is now Active (mid-turn), not paused.
    match session.advance().unwrap() {
        StepOutcome::Line(l) => assert!(!l.is_terminal(), "expected non-terminal first line"),
        StepOutcome::AwaitingExternal => panic!("no external here"),
    }
    // A mid-turn mutation is rejected.
    let err = session.set_var("whatever", Value::Int(1)).unwrap_err();
    assert!(
        matches!(err, SessionError::MutationMidTurn { op: "set_var" }),
        "{err:?}"
    );
    let err = session.go_to_path("test", &[]).unwrap_err();
    assert!(
        matches!(err, SessionError::MutationMidTurn { op: "go_to_path" }),
        "{err:?}"
    );
    let save = SaveState {
        version: brink_format::SAVE_FORMAT_VERSION,
        globals: Default::default(),
        visits: vec![],
        turns: vec![],
        turn_index: 0,
        rng_seed: 0,
        previous_random: 0,
    };
    let err = session.load_state(&save).unwrap_err();
    assert!(
        matches!(err, SessionError::MutationMidTurn { op: "load_state" }),
        "{err:?}"
    );
}

#[test]
fn mutation_at_boundary_is_allowed_and_journaled() {
    let (program, tables) = link_fixture(CHOICE_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables), None);
    // Drain to a choice pause — a turn boundary.
    let _ = run_to_pause(&mut session);
    // go_to_path at a boundary is allowed and journaled.
    session.go_to_path("test", &[]).unwrap();
    assert!(
        session
            .journal()
            .events
            .iter()
            .any(|e| matches!(&e.kind, EventKind::GoToPath { path, .. } if path == "test"))
    );
}

// ── Snapshot / diff on a real story (var change, list add/remove, turn) ──────

#[test]
fn snapshot_and_diff_track_list_membership_and_turns() {
    // The list story: `t = l1 + l2` then in `elsewhere`, `t += z`. Globals `t`.
    let (program, tables) = link_fixture(LIST_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables), None);

    // Snapshot at start (before any content).
    let before = session.snapshot();

    // Run to the first pause: this executes `~ t = l1 + l2` and prints it.
    let lines = run_to_pause(&mut session);
    assert!(!lines.is_empty());
    let after = session.snapshot();

    // `t` is a List global present in both, membership changed (added items).
    assert!(
        after.globals.contains_key("t"),
        "t global present: {:?}",
        after.globals.keys().collect::<Vec<_>>()
    );
    let d = diff(&before, &after);
    // Something changed between the two snapshots.
    assert!(
        !d.is_empty(),
        "snapshot diff should be non-empty after running"
    );

    // The list membership delta for `t` records added items (a, c, x from the
    // active-marked list items).
    if let Some(delta) = d.list_deltas.get("t") {
        assert!(
            !delta.added.is_empty(),
            "expected list items added to t: {delta:?}"
        );
    }
    // The snapshot exposes typed list membership.
    if let Some(list) = after.lists.get("t") {
        assert!(!list.items.is_empty(), "t has active list members");
        // Sorted for determinism.
        let mut sorted = list.items.clone();
        sorted.sort();
        assert_eq!(list.items, sorted);
    }
}

#[test]
fn diff_of_identical_snapshots_is_empty() {
    let (program, tables) = link_fixture(CHOICE_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables), None);
    let _ = run_to_pause(&mut session);
    let a = session.snapshot();
    let b = session.snapshot();
    assert!(diff(&a, &b).is_empty());
}

// ── Escape hatch reachability ────────────────────────────────────────────────

#[test]
fn escape_hatch_exposes_wrapped_story() {
    let (program, tables) = link_fixture(CHOICE_STORY);
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables), None);
    // Reads through the escape hatch never journal.
    let before = session.journal().len();
    let _pending = session.story().has_pending_external();
    let _ = session.story_mut().stats();
    assert_eq!(
        session.journal().len(),
        before,
        "escape-hatch reads never journal"
    );
}

// ── Live replay parks on deferred external, resumes via continue_replay ──────

/// A handler that defers (`Pending`) on its first call, then resolves.
struct DeferOnce {
    deferred: std::cell::Cell<bool>,
}

impl ExternalFnHandler for DeferOnce {
    fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
        if name == "externalFunction" && !self.deferred.get() {
            self.deferred.set(true);
            ExternalResult::Pending
        } else {
            ExternalResult::Resolved(Value::Int(0))
        }
    }
}

#[test]
fn live_replay_parks_on_pending_external() {
    let (program, tables) = link_fixture(EXTERNAL_STORY);
    // Record a normal run first.
    let rec = CountingExternal {
        value: 5,
        calls: std::cell::Cell::new(0),
    };
    let mut session = StorySession::<DotNetRng>::new(Story::new(&program, tables.clone()), None);
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(steps < 1000);
        match session.advance_with(&rec).unwrap() {
            StepOutcome::Line(l) if l.is_terminal() => break,
            StepOutcome::Line(_) => {}
            StepOutcome::AwaitingExternal => panic!(),
        }
    }
    let journal = session.journal().clone();

    // Live replay with a deferring handler parks as Failed::AwaitingExternal.
    let defer = DeferOnce {
        deferred: std::cell::Cell::new(false),
    };
    let (_replayed, outcome) = StorySession::<DotNetRng>::replay(
        Story::new(&program, tables),
        &journal,
        ExternalReplayMode::Live,
        Some(&defer),
    );
    assert!(
        matches!(
            outcome,
            ReplayOutcome::Failed {
                reason: FailReason::AwaitingExternal { .. },
                ..
            }
        ),
        "live replay must park on a deferred external, got {outcome:?}",
    );
}
