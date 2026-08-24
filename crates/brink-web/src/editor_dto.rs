use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::compile::DiagnosticJs;
use crate::editor::byte_to_utf16;
use crate::editor::utf16_index::Utf16Index;

// ── Serialization types ─────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct ProjectFileJs {
    pub(crate) path: String,
    /// Whether `path` currently resolves to a mounted stdlib copy rather
    /// than a file the project scan found or the user created (issue
    /// #2306/#2343, "Mounted stdlib presents as a read-only library node").
    /// `list_files` used to exclude these ids entirely (#2231); it now
    /// lists them with this flag set instead, so the Binder can render a
    /// distinct, read-only "Library" section rather than hiding them —
    /// dropping the filter without this flag would reintroduce the exact
    /// phantom-row bug #2231/#2303 fixed. Mirrors `EditorSession::is_read_only`.
    pub(crate) mounted: bool,
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
    /// See [`ProjectFileJs::mounted`] — same flag, same issue (#2306/#2343).
    pub(crate) mounted: bool,
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
    /// See [`ProjectFileJs::mounted`] — same flag, same issue (#2306/#2343).
    /// Always `false` for the `END`/`DONE` pseudo-nodes (no owning `file`).
    pub(crate) mounted: bool,
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
    /// TIGHT end line for containers (issue #3054 review, two-range model):
    /// last line of actual content — trailing whitespace and the next
    /// declaration's doc block excluded. Absent on non-containers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content_end_line: Option<u32>,
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
///
/// `types`/`lints` are the [`brink_analyzer::TypePolicy`]/[`brink_analyzer::LintPolicy`]
/// the diagnostics were actually produced under — `severity` renders the
/// [`brink_analyzer::effective_severity`] (issue #1367: a `[lints]`
/// re-leveled code must display at its overridden severity, not the raw
/// [`brink_ir::DiagnosticCode::severity`] default).
pub(crate) fn diagnostic_to_js(
    d: &brink_compiler::ResolvedDiagnostic,
    source: &str,
    types: brink_analyzer::TypePolicy,
    lints: &brink_analyzer::LintPolicy,
) -> DiagnosticJs {
    DiagnosticJs {
        message: d.message.clone(),
        start: byte_to_utf16(source, d.range.start().into()),
        end: byte_to_utf16(source, d.range.end().into()),
        severity: format!(
            "{:?}",
            brink_analyzer::effective_severity(d.code, types, lints)
        ),
        code: d.code.as_str().to_owned(),
        file: d.path.clone(),
    }
}

#[cfg(test)]
mod diagnostic_to_js_tests {
    use super::diagnostic_to_js;

    fn diag(code: brink_ir::DiagnosticCode) -> brink_compiler::ResolvedDiagnostic {
        brink_compiler::ResolvedDiagnostic {
            path: "main.ink".to_owned(),
            file: brink_ir::FileId(0),
            range: rowan::TextRange::new(rowan::TextSize::from(0), rowan::TextSize::from(1)),
            message: "test".to_owned(),
            // `diagnostic_to_js` never reads this field — it renders
            // `effective_severity(d.code, ...)` instead (issue #1367) — so
            // the value here is a placeholder, not a fixture under test.
            severity: code.severity(),
            code,
        }
    }

