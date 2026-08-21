//! Characterization tests for the runtime's **terminal classification**
//! seam: the deferred `RanOutOfContent` fault (`story/flow_instance.rs`,
//! the `StoryStatus::Done` reset arm) that delivers the final line as its
//! own `Done`-terminal step and only faults on the *next* `continue_single`
//! call, rather than raising on the same call that discovers the end (C#'s
//! `Story.cs` `!canContinue` branch, which raises inside the same
//! `Continue()` and suppresses the trailing text).
//!
//! **RULED 2026-08-01, issue #1574 (maintainer comment 5154454373):** this
//! divergence is INTENTIONAL and PERMANENT. "Brink continues to deliver
//! the `Done` line and fault on the *next* call; it does not adopt C#'s
//! raise-on-discovery + suppress-trailing-text behavior." `Story::
//! did_safe_exit()` remains how a caller distinguishes a real `-> DONE`
//! from running out of content. These tests pin that **ruled** behavior,
//! not a provisional one held open pending a future decision — see
//! `docs/runtime-spec.md`'s "`RanOutOfContent` divergence from C# (RULED)"
//! subsection for the durable spec home, and
//! `docs/design/yield-time-terminal-classifier.md` for the full R1/R2
//! writeup this ruling closed.
//!
//! No *runtime* test pinned this contract before #1574: the deferred fault
//! had no direct test, so a refactor could have silently changed *which*
//! `continue_single` call faults without a single runtime test going red.
//! The oracle corpus does pin this end-to-end
//! (`tests/tier1/choices/knot-body-choice-with-stitches-following` and
//! `.../nested-choice-loose-end-in-knot`, authored for #1522 around this
//! exact fault and its trailing extra step) via insta snapshots, so the
//! workspace gate as a whole was not blind to it — only the runtime crate's
//! own test suite was.
//!
//! Issue #1520's proposed yield-time classifier folded into the `Step`
//! migration (#1684, the R1 half of this same ruling round) — that moves
//! *where* the classification construction lives (the design doc's
//! six-site inventory collapsing into `Step`'s own variants), not the
//! fault's timing, which R2 above ruled permanent. Whatever shape #1684
//! lands, these tests must still see the same observable sequence: a
//! `Done` line, then `RanOutOfContent` exactly one `continue_single` call
//! later.

use brink_format::Value;
use brink_runtime::{
    DotNetRng, ExternalFnHandler, ExternalResult, RanOutOfContentCause, RuntimeError, Step, Story,
};

/// Compile ink source and link it into a runnable story.
#[expect(clippy::unwrap_used)]
fn story_from_source(src: &str) -> Story<DotNetRng> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    Story::new(std::sync::Arc::new(program), line_tables)
}

/// Drive until the first terminal step, returning it plus the text seen.
#[expect(clippy::unwrap_used, clippy::panic)]
fn drive_to_terminal(story: &mut Story<DotNetRng>) -> (Step, String) {
    let mut text = String::new();
    for _ in 0..64 {
        let step = story.continue_single().unwrap();
        text.push_str(step.text());
        if step.is_terminal() {
            return (step, text);
        }
    }
    panic!("no terminal step within the step budget");
}

