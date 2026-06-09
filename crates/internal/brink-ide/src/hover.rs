use std::fmt::Write as _;

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

        // Host-manifest enrichment: typed params / return / kind / doc for externals.
        let meta = if info.kind == brink_ir::SymbolKind::External {
            analysis.external_meta.get(&info.id)
        } else {
            None
        };

        let params_str = if info.params.is_empty() {
            String::new()
        } else {
            let parts: Vec<_> = info
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut s = String::new();
                    if p.is_ref {
                        s.push_str("ref ");
                    }
                    if p.is_divert {
                        s.push_str("-> ");
                    }
                    s.push_str(&p.name);
                    if let Some(ty) = meta
                        .and_then(|m| m.params.get(i))
                        .and_then(|rp| rp.ty.as_ref())
                    {
                        let _ = write!(s, ": {}", ty.name);
                    }
                    s
                })
                .collect();
            format!("({})", parts.join(", "))
        };

        let ret_str = meta
            .and_then(|m| m.returns.as_ref())
            .map_or(String::new(), |t| format!(" -> {}", t.name));

        let kind_tag = meta.map_or(String::new(), |m| match m.kind {
            brink_ir::ExternalKind::Plain => String::new(),
            brink_ir::ExternalKind::Query => " [query]".to_string(),
            brink_ir::ExternalKind::Effect => " [effect]".to_string(),
            brink_ir::ExternalKind::Presentation => " [presentation]".to_string(),
        });

        let detail_str = info
            .detail
            .as_deref()
            .map_or(String::new(), |d| format!(" [{d}]"));

        let doc_block = meta
            .and_then(|m| m.doc.as_deref())
            .map_or(String::new(), |d| format!("\n\n{d}"));

        let file_note = project_files
            .iter()
            .find(|(fid, _, _)| *fid == info.file)
            .map_or(String::new(), |(_, p, _)| format!("\n\n*Defined in `{p}`*"));

        format!(
            "**{kind_str}** `{}{params_str}{ret_str}`{detail_str}{kind_tag}{doc_block}{file_note}",
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

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use super::hover;
    use crate::session::IdeSession;

    #[test]
    fn hover_shows_inline_types_kind_and_doc() {
        let src = "/// Whether the player holds an item.\n/// @param item {bool}\n/// @returns {bool}\n/// @kind query\nEXTERNAL holds(item)\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let pos = u32::try_from(src.find("holds(item)").expect("decl present")).expect("offset");
        let info = hover(analysis, file_id, src, TextSize::from(pos), &[]).expect("hover");
        assert!(info.content.contains("item: bool"), "{}", info.content);
        assert!(info.content.contains("-> bool"), "{}", info.content);
        assert!(info.content.contains("[query]"), "{}", info.content);
        assert!(
            info.content.contains("Whether the player holds an item."),
            "{}",
            info.content
        );
    }
}
