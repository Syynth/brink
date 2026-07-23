//! LSP adapter helpers — self-contained free functions that convert
//! between brink's domain types and `tower_lsp` LSP types. No dependency
//! on `Backend`; extracted verbatim from `backend.rs` (#688).

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, MarkupContent, MarkupKind, Range, TextEdit,
};

use crate::convert::{self, LineIndex};

/// Convert a domain `DocumentSymbol` to an LSP `DocumentSymbol`.
#[expect(deprecated, reason = "DocumentSymbol requires deprecated fields")]
pub(crate) fn domain_symbol_to_lsp(
    sym: brink_ide::document::DocumentSymbol,
    idx: &LineIndex,
) -> tower_lsp::lsp_types::DocumentSymbol {
    let children: Vec<_> = sym
        .children
        .into_iter()
        .map(|c| domain_symbol_to_lsp(c, idx))
        .collect();

    tower_lsp::lsp_types::DocumentSymbol {
        name: sym.name,
        detail: sym.detail,
        kind: convert::symbol_kind_to_lsp(sym.kind),
        tags: None,
        deprecated: None,
        range: convert::to_lsp_range(sym.full_range, idx),
        selection_range: convert::to_lsp_range(sym.range, idx),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

/// Build a `CompletionItem` from a `SymbolInfo`.
pub(crate) fn make_completion_item(
    info: &brink_ir::SymbolInfo,
    label_override: Option<String>,
) -> CompletionItem {
    let kind = match info.kind {
        brink_ir::SymbolKind::Knot => CompletionItemKind::MODULE,
        brink_ir::SymbolKind::Stitch | brink_ir::SymbolKind::External => {
            CompletionItemKind::FUNCTION
        }
        brink_ir::SymbolKind::Variable
        | brink_ir::SymbolKind::Constant
        | brink_ir::SymbolKind::Param
        | brink_ir::SymbolKind::Temp => CompletionItemKind::VARIABLE,
        brink_ir::SymbolKind::List => CompletionItemKind::ENUM,
        brink_ir::SymbolKind::ListItem => CompletionItemKind::ENUM_MEMBER,
        brink_ir::SymbolKind::Label => CompletionItemKind::REFERENCE,
        brink_ir::SymbolKind::Struct => CompletionItemKind::STRUCT,
    };

    let detail = match info.kind {
        brink_ir::SymbolKind::Knot if info.detail.as_deref() == Some("function") => {
            Some("function knot".to_string())
        }
        _ if !info.params.is_empty() => {
            let params: Vec<_> = info.params.iter().map(|p| p.name.as_str()).collect();
            Some(format!("({})", params.join(", ")))
        }
        _ => None,
    };

    CompletionItem {
        label: label_override.unwrap_or_else(|| info.name.clone()),
        kind: Some(kind),
        detail,
        ..Default::default()
    }
}

/// Build a `CompletionItem` for a T1b stdlib slice 1 function
/// (docs/t1b-surface-spec.md §5, #589) — signature as `detail` (the
/// lvalue-mutator rule renders right there, e.g. `push(a: lvalue, v)`), the
/// one-line semantics as markdown documentation.
pub(crate) fn make_stdlib_completion_item(f: &brink_ide::stdlib::StdlibFn) -> CompletionItem {
    CompletionItem {
        label: f.name.to_owned(),
        kind: Some(CompletionItemKind::FUNCTION),
        detail: Some(f.signature_label()),
        documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
            MarkupContent {
                kind: MarkupKind::Markdown,
                value: f.doc.to_owned(),
            },
        )),
        ..Default::default()
    }
}

pub(crate) fn format_config_from_options(
    _options: &tower_lsp::lsp_types::FormattingOptions,
) -> brink_fmt::FormatConfig {
    brink_fmt::FormatConfig::default()
}

/// Convert `brink_ide::diff_to_edits` output to LSP `TextEdit`s.
pub(crate) fn diff_to_lsp_edits(old: &str, new: &str) -> Vec<TextEdit> {
    let idx = LineIndex::new(old);
    brink_ide::diff_to_edits(old, new)
        .into_iter()
        .map(|(range, new_text)| TextEdit {
            range: convert::to_lsp_range(range, &idx),
            new_text,
        })
        .collect()
}

/// Check whether two LSP ranges overlap.
pub(crate) fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
}
