//! Characterization tests for the runtime's **terminal classification**
//! seam — the behavior a future yield-time classifier (issue #1520,
//! `docs/design/yield-time-terminal-classifier.md`) is proposed to move.
//!
//! No *runtime* test pinned this contract before: the deferred
//! `RanOutOfContent` fault (`story/flow_instance.rs`, the `StoryStatus::Done`
//! reset arm) had no direct test, so a refactor could have silently changed
//! *which* `continue_single` call faults — the exact axis the design turns
//! on — without a single runtime test going red. The oracle corpus does
//! pin this end-to-end (`tests/tier1/choices/knot-body-choice-with-stitches-following`
//! and `.../nested-choice-loose-end-in-knot`, authored for #1522 around this
//! exact fault and its trailing extra step) via insta snapshots, so the
//! workspace gate as a whole was not blind to it — only the runtime crate's
//! own test suite was.
//!
//! These tests assert **today's** behavior, not a desired one. If the
//! classifier lands and moves the fault to the yield, these tests must be
//! updated deliberately, with the oracle re-run; that is the point of
//! writing them down.

use brink_runtime::{DotNetRng, Line, RuntimeError, Story};

/// Compile ink source and link it into a runnable story.
#[expect(clippy::unwrap_used)]
fn story_from_source(src: &str) -> Story<DotNetRng> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    Story::new(std::sync::Arc::new(program), line_tables)
}

/// Drive until the first terminal line, returning it plus the text seen.
#[expect(clippy::unwrap_used, clippy::panic)]
fn drive_to_terminal(story: &mut Story<DotNetRng>) -> (Line, String) {
    let mut text = String::new();
    for _ in 0..64 {
        let line = story.continue_single().unwrap();
        text.push_str(line.text());
        if line.is_terminal() {
            return (line, text);
        }
    }
    panic!("no terminal line within the step budget");
}

/// A story that falls off the end of its content with no explicit
/// `-> DONE` still **delivers its trailing text** as a `Line::Done`, and
/// only faults with `RanOutOfContent` on the *next* `continue_single`.
///
/// This is the "one deferred call" seam: *why* the turn stopped (safe exit
/// vs. ran out of content) is knowable at the yield — the flow's
/// `did_safe_exit` flag is already false when `Line::Done` is handed out —
/// but the runtime only acts on it one call later. C# ink raises its
/// equivalent error inside the *same* `Continue()` (`Story.cs`'s
/// `ContinueInternal`, in the `!canContinue` branch) and never delivers
/// the line.
#[test]
fn ran_out_of_content_faults_on_the_call_after_the_done_line() {
    // The *root* weave gets an implicit final gather + `-> DONE`
    // (`lir::lower`'s root terminus, mirroring inklecate), so running out of
    // root content is not a fault. A knot whose own body runs out is.
    let mut story = story_from_source("-> k\n== k ==\nHello.\n");

    let (terminal, text) = drive_to_terminal(&mut story);
    assert!(
        matches!(terminal, Line::Done { .. }),
        "trailing text is delivered as Done before the fault, got {terminal:?}"
    );
    assert!(
        text.contains("Hello."),
        "the line's text is delivered: {text:?}"
    );

    // The classification only surfaces on the following call.
    assert!(
        matches!(story.continue_single(), Err(RuntimeError::RanOutOfContent)),
        "the deferred fault surfaces one continue later"
    );
}

/// The safe-exit counterpart: an explicit `-> DONE` does not arm the
/// deferred fault, so the following `continue_single` is not
/// `RanOutOfContent`. Together with the test above this pins both arms of
/// the `StoryStatus::Done` reset that the classifier would absorb.
#[test]
fn explicit_done_is_a_safe_exit_and_does_not_fault() {
    let mut story = story_from_source("Hello.\n-> DONE\n");

    let (terminal, _) = drive_to_terminal(&mut story);
    assert!(matches!(terminal, Line::Done { .. }));

    // A safe exit resets `Active` and steps the VM again rather than
    // faulting; with no content left, that yields an empty `Line::Done`,
    // not `RanOutOfContent`. Pin the concrete value today's runtime
    // returns, not just the negative "it isn't the fault" — a refactor
    // that changed this to some other non-fault outcome (e.g. a step
    // limit or a different line shape) must still turn this test red.
    let result = story.continue_single();
    assert!(
        matches!(
            result,
            Ok(Line::Done {
                ref text,
                ref tags
            }) if text.is_empty() && tags.is_empty()
        ),
        "a safe exit must not raise the deferred fault, got {result:?}"
    );
}

/// An explicit `-> END` classifies at the yield with no deferral at all:
/// the terminal is `Line::End` and the next call reports `StoryEnded`.
/// This is the asymmetry the design writeup calls out — `End` is decided
/// eagerly, in its own arm, while `Done`'s fault half is decided lazily in
/// a different one.
#[test]
fn end_classifies_eagerly_unlike_done() {
    let mut story = story_from_source("Hello.\n-> END\n");

    let (terminal, _) = drive_to_terminal(&mut story);
    assert!(
        matches!(terminal, Line::End { .. }),
        "got {terminal:?}, expected End"
    );
    assert!(
        matches!(story.continue_single(), Err(RuntimeError::StoryEnded)),
        "an ended story reports StoryEnded, never RanOutOfContent"
    );
}