    /// #1367: the wasm editor's diagnostic list must render the *effective*
    /// severity, not `d.code.severity()` — a `[lints]` re-leveled code
    /// (`E014` is one of the `Warning`-default codes) must show `"Error"`.
    #[test]
    fn diagnostic_to_js_respects_lints_override() {
        let d = diag(brink_ir::DiagnosticCode::E014);
        let no_override = diagnostic_to_js(
            &d,
            "x",
            brink_analyzer::TypePolicy::Gradual,
            &brink_analyzer::LintPolicy::default(),
        );
        assert_eq!(no_override.severity, "Warning");

        let mut lints = brink_analyzer::LintPolicy::default();
        lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Deny);
        let overridden = diagnostic_to_js(&d, "x", brink_analyzer::TypePolicy::Gradual, &lints);
        assert_eq!(overridden.severity, "Error");
    }

    /// #1162: a `[lints] E014 = "info"`/`"hint"` override must render the
    /// new advisory tiers through the wasm boundary, not just error/warning —
    /// `diagnostic_to_js` renders `effective_severity` via `{:?}`, so this
    /// also locks in that the new `Severity` variants keep their `Info`/
    /// `Hint` `Debug` spelling.
    #[test]
    fn diagnostic_to_js_renders_info_and_hint_tiers() {
        let d = diag(brink_ir::DiagnosticCode::E014);

        let mut info_lints = brink_analyzer::LintPolicy::default();
        info_lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Info);
        let info = diagnostic_to_js(&d, "x", brink_analyzer::TypePolicy::Gradual, &info_lints);
        assert_eq!(info.severity, "Info");

        let mut hint_lints = brink_analyzer::LintPolicy::default();
        hint_lints
            .overrides
            .insert("E014".to_owned(), brink_analyzer::LintLevel::Hint);
        let hint = diagnostic_to_js(&d, "x", brink_analyzer::TypePolicy::Gradual, &hint_lints);
        assert_eq!(hint.severity, "Hint");
    }
}

/// Convert a symbol tree to JSON, translating byte ranges to UTF-16 offsets
/// through `index` (built over the file the symbols belong to). Takes the
/// prebuilt [`Utf16Index`] rather than the source (#3065): four conversions
/// per symbol times a naive from-zero scan made `project_outline`
/// O(symbols × file size) — the caller builds the index once per file.
pub(crate) fn convert_document_symbol(
    sym: brink_ide::document::DocumentSymbol,
    index: &Utf16Index<'_>,
) -> DocumentSymbolJs {
    DocumentSymbolJs {
        name: sym.name,
        kind: symbol_kind_str(sym.kind).to_owned(),
        detail: sym.detail,
        start: index.byte_to_utf16(sym.range.start().into()),
        end: index.byte_to_utf16(sym.range.end().into()),
        full_start: index.byte_to_utf16(sym.full_range.start().into()),
        full_end: index.byte_to_utf16(sym.full_range.end().into()),
        children: sym
            .children
            .into_iter()
            .map(|c| convert_document_symbol(c, index))
            .collect(),
    }
}

// ── Explain-match (issue #2113, NS-T seam 3/6) ──────────────────────
//
// Unlike every other DTO in this file, ranges here are **raw byte
// offsets**, not UTF-16 — see `editor::explain_match`'s own module doc for
// why: a matched handler's declaration range lives in the project's
// configured conventions module, a file this session may never have
// opened as a document, so there is no single file's text to convert
// against. The classified line's own capture ranges *could* be converted
// against the active document the ordinary way, but this DTO keeps every
// range in the same unit for one consistent contract rather than mixing
// UTF-16 (captures) with raw bytes (handler locations) in one payload.

