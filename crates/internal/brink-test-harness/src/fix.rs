//! `assert_safe_fix` — the `Safe`-tier obligation of `docs/autofix-spec.md`
//! §3, made executable on top of the observable-equivalence oracle
//! (`docs/observable-semantics-spec.md` §2/§2.2, `crate::trace`).
//!
//! A fixer that declares `Applicability::Safe` is promising two distinct
//! things, and this module checks both:
//!
//! 1. **Observable equivalence** (§2). Compile the pre-fix source, compile
//!    the post-fix source, explore the pre-fix program's run set, and replay
//!    exactly those runs on the post-fix program. The comparison is
//!    [`crate::trace::trace_diff`]'s — output steps in order, choices **by
//!    order**, external calls with their arguments, host-readable globals at
//!    every turn boundary, host-invoked function results, and the terminal
//!    kind reached.
//! 2. **Translation identity** (§2.2). Diff the two programs' exported line
//!    tables by scope id and per-line text hash, through the real exporter
//!    (`brink_intl::export_lines`) that XLIFF units and `.inkl` overlays
//!    actually bind to. Changes are **reported**, and only the ones the
//!    fixture declares (its `rewrites.txt`) are tolerated — a fix that
//!    silently orphans a translation unit is not Safe however well its trace
//!    matches.
//!
//! # Fixtures
//!
//! One directory per diagnostic code under `tests/fix/<code>/`:
//!
//! ```text
//! tests/fix/E014/
//!   before.ink        the entry, pre-fix          (or before.brink)
//!   expected.ink      the entry, post-fix         (or expected.brink)
//!   brink.toml        optional — copied verbatim, so a fixture can set
//!                     `dialect`/`types` the way a real project does
//!   rewrites.txt      optional — line-identity changes the fix necessarily
//!                     makes, one `scope-id` or `scope-id#index` per line
//!   README.md         optional — ignored
//!   anything else     copied verbatim beside the entry (e.g. an INCLUDEd file)
//! ```
//!
//! Both sides are compiled from the **same** entry file name (`story.ink` /
//! `story.brink`) in two scratch directories, so nothing in the comparison
//! can vary with the file's own name.
//!
//! # What this helper cannot see
//!
//! Stated plainly, because a green `assert_safe_fix` is about to be the
//! evidence behind an unattended batch edit (`docs/autofix-spec.md` §5):
//!
//! - **Coverage, not proof.** [`crate::trace::explore_runs`] walks a bounded
//!   choice tree under a bounded seed list with bounded steps. Every bound in
//!   [`SafeFixConfig::trace`] is a coverage bound, never a semantic claim; a
//!   divergence that needs a seventh choice, an unlisted seed, or an external
//!   result the stubs never produce is outside what was looked at.
//! - **The sensitivity of the oracle itself is measured elsewhere.** Tier 3a
//!   (`docs/observable-semantics-spec.md` §4) is what says a *real* semantic
//!   change would have been caught; it is shipped for the corpus
//!   (`tests/trace_mutation_study.rs`) but has never been run over
//!   *fix-shaped* mutations, and the program generator that would run every
//!   Safe fixer over generated stories rather than hand fixtures is #3370.
//!   Until then a passing fixture says "this fixture is equivalent", not
//!   "this fixer is equivalent on every input".
//! - **Item 4's output half is a known gap.** `Story::call_function`
//!   isolates and discards the call's own output, so no host road exposes it
//!   and the probe captures the returned value only
//!   (`docs/observable-semantics-spec.md` §3).
//! - **Module-private native globals are outside item 3 by construction.**
//!   The globals capture goes through the host's own `getVar` road, which
//!   honours `#@private`; a `.brink` `var` without `pub` is not
//!   host-readable and so not compared.
//! - **Compile diagnostics are not compared.** §2 puts the diagnostics
//!   channel outside the trace. That a fix *discharges* its diagnostic is
//!   `brink_ide::fix::obligations::assert_fix_discharges`'s job, not this
//!   one's; the two obligations are complementary and a Safe fixer owes
//!   both.
//! - **There must be a pre-image at all.** A fixer whose diagnostic prevents
//!   compilation has no program to preserve, so §2 says nothing about it —
//!   the verdict is [`SafeVerdict::NoPreImage`], and such a code cannot be
//!   `Safe`. Every code on §9's first-wave Safe list is `Warning`-severity
//!   for exactly this reason.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use brink_format::StoryData;

