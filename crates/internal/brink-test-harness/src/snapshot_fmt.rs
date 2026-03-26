//! Compact snapshot formatting for oracle corpus testing.
//!
//! Produces diffable text representations of brink episode output
//! for use with `insta` snapshot testing. Text is `{:?}`-formatted
//! so `\n` appears literally, making whitespace bugs visible in diffs.

use std::collections::HashMap;
use std::fmt::Write;

use crate::episode::{Episode, Outcome, StepOutcome};
use crate::oracle::{self, OracleDiff, OracleEpisode, OracleStepDiff};

/// Maximum number of episodes to show in full detail.
/// Cases with more episodes use a compact digest for passing episodes.
const DETAIL_THRESHOLD: usize = 50;

/// Format a single brink step as a compact line.
fn fmt_step(step: &crate::episode::StepRecord, out: &mut String) {
    let outcome = match &step.outcome {
        StepOutcome::Continue => "Continue".to_string(),
        StepOutcome::Done => "Done".to_string(),
        StepOutcome::Ended => "Ended".to_string(),
        StepOutcome::Choices {
            presented,
            selected,
        } => {
            let choices: Vec<String> = presented
                .iter()
                .map(|c| format!("{:?} ({})", c.text, c.index))
                .collect();
            format!("Choices: {} -> {selected}", choices.join(", "))
        }
    };
    let tags = if step.tags.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            step.tags
                .iter()
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    let _ = writeln!(out, "> {:?}{tags} [{outcome}]", step.text);
}

/// Format episode outcome.
fn fmt_outcome(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Ended => "Ended",
        Outcome::Done => "Done",
        Outcome::InputsExhausted { .. } => "InputsExhausted",
        Outcome::StepLimit { .. } => "StepLimit",
        Outcome::Error(_) => "Error",
    }
}

/// Format a full episode with all steps.
fn fmt_episode_full(ep: &Episode, diff: Option<&OracleDiff>, out: &mut String) {
    let status = diff.map_or("NO_ORACLE", |d| if d.matches { "PASS" } else { "FAIL" });
    let _ = writeln!(out, "=== episode {:?} === {status}", ep.choice_path);

    for step in &ep.steps {
        fmt_step(step, out);
    }
    let _ = writeln!(out, "outcome: {}", fmt_outcome(&ep.outcome));

    // For failing episodes, include the diff detail.
    if let Some(d) = diff
        && !d.matches
    {
        for (i, sd) in d.step_diffs.iter().enumerate() {
            match sd {
                OracleStepDiff::Match => {}
                OracleStepDiff::TextMismatch { expected, actual } => {
                    let _ = writeln!(out, "  step {i}: text differs");
                    let _ = writeln!(out, "    oracle: {expected:?}");
                    let _ = writeln!(out, "    brink:  {actual:?}");
                }
                OracleStepDiff::TagsMismatch { expected, actual } => {
                    let _ = writeln!(out, "  step {i}: tags differ");
                    let _ = writeln!(out, "    oracle: {expected}");
                    let _ = writeln!(out, "    brink:  {actual}");
                }
                OracleStepDiff::OutcomeMismatch { expected, actual } => {
                    let _ = writeln!(out, "  step {i}: outcome differs");
                    let _ = writeln!(out, "    oracle: {expected}");
                    let _ = writeln!(out, "    brink:  {actual}");
                }
                OracleStepDiff::MissingStep { expected_text } => {
                    let _ = writeln!(out, "  step {i}: missing (oracle: {expected_text:?})");
                }
                OracleStepDiff::ExtraStep { actual_text } => {
                    let _ = writeln!(out, "  step {i}: extra ({actual_text:?})");
                }
            }
        }
    }
}

/// Format a passing episode as a single digest line.
fn fmt_episode_digest(ep: &Episode, out: &mut String) {
    let _ = writeln!(
        out,
        "{:?}: {} steps, {}",
        ep.choice_path,
        ep.steps.len(),
        fmt_outcome(&ep.outcome),
    );
}

/// Result of processing a case for snapshot output.
pub struct CaseResult {
    /// Relative path of the case (e.g. "tier1/basics/I001-minimal-story").
    pub rel_path: String,
    /// Status for the summary line.
    pub status: CaseStatus,
}

/// Status of a single test case.
pub enum CaseStatus {
    Pass {
        episodes_pass: usize,
        episodes_total: usize,
    },
    Fail {
        episodes_pass: usize,
        episodes_total: usize,
        episodes_mismatch: usize,
        episodes_missing: usize,
    },
    CompileError(String),
    LinkError(String),
    Skip,
}

