use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::compile::DiagnosticJs;
use crate::editor::byte_to_utf16;

// ── Serialization types ─────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct ProjectFileJs {
    pub(crate) path: String,
}

/// Change spec returned by `update_document`, describing what actually
/// changed in the underlying file in UTF-16 **file** coordinates. `[start,
/// end)` is the replaced range of the file's previous content. The inserted
/// text is the caller's `source` argument, except when `text` is present —
/// then a fragment splice appended a `\n` separator and `text` carries the
/// actually-inserted text. Consumed by sibling editor views to live-mirror
/// the change as a CM6 change spec.
#[derive(Serialize)]
pub(crate) struct ChangeSpecJs {
    pub(crate) path: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct IncludeInfoJs {
    pub(crate) path: String,
    pub(crate) resolved: String,
    pub(crate) loaded: bool,
}

#[derive(Serialize)]
pub(crate) struct FileOutlineJs {
    pub(crate) path: String,
    pub(crate) symbols: Vec<DocumentSymbolJs>,
}

/// Whole-project story graph (spec §4.1) — mirrored as `StoryGraph` in
/// `@brink/wasm-types`.
#[derive(Serialize)]
pub(crate) struct StoryGraphJs {
    pub(crate) nodes: Vec<StoryGraphNodeJs>,
    pub(crate) edges: Vec<StoryGraphEdgeJs>,
}

/// A story-graph node. `file`/`start`/`end` are absent on the `END`/`DONE`
/// pseudo-nodes; `start`/`end` are UTF-16 offsets of the declaration name in
/// `file`. `parent` is the owning knot's id, present on stitches.
#[derive(Serialize)]
pub(crate) struct StoryGraphNodeJs {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<String>,
}

/// A story-graph edge. `occurrences` lists the divert sites that produced
/// it (aggregated edges keep one entry per site); omitted when empty.
#[derive(Serialize)]
pub(crate) struct StoryGraphEdgeJs {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) occurrences: Vec<StoryGraphEdgeOccurrenceJs>,
}

/// A source site of a story-graph edge: the target path's span (or the
/// whole divert statement for `-> DONE`/`-> END`), as UTF-16 offsets in
/// `file` — the same convention as node spans.
#[derive(Serialize)]
pub(crate) struct StoryGraphEdgeOccurrenceJs {
    pub(crate) file: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Serialize)]
pub(crate) struct CompletionItemJs {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) detail: Option<String>,
    /// Literal to insert when the display label differs from it — host value
    /// picker (#174): show `HarborGate`, insert `5`. `None` ⇒ insert `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) insert: Option<String>,
    /// `true` when the symbol is defined in a file NOT reachable from the
    /// current file's INCLUDE graph (#312 F). The editor tags such rows with a
    /// "from <file>" affordance and, on accept, auto-inserts the INCLUDE.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) out_of_scope: bool,
    /// The project-relative path of the file that declares this symbol, set
    /// only for out-of-scope completions — the auto-import target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_file: Option<String>,
}

/// Relative-path distance from `current` to `target` (#312 F). Lower is nearer:
/// primarily the number of `..` hops out of the current directory, then total
/// segment count, then the path string for a deterministic final tie-break.
pub(crate) fn include_distance(current: &str, target: &str) -> (usize, usize, String) {
    let rel = brink_db::compute_relative_path(current, target);
    let dotdots = rel.split('/').filter(|s| *s == "..").count();
    let segments = rel.split('/').count();
    (dotdots, segments, rel)
}

