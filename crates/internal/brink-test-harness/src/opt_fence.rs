//! The optimizer fence's single seam (`docs/optimizer-spec.md` §5).
//!
//! Everything that judges an optimizer run — the corpus fence, the negative
//! controls, and the generator property — goes through [`judge`]. That is the
//! design's load-bearing constraint: **if the positive and negative tests took
//! different code paths, the negative control would prove nothing about the
//! fence.** A control that goes red through some other route says only that the
//! diff functions work, not that the fence is wired to them.
//!
//! # The five obligations
//!
//! [`Obligations`] carries one flag per thing an optimizer must not break:
//!
//! | flag | oracle | what it catches |
//! |---|---|---|
//! | `trace_clean` | [`trace::differential`] | observable behaviour changed |
//! | `identity_clean` | [`trace::line_identity_diff`] | translations orphaned |
//! | `idempotent` | bytes | `opt(opt(A)) != opt(A)` |
//! | `stable` | bytes | two runs over one input disagree |
//! | `bytes_identical` | bytes | see below |
//!
//! The two semantic oracles are *not* redundant with each other: a
//! transformation can pass the trace diff and still orphan every translation,
//! which is why `line_identity_diff` exists. And neither can see what `stable`
//! catches, which is why the byte checks are here at all.
//!
//! # `bytes_identical` is not a tautology in v1
//!
//! With an empty pass list `optimize` is a no-op on the data, so it is tempting
//! to read every flag above as trivially true. Four of them are. The fifth is
//! not: the road here is `read_inkb → optimize → write_inkb`, so with no passes
//! `bytes_identical` asserts **`write_inkb ∘ read_inkb == id` over every real
//! corpus artifact**. Nothing else in the tree checks that — `brink-format`'s
//! own round-trip tests use synthetic and hand-built values.
//!
//! It should hold (the writer recomputes the CRC from the body and ignores
//! `StoryData::source_checksum`, the one field the reader synthesises), but it
//! is a real property and the fence reports it as its own failure line naming
//! `brink-format`, so a format round-trip bug is never misattributed to the
//! optimizer.
//!
//! # What `stable` honestly covers
//!
//! Two `optimize` calls in one process catch nondeterminism that manifests
//! within a process — which is what `control:drift` models. It does **not**
//! reliably catch `HashMap`-iteration-order nondeterminism, the actual
//! house-rule hazard: `RandomState` varies per map instance, so two runs
//! usually differ but are not guaranteed to. Cross-process ordering is covered
//! instead by this sweep running on every CI machine. Do not read `stable` as
//! more than it is.

use brink_format::StoryData;
use brink_opt::{ArtifactStats, OptConfig, OptReport, optimize};

use crate::trace::{
    LineIdentityDiff, LinkedProgram, TraceConfig, TraceError, differential, explore_traces,
    line_identity_diff, to_inkb,
};

/// Something went wrong before any obligation could be judged.
#[derive(Debug, thiserror::Error)]
pub enum OptFenceError {
    /// The input bytes could not be decoded.
    #[error("decode .inkb: {0}")]
    Decode(String),
    /// The trace oracle failed (decode, link, or an unresolvable start path).
    #[error("trace oracle: {0}")]
    Trace(#[from] TraceError),
}

/// Read, optimize, write — **the seam**.
///
/// Every green result in the fence is evidence this ran, because there is no
/// other way for a test in this module's orbit to produce a `post` artifact.
///
/// # Errors
///
/// [`OptFenceError::Decode`] when `pre` is not a readable `.inkb`.
pub fn optimize_bytes(
    pre: &[u8],
    config: &OptConfig,
) -> Result<(StoryData, Vec<u8>, OptReport), OptFenceError> {
    let mut story =
        brink_format::read_inkb(pre).map_err(|e| OptFenceError::Decode(format!("{e:?}")))?;
    let report = optimize(&mut story, config);
    let bytes = to_inkb(&story);
    Ok((story, bytes, report))
}

/// The verdict on one artifact under one optimizer configuration.
///
/// One `bool` per obligation is the point rather than an accident: the negative
/// controls' matrix (`brink_opt::control`) has a column per flag, and a control
/// asserts an exact row. Collapsing them into a set or a bitfield would make
/// "this control trips the trace oracle and provably nothing else" harder to
/// state, which is the property the whole design turns on.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per obligation; the controls assert exact rows of this matrix"
)]
pub struct Obligations {
    /// The optimized artifact behaves identically over `pre`'s explored runs.
    pub trace_clean: bool,
    /// Every translatable unit kept its `(scope, index, hash)` identity.
    pub identity_clean: bool,
    /// `opt(opt(A))` is byte-identical to `opt(A)`.
    pub idempotent: bool,
    /// Two independent runs over the same input produce identical bytes.
    pub stable: bool,
    /// The output is byte-identical to the input (expected only when no pass
    /// changed anything — see the module doc).
    pub bytes_identical: bool,
    /// Measurement of the input, for the sweep's content floors.
    pub before: ArtifactStats,
    /// Whether any pass reported a change.
    pub changed: bool,
    /// The line-identity diff itself, not just `identity_clean`.
    ///
    /// The negative controls assert the *variant* — `control:rehash` must
    /// produce `HashChanged` and not, say, `LineOnlyIn` — so that "it tripped
    /// line identity" means the thing intended rather than any identity change
    /// at all. Keeping it here rather than having the control call
    /// `line_identity_diff` itself is what preserves the one-seam property.
    pub identity: LineIdentityDiff,
    /// The rendered diff of whichever obligation failed first, for reporting.
    pub detail: String,
}

