//! Structural comparison of episodes.

use std::fmt;

use crate::episode::{Episode, StateWrite, StepOutcome, StepRecord};

/// Result of comparing two episodes.
#[derive(Debug)]
pub struct EpisodeDiff {
    /// Whether the episodes are structurally identical.
    pub matches: bool,
    /// Per-step comparison results.
    pub step_diffs: Vec<StepDiff>,
    /// Whether the overall outcome matches.
    pub outcome_matches: bool,
}

/// Comparison result for a single step.
#[derive(Debug)]
pub enum StepDiff {
    /// Steps are identical.
    Match,
    /// Text output differs.
    TextMismatch { expected: String, actual: String },
    /// Tags differ.
    TagsMismatch {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    /// Step outcome differs (Done vs Choices vs Ended).
    OutcomeMismatch {
        expected: StepOutcome,
        actual: StepOutcome,
    },
    /// State writes differ.
    WritesMismatch {
        expected: Vec<StateWrite>,
        actual: Vec<StateWrite>,
    },
    /// Expected had a step here but actual did not.
    MissingStep(StepRecord),
    /// Actual had an extra step not present in expected.
    ExtraStep(StepRecord),
}

impl fmt::Display for EpisodeDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.matches {
            return write!(f, "episodes match");
        }
        writeln!(f, "episode mismatch:")?;
        for (i, step_diff) in self.step_diffs.iter().enumerate() {
            match step_diff {
                StepDiff::Match => {}
                StepDiff::TextMismatch { expected, actual } => {
                    writeln!(f, "  step {i}: text differs")?;
                    writeln!(f, "    expected: {expected:?}")?;
                    writeln!(f, "    actual:   {actual:?}")?;
                }
                StepDiff::TagsMismatch { expected, actual } => {
                    writeln!(f, "  step {i}: tags differ")?;
                    writeln!(f, "    expected: {expected:?}")?;
                    writeln!(f, "    actual:   {actual:?}")?;
                }
                StepDiff::OutcomeMismatch { expected, actual } => {
                    writeln!(f, "  step {i}: outcome differs")?;
                    writeln!(f, "    expected: {expected:?}")?;
                    writeln!(f, "    actual:   {actual:?}")?;
                }
                StepDiff::WritesMismatch { expected, actual } => {
                    writeln!(f, "  step {i}: writes differ")?;
                    writeln!(f, "    expected: {expected:?}")?;
                    writeln!(f, "    actual:   {actual:?}")?;
                }
                StepDiff::MissingStep(step) => {
                    writeln!(f, "  step {i}: missing (expected text: {:?})", step.text)?;
                }
                StepDiff::ExtraStep(step) => {
                    writeln!(f, "  step {i}: extra (actual text: {:?})", step.text)?;
                }
            }
        }
        if !self.outcome_matches {
            writeln!(f, "  outcome differs")?;
        }
        Ok(())
    }
}

/// Compare two episodes structurally.
///
/// Assumes both episodes follow the same choice path (same `choice_path`
/// values). Compares text, tags, outcome, and writes per step.
pub fn diff(expected: &Episode, actual: &Episode) -> EpisodeDiff {
    let max_len = expected.steps.len().max(actual.steps.len());
    let mut step_diffs = Vec::with_capacity(max_len);
    let mut all_match = true;

    for i in 0..max_len {
        match (expected.steps.get(i), actual.steps.get(i)) {
            (Some(exp), Some(act)) => {
                let step_diff = compare_steps(exp, act);
                if !matches!(step_diff, StepDiff::Match) {
                    all_match = false;
                }
                step_diffs.push(step_diff);
            }
            (Some(exp), None) => {
                all_match = false;
                step_diffs.push(StepDiff::MissingStep(exp.clone()));
            }
            (None, Some(act)) => {
                all_match = false;
                step_diffs.push(StepDiff::ExtraStep(act.clone()));
            }
            (None, None) => break,
        }
    }

    let outcome_matches = outcome_eq(&expected.outcome, &actual.outcome);
    if !outcome_matches {
        all_match = false;
    }

    EpisodeDiff {
        matches: all_match,
        step_diffs,
        outcome_matches,
    }
}

fn compare_steps(expected: &StepRecord, actual: &StepRecord) -> StepDiff {
    if expected.text != actual.text {
        return StepDiff::TextMismatch {
            expected: expected.text.clone(),
            actual: actual.text.clone(),
        };
    }
    if expected.tags != actual.tags {
        return StepDiff::TagsMismatch {
            expected: expected.tags.clone(),
            actual: actual.tags.clone(),
        };
    }
    if !step_outcome_eq(&expected.outcome, &actual.outcome) {
        return StepDiff::OutcomeMismatch {
            expected: expected.outcome.clone(),
            actual: actual.outcome.clone(),
        };
    }
    // Writes comparison is best-effort: exact match on the Vec.
    if expected.writes.len() != actual.writes.len() {
        return StepDiff::WritesMismatch {
            expected: expected.writes.clone(),
            actual: actual.writes.clone(),
        };
    }
    StepDiff::Match
}

fn step_outcome_eq(a: &StepOutcome, b: &StepOutcome) -> bool {
    match (a, b) {
        (StepOutcome::Done, StepOutcome::Done) | (StepOutcome::Ended, StepOutcome::Ended) => true,
        (
            StepOutcome::Choices {
                presented: pa,
                selected: sa,
            },
            StepOutcome::Choices {
                presented: pb,
                selected: sb,
            },
        ) => {
            sa == sb
                && pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb.iter())
                    .all(|(a, b)| a.text == b.text && a.index == b.index)
        }
        _ => false,
    }
}

fn outcome_eq(a: &crate::episode::Outcome, b: &crate::episode::Outcome) -> bool {
    use crate::episode::Outcome;
    match (a, b) {
        (Outcome::Ended, Outcome::Ended)
        | (Outcome::Done, Outcome::Done)
        | (Outcome::InputsExhausted { .. }, Outcome::InputsExhausted { .. }) => true,
        (Outcome::StepLimit { limit: a }, Outcome::StepLimit { limit: b }) => a == b,
        (Outcome::Error(a), Outcome::Error(b)) => {
            // Normalize definition IDs ($XX_hex) so that compiler vs converter
            // hash differences don't cause spurious mismatches.
            normalize_def_ids(a) == normalize_def_ids(b)
        }
        _ => false,
    }
}

/// Replace definition IDs (`$XX_hexdigits`) with a placeholder so that
/// error messages compare equal regardless of hash differences.
fn normalize_def_ids(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$'
            && i + 4 < bytes.len()
            && bytes[i + 3] == b'_'
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            // Found `$XX_` prefix — skip the hex hash that follows.
            result.push_str("$XX_<id>");
            i += 4;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}