/// Collapse multiple out-of-scope definitions of one name down to the single
/// nearest one (#312 F): when a name is offered from several not-yet-reachable
/// files, keep only the closest by [`include_distance`] so the auto-import
/// targets one deterministic file. If a name also has an in-scope definition
/// (already reachable), its out-of-scope variants are dropped entirely — the
/// in-scope row inserts with no INCLUDE. Order is otherwise preserved for
/// in-scope items; the surviving out-of-scope items are stably ordered.
pub(crate) fn dedupe_out_of_scope(
    current: &str,
    items: Vec<CompletionItemJs>,
) -> Vec<CompletionItemJs> {
    use std::collections::HashSet;

    // Names that have at least one in-scope definition — their out-of-scope
    // duplicates are redundant.
    let in_scope_names: HashSet<String> = items
        .iter()
        .filter(|i| !i.out_of_scope)
        .map(|i| i.name.clone())
        .collect();

    // For each out-of-scope name, remember the index of the nearest variant.
    let mut best: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        if !item.out_of_scope || in_scope_names.contains(item.name.as_str()) {
            continue;
        }
        let dist = item
            .source_file
            .as_deref()
            .map(|f| include_distance(current, f));
        let is_better = match best.get(&item.name) {
            None => true,
            Some(&prev) => {
                let prev_dist = items[prev]
                    .source_file
                    .as_deref()
                    .map(|f| include_distance(current, f));
                dist < prev_dist
            }
        };
        if is_better {
            best.insert(item.name.clone(), idx);
        }
    }

    items
        .into_iter()
        .enumerate()
        .filter(|(idx, item)| {
            if !item.out_of_scope {
                return true;
            }
            if in_scope_names.contains(item.name.as_str()) {
                return false;
            }
            best.get(&item.name) == Some(idx)
        })
        .map(|(_, item)| item)
        .collect()
}

/// Build a typed signature detail for a callable (external, knot, stitch)
/// from its symbol metadata, e.g. `(item: bool) -> bool [query]`. `None` when
/// the symbol has no type-bearing metadata, so plain symbols keep their
/// kind-derived detail (e.g. `function`).
pub(crate) fn typed_detail(
    analysis: &brink_analyzer::AnalysisResult,
    info: &brink_ir::SymbolInfo,
) -> Option<String> {
    if !matches!(
        info.kind,
        brink_ir::SymbolKind::External | brink_ir::SymbolKind::Knot | brink_ir::SymbolKind::Stitch
    ) {
        return None;
    }
    let meta = analysis.symbol_meta.get(&info.id)?;
    let has_types = meta.params.iter().any(|p| p.ty.is_some())
        || meta.returns.is_some()
        || meta.kind != brink_ir::ExternalKind::Plain;
    if !has_types {
        return None;
    }
    let params = meta
        .params
        .iter()
        .map(|p| match &p.ty {
            Some(ty) => format!("{}: {}", p.name, ty.name),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = meta
        .returns
        .as_ref()
        .map_or(String::new(), |t| format!(" -> {}", t.name));
    let kind = match meta.kind {
        brink_ir::ExternalKind::Plain => "",
        brink_ir::ExternalKind::Query => " [query]",
        brink_ir::ExternalKind::Effect => " [effect]",
        brink_ir::ExternalKind::Presentation => " [presentation]",
    };
    Some(format!("({params}){ret}{kind}"))
}

#[derive(Serialize)]
pub(crate) struct HoverInfoJs {
    pub(crate) content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct LocationJs {
    pub(crate) file: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Serialize)]
pub(crate) struct InlayHintJs {
    pub(crate) offset: u32,
    pub(crate) label: String,
    pub(crate) kind: String,
    pub(crate) padding_right: bool,
}

#[derive(Serialize)]
pub(crate) struct ColorHintJs {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) value: String,
}

#[derive(Serialize)]
pub(crate) struct CallWidgetSiteJs {
    pub(crate) callee: String,
    pub(crate) name_start: u32,
    pub(crate) name_end: u32,
    pub(crate) slots: Vec<SlotWidgetJs>,
    pub(crate) groups: Vec<GroupWidgetSiteJs>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) declared_groups: Vec<DeclaredGroupJs>,
}