impl Obligations {
    /// Whether every obligation held.
    #[must_use]
    pub fn all_clean(&self) -> bool {
        self.trace_clean && self.identity_clean && self.idempotent && self.stable
    }
}

/// Judge one artifact under one optimizer configuration.
///
/// `pre_data` is the caller's already-decoded story — the compile helpers hand
/// back both halves (`corpus::compile_entry_to_inkb`), so re-decoding here
/// would be waste.
///
/// # Errors
///
/// Propagates decode and trace-oracle failures; an obligation being *violated*
/// is a `false` flag, not an error.
pub fn judge(
    pre_data: &StoryData,
    pre: &[u8],
    opt: &OptConfig,
    trace: &TraceConfig,
) -> Result<Obligations, OptFenceError> {
    let (post_data, post, report) = optimize_bytes(pre, opt)?;

    let trace_diff = differential(pre, &post, trace)?;
    let identity = line_identity_diff(pre_data, &post_data);

    // Idempotence: optimize the OUTPUT again and compare bytes.
    let (_, twice, _) = optimize_bytes(&post, opt)?;
    // Stability: optimize the INPUT again, independently, and compare bytes.
    let (_, again, _) = optimize_bytes(pre, opt)?;

    let identity_clean = identity.is_empty();
    let mut detail = String::new();
    if !trace_diff.is_empty() {
        detail = format!("{trace_diff}");
    } else if !identity_clean {
        detail = format!("{identity}");
    }

    Ok(Obligations {
        trace_clean: trace_diff.is_empty(),
        identity_clean,
        idempotent: twice == post,
        stable: again == post,
        bytes_identical: post == pre,
        before: report.before,
        changed: report.changed(),
        identity,
        detail,
    })
}

/// Whether `pre`'s explored runs emit text that a **line-table entry supplies**.
///
/// The grounding predicate for the text-perturbing controls, in the discipline
/// `mutate.rs` established: an ungrounded control survives because bounded
/// exploration never looked at the thing it edited, which says nothing about
/// whether the fence works. A case that is not grounded is counted `inert`,
/// never a survivor.
///
/// The obvious weaker predicate — "do the runs emit any text at all?" — is not
/// enough, and the fence caught that on its first run.
/// `tier1/diverts/I132-comparing-diverts` emits `1`/`0`/`0`/`1` from pure value
/// interpolation, which produces **no line-table entries**; its only line
/// entries belong to two knots that are never entered, existing solely as
/// divert targets to compare. Retexting those changes an artifact nobody reads,
/// so the trace stays clean and the case looked like a survivor when it was
/// simply out of the control's reach.
///
/// So the test is an overlap: some text the runs actually emitted must be text
/// a `Plain` line entry supplies. `Template` entries are skipped, which
/// **under**-grounds — the safe direction, since it can only lower the kill
/// count that the control-kill floor already guards.
///
/// # Errors
///
/// Propagates trace-oracle failures.
pub fn is_line_text_grounded(
    story: &StoryData,
    pre: &[u8],
    config: &TraceConfig,
) -> Result<bool, TraceError> {
    let literals: Vec<&str> = story
        .line_tables
        .iter()
        .flat_map(|t| &t.lines)
        .filter_map(|l| match &l.content {
            brink_format::LineContent::Plain(s) => Some(s.trim()),
            brink_format::LineContent::Template(_) => None,
        })
        .filter(|s| !s.is_empty())
        .collect();
    if literals.is_empty() {
        return Ok(false);
    }

    let linked = LinkedProgram::from_inkb(pre)?;
    let traces = explore_traces(&linked, config)?;
    Ok(traces.iter().any(|t| {
        t.events.iter().any(|e| match e {
            crate::trace::TraceEvent::Line { text, .. } => {
                literals.iter().any(|lit| contains_bounded(text, lit))
            }
            crate::trace::TraceEvent::Choices(cs) => cs
                .iter()
                .any(|c| literals.iter().any(|lit| contains_bounded(&c.text, lit))),
            _ => false,
        })
    }))
}

/// `text` contains `lit` as a whole run — not as a fragment of a longer
/// alphanumeric word. A one-letter choice label `[a]` must not ground a story
/// whose only rendered text is `beta`: the retext control cannot be observed
/// on a line the runs never reach, and plain substring containment said it
/// could (a real 1-in-6 flake on generated stories, 2026-09-05).
fn contains_bounded(text: &str, lit: &str) -> bool {
    let word = |c: char| c.is_alphanumeric();
    text.match_indices(lit).any(|(at, _)| {
        let before = text[..at].chars().next_back().is_none_or(|c| !word(c));
        let after = text[at + lit.len()..]
            .chars()
            .next()
            .is_none_or(|c| !word(c));
        (before && after) || !lit.chars().any(word)
    })
}

/// Whether the story has any line-table entry at all.
///
/// The grounding predicate for the controls that edit line metadata rather than
/// rendered text: purely static and exact, no exploration needed.
#[must_use]
pub fn has_line_entries(story: &StoryData) -> bool {
    story.line_tables.iter().any(|t| !t.lines.is_empty())
}
