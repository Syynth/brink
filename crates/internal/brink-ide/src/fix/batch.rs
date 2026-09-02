//! Batching — `docs/autofix-spec.md` §5. One algorithm for every batch
//! surface: `brink fix`, the Problems panel's "Fix all safe", the LSP
//! `fixAll` road, fix-on-save.
//!
//! Two steps, deliberately separated so each is testable on its own:
//!
//! 1. [`collect`] — walk the selection's diagnostics, compute their fixes,
//!    keep the ones the [`FixPolicy`] admits. Identical fixes collapse (one
//!    site can carry several diagnostics a single edit discharges), the same
//!    way [`fixes_at`](super::fixes_at) collapses them for the cursor menu.
//! 2. [`plan`] — order the candidates and **drop** the ones whose edits
//!    collide with an already-kept edit. Never merge: no two fixes reason
//!    about each other, and the dropped ones come back a round later on fresh
//!    analysis.
//!
//! [`apply_round`] is those two composed; [`fix_all`] runs rounds to a
//! fixpoint against a hard cap.
//!
//! **Overlap is *touching*, not just intersecting.** Two edits of the same
//! file collide when their byte ranges meet at all — including two pure
//! insertions at the same offset, which is exactly what two `E025`
//! auto-imports into one file produce. Earliest range wins: candidates are
//! ordered by their earliest edit (file id, then start, then end, then code
//! and title as the tiebreak), and the first one to claim a span keeps it.
//!
//! **The cap is never silent.** [`fix_all`] stops after `max_rounds`
//! ([`DEFAULT_MAX_ROUNDS`]) applying rounds and then re-runs the selection
//! once: whatever the policy still admits is [`Report::remaining`], and
//! [`Report::cap_hit`] says the loop ran out of rounds rather than converging.
//! A fixer that fails to discharge its own diagnostic — a bug, per §5 — shows
//! up here as a cap breach naming that diagnostic, instead of looping.

use std::collections::BTreeMap;

use brink_ir::{DiagnosticCode, FileId, suppressions::apply_suppressions};
use rowan::TextRange;

use crate::rename::FileEdit;
use crate::session::IdeSession;

use super::policy::FixPolicy;
use super::select::Select;
use super::{Fix, FixCx, fix_key, fixes_for};

/// The §5 round cap.
pub const DEFAULT_MAX_ROUNDS: u8 = 5;

/// Where a fix was taken: the diagnostic it discharges, and where that
/// diagnostic sat in the round's analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixSite {
    /// The diagnostic this fix discharges.
    pub code: DiagnosticCode,
    /// The file the diagnostic is reported in — *not* necessarily the file
    /// the edits land in (§4).
    pub file: FileId,
    /// The diagnostic's own range, as of the round that took the fix.
    pub range: TextRange,
}

/// One fix chosen for a round, paired with the diagnostic site it discharges.
pub struct Candidate {
    /// The diagnostic the fix answers.
    pub site: FixSite,
    /// The fix itself.
    pub fix: Fix,
}

/// The outcome of one round: what to apply, and what was deferred.
pub struct Round {
    /// The edits to apply, ordered by `(file, start, end)`. Non-overlapping
    /// by construction, so a caller may splice them in any order it likes.
    pub edits: Vec<FileEdit>,
    /// One entry per fix whose edits are in [`edits`](Self::edits).
    pub applied: Vec<FixSite>,
    /// Fixes dropped because their edits touched an already-kept edit. They
    /// are not lost — the next round recomputes them against fresh analysis.
    pub skipped_overlap: Vec<FixSite>,
}

/// The result of [`fix_all`].
pub struct Report {
    /// Every fix applied, in the order the rounds applied them.
    pub applied: Vec<FixSite>,
    /// How many times a fix was deferred to a later round because its edits
    /// collided. Summed across rounds, so a fix deferred once and applied
    /// next round counts once.
    pub skipped_overlap: usize,
    /// Diagnostics the policy still admits a fix for when the loop stopped.
    /// Empty on convergence; non-empty exactly when the round cap was hit.
    pub remaining: Vec<FixSite>,
    /// How many rounds actually applied something.
    pub rounds: u8,
    /// The loop ran out of rounds with work still admitted (§5 — reported,
    /// never silent).
    pub cap_hit: bool,
}

/// Every fix of the selection the policy admits for batching, in the
/// compilation's own file order.
///
/// Identical fixes collapse: one site can carry several diagnostics of the
/// same code whose single edit discharges all of them (`E080` reports one per
/// unbound `ref` param), and a batch must apply that edit once.
#[must_use]
pub fn collect(cx: &FixCx<'_>, select: &Select, policy: &FixPolicy) -> Vec<Candidate> {
    let db = cx.db;
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for file in select.files(db) {
        let (Some(raw), Some(source)) = (db.diagnostics(file), db.source(file)) else {
            continue;
        };
        // The Problems panel never shows a suppressed diagnostic, so a batch
        // must not fix one either.
        let diagnostics = match db.suppressions(file) {
            Some(sup) => apply_suppressions(file, source, raw.to_vec(), sup),
            None => raw.to_vec(),
        };
        for d in &diagnostics {
            if !select.matches(db, d) {
                continue;
            }
            for fix in fixes_for(cx, d) {
                if !select.admits_tier(fix.applicability)
                    || !policy.admits(fix.code, fix.applicability)
                {
                    continue;
                }
                let key = fix_key(&fix);
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                out.push(Candidate {
                    site: FixSite {
                        code: d.code,
                        file: d.file,
                        range: d.range,
                    },
                    fix,
                });
            }
        }
    }
    out
}