#[derive(Serialize)]
pub(crate) struct DeclaredGroupJs {
    #[serde(rename = "type")]
    pub(crate) ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) surface: Option<String>,
    pub(crate) param_indices: Vec<u32>,
    pub(crate) param_names: Vec<String>,
    pub(crate) context_params: std::collections::BTreeMap<String, u32>,
}

pub(crate) fn declared_group_js(g: &brink_ide::argument_widgets::DeclaredGroup) -> DeclaredGroupJs {
    DeclaredGroupJs {
        ty: g.ty.clone(),
        surface: g.surface.clone(),
        param_indices: g.param_indices.clone(),
        param_names: g.param_names.clone(),
        context_params: g.context_params.iter().cloned().collect(),
    }
}

#[derive(Serialize)]
pub(crate) struct GroupWidgetSiteJs {
    #[serde(rename = "type")]
    pub(crate) ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) surface: Option<String>,
    pub(crate) param_indices: Vec<u32>,
    pub(crate) param_names: Vec<String>,
    pub(crate) state: GroupStateJs,
    pub(crate) context: std::collections::BTreeMap<String, String>,
    /// Raw key → param-index map (#174) — the Form resolves context from its
    /// live draft values via this, before anything is written to the document.
    pub(crate) context_params: std::collections::BTreeMap<String, u32>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum GroupStateJs {
    Filled {
        spans: Vec<(u32, u32)>,
        values: Vec<String>,
    },
    Empty {
        insert_at: u32,
        needs_leading_comma: bool,
    },
}

#[derive(Serialize)]
pub(crate) struct SlotWidgetJs {
    pub(crate) param_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) widget: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) type_name: Option<String>,
    /// The honest display string for `type_name` (#1027/#1053) — the bare
    /// name when registered, `name ⚠ unregistered semantic type — E040`
    /// otherwise. The Form's label must render this, not `type_name`
    /// (`type_name` stays raw for widget-kind matching).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) type_display: Option<String>,
    /// Static value-list items (#174) for the Form dropdown; omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) values: Vec<ValueItemJs>,
    pub(crate) state: SlotStateJs,
}

#[derive(Serialize)]
pub(crate) struct ValueItemJs {
    pub(crate) value: String,
    pub(crate) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SlotStateJs {
    Filled {
        start: u32,
        end: u32,
        value: String,
    },
    Empty {
        insert_at: u32,
        needs_leading_comma: bool,
    },
    Expr,
}

#[derive(Serialize)]
pub(crate) struct SignatureInfoJs {
    pub(crate) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) documentation: Option<String>,
    pub(crate) parameters: Vec<ParamLabelJs>,
    pub(crate) active_parameter: u32,
}

#[derive(Serialize)]
pub(crate) struct ParamLabelJs {
    pub(crate) label: String,
}

#[derive(Serialize)]
pub(crate) struct FoldRangeJs {
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) collapsed_text: Option<String>,
    /// Whole-line declaration fold (docs + header + body); the editor folds
    /// from the start of `start_line` and renders a header placeholder.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) from_line_start: bool,
    /// The fold's kind (#365): `"structural"` (everything folding.rs emitted
    /// before #365 — never auto-collapsed), `"machinery"`, or `"narrative"`
    /// (run-based folds over the line classification).
    pub(crate) kind: &'static str,
}

/// `FoldKind` → the wire string the editor's `foldingExtension` switches on.
pub(crate) fn fold_kind_str(kind: brink_ide::folding::FoldKind) -> &'static str {
    match kind {
        brink_ide::folding::FoldKind::Structural => "structural",
        brink_ide::folding::FoldKind::Machinery => "machinery",
        brink_ide::folding::FoldKind::Narrative => "narrative",
    }
}

#[derive(Serialize)]
pub(crate) struct DocumentSymbolJs {
    pub(crate) name: String,
    pub(crate) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) full_start: u32,
    pub(crate) full_end: u32,
    pub(crate) children: Vec<DocumentSymbolJs>,
}