/// A story that falls off the end of its content with no explicit
/// `-> DONE` still **delivers its trailing text** first, as its own
/// `Step::Line` (terminals carry no payload — §7), and only faults with
/// `RanOutOfContent` on the *next* `continue_single`.
///
/// This is the "one deferred call" seam: *why* the turn stopped (safe exit
/// vs. ran out of content) is knowable at the yield — the flow's
/// `did_safe_exit` flag is already false when `Step::Done` is handed out —
/// but the runtime only acts on it one call later. C# ink raises its
/// equivalent error inside the *same* `Continue()` (`Story.cs`'s
/// `ContinueInternal`, in the `!canContinue` branch) and never delivers
/// the line. This divergence is RULED PERMANENT (#1574, 2026-08-01) — see
/// the module docs above.
#[test]
fn ran_out_of_content_faults_on_the_call_after_the_done_line() {
    // The *root* weave gets an implicit final gather + `-> DONE`
    // (`lir::lower`'s root terminus, mirroring inklecate), so running out of
    // root content is not a fault. A knot whose own body runs out is.
    let mut story = story_from_source("-> k\n== k ==\nHello.\n");

    let (terminal, text) = drive_to_terminal(&mut story);
    assert!(
        matches!(terminal, Step::Done),
        "the fault-arming Done terminal follows the trailing text, got {terminal:?}"
    );
    assert!(
        text.contains("Hello."),
        "the line's text is delivered: {text:?}"
    );

    // Issue #1573: the classification is *also* readable directly at the
    // yield, via the production `did_safe_exit` accessor — no deferred call
    // required.
    assert!(
        !story.did_safe_exit(),
        "did_safe_exit must be false right after the ran-out-of-content Done line"
    );

    // The classification only surfaces on the following call. `k`'s body
    // is reached by a plain `-> k` divert (no tunnel/function push), so the
    // call stack is just the root frame at fault time — the "plain" cause,
    // matching C#'s `!callStack.canPop` branch (issue #1993).
    assert!(
        matches!(
            story.continue_single(),
            Err(RuntimeError::RanOutOfContent(RanOutOfContentCause::Plain))
        ),
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
    assert!(matches!(terminal, Step::Done));

    // Issue #1573: `did_safe_exit` is readable on the production API and
    // distinguishes this case from the ran-out-of-content one above without
    // an extra `continue_single` call.
    assert!(
        story.did_safe_exit(),
        "did_safe_exit must be true right after an explicit -> DONE"
    );

    // A safe exit resets `Active` and steps the VM again rather than
    // faulting; with no content left, that yields a bare `Step::Done` (no
    // text to flush first), not `RanOutOfContent`. Pin the concrete value
    // today's runtime returns, not just the negative "it isn't the fault" —
    // a refactor that changed this to some other non-fault outcome (e.g. a
    // step limit or a different step shape) must still turn this test red.
    let result = story.continue_single();
    assert!(
        matches!(result, Ok(Step::Done)),
        "a safe exit must not raise the deferred fault, got {result:?}"
    );
}

/// An explicit `-> END` classifies at the yield with no deferral at all:
/// the terminal is `Step::End` and the next call reports `StoryEnded`.
/// This is the asymmetry the design writeup calls out — `End` is decided
/// eagerly, in its own arm, while `Done`'s fault half is decided lazily in
/// a different one.
#[test]
fn end_classifies_eagerly_unlike_done() {
    let mut story = story_from_source("Hello.\n-> END\n");

    let (terminal, _) = drive_to_terminal(&mut story);
    assert!(
        matches!(terminal, Step::End),
        "got {terminal:?}, expected End"
    );
    assert!(
        matches!(story.continue_single(), Err(RuntimeError::StoryEnded)),
        "an ended story reports StoryEnded, never RanOutOfContent"
    );
}

/// Characterizes a gap discovered while implementing #1993 (four
/// call-stack-keyed `RanOutOfContent` causes): a tunnel (`->t->`) that runs
/// out of content with **no** `->->` should classify as
/// [`RanOutOfContentCause::Tunnel`] per C# (`Story.cs`'s
/// `callStack.CanPop(PushPopType.Tunnel)` arm) — but today it classifies as
/// [`RanOutOfContentCause::Plain`] instead, because this runtime's
/// `vm::handle_frame_exhaustion` unconditionally pops an exhausted Tunnel
/// frame when no choices are pending (silently treating it like an
/// implicit function return and resuming the caller), where C# never
/// auto-pops a Tunnel frame — only a Function. By the time the deferred
/// fault fires, the tunnel frame (and then the caller's own frame) are
/// already gone, so the classification captured at the *last* exhaustion
/// event is the outermost one: Plain.
///
/// This pins **today's** (C#-divergent) behavior, not the desired one — see
/// the module docs' philosophy. If a future fix makes tunnels match C#'s
/// no-auto-pop semantics, this test must flip to `Tunnel` deliberately, with
/// the oracle corpus re-run (`docs/decision-log.md`-worthy: it changes VM
/// frame-popping, not just message text). Tracked in #2005.
#[test]
fn tunnel_fall_off_classifies_as_plain_not_tunnel_today() {
    let mut story =
        story_from_source("-> main\n=== main ===\n-> tunnel ->\n\n=== tunnel ===\nHello.\n");

    let (terminal, text) = drive_to_terminal(&mut story);
    assert!(matches!(terminal, Step::Done));
    assert!(text.contains("Hello."));
    assert!(!story.did_safe_exit());

    assert!(
        matches!(
            story.continue_single(),
            Err(RuntimeError::RanOutOfContent(RanOutOfContentCause::Plain))
        ),
        "tunnel fall-off is misclassified as Plain today (see #2005) — \
         a future fix to Tunnel auto-pop semantics must flip this to Tunnel deliberately"
    );
}

/// The function counterpart of the test above: a function (`f()`) that
/// runs out of content with no `~ return` should classify as
/// [`RanOutOfContentCause::Function`] per C# — but the classification is
/// only ever transient here too. Unlike the tunnel case, brink's
/// auto-pop-on-exhaustion for Function frames **does** match C#'s
/// (`callStack.CanPop(PushPopType.Function)`) — but since there is always a
/// `Root` frame beneath it, the auto-pop always succeeds and execution
/// cascades down to the `Root` frame's own exhaustion, which reclassifies
/// as Plain before the deferred fault ever reads it. `Function` is
/// reachable in principle (e.g. if a future change lets some frame type
/// block the cascade partway up without itself being poppable), but not
/// through any call-stack shape this runtime can currently produce.
/// Tracked in #2005 (filed for the Tunnel case; revisit this test's framing
/// once that lands, in case it also makes `Function` reachable).
#[test]
fn function_fall_off_classifies_as_plain_not_function_today() {
    let mut story =
        story_from_source("-> main\n=== main ===\n~ temp x = f()\n\n=== function f ===\nHello.\n");

    let (terminal, text) = drive_to_terminal(&mut story);
    assert!(matches!(terminal, Step::Done));
    assert!(text.contains("Hello."));
    assert!(!story.did_safe_exit());

    assert!(
        matches!(
            story.continue_single(),
            Err(RuntimeError::RanOutOfContent(RanOutOfContentCause::Plain))
        ),
        "function fall-off cascades down to the root frame's own Plain \
         classification today (see #2005)"
    );
}

/// A no-op [`ExternalFnHandler`] — the regression below calls no externals,
/// it only needs a handler to satisfy [`Story::call_function`]'s signature.
struct NoExternals;
impl ExternalFnHandler for NoExternals {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Fallback
    }
}

