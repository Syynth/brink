//! Single-point terminal-step classification shared by the episode
//! builders (`explorer.rs`, `runner.rs`).
//!
//! The Step/OutputLine runtime-output redesign (issue #1684) made
//! terminals payload-free: `Step::Done`/`Step::Choices`/`Step::End` carry
//! no text — any trailing content the old fused `Line::Done`/`Line::Choices`/
//! `Line::End` used to bundle in now arrives *first*, as its own ordinary
//! `Step::Line`, and the runtime hands the harness the bare terminal on the
//! very next `continue_single_observed` call
//! (`FlowInstance::advance_with_limit`'s `pending_terminal` split). So the
//! text is already sitting in `steps.last()` (pushed as `StepOutcome::
//! Continue`) by the time a terminal event arrives — [`push_terminal`]'s
//! whole job is to fold the terminal's classification onto that record
//! rather than push a second, empty one. This is exactly the fold logic
//! this function was reserved for since PR #1513 landed it as a
//! byte-identical pass-through specifically to protect this attribution:
//! if the ratchet moves in either direction now, the fold is wrong — it
//! does not mean conformance changed.
//!
//! [`classify_done`] is the other half: the runtime defers the "ran out of
//! content" error (no explicit `-> DONE`) to the *next* `continue` call
//! (`RuntimeError::RanOutOfContent`, `flow_instance.rs`). Probing for it
//! once, here, is what both builders need to classify a `Done` terminal —
//! matching the oracle's deferred-error termination model (`Explorer.cs`'s
//! `TerminalError` path), where the C# runtime throws from `Continue()` on
//! the condition brink defers.
//!
//! Deleting that probe is the runtime-side ask of issue #1520; see
//! `docs/design/yield-time-terminal-classifier.md` for why it is blocked
//! on two maintainer rulings (where the classification surfaces, and
//! whether the fault moves to the same `continue`).

use brink_runtime::{Story, StoryRng, WriteObserver};

use crate::episode::{Outcome, StateWrite, StepOutcome, StepRecord};

/// Fold a terminal event (`Step::Done`/`Step::Choices`/`Step::End`/
/// `Step::Suspended` — all payload-free now) into `steps`.
///
/// If the last record already pushed this turn is still open
/// (`StepOutcome::Continue` — i.e. an ordinary content line the runtime
/// just emitted, with no terminal classification yet), the terminal
/// stamps onto it directly: its `outcome` becomes `outcome` and `writes`
/// extends its own. That is the common case — the runtime always emits
/// trailing content as its own `Step::Line` before a payload-bearing
/// terminal would have fused it in the old model.
///
/// If `steps` is empty, or its last record already closed a *previous*
/// turn (any non-`Continue` outcome), there is nothing from *this* turn to
/// stamp onto — a terminal arrived with zero preceding content — so an
/// empty step is synthesized to carry the classification instead of
/// corrupting the previous turn's record.
pub(crate) fn push_terminal(
    steps: &mut Vec<StepRecord>,
    outcome: StepOutcome,
    writes: Vec<StateWrite>,
) {
    match steps.last_mut() {
        Some(last) if matches!(last.outcome, StepOutcome::Continue) => {
            last.outcome = outcome;
            last.writes.extend(writes);
        }
        _ => steps.push(StepRecord::new(String::new(), Vec::new(), outcome, writes)),
    }
}

/// Classify a `Done` terminal into its episode [`Outcome`], probing the
/// runtime's deferred "ran out of content" check exactly once.
pub(crate) fn classify_done<R: StoryRng>(
    story: &mut Story<R>,
    observer: &mut dyn WriteObserver,
) -> Outcome {
    if story.did_safe_exit() {
        Outcome::Done
    } else {
        match story.continue_single_observed(observer) {
            Err(e) => Outcome::Error(e.to_string()),
            Ok(_) => Outcome::Done,
        }
    }
}