use crate::trace::{
    LineIdentityChange, LinkedProgram, RunSpec, TraceConfig, TraceDiff, TraceEvent, explore_traces,
    to_inkb, trace_diff_with,
};

/// Everything that can go wrong loading a fixture directory off disk.
#[derive(Debug, thiserror::Error)]
pub enum FixFixtureError {
    /// The directory is missing a required file, or holds a contradictory
    /// pair (a `before.ink` beside an `expected.brink`, say).
    #[error("fix fixture {dir}: {reason}")]
    Invalid {
        /// The fixture directory, as it was named.
        dir: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The directory could not be read.
    #[error("fix fixture {dir}: {source}")]
    Io {
        /// The fixture directory, as it was named.
        dir: String,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

/// A pre-fix / post-fix source pair plus whatever else the compilation needs.
#[derive(Debug, Clone)]
pub struct FixFixture {
    /// How this fixture names itself in failure output — the directory's
    /// own name for an on-disk fixture.
    pub label: String,
    /// The entry file name both sides compile under. Deliberately the same
    /// on both sides so nothing in the comparison varies with it.
    pub entry_name: String,
    /// The entry's source before the fix.
    pub before: String,
    /// The entry's source after the fix.
    pub expected: String,
    /// Files copied verbatim beside the entry, in name order.
    pub support: Vec<(String, String)>,
    /// Line-identity changes the fix necessarily makes, as `scope-id` (the
    /// whole scope) or `scope-id#index` (one line). Anything the diff
    /// reports that is not listed here fails §2.2.
    pub allow_rewritten_units: Vec<String>,
}

/// Bounds and allowances for one [`check_safe_fix`] run.
#[derive(Debug, Clone)]
pub struct SafeFixConfig {
    /// Exploration and capture bounds handed to the oracle. Widening the
    /// seed list or the depth widens coverage; it never changes what
    /// equivalence *means*.
    pub trace: TraceConfig,
}

impl Default for SafeFixConfig {
    /// Deeper than the corpus sweep's bounds and across several seeds: a fix
    /// fixture is one small story, so it can afford exploration the
    /// whole-corpus sweep cannot.
    ///
    /// The seeds matter. §2.1 makes equivalence hold under *every* seed, so
    /// a transformation that removes or reorders an RNG draw is only caught
    /// by a run whose later draws shift — which needs a seeded run to
    /// compare.
    fn default() -> Self {
        Self {
            trace: TraceConfig {
                max_steps: 5_000,
                max_depth: 5,
                max_runs: 48,
                seeds: vec![1, 7, 42],
                ..TraceConfig::default()
            },
        }
    }
}

/// What the helper was able to conclude about one fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeVerdict {
    /// Both sides compiled, every explored run agreed, and every reported
    /// line-identity change was one the fixture declared. This is the only
    /// verdict that discharges §3's `Safe` obligation.
    ObservablyEquivalent,
    /// A run diverged — §2 fails.
    TraceDiverged,
    /// The trace held, but a line-identity change the fixture did not
    /// declare appeared — §2.2 fails.
    TranslationIdentityLost,
    /// The **pre-fix** source does not compile, so there is no program whose
    /// behaviour could be preserved. A fixer whose diagnostic blocks
    /// compilation can never be `Safe`; this is a property of the code, not
    /// of the fixture.
    NoPreImage,
    /// The **post-fix** source does not compile — the fix is broken outright.
    PostImageDoesNotCompile,
    /// Both sides compiled but the oracle could not run (a link failure, an
    /// unresolvable start path). Reported rather than folded into a
    /// divergence, since it is a harness fault, not a story difference.
    OracleFailed,
    /// Both sides compiled and agreed, but the **pre-fix** program produced
    /// no content event at all across the whole explored run set — no line,
    /// no choice, no external call, no probe. Two stories that both do
    /// nothing agree trivially, so this is *not* evidence of equivalence.
    ///
    /// The usual cause is a fixture whose content sits under a knot the
    /// story root never diverts into: ink runs the root flow, finds nothing,
    /// and ends. Give the fixture root-level content or a start path.
    VacuousExploration,
}

impl SafeVerdict {
    /// A short label for failure output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ObservablyEquivalent => "observably equivalent",
            Self::TraceDiverged => "trace diverged",
            Self::TranslationIdentityLost => "translation identity lost",
            Self::NoPreImage => "pre-fix source does not compile",
            Self::PostImageDoesNotCompile => "post-fix source does not compile",
            Self::OracleFailed => "oracle failed to run",
            Self::VacuousExploration => {
                "the pre-fix program produced no content — nothing was compared"
            }
        }
    }
}