/// One handler location — a name plus its declaration-site byte range in
/// the project's conventions module.
#[derive(Serialize)]
pub(crate) struct ExplainHandlerJs {
    pub(crate) name: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// One named capture, as a raw byte range into the classified line's own
/// file.
#[derive(Serialize)]
pub(crate) struct ExplainCaptureJs {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// One resolved field of an `attach = StructName` schema — mirrors
/// [`brink_ir::ConventionAttachField`] verbatim: a declared field name plus
/// its resolved type shape, never a value any handler computed (issue
/// #2311, matching the "schema, never values" contract
/// `ConventionAttachSchema`'s own doc states).
#[derive(Serialize)]
pub(crate) struct ExplainAttachFieldJs {
    pub(crate) name: String,
    pub(crate) ty: SchemaTypeShapeJs,
}

/// A field type's structural shape — mirrors [`brink_ir::SchemaTypeShape`]
/// verbatim, span-free (issue #2311). Recursive: `Generic`'s `args` and
/// `Fn`'s `params`/`ret` are themselves [`SchemaTypeShapeJs`].
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SchemaTypeShapeJs {
    Named {
        name: String,
    },
    Generic {
        name: String,
        args: Vec<SchemaTypeShapeJs>,
    },
    Fn {
        params: Vec<SchemaTypeShapeJs>,
        ret: Box<SchemaTypeShapeJs>,
    },
}

fn schema_type_shape_to_js(ty: brink_ir::SchemaTypeShape) -> SchemaTypeShapeJs {
    match ty {
        brink_ir::SchemaTypeShape::Named(name) => SchemaTypeShapeJs::Named { name },
        brink_ir::SchemaTypeShape::Generic { name, args } => SchemaTypeShapeJs::Generic {
            name,
            args: args.into_iter().map(schema_type_shape_to_js).collect(),
        },
        brink_ir::SchemaTypeShape::Fn { params, ret } => SchemaTypeShapeJs::Fn {
            params: params.into_iter().map(schema_type_shape_to_js).collect(),
            ret: Box::new(schema_type_shape_to_js(*ret)),
        },
    }
}

/// The `attach = StructName` clause's resolution outcome — mirrors
/// [`brink_ir::ConventionAttachSchema`] verbatim (issue #2311): `Resolved`
/// carries the struct's declared name plus every field, `Unresolved`
/// carries just the declared name a consumer can still report even though
/// it did not resolve to a real struct (house rule: flag silent data
/// drops — this is the wasm-facing half of that same "never drop it
/// silently" contract).
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExplainAttachSchemaJs {
    Resolved {
        name: String,
        fields: Vec<ExplainAttachFieldJs>,
    },
    Unresolved {
        name: String,
    },
}

fn explain_attach_schema_to_js(schema: brink_ir::ConventionAttachSchema) -> ExplainAttachSchemaJs {
    match schema {
        brink_ir::ConventionAttachSchema::Resolved { name, fields } => {
            ExplainAttachSchemaJs::Resolved {
                name,
                fields: fields
                    .into_iter()
                    .map(|f| ExplainAttachFieldJs {
                        name: f.name,
                        ty: schema_type_shape_to_js(f.ty),
                    })
                    .collect(),
            }
        }
        brink_ir::ConventionAttachSchema::Unresolved(name) => {
            ExplainAttachSchemaJs::Unresolved { name }
        }
    }
}

fn explain_disposition_to_js(disposition: brink_ir::ElementDisposition) -> &'static str {
    match disposition {
        brink_ir::ElementDisposition::Call => "call",
    }
}

fn explain_mode_to_js(mode: brink_ir::ConventionMode) -> &'static str {
    match mode {
        brink_ir::ConventionMode::Attach => "attach",
        brink_ir::ConventionMode::Wrap => "wrap",
    }
}

/// One handler's classification-time match — the winner or one of the
/// shadowed runners-up; see `ExplainMatchJs`'s own doc.
#[derive(Serialize)]
pub(crate) struct ExplainClassifiedMatchJs {
    pub(crate) handler: ExplainHandlerJs,
    pub(crate) order: i64,
    pub(crate) mode: &'static str,
    /// The claimed line's compile-time structural shape
    /// (`brink_ir::ElementKind`, issue #2310) — populated only on `winner`,
    /// omitted from the JSON entirely (not `null`) whenever it is absent.
    /// A `shadowed` entry never carries one: only the actual winning claim
    /// has a compiled `ElementMatch` to read it from (see
    /// `editor::explain_match`'s own module doc for why this is a read of
    /// the last-compiled snapshot, not a re-derivation, and why it can
    /// decline rather than guess).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<&'static str>,
    pub(crate) disposition: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) attach: Option<ExplainAttachSchemaJs>,
    pub(crate) captures: Vec<ExplainCaptureJs>,
}

