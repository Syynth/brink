use std::fmt::Write as _;

use brink_analyzer::AnalysisResult;
use brink_ir::FileId;
use rowan::{TextRange, TextSize};

use crate::navigation::find_def_at_offset;
use crate::{builtin_hover_text, stdlib_hover_text, word_at_offset, word_range_at_offset};

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
            brink_ir::SymbolKind::Struct => "struct",
        };

        // Symbol-metadata enrichment: docs and typed params/returns for
        // externals, knots, and stitches; initializer info for VAR/CONST.
        let meta = analysis.symbol_meta.get(&info.id);

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

        // Initializer info: `health: int`, `SPEED: float = 0.5`.
        let value_str = meta
            .and_then(|m| m.value.as_ref())
            .map_or(String::new(), |v| {
                let mut s = String::new();
                if let Some(ty) = v.ty {
                    let _ = write!(s, ": {}", ty.name());
                }
                if let Some(text) = &v.value_text {
                    let _ = write!(s, " = {text}");
                }
                s
            });

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
            "**{kind_str}** `{}{value_str}{params_str}{ret_str}`{detail_str}{kind_tag}{doc_block}{file_note}",
            info.name
        )
    } else {
        let word = word_at_offset(source, offset)?;
        builtin_hover_text(word).or_else(|| stdlib_hover_text(word))?
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

    /// Hover content for the first occurrence of `needle` in `src`.
    fn hover_at(src: &str, needle: &str) -> String {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");
        hover(analysis, file_id, src, TextSize::from(pos), &[])
            .expect("hover")
            .content
    }

    #[test]
    fn hover_shows_function_knot_doc_and_types() {
        let src = "\
/// Damage roll for an attack.
/// @param weapon {int}
/// @returns {int}
== function damage(weapon) ==
~ return weapon
";
        let content = hover_at(src, "damage(weapon)");
        assert!(content.contains("**knot**"), "{content}");
        assert!(content.contains("weapon: int"), "{content}");
        assert!(content.contains("-> int"), "{content}");
        assert!(content.contains("[function]"), "{content}");
        assert!(content.contains("Damage roll for an attack."), "{content}");
    }

    #[test]
    fn hover_shows_var_inferred_type_and_doc() {
        let src = "/// Player health.\nVAR health = 100\n-> END\n";
        let content = hover_at(src, "health = 100");
        assert!(content.contains("`health: int`"), "{content}");
        assert!(content.contains("Player health."), "{content}");
        assert!(
            !content.contains(" = 100"),
            "VARs don't show values: {content}"
        );
    }

    #[test]
    fn hover_shows_const_type_and_value() {
        let src = "CONST SPEED = 0.5\n-> END\n";
        let content = hover_at(src, "SPEED");
        assert!(content.contains("`SPEED: float = 0.5`"), "{content}");
    }

    #[test]
    fn hover_shows_stitch_doc() {
        let src = "\
== hub ==
intro
/// The market square.
= market
stalls
";
        let content = hover_at(src, "market\n");
        assert!(content.contains("**stitch**"), "{content}");
        assert!(content.contains("The market square."), "{content}");
    }

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

    // ── Stdlib slice 1 hover (#589) ─────────────────────────────────────

    #[test]
    fn hover_shows_stdlib_pure_function_signature_and_semantics() {
        let src = "~ temp n = len(inventory)\n-> END\n";
        let content = hover_at(src, "len(inventory)");
        assert!(content.contains("**brink stdlib**"), "{content}");
        assert!(content.contains("len(x) -> int"), "{content}");
        assert!(content.contains("keys in a map"), "{content}");
    }

    #[test]
    fn hover_shows_stdlib_mutator_with_lvalue_signature() {
        let src = "~ push(inventory, \"sword\")\n-> END\n";
        let content = hover_at(src, "push(inventory");
        assert!(
            content.contains("push(a: lvalue, v)"),
            "shows the lvalue-mutator signature: {content}"
        );
        assert!(content.contains("mutates its first argument"), "{content}");
    }

    #[test]
    fn hover_stdlib_name_is_available_even_when_unresolved() {
        // No `inventory` symbol declared at all — hover on `contains` must
        // still explain the stdlib function rather than falling through to
        // nothing, mirroring `builtin_hover_text`'s unconditional shape.
        let src = "~ temp ok = contains(items, 1)\n-> END\n";
        let content = hover_at(src, "contains(items");
        assert!(content.contains("contains(x, v) -> bool"), "{content}");
        assert!(
            content.contains("totality") || content.contains("total:"),
            "{content}"
        );
    }

    #[test]
    fn hover_non_stdlib_word_has_no_stdlib_content() {
        let src = "~ temp x = 1\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let pos = u32::try_from(src.find("temp x").expect("present") + 5).expect("offset");
        let info = hover(analysis, file_id, src, TextSize::from(pos), &[]);
        assert!(
            info.is_none() || !info.expect("checked").content.contains("brink stdlib"),
            "`x` is not a stdlib name"
        );
    }
}
