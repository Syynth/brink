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
    }
}

pub fn severity_to_lsp(sev: brink_ir::Severity) -> lsp_types::DiagnosticSeverity {
    match sev {
        brink_ir::Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
        brink_ir::Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
    }
}

pub fn diagnostic_to_lsp(diag: &brink_ir::Diagnostic, idx: &LineIndex) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: to_lsp_range(diag.range, idx),
        severity: Some(severity_to_lsp(diag.code.severity())),
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
}