/// The stable wire spelling for [`brink_ir::ElementKind`] (issue #2310) —
/// `snake_case`, matching this module's other stringified enums (`mode`
/// above).
fn element_kind_str(kind: brink_ir::ElementKind) -> &'static str {
    match kind {
        brink_ir::ElementKind::ContentLine => "content_line",
        brink_ir::ElementKind::SceneHeading => "scene_heading",
        brink_ir::ElementKind::BangDispatch => "bang_dispatch",
        brink_ir::ElementKind::Cue => "cue",
        brink_ir::ElementKind::Parenthetical => "parenthetical",
    }
}

/// One entry the walk attempted but that did not match — the miss-case
/// sibling of `ExplainClassifiedMatchJs`, carrying the pattern source
/// instead of captures (there is nothing to capture from a non-match).
#[derive(Serialize)]
pub(crate) struct ExplainAttemptedJs {
    pub(crate) handler: ExplainHandlerJs,
    pub(crate) order: i64,
    pub(crate) mode: &'static str,
    pub(crate) disposition: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) attach: Option<ExplainAttachSchemaJs>,
    pub(crate) pattern: String,
}

/// The explain-match query's full per-line answer (issue #2113): is this
/// line matched, by what, what did it bind, and — on a miss — what was
/// attempted, or — on a hit — what else matched but was shadowed.
/// `winner`/`shadowed` are populated only when `matched` is `true`;
/// `attempted` only when it is `false` — the two are mutually exclusive by
/// construction (`brink_ir::LineExplanation`'s own shape), not by
/// convention here.
#[derive(Serialize)]
pub(crate) struct ExplainMatchJs {
    pub(crate) matched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) winner: Option<ExplainClassifiedMatchJs>,
    pub(crate) shadowed: Vec<ExplainClassifiedMatchJs>,
    pub(crate) attempted: Vec<ExplainAttemptedJs>,
}

fn explain_handler_to_js(name: &brink_ir::Name) -> ExplainHandlerJs {
    ExplainHandlerJs {
        name: name.text.clone(),
        start: name.range.start().into(),
        end: name.range.end().into(),
    }
}

fn explain_classified_match_to_js(
    m: brink_ir::ClassifiedMatch,
    kind: Option<brink_ir::ElementKind>,
) -> ExplainClassifiedMatchJs {
    ExplainClassifiedMatchJs {
        handler: explain_handler_to_js(&m.handler),
        order: m.order,
        mode: explain_mode_to_js(m.mode),
        kind: kind.map(element_kind_str),
        disposition: explain_disposition_to_js(m.disposition),
        attach: m.attach.map(explain_attach_schema_to_js),
        captures: m
            .captures
            .into_iter()
            .map(|c| ExplainCaptureJs {
                name: c.name,
                text: c.text,
                start: c.range.start().into(),
                end: c.range.end().into(),
            })
            .collect(),
    }
}

fn explain_attempted_to_js(entry: brink_ir::ConventionProjectionEntry) -> ExplainAttemptedJs {
    ExplainAttemptedJs {
        handler: explain_handler_to_js(&entry.name),
        order: entry.order,
        mode: explain_mode_to_js(entry.mode),
        disposition: explain_disposition_to_js(entry.disposition),
        attach: entry.attach.map(explain_attach_schema_to_js),
        pattern: entry.pattern,
    }
}

/// Convert [`brink_ir::LineExplanation`] into its wasm-facing JSON shape —
/// see `ExplainMatchJs`'s own doc for the contract. `kind` is the caller's
/// own [`brink_ir::ElementKind`] composition (issue #2310,
/// `editor::explain_match`'s own module doc) — `None` whenever the caller
/// has no compiled kind to report; ignored entirely on the `Unmatched` arm,
/// which never has one.
pub(crate) fn explain_match_to_js(
    explanation: brink_ir::LineExplanation,
    kind: Option<brink_ir::ElementKind>,
) -> ExplainMatchJs {
    match explanation {
        brink_ir::LineExplanation::Matched { winner, shadowed } => ExplainMatchJs {
            matched: true,
            winner: Some(explain_classified_match_to_js(winner, kind)),
            shadowed: shadowed
                .into_iter()
                .map(|m| explain_classified_match_to_js(m, None))
                .collect(),
            attempted: Vec::new(),
        },
        brink_ir::LineExplanation::Unmatched { attempted } => ExplainMatchJs {
            matched: false,
            winner: None,
            shadowed: Vec::new(),
            attempted: attempted.into_iter().map(explain_attempted_to_js).collect(),
        },
    }
}

