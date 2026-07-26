//! Single-point terminal-step classification shared by the episode
//! builders (`explorer.rs`, `runner.rs`).
//!
//! Prerequisite refactor for the Step/OutputLine runtime-output redesign
//! (issue #1449). Today `Line::Done`/`Line::End` already carry the fused
//! trailing text for the turn — the runtime does the fold internally
//! (`flow_instance.rs`'s yield handling) before the harness ever sees a
//! `Line`. [`terminal_step`] is the single place both builders turn a
//! terminal `Line`'s payload into a [`StepRecord`]; when the redesign makes
//! terminals payload-free, this is the only place that needs to grow real
//! fold logic (stamp the terminal's outcome onto the previous
//! `StepRecord`, or synthesize an empty one if none precedes it in the
//! turn) — call sites in `explorer.rs`/`runner.rs` do not change.
//!
//! [`classify_done`] is the other half: the runtime defers the "ran out of
//! content" error (no explicit `-> DONE`) to the *next* `continue` call
//! (`RuntimeError::RanOutOfContent`, `flow_instance.rs`). Probing for it
//! once, here, is what both builders need to classify a `Done` terminal —
//! matching the C# oracle's single-`Continue()` termination model
//! (`Explorer.cs`'s `SetLastStepOutcome`).

use brink_runtime::{Story, StoryRng, WriteObserver};

use crate::episode::{Outcome, StateWrite, StepOutcome, StepRecord};

/// Build the [`StepRecord`] for a terminal `Line` (`Done`/`End`/`Suspended`).
///
/// See the module docs: this always takes the "new step" path today
/// because the runtime hands terminals their text already fused in.
pub(crate) fn terminal_step(
    text: String,
    tags: Vec<String>,
    outcome: StepOutcome,
    writes: Vec<StateWrite>,
) -> StepRecord {
    StepRecord::new(text, tags, outcome, writes)
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