/// A candidate's position in the round's total order: its earliest edit's
/// `(file, start, end)`, then the code and title as a stable tiebreak for two
/// fixes anchored at the same byte.
type OrderKey = (u32, u32, u32, &'static str, String);

/// The earliest edit of a fix, as the sort key `(file, start, end)`. `None`
/// for a fix with no edits — such a fix changes nothing and is not part of a
/// round.
fn anchor(fix: &Fix) -> Option<(u32, u32, u32)> {
    fix.edits
        .iter()
        .map(|e| {
            (
                e.file.0,
                u32::from(e.range.start()),
                u32::from(e.range.end()),
            )
        })
        .min()
}

/// Whether two edits of the same file collide. *Touching* counts: adjacent
/// ranges, and two insertions at the same offset, are both collisions.
fn edits_touch(a: &FileEdit, b: &FileEdit) -> bool {
    a.file == b.file && a.range.start() <= b.range.end() && b.range.start() <= a.range.end()
}

/// Order the candidates and drop the ones that collide — §5 step 2.
///
/// The unit of dropping is the **fix**, not the individual edit: a fix's
/// edits are one atomic change (they may span files), and applying half of it
/// would leave the compilation in a state no fixer intended. A candidate any
/// of whose edits touches an already-kept edit is deferred whole.
#[must_use]
pub fn plan(candidates: Vec<Candidate>) -> Round {
    let mut ordered: Vec<(OrderKey, Candidate)> = candidates
        .into_iter()
        .filter_map(|c| {
            let (file, start, end) = anchor(&c.fix)?;
            Some((
                (file, start, end, c.fix.code.as_str(), c.fix.title.clone()),
                c,
            ))
        })
        .collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));

    let mut edits: Vec<FileEdit> = Vec::new();
    let mut applied = Vec::new();
    let mut skipped_overlap = Vec::new();
    for (_, candidate) in ordered {
        let Candidate { site, fix } = candidate;
        if fix
            .edits
            .iter()
            .any(|new| edits.iter().any(|kept| edits_touch(kept, new)))
        {
            skipped_overlap.push(site);
            continue;
        }
        edits.extend(fix.edits);
        applied.push(site);
    }
    edits.sort_by_key(|e| (e.file, e.range.start(), e.range.end()));

    Round {
        edits,
        applied,
        skipped_overlap,
    }
}

/// One round of §5: the fixes the selection and policy admit, minus the ones
/// that collide, as a single edit set.
#[must_use]
pub fn apply_round(cx: &FixCx<'_>, select: &Select, policy: &FixPolicy) -> Round {
    plan(collect(cx, select, policy))
}

/// Run rounds to a fixpoint: apply a round's edits, re-analyze the
/// compilation, repeat until a round applies nothing or `max_rounds` is
/// spent.
///
/// The session is the compilation — re-analysis is a mutation, which is why
/// this takes the session that owns the [`ProjectDb`](brink_db::ProjectDb)
/// rather than the read-only [`FixCx`] the spec sketch names.
pub fn fix_all(
    session: &mut IdeSession,
    select: &Select,
    policy: &FixPolicy,
    max_rounds: u8,
) -> Report {
    let mut applied = Vec::new();
    let mut skipped_overlap = 0usize;
    let mut rounds = 0u8;

    while rounds < max_rounds {
        let round = apply_round(&FixCx::new(session.db()), select, policy);
        if round.edits.is_empty() {
            break;
        }
        rounds += 1;
        skipped_overlap += round.skipped_overlap.len();
        applied.extend(round.applied);
        apply_edits(session, &round.edits);
    }

    // One more selection pass, no edits: whatever is still admitted is work
    // the cap cut off (or a fixer that never discharged its diagnostic).
    let remaining: Vec<FixSite> = collect(&FixCx::new(session.db()), select, policy)
        .into_iter()
        .map(|c| c.site)
        .collect();
    let cap_hit = !remaining.is_empty();

    Report {
        applied,
        skipped_overlap,
        remaining,
        rounds,
        cap_hit,
    }
}

/// Splice a round's edits into the session's sources and re-analyze.
///
/// Within a file the edits are non-overlapping (that is what [`plan`]
/// guarantees), so splicing from the end keeps every earlier offset valid.
/// The bounds/boundary guard mirrors [`gate`](crate::structural_result::gate):
/// a malformed edit is skipped rather than panicking the batch.
fn apply_edits(session: &mut IdeSession, edits: &[FileEdit]) {
    let mut by_file: BTreeMap<FileId, Vec<&FileEdit>> = BTreeMap::new();
    for e in edits {
        by_file.entry(e.file).or_default().push(e);
    }

    let mut writes: Vec<(String, String)> = Vec::new();
    for (file, mut file_edits) in by_file {
        let (Some(path), Some(src)) = (session.file_path(file), session.source(file)) else {
            continue;
        };
        let path = path.to_owned();
        let mut out = src.to_owned();
        file_edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in file_edits {
            let (start, end) = (usize::from(e.range.start()), usize::from(e.range.end()));
            if start <= end
                && end <= out.len()
                && out.is_char_boundary(start)
                && out.is_char_boundary(end)
            {
                out.replace_range(start..end, &e.new_text);
            }
        }
        writes.push((path, out));
    }

    for (path, source) in writes {
        session.update_source(&path, source);
    }
    session.refresh_analysis();
}

#[cfg(test)]
mod tests;