impl CaseResult {
    /// Format as a single summary line.
    pub fn summary_line(&self) -> String {
        match &self.status {
            CaseStatus::Pass {
                episodes_pass,
                episodes_total,
            } => format!(
                "{}: PASS ({episodes_pass}/{episodes_total} episodes)",
                self.rel_path
            ),
            CaseStatus::Fail {
                episodes_pass,
                episodes_total,
                episodes_mismatch,
                episodes_missing,
            } => {
                let mut parts = Vec::new();
                if *episodes_mismatch > 0 {
                    parts.push(format!("{episodes_mismatch} mismatch"));
                }
                if *episodes_missing > 0 {
                    parts.push(format!("{episodes_missing} missing"));
                }
                format!(
                    "{}: FAIL ({episodes_pass}/{episodes_total} episodes, {})",
                    self.rel_path,
                    parts.join(", "),
                )
            }
            CaseStatus::CompileError(e) => format!("{}: COMPILE_ERROR ({e})", self.rel_path),
            CaseStatus::LinkError(e) => format!("{}: LINK_ERROR ({e})", self.rel_path),
            CaseStatus::Skip => format!("{}: SKIP", self.rel_path),
        }
    }
}

/// Format the full per-case snapshot content.
///
/// `oracle_eps` and `brink_eps` are the oracle and brink episodes for this case.
/// Episodes are matched by `choice_path`.
pub fn format_case_snapshot(
    rel_path: &str,
    oracle_eps: &[OracleEpisode],
    brink_eps: &[Episode],
) -> String {
    let brink_index: HashMap<&[usize], &Episode> = brink_eps
        .iter()
        .map(|ep| (ep.choice_path.as_slice(), ep))
        .collect();

    // Compute diffs for each oracle episode.
    let mut matched: Vec<(&Episode, OracleDiff)> = Vec::new();
    let mut missing_paths: Vec<&[usize]> = Vec::new();

    for oracle_ep in oracle_eps {
        if let Some(brink_ep) = brink_index.get(oracle_ep.choice_path.as_slice()) {
            let diff = oracle::diff_oracle(oracle_ep, brink_ep);
            matched.push((brink_ep, diff));
        } else {
            missing_paths.push(&oracle_ep.choice_path);
        }
    }

    // Also collect brink episodes that have no oracle match (extra episodes).
    // We don't snapshot these since they're not comparable.

    let pass_count = matched.iter().filter(|(_, d)| d.matches).count();
    let fail_count = matched.iter().filter(|(_, d)| !d.matches).count();
    let total = oracle_eps.len();

    let mut out = String::new();
    let _ = writeln!(out, "case: {rel_path}");
    let _ = writeln!(
        out,
        "oracle: {total}, brink: {}, matched: {}",
        brink_eps.len(),
        matched.len(),
    );
    let _ = writeln!(out, "passing: {pass_count}/{total}");

    if total <= DETAIL_THRESHOLD {
        // Small case: show every episode in full.
        let _ = writeln!(out);
        for (ep, diff) in &matched {
            fmt_episode_full(ep, Some(diff), &mut out);
            let _ = writeln!(out);
        }
    } else {
        // Large case: show failing episodes in full, passing as digest.
        if fail_count > 0 || !missing_paths.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "--- failing episodes ---");
            let _ = writeln!(out);
            for (ep, diff) in &matched {
                if !diff.matches {
                    fmt_episode_full(ep, Some(diff), &mut out);
                    let _ = writeln!(out);
                }
            }
        }

        if !missing_paths.is_empty() {
            let _ = writeln!(out, "--- missing episodes ({}) ---", missing_paths.len());
            for path in &missing_paths {
                let _ = writeln!(out, "{path:?}");
            }
            let _ = writeln!(out);
        }

        let _ = writeln!(out, "--- passing digest ---");
        for (ep, diff) in &matched {
            if diff.matches {
                fmt_episode_digest(ep, &mut out);
            }
        }
    }

    if !missing_paths.is_empty() && total <= DETAIL_THRESHOLD {
        let _ = writeln!(out, "--- missing episodes ({}) ---", missing_paths.len());
        for path in &missing_paths {
            let _ = writeln!(out, "{path:?}");
        }
    }

    // Trim trailing whitespace.
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}
