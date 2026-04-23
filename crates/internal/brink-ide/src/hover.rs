use brink_analyzer::AnalysisResult;
use brink_ir::FileId;
use rowan::{TextRange, TextSize};

use crate::navigation::find_def_at_offset;
use crate::{builtin_hover_text, word_at_offset, word_range_at_offset};

/// Hover information for a symbol.
pub struct HoverInfo {
    /// Markdown-formatted content.
    pub content: String,
    /// The range of the hovered symbol.
    pub range: Option<TextRange>,
}

/// Compute hover info for the symbol at `offset`.
///
/// `project_files` provides `(FileId, path, source)` tuples for cross-file
/// definition lookup (e.g. showing "Defined in `path`").
pub fn hover(
    analysis: &AnalysisResult,
    file_id: FileId,
    source: &str,
    offset: TextSize,
    project_files: &[(FileId, String, String)],
) -> Option<HoverInfo> {
    let content = if let Some(info) = find_def_at_offset(analysis, file_id, offset) {
        let kind_str = match info.kind {
            brink_ir::SymbolKind::Knot => "knot",
            brink_ir::SymbolKind::Stitch => "stitch",
            brink_ir::SymbolKind::Variable => "variable",
            brink_ir::SymbolKind::Constant => "constant",
            brink_ir::SymbolKind::List => "list",
            brink_ir::SymbolKind::ListItem => "list item",
            brink_ir::SymbolKind::External => "external function",
            brink_ir::SymbolKind::Label => "label",
            brink_ir::SymbolKind::Param => "parameter",
            brink_ir::SymbolKind::Temp => "temp variable",
        };

        let params_str = if info.params.is_empty() {
            String::new()
        } else {
            let parts: Vec<_> = info
                .params
                .iter()
                .map(|p| {
                    let mut s = String::new();
                    if p.is_ref {
                        s.push_str("ref ");
                    }
                    if p.is_divert {
                        s.push_str("-> ");
                    }
                    s.push_str(&p.name);
                    s
                })
                .collect();
            format!("({})", parts.join(", "))
        };

        let detail_str = info
            .detail
            .as_deref()
            .map_or(String::new(), |d| format!(" [{d}]"));

        let file_note = project_files
            .iter()
            .find(|(fid, _, _)| *fid == info.file)
            .map_or(String::new(), |(_, p, _)| format!("\n\n*Defined in `{p}`*"));

        format!(
            "**{kind_str}** `{}{params_str}`{detail_str}{file_note}",
            info.name
        )
    } else {
        word_at_offset(source, offset).and_then(builtin_hover_text)?
    };

    let range = analysis
        .resolutions
        .iter()
        .find(|r| r.file == file_id && (r.range.contains(offset) || r.range.start() == offset))
        .map(|r| r.range)
        .or_else(|| word_range_at_offset(source, offset));

    Some(HoverInfo { content, range })
}