#[derive(Serialize)]
pub(crate) struct CodeActionJs {
    pub(crate) title: String,
    pub(crate) kind: String,
    /// Self-describing, internally-tagged payload (the `action` field is the
    /// discriminator) identifying which transformation this action performs.
    /// Pass it straight back to `resolve_code_action` to apply the action — the
    /// studio never has to reconstruct it from the cursor position.
    pub(crate) data: serde_json::Value,
}

#[derive(Serialize)]
pub(crate) struct TokenJs {
    pub(crate) line: u32,
    pub(crate) start_char: u32,
    pub(crate) length: u32,
    pub(crate) token_type: u32,
    pub(crate) modifiers: u32,
}

/// One projected HIR span for the editor overlay (#454). Positions are
/// 0-based lines with UTF-16 columns; `def_id`/`target_id` serialize as
/// `DefinitionId` strings (`$tt_hash`), safe for JS equality.
#[derive(Serialize)]
pub(crate) struct HirSpanJs {
    pub(crate) start_line: u32,
    pub(crate) start_char: u32,
    pub(crate) end_line: u32,
    pub(crate) end_char: u32,
    pub(crate) kind: &'static str,
    pub(crate) container: bool,
    pub(crate) depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) def_id: Option<brink_format::DefinitionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_id: Option<brink_format::DefinitionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) handle: Option<u32>,
}

/// One entry of a line's container stack (outermost→innermost by depth).
#[derive(Serialize)]
pub(crate) struct HirLineContainerJs {
    pub(crate) kind: &'static str,
    pub(crate) handle: u32,
    pub(crate) depth: u32,
}

/// The full projection payload: spans + per-line container stacks.
#[derive(Serialize)]
pub(crate) struct HirProjectionJs {
    pub(crate) spans: Vec<HirSpanJs>,
    pub(crate) lines: Vec<Vec<HirLineContainerJs>>,
}

pub(crate) fn span_kind_str(kind: brink_ide::hir_projection::SpanKind) -> &'static str {
    use brink_ide::hir_projection::SpanKind as K;
    match kind {
        K::Knot => "knot",
        K::Stitch => "stitch",
        K::Choice => "choice",
        K::Gather => "gather",
        K::ConditionalBranch => "cond_branch",
        K::SequenceBranch => "seq_branch",
        K::Label => "label",
        K::Param => "param",
        K::VarDecl => "var_decl",
        K::ConstDecl => "const_decl",
        K::ListDecl => "list_decl",
        K::ListMember => "list_member",
        K::External => "external",
        K::TempDecl => "temp_decl",
        K::Divert => "divert",
        K::VarRef => "var_ref",
        K::Call => "call",
        K::Content => "content",
        K::Interpolation => "interpolation",
        K::Tag => "tag",
        K::Include => "include",
        K::DivertStmt => "divert_stmt",
        K::TunnelStmt => "tunnel_stmt",
        K::ThreadStmt => "thread_stmt",
        K::DivertTerminal => "divert_terminal",
        K::Logic => "logic",
        K::Conditional => "conditional",
        K::Sequence => "sequence",
    }
}

// ── Helper functions ────────────────────────────────────────────────

pub(crate) fn symbol_kind_str(kind: brink_ir::SymbolKind) -> &'static str {
    match kind {
        brink_ir::SymbolKind::Knot => "knot",
        brink_ir::SymbolKind::Stitch => "stitch",
        brink_ir::SymbolKind::Variable => "variable",
        brink_ir::SymbolKind::Constant => "constant",
        brink_ir::SymbolKind::List => "list",
        brink_ir::SymbolKind::ListItem => "list_item",
        brink_ir::SymbolKind::External => "external",
        brink_ir::SymbolKind::Label => "label",
        brink_ir::SymbolKind::Param => "param",
        brink_ir::SymbolKind::Temp => "temp",
        brink_ir::SymbolKind::Struct => "struct",
    }
}