#[cfg(test)]
mod explain_match_to_js_tests {
    use rowan::TextRange;

    use super::explain_match_to_js;

    fn name(text: &str, start: u32, end: u32) -> brink_ir::Name {
        brink_ir::Name {
            text: text.to_owned(),
            range: TextRange::new(start.into(), end.into()),
        }
    }

    /// The `Matched` arm: `matched` is `true`, `winner` serializes with its
    /// handler/order/mode/disposition/attach/captures, `shadowed` lists
    /// every runner-up in the same shape, and `attempted` is the empty array
    /// (never populated on a hit) — this is the arm nothing else in this
    /// crate exercises (issue #2113 review, w143; `disposition`/`attach`
    /// added by #2311). No `kind` is passed here, so — per issue #2310's
    /// `skip_serializing_if` contract — neither `winner` nor `shadowed`
    /// carries a `kind` key at all.
    #[test]
    fn matched_arm_serializes_winner_and_shadowed_with_attempted_empty() {
        let winner = brink_ir::ClassifiedMatch {
            handler: name("interior", 0, 8),
            order: 10,
            mode: brink_ir::ConventionMode::Attach,
            disposition: brink_ir::ElementDisposition::Call,
            attach: Some(brink_ir::ConventionAttachSchema::Resolved {
                name: "Cue".to_owned(),
                fields: vec![brink_ir::ConventionAttachField {
                    name: "place".to_owned(),
                    ty: brink_ir::SchemaTypeShape::Named("string".to_owned()),
                }],
            }),
            captures: vec![brink_ir::ClassifiedCapture {
                name: "place".to_owned(),
                text: "MARKET SQUARE".to_owned(),
                range: TextRange::new(105.into(), 118.into()),
            }],
        };
        let shadowed = brink_ir::ClassifiedMatch {
            handler: name("any_line", 20, 28),
            order: 20,
            mode: brink_ir::ConventionMode::Wrap,
            disposition: brink_ir::ElementDisposition::Call,
            attach: None,
            captures: Vec::new(),
        };
        let explanation = brink_ir::LineExplanation::Matched {
            winner,
            shadowed: vec![shadowed],
        };

        let json =
            serde_json::to_value(explain_match_to_js(explanation, None)).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "matched": true,
                "winner": {
                    "handler": {"name": "interior", "start": 0, "end": 8},
                    "order": 10,
                    "mode": "attach",
                    "disposition": "call",
                    "attach": {
                        "kind": "resolved",
                        "name": "Cue",
                        "fields": [
                            {"name": "place", "ty": {"kind": "named", "name": "string"}},
                        ],
                    },
                    "captures": [
                        {"name": "place", "text": "MARKET SQUARE", "start": 105, "end": 118},
                    ],
                },
                "shadowed": [
                    {
                        "handler": {"name": "any_line", "start": 20, "end": 28},
                        "order": 20,
                        "mode": "wrap",
                        "disposition": "call",
                        "captures": [],
                    },
                ],
                "attempted": [],
            })
        );
    }

    /// Issue #2310: when the caller supplies a `kind`, it lands on `winner`
    /// only — `shadowed` never carries one, since only the actual winning
    /// claim has a compiled `ElementMatch` to read a kind from at all (see
    /// `editor::explain_match`'s own module doc).
    #[test]
    fn a_supplied_kind_serializes_on_winner_only_never_on_shadowed() {
        let winner = brink_ir::ClassifiedMatch {
            handler: name("cue", 0, 3),
            order: 10,
            mode: brink_ir::ConventionMode::Attach,
            disposition: brink_ir::ElementDisposition::Call,
            attach: None,
            captures: Vec::new(),
        };
        let shadowed = brink_ir::ClassifiedMatch {
            handler: name("any_line", 20, 28),
            order: 20,
            mode: brink_ir::ConventionMode::Wrap,
            disposition: brink_ir::ElementDisposition::Call,
            attach: None,
            captures: Vec::new(),
        };
        let explanation = brink_ir::LineExplanation::Matched {
            winner,
            shadowed: vec![shadowed],
        };

        let json = serde_json::to_value(explain_match_to_js(
            explanation,
            Some(brink_ir::ElementKind::Cue),
        ))
        .expect("serializes");
        assert_eq!(
            json["winner"]["kind"],
            serde_json::json!("cue"),
            "got {json}"
        );
        assert!(
            json["shadowed"][0].get("kind").is_none(),
            "a shadowed entry must never carry a kind — got {json}"
        );
    }

    /// The `Unmatched` arm: `matched` is `false`, `winner` is omitted
    /// entirely (`skip_serializing_if`), `shadowed` is the empty array, and
    /// `attempted` lists every tried entry with its pattern/mode/
    /// disposition/attach schema (never a `ClassifiedMatch` shape — a miss
    /// has no captures). An `attach` clause naming a struct that failed to
    /// resolve serializes as `"unresolved"`, not silently dropped (issue
    /// #2311, house rule: flag silent data drops) — and, on the sibling
    /// entry with no `attach` clause at all, the key is omitted from the
    /// wire object entirely (`skip_serializing_if`), not emitted as `null`
    /// (#2311 review, finding 2: `ExplainAttemptedJs`'s own omission
    /// contract, which `packages/wasm-types/src/index.ts`'s
    /// `ExplainAttempted.attach?` optional depends on).
    #[test]
    fn unmatched_arm_serializes_attempted_with_winner_omitted() {
        let attempted = vec![
            brink_ir::ConventionProjectionEntry {
                name: name("interior", 0, 8),
                dispatch_name: None,
                pattern: "^INT\\. (?<place>.+)$".to_owned(),
                order: 10,
                mode: brink_ir::ConventionMode::Attach,
                disposition: brink_ir::ElementDisposition::Call,
                attach: Some(brink_ir::ConventionAttachSchema::Unresolved(
                    "MissingStruct".to_owned(),
                )),
            },
            brink_ir::ConventionProjectionEntry {
                name: name("any_line", 20, 28),
                dispatch_name: None,
                pattern: "^.*$".to_owned(),
                order: 20,
                mode: brink_ir::ConventionMode::Wrap,
                disposition: brink_ir::ElementDisposition::Call,
                attach: None,
            },
        ];
        let explanation = brink_ir::LineExplanation::Unmatched { attempted };

        let json =
            serde_json::to_value(explain_match_to_js(explanation, None)).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "matched": false,
                "shadowed": [],
                "attempted": [
                    {
                        "handler": {"name": "interior", "start": 0, "end": 8},
                        "order": 10,
                        "mode": "attach",
                        "disposition": "call",
                        "attach": {"kind": "unresolved", "name": "MissingStruct"},
                        "pattern": "^INT\\. (?<place>.+)$",
                    },
                    {
                        "handler": {"name": "any_line", "start": 20, "end": 28},
                        "order": 20,
                        "mode": "wrap",
                        "disposition": "call",
                        "pattern": "^.*$",
                    },
                ],
            })
        );
        assert!(
            json.get("winner").is_none(),
            "winner must be omitted (skip_serializing_if), not null"
        );
        assert!(
            json["attempted"][1].get("attach").is_none(),
            "an attempted entry with no attach clause must omit the key \
             entirely, not serialize it as null — got {json}"
        );
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
