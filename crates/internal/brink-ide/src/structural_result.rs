//! The unified result type for every mutating structural op (#316).
//!
//! Rename, move, promote, demote, reorder, file-rename, and delete all return a
//! single [`StructuralResult`]: the rewritten primary-file source plus any
//! cross-file reference edits, carrying the safe-by-default breakage gate
//! (`safe` + `introduced_diagnostics`). Reorders — which change no qualification
//! — are trivially safe and skip the overlay reanalysis. Every other op runs the
//! op-agnostic [`gate`] to surface the diagnostics it would introduce.

use std::collections::BTreeMap;

use brink_analyzer::AnalysisResult;
use brink_ir::{DiagnosticCode, FileId, Severity};

use crate::line_index::LineIndex;
use crate::rename::FileEdit;
use crate::session::IdeSession;

/// A diagnostic an op *would introduce* — present after the edit but not before.
/// Carries everything the studio's breakage report needs to render and navigate
/// (1-based line/col, matching the CLI's `DiagEntry`).
pub struct IntroducedDiagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    /// Project-relative path of the file the diagnostic lands in.
    pub path: String,
    /// 1-based line of the diagnostic's start.
    pub line: u32,
    /// 1-based column of the diagnostic's start.
    pub col: u32,
}

/// The unified output of a structural op.
///
/// `new_source` is the rewritten content of the primary file (the file the op
/// acted on); `cross_file_edits` are the reference rewrites that land in *other*
/// files. `introduced.is_empty()` ⇒ `safe` ⇒ the edits apply directly;
/// otherwise the caller shows a breakage report and applies only on force.
pub struct StructuralResult {
    /// The new full source text for the primary file (`None` for ops, like a
    /// pure delete with no rewritten remainder, that produce no primary source —
    /// in practice always `Some` for the current ops).
    pub new_source: Option<String>,
    /// Reference edits in other files that must be applied.
    pub cross_file_edits: Vec<FileEdit>,
    /// True when the op introduces no new diagnostics.
    pub safe: bool,
    /// Diagnostics present after the op but not before. Empty ⇒ `safe`.
    pub introduced: Vec<IntroducedDiagnostic>,
}

impl StructuralResult {
    /// A trivially-safe result: no reanalysis, no introduced diagnostics. Used by
    /// reorders (which change no qualification) and other no-op-safe rewrites.
    #[must_use]
    pub fn safe_source(new_source: String) -> Self {
        Self {
            new_source: Some(new_source),
            cross_file_edits: Vec::new(),
            safe: true,
            introduced: Vec::new(),
        }
    }
}

/// The op-agnostic safe-by-default gate (#316): overlay `edits` onto the current
/// project sources, re-analyze, and return the diagnostics the edit *would*
/// introduce. Generalizes the rename gate to any structural op — the caller
/// supplies the full set of computed edits (primary + cross-file) and gets back
/// the breakage report. The session is not mutated.
#[must_use]
pub fn gate(session: &IdeSession, edits: &[FileEdit]) -> Vec<IntroducedDiagnostic> {
    let Some(analysis) = session.analysis() else {
        return Vec::new();
    };

    // Build the overlay: apply each file's edits to its current source.
    // `FileId` isn't `Ord`, so key the group map by its raw id.
    let mut by_file: BTreeMap<u32, Vec<&FileEdit>> = BTreeMap::new();
    for e in edits {
        by_file.entry(e.file.0).or_default().push(e);
    }
    let mut overlay: BTreeMap<String, String> = BTreeMap::new();
    for (raw, mut file_edits) in by_file {
        let fid = FileId(raw);
        let (Some(path), Some(src)) = (session.file_path(fid), session.source(fid)) else {
            continue;
        };
        let mut s = src.to_owned();
        // Splice from the end so earlier offsets stay valid.
        file_edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in file_edits {
            let (start, end) = (usize::from(e.range.start()), usize::from(e.range.end()));
            if start <= end
                && end <= s.len()
                && s.is_char_boundary(start)
                && s.is_char_boundary(end)
            {
                s.replace_range(start..end, &e.new_text);
            }
        }
        overlay.insert(path.to_owned(), s);
    }

    let (new_analysis, new_db) = session.analyze_overlay(&overlay);
    introduced_diagnostics(analysis, &new_analysis, &new_db)
}

/// The overlay-based gate for ops whose primary edit is a whole-file source
/// replacement (not a set of byte-range [`FileEdit`]s). `primary_path` is
/// replaced wholesale with `new_source`; `cross_file_edits` overlay onto their
/// own files. Used by structural moves / delete, which rebuild the primary file
/// by text splicing and can't express that as a single byte-range edit.
#[must_use]
pub fn gate_with_source(
    session: &IdeSession,
    primary_path: &str,
    new_source: &str,
    cross_file_edits: &[FileEdit],
) -> Vec<IntroducedDiagnostic> {
    let Some(analysis) = session.analysis() else {
        return Vec::new();
    };

    let mut by_file: BTreeMap<u32, Vec<&FileEdit>> = BTreeMap::new();
    for e in cross_file_edits {
        by_file.entry(e.file.0).or_default().push(e);
    }
    let mut overlay: BTreeMap<String, String> = BTreeMap::new();
    overlay.insert(primary_path.to_owned(), new_source.to_owned());
    for (raw, mut file_edits) in by_file {
        let fid = FileId(raw);
        let (Some(path), Some(src)) = (session.file_path(fid), session.source(fid)) else {
            continue;
        };
        if path == primary_path {
            // The primary file's full source already overrides any byte edits.
            continue;
        }
        let mut s = src.to_owned();
        file_edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in file_edits {
            let (start, end) = (usize::from(e.range.start()), usize::from(e.range.end()));
            if start <= end
                && end <= s.len()
                && s.is_char_boundary(start)
                && s.is_char_boundary(end)
            {
                s.replace_range(start..end, &e.new_text);
            }
        }
        overlay.insert(path.to_owned(), s);
    }

    let (new_analysis, new_db) = session.analyze_overlay(&overlay);
    introduced_diagnostics(analysis, &new_analysis, &new_db)
}

/// Diff `new_analysis` against the baseline `analysis`, returning the
/// diagnostics that the edit introduced — present now but not before, matched
/// as a multiset keyed by `(code, message)` so duplicate messages are counted.
/// Locations resolve through `new_db` (the overlay db owns the new `FileId`s).
fn introduced_diagnostics(
    analysis: &AnalysisResult,
    new_analysis: &AnalysisResult,
    new_db: &brink_db::ProjectDb,
) -> Vec<IntroducedDiagnostic> {
    let mut baseline: BTreeMap<(&str, &str), i32> = BTreeMap::new();
    for d in &analysis.diagnostics {
        *baseline
            .entry((d.code.as_str(), d.message.as_str()))
            .or_default() += 1;
    }

    let mut introduced = Vec::new();
    for d in &new_analysis.diagnostics {
        let count = baseline
            .entry((d.code.as_str(), d.message.as_str()))
            .or_default();
        if *count > 0 {
            *count -= 1;
            continue;
        }
        let path = new_db.file_path(d.file).unwrap_or_default().to_owned();
        let src = new_db.source(d.file).unwrap_or_default();
        let (line, col) = LineIndex::new(src).line_col(d.range.start());
        introduced.push(IntroducedDiagnostic {
            severity: d.code.severity(),
            code: d.code,
            message: d.message.clone(),
            path,
            line: line + 1,
            col: col + 1,
        });
    }
    introduced
}