pub(crate) fn story_node_kind_str(kind: brink_ide::story_graph::StoryNodeKind) -> &'static str {
    match kind {
        brink_ide::story_graph::StoryNodeKind::Knot => "knot",
        brink_ide::story_graph::StoryNodeKind::Stitch => "stitch",
        brink_ide::story_graph::StoryNodeKind::End => "end",
        brink_ide::story_graph::StoryNodeKind::Done => "done",
    }
}

pub(crate) fn story_edge_kind_str(kind: brink_ide::story_graph::StoryEdgeKind) -> &'static str {
    match kind {
        brink_ide::story_graph::StoryEdgeKind::Divert => "divert",
        brink_ide::story_graph::StoryEdgeKind::Choice => "choice",
        brink_ide::story_graph::StoryEdgeKind::Tunnel => "tunnel",
        brink_ide::story_graph::StoryEdgeKind::Thread => "thread",
    }
}

pub(crate) fn code_action_kind_str(kind: &brink_ide::code_actions::CodeActionKind) -> &'static str {
    match kind {
        brink_ide::code_actions::CodeActionKind::QuickFix => "quickfix",
        brink_ide::code_actions::CodeActionKind::Refactor => "refactor",
        brink_ide::code_actions::CodeActionKind::Source => "source",
    }
}

pub(crate) fn inlay_hint_kind_str(kind: &brink_ide::inlay_hints::InlayHintKind) -> &'static str {
    match kind {
        brink_ide::inlay_hints::InlayHintKind::Parameter => "parameter",
        brink_ide::inlay_hints::InlayHintKind::Value => "value",
        brink_ide::inlay_hints::InlayHintKind::InferredType => "inferred_type",
    }
}

/// Convert a compiler diagnostic to JSON, translating its byte range to UTF-16
/// offsets against `source` (the diagnostic's own file) and attaching `file`
/// (that file's path).
/// Convert a resolved diagnostic to its JS shape. `source` is the diagnostic's
/// OWN file source (offsets are file-relative), used only to translate byte
/// offsets into UTF-16 for the editor. The file path comes from the resolved
/// diagnostic itself, so an included file's error lands on the right tab.
pub(crate) fn diagnostic_to_js(
    d: &brink_compiler::ResolvedDiagnostic,
    source: &str,
) -> DiagnosticJs {
    DiagnosticJs {
        message: d.message.clone(),
        start: byte_to_utf16(source, d.range.start().into()),
        end: byte_to_utf16(source, d.range.end().into()),
        severity: format!("{:?}", d.code.severity()),
        code: d.code.as_str().to_owned(),
        file: d.path.clone(),
    }
}

/// Convert a symbol tree to JSON, translating byte ranges to UTF-16 offsets
/// against `source` (the file the symbols belong to).
pub(crate) fn convert_document_symbol(
    sym: brink_ide::document::DocumentSymbol,
    source: &str,
) -> DocumentSymbolJs {
    DocumentSymbolJs {
        name: sym.name,
        kind: symbol_kind_str(sym.kind).to_owned(),
        detail: sym.detail,
        start: byte_to_utf16(source, sym.range.start().into()),
        end: byte_to_utf16(source, sym.range.end().into()),
        full_start: byte_to_utf16(source, sym.full_range.start().into()),
        full_end: byte_to_utf16(source, sym.full_range.end().into()),
        children: sym
            .children
            .into_iter()
            .map(|c| convert_document_symbol(c, source))
            .collect(),
    }
}

// ── Legacy stateless functions (token legend) ───────────────────────

/// Get token type names for the legend.
#[wasm_bindgen]
pub fn token_type_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_type_names()).unwrap_or_default()
}

/// Get token modifier names for the legend.
#[wasm_bindgen]
pub fn token_modifier_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_modifier_names()).unwrap_or_default()
}
