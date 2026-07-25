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
    let options = session.analysis_options();
    introduced_diagnostics(
        analysis,
        &new_analysis,
        &new_db,
        options.type_policy(),
        &options.lints,
    )
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
    let options = session.analysis_options();
    introduced_diagnostics(
        analysis,
        &new_analysis,
        &new_db,
        options.type_policy(),
        &options.lints,
    )
}

/// Diff `new_analysis` against the baseline `analysis`, returning the
/// diagnostics that the edit introduced — present now but not before, matched
/// as a multiset keyed by `(code, message)` so duplicate messages are counted.
/// Locations resolve through `new_db` (the overlay db owns the new `FileId`s).
///
/// `types` is the session's resolved TM-3 policy and `lints` its resolved
/// `[lints]` policy (both from [`IdeSession::analysis_options`]) — `severity`
/// renders the [`brink_analyzer::effective_severity`] (issue #1367), not the
/// raw [`DiagnosticCode::severity`] default. Every caller currently passes
/// `IdeSession::analysis_options().lints`, which is always
/// `LintPolicy::default()`: `IdeSession` has no `[lints]`-resolution input
/// wired yet (same #1160 scope note as [`IdeSession::analysis_options`]), so
/// this is a no-op today — but taking `lints` as a parameter (rather than
/// manufacturing a fresh `LintPolicy::default()` in here) means a future
/// `[lints]` source wired into `IdeSession` (#1366) flows through
/// automatically instead of silently going stale one layer down from the
/// seam this function exists to keep live.
pub(crate) fn introduced_diagnostics(
    analysis: &AnalysisResult,
    new_analysis: &AnalysisResult,
    new_db: &brink_db::ProjectDb,
    types: brink_analyzer::TypePolicy,
    lints: &brink_analyzer::LintPolicy,
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
            severity: brink_analyzer::effective_severity(d.code, types, lints),
            code: d.code,
            message: d.message.clone(),
            path,
            line: line + 1,
            col: col + 1,
        });
    }
    introduced
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_analyzer::TypePolicy;
    use brink_ir::{Diagnostic, SymbolIndex};
    use rowan::{TextRange, TextSize};
    use std::sync::Arc;

    fn empty_analysis() -> AnalysisResult {
        AnalysisResult {
            index: Arc::new(SymbolIndex::default()),
            resolutions: Vec::new(),
            diagnostics: Vec::new(),
            symbol_meta: BTreeMap::new(),
        }
    }

    fn analysis_with(diagnostics: Vec<Diagnostic>) -> AnalysisResult {
        AnalysisResult {
            diagnostics,
            ..empty_analysis()
        }
    }

    /// #1367: `introduced_diagnostics` must render the *effective* severity,
    /// not the raw `DiagnosticCode::severity()` default. `E063` (annotation-
    /// vs-inference mismatch) is `Warning` by default but `Error` under
    /// `types = strict` (`brink-analyzer::strict`'s #640-round ruling) — the
    /// one TM-3 carve-out `effective_severity` applies regardless of
    /// `[lints]`, so it's reachable even though `IdeSession` has no
    /// `[lints]`-resolution input wired yet (see this function's doc
    /// comment).
    #[test]
    fn reports_effective_severity_not_raw_default() {
        assert_eq!(
            DiagnosticCode::E063.severity(),
            Severity::Warning,
            "precondition: E063's raw default is Warning"
        );

        let baseline = empty_analysis();
        let new_analysis = analysis_with(vec![Diagnostic {
            file: FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "type annotation disagrees with inferred type".to_owned(),
            code: DiagnosticCode::E063,
        }]);
        let db = brink_db::ProjectDb::new();
        let lints = brink_analyzer::LintPolicy::default();

        let gradual =
            introduced_diagnostics(&baseline, &new_analysis, &db, TypePolicy::Gradual, &lints);
        assert_eq!(
            gradual.first().map(|d| d.severity),
            Some(Severity::Warning),
            "types = gradual: E063 stays at its raw Warning default"
        );

        let strict =
            introduced_diagnostics(&baseline, &new_analysis, &db, TypePolicy::Strict, &lints);
        assert_eq!(
            strict.first().map(|d| d.severity),
            Some(Severity::Error),
            "types = strict: E063 must promote to Error via effective_severity, \
             not stay at the raw Warning default"
        );
    }
}