/// The full result of checking one fixture — kept as data so a caller can
/// *record* a verdict it does not intend to assert.
#[derive(Debug, Clone)]
pub struct SafeFixReport {
    /// The fixture's label.
    pub label: String,
    /// The conclusion.
    pub verdict: SafeVerdict,
    /// How many runs were replayed on both sides. Zero when neither side ran.
    pub runs: usize,
    /// How many **content** events (lines, choice presentations, external
    /// calls, probe results) the pre-fix program produced across the whole
    /// explored run set. Zero means the comparison was vacuous — see
    /// [`SafeVerdict::VacuousExploration`].
    pub pre_content_events: usize,
    /// The trace diff, when the oracle ran.
    pub trace: Option<TraceDiff>,
    /// Line-identity changes the fixture declared — the units the fix
    /// necessarily rewrites, reported as the issue asks.
    pub rewritten_units: Vec<LineIdentityChange>,
    /// Line-identity changes nobody declared. Non-empty means §2.2 failed.
    pub unaccounted_units: Vec<LineIdentityChange>,
    /// The compile or oracle error, when there was one.
    pub detail: Option<String>,
}

impl SafeFixReport {
    /// Whether this fixture discharges §3's `Safe` obligation.
    #[must_use]
    pub fn is_safe(&self) -> bool {
        self.verdict == SafeVerdict::ObservablyEquivalent
    }
}

impl std::fmt::Display for SafeFixReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}: {} ({} run(s) replayed, {} pre-fix content event(s))",
            self.label,
            self.verdict.as_str(),
            self.runs,
            self.pre_content_events
        )?;
        if let Some(detail) = &self.detail {
            writeln!(f, "  detail: {detail}")?;
        }
        if let Some(trace) = &self.trace
            && !trace.is_empty()
        {
            write!(f, "{trace}")?;
        }
        if !self.rewritten_units.is_empty() {
            writeln!(
                f,
                "  {} declared line-identity rewrite(s):",
                self.rewritten_units.len()
            )?;
            for change in &self.rewritten_units {
                writeln!(f, "    {change:?}")?;
            }
        }
        if !self.unaccounted_units.is_empty() {
            writeln!(
                f,
                "  {} UNDECLARED line-identity change(s) — add them to rewrites.txt only if the fix genuinely rewrites them:",
                self.unaccounted_units.len()
            )?;
            for change in &self.unaccounted_units {
                writeln!(f, "    {} — {change:?}", identity_key(change))?;
            }
        }
        Ok(())
    }
}

// ── Fixture loading ─────────────────────────────────────────────────────────

/// The fixture root, `tests/fix/`, resolved from this crate's manifest so it
/// works from any working directory.
#[must_use]
pub fn fix_fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("fix")
}