/// Regression for the review finding on #1993's `handle_frame_exhaustion`
/// change: the captured cause must not be overwritten by a transient
/// exhaustion that happens *after* the real fall-off, on a call the
/// deferred fault never reads.
///
/// Sequence: the main flow falls off the end (`Plain`, exactly like
/// [`ran_out_of_content_faults_on_the_call_after_the_done_line`]) and
/// delivers its trailing text first, then a bare `Step::Done`. Before the caller ever asks
/// for the *next* line, the engine drives an out-of-band
/// [`Story::call_function`] evaluation on the *same* flow: `f` calls a void
/// helper `g` (no `~ return`, so `g`'s frame exhausts naturally through
/// `handle_frame_exhaustion` with `frame_type: Function`), then `f` itself
/// returns explicitly via `~ return` (which goes through `pop_call_frame`
/// directly, never through `handle_frame_exhaustion`). `g`'s exhaustion
/// must not clobber the `Plain` cause the earlier fall-off recorded: the
/// *next* `continue_single` after the eval still has to report the
/// original `Plain` reason, not `Function`.
#[test]
fn call_function_void_helper_exhaustion_does_not_clobber_the_pending_plain_cause() {
    let mut story = story_from_source(
        "-> k\n\
         == k ==\n\
         Hello.\n\
         \n\
         === function f ===\n\
         ~ g()\n\
         ~ return true\n\
         \n\
         === function g ===\n\
         World.\n",
    );

    let (terminal, text) = drive_to_terminal(&mut story);
    assert!(matches!(terminal, Step::Done));
    assert!(text.contains("Hello."));
    assert!(
        !story.did_safe_exit(),
        "k's body falls off the end with no -> DONE"
    );

    // Out-of-band eval on the same flow: `g`'s implicit-return exhaustion
    // must not touch `ran_out_of_content_cause`, since this call doesn't
    // yield a `Done` of its own.
    let ret = story
        .call_function("f", &[], &NoExternals)
        .expect("f() should evaluate cleanly with no pending external");
    assert_eq!(ret, Value::Bool(true));

    assert!(
        matches!(
            story.continue_single(),
            Err(RuntimeError::RanOutOfContent(RanOutOfContentCause::Plain))
        ),
        "g's void-helper exhaustion during call_function must not clobber \
         the Plain cause the earlier fall-off recorded"
    );
}
