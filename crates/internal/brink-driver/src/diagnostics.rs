//! Diagnostic collection, suppression, and partitioning.

use std::collections::HashMap;

use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_ir::{Diagnostic, FileId, Severity};

/// Partitioned diagnostics after suppression filtering.
pub struct DiagnosticReport {
    /// Diagnostics with `Severity::Error`.
    pub errors: Vec<Diagnostic>,
    /// Diagnostics with `Severity::Warning`.
    pub warnings: Vec<Diagnostic>,
}

/// Collect all diagnostics (lowering + analysis), apply suppressions, partition.
///
/// `entry`: if `Some`, checks its suppressions for `disable_all` (compiler mode).
///          if `None`, analysis diagnostics are always included (LSP mode).
pub fn collect_diagnostics(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    entry: Option<FileId>,
) -> DiagnosticReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check if the entry file has brink-disable-all
    let disable_all = entry
        .and_then(|id| db.suppressions(id))
        .is_some_and(|s| s.disable_all);

    // Per-file lowering diagnostics
    for id in db.file_ids() {
        let raw: Vec<Diagnostic> = db.file_diagnostics(id).unwrap_or_default().to_vec();
        let source = db.source(id).unwrap_or_default();
        let suppressions = db.suppressions(id).cloned().unwrap_or_default();
        let filtered = brink_ir::suppressions::apply_suppressions(id, source, raw, &suppressions);
        for d in filtered {
            if d.code.severity() == Severity::Error {
                errors.push(d);
            } else {
                warnings.push(d);
            }
        }
    }

    // Analysis diagnostics (unless disable_all)
    if !disable_all {
        let mut by_file: HashMap<FileId, Vec<Diagnostic>> = HashMap::new();
        for d in &analysis.diagnostics {
            by_file.entry(d.file).or_default().push(d.clone());
        }
        // Sort by FileId for determinism
        let mut file_ids: Vec<_> = by_file.keys().copied().collect();
        file_ids.sort_by_key(|id| id.0);
        for fid in file_ids {
            let diags = by_file.remove(&fid).unwrap_or_default();
            let source = db.source(fid).unwrap_or_default();
            let suppressions = db.suppressions(fid).cloned().unwrap_or_default();
            let filtered =
                brink_ir::suppressions::apply_suppressions(fid, source, diags, &suppressions);
            for d in filtered {
                if d.code.severity() == Severity::Error {
                    errors.push(d);
                } else {
                    warnings.push(d);
                }
            }
        }
    }

    DiagnosticReport { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_analyzer::AnalysisResult;
    use brink_db::ProjectDb;

    fn empty_analysis() -> AnalysisResult {
        AnalysisResult {
            index: brink_ir::SymbolIndex::default(),
            resolutions: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn empty_db_returns_empty_report() {
        let db = ProjectDb::new();
        let analysis = empty_analysis();
        let report = collect_diagnostics(&db, &analysis, None);
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn lowering_errors_partitioned_correctly() {
        let mut db = ProjectDb::new();
        // A file with a parse error (missing knot name)
        db.set_file("test.ink", "=== \nHello\n".to_string());
        let analysis = empty_analysis();
        let entry = db.file_id("test.ink");
        let report = collect_diagnostics(&db, &analysis, entry);
        // The missing knot name should produce an error
        assert!(!report.errors.is_empty());
    }

    fn run_analysis(db: &ProjectDb) -> AnalysisResult {
        let inputs = db.analysis_inputs();
        let file_refs: Vec<_> = inputs
            .iter()
            .map(|(id, hir, manifest)| (*id, hir, manifest))
            .collect();
        brink_analyzer::analyze(&file_refs)
    }

    #[test]
    fn analysis_diagnostics_included_when_no_disable_all() {
        let mut db = ProjectDb::new();
        // A file with an unresolved divert target (will produce analysis diagnostic)
        db.set_file("test.ink", "-> missing_knot\n".to_string());
        let analysis_result = run_analysis(&db);
        let entry = db.file_id("test.ink");
        let report = collect_diagnostics(&db, &analysis_result, entry);
        // Should have the unresolved divert as an error
        let total = report.errors.len() + report.warnings.len();
        assert!(total > 0);
    }

    #[test]
    fn disable_all_skips_analysis_diagnostics() {
        let mut db = ProjectDb::new();
        // brink-disable-all suppresses analysis diagnostics
        db.set_file(
            "test.ink",
            "// brink-disable-all\n-> missing_knot\n".to_string(),
        );
        let analysis_result = run_analysis(&db);
        let entry = db.file_id("test.ink");
        let report = collect_diagnostics(&db, &analysis_result, entry);
        // Analysis diagnostics should be skipped; only lowering diagnostics remain
        // The lowering diag for the unresolved divert is a lowering error, not analysis
        // So we just verify no analysis-level diagnostics leaked through
        let analysis_diag_count = analysis_result.diagnostics.len();
        // With disable_all, analysis diagnostics should not appear in the report
        let report_total = report.errors.len() + report.warnings.len();
        // The report total should be less than if we included analysis diagnostics
        // (unless there are no analysis diagnostics at all)
        if analysis_diag_count > 0 {
            let report_without_disable = collect_diagnostics(&db, &analysis_result, None);
            let without_total =
                report_without_disable.errors.len() + report_without_disable.warnings.len();
            assert!(report_total < without_total);
        }
    }
}
