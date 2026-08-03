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

#[cfg(test)]
mod tests {
    use super::push_terminal;
    use crate::episode::{StepOutcome, StepRecord};

    /// The common case: a `Step::Line` (recorded as an open
    /// `StepOutcome::Continue` step) precedes the terminal this same turn —
    /// `push_terminal` must stamp the terminal's classification onto that
    /// *same* record (preserving its text/tags) rather than pushing a
    /// second, empty one. A naive pass-through (this function's shape
    /// before #1684 — `steps.push(StepRecord::new(text, tags, outcome,
    /// writes))` with fused text/tags parameters that no longer exist)
    /// would instead leave two records — this is exactly the drift the
    /// oracle ratchet is sensitive to.
    #[test]
    fn stamps_onto_the_open_continue_step_from_this_turn() {
        let mut steps = vec![StepRecord::new(
            "Hello.\n".to_string(),
            vec!["tag".to_string()],
            StepOutcome::Continue,
            Vec::new(),
        )];

        push_terminal(&mut steps, StepOutcome::Done, Vec::new());

        assert_eq!(
            steps.len(),
            1,
            "must fold onto the existing step, not add one"
        );
        assert_eq!(
            steps[0].text, "Hello.\n",
            "the open step's text must survive the fold"
        );
        assert_eq!(steps[0].tags, vec!["tag".to_string()]);
        assert_eq!(steps[0].outcome, StepOutcome::Done);
    }

    /// A terminal arriving with nothing open this turn (either the very
    /// first event of the episode, or immediately after a previous turn's
    /// terminal already closed the last record) must synthesize a fresh
    /// empty step rather than overwrite the previous turn's classification.
    #[test]
    fn synthesizes_an_empty_step_when_nothing_precedes_it_in_the_turn() {
        let mut steps = vec![StepRecord::new(
            String::new(),
            Vec::new(),
            StepOutcome::Done,
            Vec::new(),
        )];

        push_terminal(&mut steps, StepOutcome::Ended, Vec::new());

        assert_eq!(
            steps.len(),
            2,
            "the prior turn's Done record must be left alone, not overwritten"
        );
        assert_eq!(steps[0].outcome, StepOutcome::Done, "prior turn untouched");
        assert_eq!(steps[1].text, "");
        assert!(steps[1].tags.is_empty());
        assert_eq!(steps[1].outcome, StepOutcome::Ended);
    }

    /// Same synthesize case, from a totally empty episode (no steps at
    /// all yet) — an immediate terminal with zero preceding content.
    #[test]
    fn synthesizes_an_empty_step_for_an_empty_episode() {
        let mut steps = Vec::new();

        push_terminal(&mut steps, StepOutcome::Ended, Vec::new());

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].text, "");
        assert_eq!(steps[0].outcome, StepOutcome::Ended);
    }

    /// Writes observed during the terminal transition (e.g. an
    /// `increment_turn_index` fired by the yield opcode) must be preserved
    /// alongside whatever writes the open step already recorded — the fold
    /// extends, it doesn't replace.
    #[test]
    fn extends_writes_rather_than_replacing_them() {
        use crate::episode::StateWrite;

        let mut steps = vec![StepRecord::new(
            "Hi.\n".to_string(),
            Vec::new(),
            StepOutcome::Continue,
            vec![StateWrite::SetRngSeed { new_seed: 1 }],
        )];

        push_terminal(
            &mut steps,
            StepOutcome::Done,
            vec![StateWrite::IncrementTurnIndex { new_value: 1 }],
        );

        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].writes,
            vec![
                StateWrite::SetRngSeed { new_seed: 1 },
                StateWrite::IncrementTurnIndex { new_value: 1 },
            ]
        );
    }
}
