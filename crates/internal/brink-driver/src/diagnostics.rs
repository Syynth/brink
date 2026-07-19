//! Diagnostic collection, suppression, and partitioning.

use brink_analyzer::AnalysisResult;
use brink_db::{FileDiagnostics, ProjectDb, partition_diagnostics};
use brink_ir::{Diagnostic, FileId};

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
///
/// The partitioning core is shared with the db's `lir` query
/// ([`brink_db::partition_diagnostics`]) so the two paths cannot drift.
pub fn collect_diagnostics(
    db: &ProjectDb,
    analysis: &AnalysisResult,
    entry: Option<FileId>,
) -> DiagnosticReport {
    // Check if the entry file has brink-disable-all
    let disable_all = entry
        .and_then(|id| db.suppressions(id))
        .is_some_and(|s| s.disable_all);

    let inputs: Vec<FileDiagnostics<'_>> = db
        .file_ids()
        .filter_map(|id| {
            Some(FileDiagnostics {
                file: id,
                source: db.source(id)?,
                suppressions: db.suppressions(id)?,
                lowering: db.file_diagnostics(id)?,
            })
        })
        .collect();

    let types = db.analysis_options().type_policy();
    let (errors, warnings) =
        partition_diagnostics(&inputs, &analysis.diagnostics, disable_all, types);
    DiagnosticReport { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_analyzer::AnalysisResult;
    use brink_db::ProjectDb;

    fn empty_analysis() -> AnalysisResult {
        AnalysisResult {
            index: std::sync::Arc::new(brink_ir::SymbolIndex::default()),
            resolutions: Vec::new(),
            diagnostics: Vec::new(),
            symbol_meta: std::collections::BTreeMap::new(),
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

    /// Regression test for #43: a diagnostic originating in an included
    /// (non-entry) file must be attributed to *that* file, not collapsed onto
    /// the entry file. The studio currently shows every included-file error on
    /// the entry (`main.ink`), which makes multi-file errors unlocatable.
    #[test]
    fn diagnostic_from_included_file_carries_its_file_id() {
        let mut db = ProjectDb::new();
        db.set_file("main.ink", "INCLUDE helper.ink\n-> top\n".to_string());
        db.set_file("helper.ink", "=== top ===\n-> does_not_exist\n".to_string());
        let analysis = run_analysis(&db);
        let entry = db.file_id("main.ink");
        let helper = db
            .file_id("helper.ink")
            .expect("helper.ink should have a FileId");
        let report = collect_diagnostics(&db, &analysis, entry);

        let all: Vec<_> = report.errors.iter().chain(report.warnings.iter()).collect();
        assert!(
            !all.is_empty(),
            "the unresolved divert in helper.ink should produce a diagnostic"
        );
        // The error is wholly within helper.ink, so every diagnostic it produces
        // must be attributed to helper.ink — not the entry file.
        for d in &all {
            assert_eq!(
                d.file, helper,
                "diagnostic `{}` for an error inside helper.ink should carry \
                 helper.ink's FileId ({:?}), not the entry's ({:?})",
                d.message, helper, entry
            );
        }
    }

    /// Regression test for #187 (secondary): an *analysis* diagnostic (E033,
    /// unreachable code) originating in an included file must be attributed to
    /// that file, not the entry. The original report saw such warnings collapsed
    /// onto `main.ink` at an offset past its EOF. This guards the analysis path
    /// specifically — #43 only covered lowering diagnostics.
    #[test]
    fn analysis_diagnostic_from_included_file_carries_its_file_id() {
        let mut db = ProjectDb::new();
        db.set_file("main.ink", "INCLUDE helper.ink\n-> top\n".to_string());
        // `-> END` is terminal; the following content is unreachable → E033.
        db.set_file(
            "helper.ink",
            "=== top ===\n-> END\nunreachable line\n".to_string(),
        );
        let analysis = run_analysis(&db);
        let entry = db.file_id("main.ink");
        let helper = db
            .file_id("helper.ink")
            .expect("helper.ink should have a FileId");
        let report = collect_diagnostics(&db, &analysis, entry);

        let e033s: Vec<_> = report
            .errors
            .iter()
            .chain(report.warnings.iter())
            .filter(|d| d.code == brink_ir::DiagnosticCode::E033)
            .collect();
        assert!(
            !e033s.is_empty(),
            "the unreachable line in helper.ink should produce an E033"
        );
        for d in &e033s {
            assert_eq!(
                d.file, helper,
                "E033 for unreachable code inside helper.ink should carry \
                 helper.ink's FileId ({helper:?}), not the entry's ({entry:?})"
            );
        }
    }
}
