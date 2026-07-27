use rowan::{TextRange, TextSize};
use tower_lsp::lsp_types;

pub use brink_ide::LineIndex;

pub fn to_lsp_range(range: TextRange, idx: &LineIndex) -> lsp_types::Range {
    let (start_line, start_col) = idx.line_col(range.start());
    let (end_line, end_col) = idx.line_col(range.end());
    lsp_types::Range {
        start: lsp_types::Position::new(start_line, start_col),
        end: lsp_types::Position::new(end_line, end_col),
    }
}

pub fn to_text_size(pos: lsp_types::Position, idx: &LineIndex) -> TextSize {
    idx.offset(pos.line, pos.character)
}

pub fn symbol_kind_to_lsp(kind: brink_ir::SymbolKind) -> lsp_types::SymbolKind {
    match kind {
        brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::External => {
            lsp_types::SymbolKind::FUNCTION
        }
        brink_ir::SymbolKind::Stitch => lsp_types::SymbolKind::METHOD,
        brink_ir::SymbolKind::Variable
        | brink_ir::SymbolKind::Param
        | brink_ir::SymbolKind::Temp => lsp_types::SymbolKind::VARIABLE,
        brink_ir::SymbolKind::Constant => lsp_types::SymbolKind::CONSTANT,
        brink_ir::SymbolKind::List => lsp_types::SymbolKind::ENUM,
        brink_ir::SymbolKind::ListItem => lsp_types::SymbolKind::ENUM_MEMBER,
        brink_ir::SymbolKind::Label => lsp_types::SymbolKind::KEY,
        brink_ir::SymbolKind::Struct => lsp_types::SymbolKind::STRUCT,
    }
}

/// `brink_ir::Severity::Info`/`Hint` map to LSP's `INFORMATION`/`HINT`
/// respectively (issue #1162) — the LSP spec keeps these as two distinct
/// severities (`textDocument/publishDiagnostics`'s `DiagnosticSeverity`), so
/// this maps both explicitly rather than collapsing them onto one.
pub fn severity_to_lsp(sev: brink_ir::Severity) -> lsp_types::DiagnosticSeverity {
    match sev {
        brink_ir::Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
        brink_ir::Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
        brink_ir::Severity::Info => lsp_types::DiagnosticSeverity::INFORMATION,
        brink_ir::Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
    }
}

/// `types`/`lints` are the resolved [`brink_analyzer::TypePolicy`]/
/// [`brink_analyzer::LintPolicy`] the diagnostic was produced under —
/// `severity` publishes [`brink_analyzer::effective_severity`], not the raw
/// [`brink_ir::DiagnosticCode::severity`] default, so a `[lints]` re-leveled
/// code shows at its overridden severity in the client (issue #1367).
pub fn diagnostic_to_lsp(
    diag: &brink_ir::Diagnostic,
    idx: &LineIndex,
    types: brink_analyzer::TypePolicy,
    lints: &brink_analyzer::LintPolicy,
) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: to_lsp_range(diag.range, idx),
        severity: Some(severity_to_lsp(brink_analyzer::effective_severity(
            diag.code, types, lints,
        ))),
        code: Some(lsp_types::NumberOrString::String(
            diag.code.as_str().to_owned(),
        )),
        source: Some("brink".to_owned()),
        message: diag.message.clone(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_lsp_range_roundtrip() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src);
        let range = TextRange::new(TextSize::from(4), TextSize::from(7));
        let lsp = to_lsp_range(range, &idx);
        assert_eq!(lsp.start, lsp_types::Position::new(1, 0));
        assert_eq!(lsp.end, lsp_types::Position::new(1, 3));

        let start = to_text_size(lsp.start, &idx);
        let end = to_text_size(lsp.end, &idx);
        assert_eq!(start, TextSize::from(4));
        assert_eq!(end, TextSize::from(7));
    }

    #[test]
    fn symbol_kind_mapping() {
        assert_eq!(
            symbol_kind_to_lsp(brink_ir::SymbolKind::Knot),
            lsp_types::SymbolKind::FUNCTION,
        );
        assert_eq!(
            symbol_kind_to_lsp(brink_ir::SymbolKind::List),
            lsp_types::SymbolKind::ENUM,
        );
        assert_eq!(
            symbol_kind_to_lsp(brink_ir::SymbolKind::Label),
            lsp_types::SymbolKind::KEY,
        );
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Error),
            lsp_types::DiagnosticSeverity::ERROR,
        );
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Warning),
            lsp_types::DiagnosticSeverity::WARNING,
        );
    }

    /// #1162: `Info`/`Hint` must map to LSP's `INFORMATION`/`HINT`
    /// respectively, not collapse onto `WARNING` or onto each other.
    #[test]
    fn severity_mapping_info_and_hint_are_distinct() {
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Info),
            lsp_types::DiagnosticSeverity::INFORMATION,
        );
        assert_eq!(
            severity_to_lsp(brink_ir::Severity::Hint),
            lsp_types::DiagnosticSeverity::HINT,
        );
    }

    /// #1163 regression: a `DiagnosticCode` whose default severity is
    /// `Warning` (E014 is one of the 17 warning-default codes) must surface
    /// as `DiagnosticSeverity::WARNING`, not `ERROR`, once routed through
    /// `diagnostic_to_lsp` with no `[lints]` override in play.
    #[test]
    fn diagnostic_to_lsp_respects_warning_default_code() {
        assert_eq!(
            brink_ir::DiagnosticCode::E014.severity(),
            brink_ir::Severity::Warning
        );

        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let lsp = diagnostic_to_lsp(
            &diag,
            &idx,
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::WARNING));
    }

    /// #1367: a `[lints] E014 = "deny"` override must publish as
    /// `ERROR`, not the code's raw `Warning` default — `diagnostic_to_lsp`
    /// must route severity through `effective_severity`, not
    /// `diag.code.severity()`.
    #[test]
    fn diagnostic_to_lsp_respects_lints_override() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let mut lints = brink_analyzer::LintPolicy::default();
        lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Deny);
        let lsp = diagnostic_to_lsp(&diag, &idx, brink_analyzer::TypePolicy::Gradual, &lints);
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    }

    /// #1162: a `[lints] E014 = "hint"` override must publish `HINT` through
    /// the same `effective_severity` seam `diagnostic_to_lsp_respects_lints_override`
    /// exercises for `deny`.
    #[test]
    fn diagnostic_to_lsp_respects_lints_hint_override() {
        let diag = brink_ir::Diagnostic {
            file: brink_ir::FileId(0),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            message: "test".to_owned(),
            code: brink_ir::DiagnosticCode::E014,
        };
        let idx = LineIndex::new("x");
        let mut lints = brink_analyzer::LintPolicy::default();
        lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Hint);
        let lsp = diagnostic_to_lsp(&diag, &idx, brink_analyzer::TypePolicy::Gradual, &lints);
        assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::HINT));
    }
}