/// Every fixture directory under [`fix_fixtures_root`], in name order.
pub fn fix_fixture_dirs() -> Result<Vec<PathBuf>, FixFixtureError> {
    let root = fix_fixtures_root();
    let mut dirs: Vec<PathBuf> = read_dir_sorted(&root)?
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, FixFixtureError> {
    let entries = std::fs::read_dir(dir).map_err(|source| FixFixtureError::Io {
        dir: dir.display().to_string(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| FixFixtureError::Io {
            dir: dir.display().to_string(),
            source,
        })?;
        out.push(entry.path());
    }
    out.sort();
    Ok(out)
}

/// Load one `tests/fix/<code>/` fixture directory.
pub fn load_fix_fixture(dir: &Path) -> Result<FixFixture, FixFixtureError> {
    let label = dir
        .file_name()
        .map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into());
    let invalid = |reason: String| FixFixtureError::Invalid {
        dir: dir.display().to_string(),
        reason,
    };

    let mut before: Option<(String, String)> = None;
    let mut expected: Option<(String, String)> = None;
    let mut support: Vec<(String, String)> = Vec::new();
    let mut allow_rewritten_units: Vec<String> = Vec::new();

    for path in read_dir_sorted(dir)? {
        if path.is_dir() {
            return Err(invalid(format!(
                "nested directory {:?} — a fix fixture is flat",
                path.file_name()
            )));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| invalid("unnamed entry".to_owned()))?;
        let text = std::fs::read_to_string(&path).map_err(|source| FixFixtureError::Io {
            dir: path.display().to_string(),
            source,
        })?;
        match name.as_str() {
            "before.ink" | "before.brink" => before = Some((name, text)),
            "expected.ink" | "expected.brink" => expected = Some((name, text)),
            "rewrites.txt" => allow_rewritten_units = parse_rewrites(&text),
            "README.md" => {}
            _ => support.push((name, text)),
        }
    }

    let (before_name, before) =
        before.ok_or_else(|| invalid("no before.ink / before.brink".to_owned()))?;
    let (expected_name, expected) =
        expected.ok_or_else(|| invalid("no expected.ink / expected.brink".to_owned()))?;
    let before_ext = before_name.rsplit('.').next().unwrap_or_default();
    let expected_ext = expected_name.rsplit('.').next().unwrap_or_default();
    if before_ext != expected_ext {
        return Err(invalid(format!(
            "{before_name} and {expected_name} are different surfaces — both sides must be the same"
        )));
    }

    Ok(FixFixture {
        label,
        entry_name: format!("story.{before_ext}"),
        before,
        expected,
        support,
        allow_rewritten_units,
    })
}

/// One allowance per non-empty, non-`#` line.
fn parse_rewrites(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

// ── The check ───────────────────────────────────────────────────────────────

/// Compile both sides of `fixture`, replay the pre-fix program's run set on
/// the post-fix program, and diff the line tables.
///
/// Returns a report rather than asserting, so a caller can record a verdict
/// it does not intend to require (which is how the migrated `Suggested`
/// fixers are exercised — see `tests/fix_safe_obligations.rs`). Use
/// [`assert_safe_fix`] for the obligation itself.
#[must_use]
pub fn check_safe_fix(fixture: &FixFixture, config: &SafeFixConfig) -> SafeFixReport {
    let base = SafeFixReport {
        label: fixture.label.clone(),
        verdict: SafeVerdict::ObservablyEquivalent,
        runs: 0,
        pre_content_events: 0,
        trace: None,
        rewritten_units: Vec::new(),
        unaccounted_units: Vec::new(),
        detail: None,
    };

    let before = match compile_side(fixture, &fixture.before) {
        Ok(pair) => pair,
        Err(e) => {
            return SafeFixReport {
                verdict: SafeVerdict::NoPreImage,
                detail: Some(e),
                ..base
            };
        }
    };
    let after = match compile_side(fixture, &fixture.expected) {
        Ok(pair) => pair,
        Err(e) => {
            return SafeFixReport {
                verdict: SafeVerdict::PostImageDoesNotCompile,
                detail: Some(e),
                ..base
            };
        }
    };

    // The pre-fix program owns the run set — it is the behaviour being
    // preserved — and its own traces are captured here rather than inside
    // `differential`, so the baseline can be checked for content before its
    // agreement with the post-fix program means anything.
    let baseline = match LinkedProgram::from_inkb(&before.1)
        .and_then(|linked| explore_traces(&linked, &config.trace))
    {
        Ok(traces) => traces,
        Err(e) => {
            return SafeFixReport {
                verdict: SafeVerdict::OracleFailed,
                detail: Some(e.to_string()),
                ..base
            };
        }
    };
    let pre_content_events = baseline
        .iter()
        .flat_map(|t| &t.events)
        .filter(|e| is_content_event(e))
        .count();
    let runs: Vec<RunSpec> = baseline.iter().map(|t| t.run.clone()).collect();

    let trace = match trace_diff_with(&before.1, &after.1, &runs, &config.trace) {
        Ok(diff) => diff,
        Err(e) => {
            return SafeFixReport {
                verdict: SafeVerdict::OracleFailed,
                detail: Some(e.to_string()),
                ..base
            };
        }
    };

    let identity = crate::trace::line_identity_diff(&before.0, &after.0);
    let (rewritten_units, unaccounted_units) =
        split_identity_changes(&identity.changes, &fixture.allow_rewritten_units);

    let verdict = if trace.is_empty() {
        if !unaccounted_units.is_empty() {
            SafeVerdict::TranslationIdentityLost
        } else if pre_content_events == 0 {
            SafeVerdict::VacuousExploration
        } else {
            SafeVerdict::ObservablyEquivalent
        }
    } else {
        SafeVerdict::TraceDiverged
    };

    SafeFixReport {
        runs: trace.total_runs,
        pre_content_events,
        verdict,
        trace: Some(trace),
        rewritten_units,
        unaccounted_units,
        ..base
    }
}

/// Whether an event is the story *doing* something a host can see, as
/// opposed to the turn-boundary bookkeeping every run emits regardless.
///
/// [`TraceEvent::Globals`] and [`TraceEvent::Terminal`] are recorded even for
/// a story that runs out of content immediately, so counting them would let a
/// fixture that never reaches its own content pass vacuously.
fn is_content_event(event: &TraceEvent) -> bool {
    matches!(
        event,
        TraceEvent::Line { .. }
            | TraceEvent::Choices(_)
            | TraceEvent::External { .. }
            | TraceEvent::Probe { .. }
    )
}

/// §3's `Safe` obligation: `check_safe_fix` must return
/// [`SafeVerdict::ObservablyEquivalent`].
///
/// This is the assertion a `Safe`-max fixer's fixture is run through. It
/// reports the full diff on failure — which run, which turn, which
/// observable, or which translation unit moved.
pub fn assert_safe_fix(fixture: &FixFixture, config: &SafeFixConfig) -> SafeFixReport {
    let report = check_safe_fix(fixture, config);
    assert!(
        report.is_safe(),
        "{} is not a Safe fix (docs/autofix-spec.md §3):\n{report}",
        fixture.label
    );
    report
}

/// The key an allowance in `rewrites.txt` is matched against.
fn identity_key(change: &LineIdentityChange) -> String {
    match change {
        LineIdentityChange::ScopeOnlyIn { scope_id, .. } => scope_id.clone(),
        LineIdentityChange::LineOnlyIn {
            scope_id, index, ..
        }
        | LineIdentityChange::HashChanged {
            scope_id, index, ..
        } => format!("{scope_id}#{index}"),
    }
}

/// Split reported identity changes into the declared ones and the rest. An
/// allowance naming a bare scope id covers every line in that scope.
fn split_identity_changes(
    changes: &[LineIdentityChange],
    allowed: &[String],
) -> (Vec<LineIdentityChange>, Vec<LineIdentityChange>) {
    let mut declared = Vec::new();
    let mut rest = Vec::new();
    for change in changes {
        let key = identity_key(change);
        let scope = match change {
            LineIdentityChange::ScopeOnlyIn { scope_id, .. }
            | LineIdentityChange::LineOnlyIn { scope_id, .. }
            | LineIdentityChange::HashChanged { scope_id, .. } => scope_id,
        };
        if allowed.iter().any(|a| a == &key || a == scope) {
            declared.push(change.clone());
        } else {
            rest.push(change.clone());
        }
    }
    (declared, rest)
}

/// Write one side of the fixture into a scratch directory and compile it
/// through the **production** road (`brink_environment::compile`), so a
/// fixture's `brink.toml` is honoured exactly as it would be in a real
/// project.
fn compile_side(fixture: &FixFixture, entry_src: &str) -> Result<(StoryData, Vec<u8>), String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "brink-fix-{}-{}-{nonce}",
        sanitize(&fixture.label),
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    let result = (|| {
        std::fs::create_dir_all(&dir).map_err(|e| format!("create scratch dir: {e}"))?;
        for (name, text) in &fixture.support {
            std::fs::write(dir.join(name), text).map_err(|e| format!("write {name}: {e}"))?;
        }
        let entry = dir.join(&fixture.entry_name);
        std::fs::write(&entry, entry_src)
            .map_err(|e| format!("write {}: {e}", fixture.entry_name))?;
        let data = crate::corpus::compile_via_environment(&entry)?;
        let bytes = to_inkb(&data);
        Ok((data, bytes))
    })();
    std::fs::remove_dir_all(&dir).ok();
    result
}

/// Scratch-directory-safe form of a fixture label.
fn sanitize(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory fixture with `dialect = "brink"` left off — plain
    /// ink, which is what every self-check here needs.
    fn fixture(label: &str, before: &str, expected: &str) -> FixFixture {
        FixFixture {
            label: label.to_owned(),
            entry_name: "story.ink".to_owned(),
            before: before.to_owned(),
            expected: expected.to_owned(),
            support: Vec::new(),
            allow_rewritten_units: Vec::new(),
        }
    }

    const BARE_TILDE: &str = "VAR score = 0\nHello.\n~\n~ score = score + 1\nScore is {score}.\n* [Go on]\n  Onward.\n  -> DONE\n";
    const NO_TILDE: &str = "VAR score = 0\nHello.\n~ score = score + 1\nScore is {score}.\n* [Go on]\n  Onward.\n  -> DONE\n";

    /// The positive arm: a real §9 first-wave `Safe` candidate (E014, delete
    /// the bare `~`) is certified — same trace, same translation identity.
    #[test]
    fn deleting_a_bare_tilde_is_observably_equivalent() {
        let report = check_safe_fix(
            &fixture("bare-tilde", BARE_TILDE, NO_TILDE),
            &SafeFixConfig::default(),
        );
        assert!(report.is_safe(), "{report}");
        assert!(report.runs > 0, "the run set must not be empty: {report}");
        assert!(
            report.pre_content_events > 0,
            "the baseline must actually print something: {report}"
        );
    }

    /// Two stories that both run out of content immediately agree trivially.
    /// The vacuity guard is what stops that from reading as evidence — and
    /// the shape is not hypothetical: content parked under a knot the root
    /// never diverts into is the natural way to write this fixture wrong.
    #[test]
    fn a_baseline_that_never_reaches_its_content_is_vacuous() {
        let before = "VAR score = 0\n=== main ===\nHello.\n~\n-> DONE\n";
        let after = "VAR score = 0\n=== main ===\nHello.\n-> DONE\n";
        let report = check_safe_fix(
            &fixture("vacuous", before, after),
            &SafeFixConfig::default(),
        );
        assert_eq!(report.verdict, SafeVerdict::VacuousExploration, "{report}");
        assert_eq!(report.pre_content_events, 0, "{report}");
    }

    /// The negative arm for §2 item 1: a changed output line is caught. If
    /// the trace half of the check were deleted, this would pass.
    #[test]
    fn a_changed_output_line_is_not_safe() {
        let report = check_safe_fix(
            &fixture(
                "changed-line",
                BARE_TILDE,
                &NO_TILDE.replace("Hello.", "Howdy."),
            ),
            &SafeFixConfig::default(),
        );
        assert_eq!(report.verdict, SafeVerdict::TraceDiverged, "{report}");
    }

    /// The negative arm for §2 item 3: a host-readable global that ends up
    /// with a different value is caught even though no line changed.
    #[test]
    fn a_changed_host_readable_global_is_not_safe() {
        let report = check_safe_fix(
            &fixture(
                "changed-global",
                BARE_TILDE,
                &NO_TILDE.replace("~ score = score + 1", "~ score = score + 2"),
            ),
            &SafeFixConfig::default(),
        );
        assert_eq!(report.verdict, SafeVerdict::TraceDiverged, "{report}");
    }

    /// The negative arm for §2.1's "choices compare by order, not by set".
    #[test]
    fn swapping_two_choices_is_not_safe() {
        let before = "Pick.\n* [Left]\n  L\n  -> DONE\n* [Right]\n  R\n  -> DONE\n";
        let after = "Pick.\n* [Right]\n  R\n  -> DONE\n* [Left]\n  L\n  -> DONE\n";
        let report = check_safe_fix(
            &fixture("swapped-choices", before, after),
            &SafeFixConfig::default(),
        );
        assert_eq!(report.verdict, SafeVerdict::TraceDiverged, "{report}");
    }

    /// A fixer whose diagnostic prevents compilation has no pre-image, so §2
    /// says nothing about it — the verdict names that rather than pretending
    /// equivalence was checked.
    #[test]
    fn a_pre_fix_source_that_does_not_compile_has_no_pre_image() {
        let report = check_safe_fix(
            &fixture("no-pre-image", "-> nowhere\n", NO_TILDE),
            &SafeFixConfig::default(),
        );
        assert_eq!(report.verdict, SafeVerdict::NoPreImage, "{report}");
        assert!(report.detail.is_some(), "{report}");
    }

    /// A broken fix is reported as such rather than as a divergence.
    #[test]
    fn a_post_fix_source_that_does_not_compile_is_reported() {
        let report = check_safe_fix(
            &fixture("broken-fix", BARE_TILDE, "-> nowhere\n"),
            &SafeFixConfig::default(),
        );
        assert_eq!(
            report.verdict,
            SafeVerdict::PostImageDoesNotCompile,
            "{report}"
        );
    }

    /// `assert_safe_fix` fails loudly rather than returning a bad verdict.
    #[test]
    #[should_panic(expected = "is not a Safe fix")]
    fn assert_safe_fix_rejects_a_divergent_pair() {
        let _ = assert_safe_fix(
            &fixture("rejects", BARE_TILDE, &NO_TILDE.replace("Hello.", "Howdy.")),
            &SafeFixConfig::default(),
        );
    }

    /// `rewrites.txt` parsing: comments and blank lines are not allowances.
    #[test]
    fn rewrites_file_ignores_comments_and_blanks() {
        let parsed = parse_rewrites("# a note\n\n0x00ff#3\n  0x00ff  \n");
        assert_eq!(parsed, vec!["0x00ff#3".to_owned(), "0x00ff".to_owned()]);
    }

    /// A bare scope id in `rewrites.txt` covers every line in that scope; an
    /// unlisted scope stays unaccounted.
    #[test]
    fn a_scope_allowance_covers_its_lines() {
        let changes = vec![
            LineIdentityChange::HashChanged {
                scope_id: "0xaa".to_owned(),
                index: 2,
                before: "1".to_owned(),
                after: "2".to_owned(),
            },
            LineIdentityChange::HashChanged {
                scope_id: "0xbb".to_owned(),
                index: 0,
                before: "1".to_owned(),
                after: "2".to_owned(),
            },
        ];
        let (declared, rest) = split_identity_changes(&changes, &["0xaa".to_owned()]);
        assert_eq!(declared.len(), 1, "{declared:?}");
        assert_eq!(rest.len(), 1, "{rest:?}");
        assert_eq!(identity_key(&rest[0]), "0xbb#0");
    }

    /// End-to-end for §2.2's "except units the fix necessarily rewrites":
    /// a real fixture directory on disk, loaded by [`load_fix_fixture`], whose
    /// `rewrites.txt` turns a reported identity change from a failure into a
    /// declared rewrite.
    ///
    /// The transformation edits a line in a knot the story never enters, so
    /// the trace is untouched and the *only* thing standing between the pair
    /// and `ObservablyEquivalent` is the §2.2 half — which is exactly the
    /// separation the spec draws between the two invariants.
    #[test]
    fn a_declared_rewrite_in_rewrites_txt_is_tolerated_end_to_end() {
        let before = "Hello.\n-> DONE\n\n=== unvisited ===\nOld words.\n-> DONE\n";
        let after = "Hello.\n-> DONE\n\n=== unvisited ===\nNew words.\n-> DONE\n";
        let dir = std::env::temp_dir().join(format!(
            "brink-fix-rewrites-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        std::fs::write(dir.join("before.ink"), before).expect("write before.ink");
        std::fs::write(dir.join("expected.ink"), after).expect("write expected.ink");

        let config = SafeFixConfig::default();
        let loaded = load_fix_fixture(&dir).expect("load fixture");
        assert_eq!(loaded.entry_name, "story.ink");
        assert!(
            loaded.allow_rewritten_units.is_empty(),
            "no rewrites.txt yet: {:?}",
            loaded.allow_rewritten_units
        );
        let undeclared = check_safe_fix(&loaded, &config);
        assert_eq!(
            undeclared.verdict,
            SafeVerdict::TranslationIdentityLost,
            "{undeclared}"
        );
        assert_eq!(undeclared.unaccounted_units.len(), 1, "{undeclared}");

        // Declare exactly the unit that moved, in the spelling the failure
        // message prints, and the same pair now clears the bar.
        let key = identity_key(&undeclared.unaccounted_units[0]);
        std::fs::write(
            dir.join("rewrites.txt"),
            format!("# the edited line\n{key}\n"),
        )
        .expect("write rewrites.txt");
        let declared_fixture = load_fix_fixture(&dir).expect("reload fixture");
        assert_eq!(declared_fixture.allow_rewritten_units, vec![key]);
        let declared = check_safe_fix(&declared_fixture, &config);
        std::fs::remove_dir_all(&dir).ok();

        assert!(declared.is_safe(), "{declared}");
        assert_eq!(declared.rewritten_units.len(), 1, "{declared}");
        assert!(declared.unaccounted_units.is_empty(), "{declared}");
    }
}
