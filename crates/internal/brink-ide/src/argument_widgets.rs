//! Argument widgets — the whole-call query behind inline editing (Edit), empty-
//! slot filling (Fill), and (later) the call form (argument-widget spec §4).
//!
//! Generalizes `color_hints`: instead of reporting only `hex_color` literals, it
//! reports every `EXTERNAL`/knot/stitch call in range as a [`CallWidgetSite`]
//! with one [`SlotWidget`] per declared parameter and its [`SlotState`]:
//! `Filled` (a literal — Edit/replace), `Empty` (no arg — Fill/insert), or
//! `Expr` (a non-literal — leave alone). Reuses the same call-site →
//! semantic-type join as inlay hints; tooling-only, never touches the program.

use brink_analyzer::AnalysisResult;
use brink_syntax::SyntaxNode;
use brink_syntax::ast::AstNode;
use rowan::TextSize;

/// A call site with a per-parameter widget slot.
pub struct CallWidgetSite {
    /// The callee name as written.
    pub callee: String,
    /// The call-name span — anchors the call-level form affordance (spec §1.1).
    pub name_start: TextSize,
    pub name_end: TextSize,
    /// One slot per declared parameter.
    pub slots: Vec<SlotWidget>,
    /// Arg-group widgets (spec §2) — a widget spanning several params, emitted
    /// only when the group is uniformly Filled/Empty. Drives the *inline* group
    /// chip/ghost (which needs the arg state). Grouped params still appear in
    /// `slots`; inline rendering skips them.
    pub groups: Vec<GroupWidgetSite>,
    /// Every declared arg-group widget for the callee, independent of the
    /// current arguments — the **Form** renders these (seeding member values
    /// from `slots`), so a partial or over-full call still gets its widgets.
    pub declared_groups: Vec<DeclaredGroup>,
}

/// A declared arg-group widget: the manifest structure with no arg-state. The
/// Form always renders one control per declared group regardless of how many
/// arguments the call currently has.
pub struct DeclaredGroup {
    /// Widget / semantic type (matches a host `ArgumentWidget.type`).
    pub ty: String,
    /// Editor container — `"popover"` (default) or `"modal"`.
    pub surface: Option<String>,
    pub param_indices: Vec<u32>,
    pub param_names: Vec<String>,
    /// Raw inter-arg context: key → the sibling param index.
    pub context_params: Vec<(String, u32)>,
}

/// An arg-group widget at a call site — one widget over several params, emitted
/// only when the whole group is uniformly Filled (Edit) or Empty (Fill).
pub struct GroupWidgetSite {
    /// Widget / semantic type (matches a host `ArgumentWidget.type`).
    pub ty: String,
    /// Editor container — `"popover"` (default) or `"modal"`.
    pub surface: Option<String>,
    /// The param indices the group spans (for the studio to skip those slots).
    pub param_indices: Vec<u32>,
    pub param_names: Vec<String>,
    pub state: GroupState,
    /// Resolved inter-arg context: key → the sibling arg's literal value (from
    /// the document — what inline editing uses).
    pub context: Vec<(String, String)>,
    /// Raw inter-arg context: key → the sibling param index. The Form resolves
    /// context from its own live draft values via this map, so picking the map
    /// first drives the point picker before anything is written to the document.
    pub context_params: Vec<(String, u32)>,
}

/// The authoring state of an arg-group (uniform across its members).
pub enum GroupState {
    /// All members are literals — Edit replaces each `spans[k]` with `values[k]`.
    Filled {
        spans: Vec<(TextSize, TextSize)>,
        values: Vec<String>,
    },
    /// All members are empty — Fill inserts the members joined by `, ` at
    /// `insert_at` (`, `-prefixed when the call already has args).
    Empty {
        insert_at: TextSize,
        needs_leading_comma: bool,
    },
}

/// One parameter slot of a call.
pub struct SlotWidget {
    pub param_name: String,
    /// The built-in widget kind for this slot's type (`color`, …), if any.
    pub widget: Option<String>,
    /// The semantic-type name, if the param is typed. Always the bare
    /// written name — consumers matching against a widget kind
    /// (`matchHostWidget`'s `type_name` fallback) must use this field, not
    /// `type_display`.
    pub type_name: Option<String>,
    /// The honest display string for `type_name` (#1027/#1053, reusing
    /// `hover::honest_type_display`'s marker convention exactly): the bare
    /// name when it resolves to a base keyword or a registered semantic
    /// type, `name ⚠ unregistered semantic type — E040` otherwise. `None`
    /// iff `type_name` is `None`. The Form's parameter label must render
    /// this, not `type_name` — rendering a bare, confident name for an
    /// unregistered type is exactly the #1004 divergence #1027 closed for
    /// hover/signature help.
    pub type_display: Option<String>,
    /// Pickable values for this slot's type (#174) — the Form renders these as a
    /// dropdown. Only static manifest items are surfaced here; empty otherwise.
    pub values: Vec<brink_ir::ValueItem>,
    pub state: SlotState,
}

/// The authoring state of a slot.
pub enum SlotState {
    /// A literal argument — Edit replaces `[start, end)`; `value` is the literal
    /// with surrounding quotes stripped.
    Filled {
        start: TextSize,
        end: TextSize,
        value: String,
    },
    /// No argument at this position — Fill inserts at `insert_at`. When the call
    /// already has arguments, `needs_leading_comma` asks for a `, ` separator.
    Empty {
        insert_at: TextSize,
        needs_leading_comma: bool,
    },
    /// A non-literal expression (variable / computed) — no inline affordance.
    Expr,
}

/// Argument-widget sites for every call within `range`.
#[must_use]
pub fn argument_widgets(
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    range: rowan::TextRange,
    host_values: Option<&crate::HostValues>,
) -> Vec<CallWidgetSite> {
    let mut sites = Vec::new();
    for node in root.descendants() {
        let node_range = node.text_range();
        if node_range.end() < range.start() || node_range.start() > range.end() {
            continue;
        }
        if let Some(call) = brink_syntax::ast::FunctionCall::cast(node.clone()) {
            if let Some(name) = call.name()
                && let Some(id) = call.identifier()
                && let Some(site) = collect(
                    &name,
                    id.syntax().text_range(),
                    arg_spans(call.arg_list()),
                    open_paren_inside(&node),
                    analysis,
                    host_values,
                )
            {
                sites.push(site);
            }
        } else if let Some(target) = brink_syntax::ast::DivertTargetWithArgs::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            let full = path_node.full_name();
            if let Some(site) = collect(
                &full,
                path_node.syntax().text_range(),
                arg_spans(target.arg_list()),
                open_paren_inside(&node),
                analysis,
                host_values,
            ) {
                sites.push(site);
            }
        }
    }
    sites
}

/// The native (`.brink`) sibling of [`argument_widgets`] (issue #2359) —
/// same call-site → semantic-type join, over `brink_syntax_native`'s
/// `CallExpr`/`DivertTarget` instead of ink's typed AST, which has no
/// native equivalent to cast into (#2291's `syntax_root` failure mode).
/// Never pass a `brink_syntax::SyntaxNode` root parsed from native source
/// text here — use [`crate::session::IdeSession::syntax_root_native`].
#[must_use]
pub fn argument_widgets_native(
    root: &brink_syntax_native::SyntaxNode,
    analysis: &AnalysisResult,
    range: rowan::TextRange,
    host_values: Option<&crate::HostValues>,
) -> Vec<CallWidgetSite> {
    use brink_syntax_native::ast::AstNode as _;
    let mut sites = Vec::new();
    for node in root.descendants() {
        let node_range = node.text_range();
        if node_range.end() < range.start() || node_range.start() > range.end() {
            continue;
        }
        if let Some(call) = brink_syntax_native::ast::CallExpr::cast(node.clone()) {
            if let Some(callee) = call.callee()
                && let Some(site) = collect(
                    &crate::color::native_path_name(&callee),
                    callee.syntax().text_range(),
                    arg_spans_native(call.arg_list()),
                    open_paren_inside_native(&node),
                    analysis,
                    host_values,
                )
            {
                sites.push(site);
            }
        } else if let Some(target) = brink_syntax_native::ast::DivertTarget::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            let full = crate::color::native_path_name(&path_node);
            if let Some(site) = collect(
                &full,
                path_node.syntax().text_range(),
                arg_spans_native(target.call_args()),
                open_paren_inside_native(&node),
                analysis,
                host_values,
            ) {
                sites.push(site);
            }
        }
    }
    sites
}

/// One argument's source text and span — the frontend-agnostic shape
/// [`collect`]/[`build_group`] need, so neither depends on which CST an
/// argument node came from.
struct ArgSpan {
    text: String,
    start: TextSize,
    end: TextSize,
}

/// `()` can yield a single empty/whitespace arg node — drop it so an empty
/// call reads as zero args (an Empty slot, not a phantom Expr). Returns the
/// filtered arg spans plus the position just inside `(` (the `ArgList`'s own
/// `arg_list_inner` — the empty-call fallback insert point when there are no
/// args yet but the parens themselves parsed as a real node).
fn arg_spans(arg_list: Option<brink_syntax::ast::ArgList>) -> (Vec<ArgSpan>, Option<TextSize>) {
    match arg_list {
        Some(al) => {
            let inner = al.syntax().text_range().start() + TextSize::from(1);
            let args = al
                .args()
                .filter_map(|a| {
                    let text = a.syntax().text().to_string();
                    if text.trim().is_empty() {
                        return None;
                    }
                    Some(ArgSpan {
                        text,
                        start: a.syntax().text_range().start(),
                        end: a.syntax().text_range().end(),
                    })
                })
                .collect();
            (args, Some(inner))
        }
        None => (Vec::new(), None),
    }
}

/// The native (`.brink`) sibling of [`arg_spans`] — native's `ArgList` has
/// no `.args()` wrapper (its children are bare expression nodes, see
/// `brink_ir::hir::lower_native::expr::lower_call`), so this reads
/// `al.syntax().children()` directly.
fn arg_spans_native(
    arg_list: Option<brink_syntax_native::ast::ArgList>,
) -> (Vec<ArgSpan>, Option<TextSize>) {
    use brink_syntax_native::ast::AstNode as _;
    match arg_list {
        Some(al) => {
            let inner = al.syntax().text_range().start() + TextSize::from(1);
            let args = al
                .syntax()
                .children()
                .filter_map(|a| {
                    let text = a.text().to_string();
                    if text.trim().is_empty() {
                        return None;
                    }
                    Some(ArgSpan {
                        text,
                        start: a.text_range().start(),
                        end: a.text_range().end(),
                    })
                })
                .collect();
            (args, Some(inner))
        }
        None => (Vec::new(), None),
    }
}

/// The join point [`argument_widgets`]/[`argument_widgets_native`] share:
/// resolve `callee_name` to a signature, then build its slots/groups from
/// the already-CST-agnostic `args`. `open_paren_fallback` is the insert
/// point for a zero-arg call whose parens produced no `ArgList` node at all
/// (each frontend's own [`open_paren_inside`]/[`open_paren_inside_native`]).
fn collect(
    callee_name: &str,
    name_range: rowan::TextRange,
    (args, arg_list_inner): (Vec<ArgSpan>, Option<TextSize>),
    open_paren_fallback: Option<TextSize>,
    analysis: &AnalysisResult,
    host_values: Option<&crate::HostValues>,
) -> Option<CallWidgetSite> {
    // Resolve the callee symbol by name (the most-params callable wins, so a
    // partially-typed call still maps onto the full signature).
    let ids = analysis.index.by_name.get(callee_name)?;
    let info = ids
        .iter()
        .filter_map(|id| analysis.index.symbols.get(id))
        .filter(|info| {
            matches!(
                info.kind,
                brink_ir::SymbolKind::Knot
                    | brink_ir::SymbolKind::Stitch
                    | brink_ir::SymbolKind::External
            )
        })
        .max_by_key(|info| info.params.len())?;
    if info.params.is_empty() {
        return None;
    }
    let meta = analysis.symbol_meta.get(&info.id);

    // The append point for trailing-empty slots: after the last arg, or just
    // inside `(` when the call has no arguments yet. Empty parens produce no
    // `ArgList` node, so fall back to scanning the call node for `(`.
    let append_at = if let Some(last) = args.last() {
        Some(last.end)
    } else {
        arg_list_inner.or(open_paren_fallback)
    };

    // The authoring state of param `i` (a literal → Filled, no arg → Empty, a
    // non-literal → Expr).
    let state_for = |i: usize| -> SlotState {
        match args.get(i) {
            Some(arg) => match literal_value(&arg.text) {
                Some(value) => SlotState::Filled {
                    start: arg.start,
                    end: arg.end,
                    value,
                },
                None => SlotState::Expr,
            },
            None => match append_at {
                Some(insert_at) => SlotState::Empty {
                    insert_at,
                    needs_leading_comma: !args.is_empty(),
                },
                None => SlotState::Expr,
            },
        }
    };

    let slots = build_slots(info, meta, host_values, &state_for);

    // Arg-group widgets (spec §2): emit one per declared group, but only when
    // every member is uniformly Filled (Edit) or Empty (Fill) — drives inline.
    // Separately, surface every declared group (structure only) for the Form.
    let mut groups = Vec::new();
    let mut declared_groups = Vec::new();
    if let Some(meta) = meta {
        for gw in &meta.group_widgets {
            if let Some(site) = build_group(gw, info, &args, append_at, &state_for) {
                groups.push(site);
            }
            if let Some(declared) = declare_group(gw, info) {
                declared_groups.push(declared);
            }
        }
    }

    Some(CallWidgetSite {
        callee: callee_name.to_string(),
        name_start: name_range.start(),
        name_end: name_range.end(),
        slots,
        groups,
        declared_groups,
    })
}

/// One [`SlotWidget`] per declared parameter — its built-in widget, semantic
/// type, value-list items (static from the manifest or host-sourced from the
/// pushed cache, like inlay hints), and authoring state.
fn build_slots(
    info: &brink_ir::SymbolInfo,
    meta: Option<&brink_analyzer::SymbolMeta>,
    host_values: Option<&crate::HostValues>,
    state_for: &impl Fn(usize) -> SlotState,
) -> Vec<SlotWidget> {
    let mut slots = Vec::with_capacity(info.params.len());
    for (i, param) in info.params.iter().enumerate() {
        let ty = meta
            .and_then(|m| m.params.get(i))
            .and_then(|rp| rp.ty.as_ref());
        let values = ty
            .and_then(|t| match t.values.as_ref() {
                Some(brink_ir::ValueSource::Static { items }) => Some(items.clone()),
                Some(brink_ir::ValueSource::Host) => {
                    host_values.and_then(|hv| hv.get(&t.name)).cloned()
                }
                None => None,
            })
            .unwrap_or_default();
        slots.push(SlotWidget {
            param_name: param.name.clone(),
            widget: ty.and_then(|t| t.widget.as_ref()).map(|w| w.kind.clone()),
            type_name: ty.map(|t| t.name.clone()),
            type_display: ty.map(crate::hover::honest_type_display),
            values,
            state: state_for(i),
        });
    }
    slots
}

/// The declared structure of one arg-group (no arg-state), for the Form. `None`
/// when the group is empty or names a param index the signature doesn't have.
fn declare_group(
    gw: &brink_ir::ArgGroupWidget,
    info: &brink_ir::SymbolInfo,
) -> Option<DeclaredGroup> {
    if gw.group.is_empty() {
        return None;
    }
    let mut param_names = Vec::with_capacity(gw.group.len());
    for &idx in &gw.group {
        param_names.push(info.params.get(idx as usize)?.name.clone());
    }
    Some(DeclaredGroup {
        ty: gw.ty.clone(),
        surface: gw.surface.clone(),
        param_indices: gw.group.clone(),
        param_names,
        context_params: gw.context.iter().map(|(k, &v)| (k.clone(), v)).collect(),
    })
}

/// Build a [`GroupWidgetSite`] for one declared arg-group, or `None` when the
/// group is out of range or its members are mixed/non-literal (degrade to the
/// per-slot widgets + the Form).
fn build_group(
    gw: &brink_ir::ArgGroupWidget,
    info: &brink_ir::SymbolInfo,
    args: &[ArgSpan],
    append_at: Option<TextSize>,
    state_for: &impl Fn(usize) -> SlotState,
) -> Option<GroupWidgetSite> {
    if gw.group.is_empty() {
        return None;
    }
    let mut param_names = Vec::with_capacity(gw.group.len());
    let mut states = Vec::with_capacity(gw.group.len());
    for &idx in &gw.group {
        let i = idx as usize;
        let param = info.params.get(i)?;
        param_names.push(param.name.clone());
        states.push(state_for(i));
    }

    // Uniform Filled → Edit; uniform Empty → Fill; anything else → skip.
    let state = if states.iter().all(|s| matches!(s, SlotState::Filled { .. })) {
        let mut spans = Vec::with_capacity(states.len());
        let mut values = Vec::with_capacity(states.len());
        for s in &states {
            if let SlotState::Filled { start, end, value } = s {
                spans.push((*start, *end));
                values.push(value.clone());
            }
        }
        GroupState::Filled { spans, values }
    } else if states.iter().all(|s| matches!(s, SlotState::Empty { .. })) {
        let insert_at = append_at?;
        GroupState::Empty {
            insert_at,
            needs_leading_comma: !args.is_empty(),
        }
    } else {
        return None;
    };

    // Inter-arg context: each key → the sibling arg's literal value (document-
    // resolved, for inline editing) plus the raw key → param-index map (for the
    // Form to resolve from its live draft values).
    let mut context = Vec::new();
    let mut context_params = Vec::new();
    for (key, &arg_idx) in &gw.context {
        context_params.push((key.clone(), arg_idx));
        if let Some(arg) = args.get(arg_idx as usize)
            && let Some(value) = literal_value(&arg.text)
        {
            context.push((key.clone(), value));
        }
    }

    Some(GroupWidgetSite {
        ty: gw.ty.clone(),
        surface: gw.surface.clone(),
        param_indices: gw.group.clone(),
        param_names,
        state,
        context,
        context_params,
    })
}

/// The position just inside the call's `(` — the insert point for an empty
/// call whose parens produced no `ArgList` node. `None` if the call has no `(`.
fn open_paren_inside(node: &SyntaxNode) -> Option<TextSize> {
    node.descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.text() == "(")
        .map(|t| t.text_range().start() + TextSize::from(1))
}

/// The native (`.brink`) sibling of [`open_paren_inside`] — identical
/// token-text scan, over `brink_syntax_native::SyntaxNode`. Never depends on
/// which CST it's walking beyond the node type itself.
fn open_paren_inside_native(node: &brink_syntax_native::SyntaxNode) -> Option<TextSize> {
    node.descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.text() == "(")
        .map(|t| t.text_range().start() + TextSize::from(1))
}

/// The literal value of an argument's trimmed source text (quotes stripped
/// for strings), or `None` when the argument is a non-literal expression.
/// Takes the arg's own text rather than a typed AST node — shared by
/// [`collect`] (ink and native args alike, via [`ArgSpan`]) and
/// [`build_group`]'s inter-arg context resolution.
fn literal_value(arg_text: &str) -> Option<String> {
    let t = arg_text.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return Some(t[1..t.len() - 1].to_string());
    }
    if t == "true" || t == "false" {
        return Some(t.to_string());
    }
    if t.parse::<f64>().is_ok() {
        return Some(t.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{GroupState, SlotState, argument_widgets};
    use crate::session::IdeSession;
    use brink_ir::{
        ArgGroupWidget, BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam,
        SemanticTypeDef, TypeRef, ValueItem, ValueSource, WidgetDecl,
    };
    use std::collections::BTreeMap;

    #[expect(clippy::too_many_lines, reason = "test fixture: a manifest literal")]
    fn manifest() -> HostManifest {
        HostManifest {
            markup: Vec::new(),
            externals: vec![
                ManifestExternal {
                    name: "set_tint".into(),
                    params: vec![ManifestParam {
                        name: "color".into(),
                        ty: TypeRef("hex_color".into()),
                    }],
                    returns: TypeRef::default(),
                    kind: ExternalKind::Effect,
                    doc: None,

                    widgets: vec![],
                    path: Vec::new(),
                },
                ManifestExternal {
                    name: "place_object".into(),
                    params: vec![
                        ManifestParam {
                            name: "x".into(),
                            ty: TypeRef("int".into()),
                        },
                        ManifestParam {
                            name: "y".into(),
                            ty: TypeRef("int".into()),
                        },
                    ],
                    returns: TypeRef::default(),
                    kind: ExternalKind::Effect,
                    doc: None,
                    // One map_point widget over both params (x, y).
                    widgets: vec![ArgGroupWidget {
                        group: vec![0, 1],
                        ty: "map_point".into(),
                        surface: Some("modal".into()),
                        context: BTreeMap::new(),
                    }],
                    path: Vec::new(),
                },
                // Mirrors the demo: a value-list `map` arg + an (x, y) group that
                // takes the map as inter-arg context.
                ManifestExternal {
                    name: "teleport".into(),
                    params: vec![
                        ManifestParam {
                            name: "map".into(),
                            ty: TypeRef("map_id".into()),
                        },
                        ManifestParam {
                            name: "x".into(),
                            ty: TypeRef("int".into()),
                        },
                        ManifestParam {
                            name: "y".into(),
                            ty: TypeRef("int".into()),
                        },
                    ],
                    returns: TypeRef::default(),
                    kind: ExternalKind::Effect,
                    doc: None,
                    widgets: vec![ArgGroupWidget {
                        group: vec![1, 2],
                        ty: "map_point".into(),
                        surface: Some("modal".into()),
                        context: BTreeMap::from([("map".to_string(), 0)]),
                    }],
                    path: Vec::new(),
                },
                // A host-sourced value-list: `region` items come from the pushed
                // host-values cache, not the manifest.
                ManifestExternal {
                    name: "go_region".into(),
                    params: vec![ManifestParam {
                        name: "region".into(),
                        ty: TypeRef("region_id".into()),
                    }],
                    returns: TypeRef::default(),
                    kind: ExternalKind::Effect,
                    doc: None,
                    widgets: vec![],
                    path: Vec::new(),
                },
            ],
            types: vec![
                SemanticTypeDef {
                    name: "region_id".into(),
                    base: BaseType::String,
                    constraint: None,
                    values: Some(ValueSource::Host),
                    widget: None,
                },
                SemanticTypeDef {
                    name: "hex_color".into(),
                    base: BaseType::String,
                    constraint: None,
                    values: None,
                    widget: Some(WidgetDecl {
                        kind: "color".into(),
                    }),
                },
                SemanticTypeDef {
                    name: "map_id".into(),
                    base: BaseType::String,
                    constraint: None,
                    values: Some(ValueSource::Static {
                        items: vec![
                            ValueItem {
                                value: "harbor".into(),
                                label: "Harbor".into(),
                                detail: None,
                            },
                            ValueItem {
                                value: "old_temple".into(),
                                label: "Old Temple".into(),
                                detail: None,
                            },
                        ],
                    }),
                    widget: None,
                },
            ],
        }
    }

    fn sites(src: &str) -> Vec<super::CallWidgetSite> {
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(manifest());
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax::parse(src);
        argument_widgets(
            &parsed.syntax(),
            analysis,
            rowan::TextRange::new(0.into(), rowan::TextSize::of(src)),
            None,
        )
    }

    #[test]
    fn filled_color_literal_is_edit() {
        let s = sites("EXTERNAL set_tint(color)\n~ set_tint(\"#FF8800\")\n-> END\n");
        let call = s.iter().find(|c| c.callee == "set_tint").expect("call");
        assert_eq!(call.slots.len(), 1);
        let slot = &call.slots[0];
        assert_eq!(slot.widget.as_deref(), Some("color"));
        assert!(
            matches!(&slot.state, SlotState::Filled { value, .. } if value == "#FF8800"),
            "expected Filled #FF8800"
        );
    }

    #[test]
    fn empty_call_is_fill_without_comma() {
        let s = sites("EXTERNAL set_tint(color)\n~ set_tint()\n-> END\n");
        let call = s.iter().find(|c| c.callee == "set_tint").expect("call");
        assert_eq!(call.slots.len(), 1);
        assert!(
            matches!(
                call.slots[0].state,
                SlotState::Empty {
                    needs_leading_comma: false,
                    ..
                }
            ),
            "expected Empty, first slot, no comma"
        );
    }

    #[test]
    fn variable_argument_is_expr() {
        let s = sites("EXTERNAL set_tint(color)\n~ temp c = \"#000000\"\n~ set_tint(c)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "set_tint").expect("call");
        assert!(matches!(call.slots[0].state, SlotState::Expr));
    }

    #[test]
    fn partial_call_trailing_slot_needs_comma() {
        let s = sites("EXTERNAL place_object(x, y)\n~ place_object(5)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.slots.len(), 2);
        assert!(matches!(call.slots[0].state, SlotState::Filled { .. }));
        assert!(
            matches!(
                call.slots[1].state,
                SlotState::Empty {
                    needs_leading_comma: true,
                    ..
                }
            ),
            "trailing slot after an arg needs a comma"
        );
    }

    #[test]
    fn filled_group_is_edit_over_all_members() {
        let s = sites("EXTERNAL place_object(x, y)\n~ place_object(3, 5)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.groups.len(), 1);
        let g = &call.groups[0];
        assert_eq!(g.ty, "map_point");
        assert_eq!(g.surface.as_deref(), Some("modal"));
        assert_eq!(g.param_indices, vec![0, 1]);
        assert!(
            matches!(
                &g.state,
                GroupState::Filled { spans, values }
                    if spans.len() == 2 && values == &vec!["3".to_string(), "5".to_string()]
            ),
            "expected Filled group over both members",
        );
    }

    #[test]
    fn empty_group_is_fill() {
        let s = sites("EXTERNAL place_object(x, y)\n~ place_object()\n-> END\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.groups.len(), 1);
        assert!(matches!(call.groups[0].state, GroupState::Empty { .. }));
    }

    #[test]
    fn mixed_group_is_not_emitted() {
        // x filled, y empty → not a uniform group; degrades to per-slot.
        let s = sites("EXTERNAL place_object(x, y)\n~ place_object(3)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.groups.len(), 0);
    }

    #[test]
    fn static_value_list_is_surfaced_on_slot() {
        // The map slot's type is a static value-list; its items reach the Form.
        let s = sites("EXTERNAL teleport(map, x, y)\n~ teleport(\"harbor\", 1, 2)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "teleport").expect("call");
        let map = &call.slots[0];
        assert_eq!(map.type_name.as_deref(), Some("map_id"));
        let labels: Vec<_> = map.values.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(labels, vec!["Harbor", "Old Temple"]);
        // Non-value-list slots carry no items.
        assert!(call.slots[1].values.is_empty());
    }

    #[test]
    fn declared_group_present_even_when_args_are_mixed() {
        // x filled, y missing → not a uniform group, so the inline `groups` is
        // empty, but the Form's `declared_groups` still carries the map_point
        // widget (driven by the signature, not the partial call).
        let s = sites("EXTERNAL teleport(map, x, y)\n~ teleport(\"harbor\", 1)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "teleport").expect("call");
        assert_eq!(call.groups.len(), 0, "mixed args → no inline group");
        assert_eq!(call.declared_groups.len(), 1);
        let g = &call.declared_groups[0];
        assert_eq!(g.ty, "map_point");
        assert_eq!(g.param_indices, vec![1, 2]);
        assert_eq!(g.param_names, vec!["x".to_string(), "y".to_string()]);
        assert_eq!(g.context_params, vec![("map".to_string(), 0)]);
    }

    #[test]
    fn host_sourced_value_list_is_surfaced_from_cache() {
        let src = "EXTERNAL go_region(region)\n~ go_region(\"harbor\")\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(manifest());
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax::parse(src);
        let range = rowan::TextRange::new(0.into(), rowan::TextSize::of(src));
        let host_values: crate::HostValues = std::collections::HashMap::from([(
            "region_id".to_string(),
            vec![ValueItem {
                value: "harbor".into(),
                label: "Harbor".into(),
                detail: None,
            }],
        )]);

        let with = argument_widgets(&parsed.syntax(), analysis, range, Some(&host_values));
        let call = with.iter().find(|c| c.callee == "go_region").expect("call");
        let labels: Vec<_> = call.slots[0]
            .values
            .iter()
            .map(|v| v.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec!["Harbor"],
            "host items come from the pushed cache"
        );

        // Without the cache, a `host` value-list surfaces nothing.
        let without = argument_widgets(&parsed.syntax(), analysis, range, None);
        let call = without
            .iter()
            .find(|c| c.callee == "go_region")
            .expect("call");
        assert!(call.slots[0].values.is_empty());
    }

    // ── Issue #1053: the argument-widget surface must be honest about an
    // unregistered semantic type, not render it with the same bare
    // confidence as a registered one (extends #1027's hover/signature-help
    // fix to this surface). `type_name` stays the bare name (widget-kind
    // matching depends on it); `type_display` carries the honesty marker. ──

    #[test]
    fn unregistered_semantic_type_slot_keeps_bare_type_name_but_flags_type_display() {
        use brink_ir::{BaseType, HostManifest, SemanticTypeDef};

        // `var_id` is named in the inline `@param` doc, but the registered
        // manifest only defines a *sibling* type (`actor_id`) — the #1004/
        // #1027 case, reproduced on the argument-widget surface.
        let src = "/// @param id {var_id}\nEXTERNAL get_variable(id)\n~ get_variable(5)\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
            externals: vec![],
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            }],
        });
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax::parse(src);
        let sites = argument_widgets(
            &parsed.syntax(),
            analysis,
            rowan::TextRange::new(0.into(), rowan::TextSize::of(src)),
            None,
        );
        let call = sites
            .iter()
            .find(|c| c.callee == "get_variable")
            .expect("call");
        let slot = &call.slots[0];

        assert_eq!(
            slot.type_name.as_deref(),
            Some("var_id"),
            "type_name must stay the bare name — widget-kind matching depends on it"
        );
        let display = slot.type_display.as_deref().expect("type_display present");
        assert!(
            display.contains("var_id"),
            "still shows the written name: {display}"
        );
        assert!(
            display.contains('\u{26A0}'),
            "must carry the explicit warning marker: {display}"
        );
        assert!(
            display.contains("E040"),
            "must cross-reference the E040 diagnostic code: {display}"
        );
    }

    #[test]
    fn registered_semantic_type_slot_has_matching_name_and_display_with_no_warning() {
        use brink_ir::{BaseType, HostManifest, SemanticTypeDef};

        let src =
            "/// @param id {actor_id}\nEXTERNAL get_variable(id)\n~ get_variable(5)\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
            externals: vec![],
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: None,
            }],
        });
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax::parse(src);
        let sites = argument_widgets(
            &parsed.syntax(),
            analysis,
            rowan::TextRange::new(0.into(), rowan::TextSize::of(src)),
            None,
        );
        let call = sites
            .iter()
            .find(|c| c.callee == "get_variable")
            .expect("call");
        let slot = &call.slots[0];

        assert_eq!(slot.type_name.as_deref(), Some("actor_id"));
        assert_eq!(slot.type_display.as_deref(), Some("actor_id"));
    }

    #[test]
    fn group_context_params_carries_index_map() {
        // The group both resolves `map` from the document AND exposes the raw
        // key→index map so the Form can resolve from its live drafts.
        let s = sites("EXTERNAL teleport(map, x, y)\n~ teleport(\"harbor\", 1, 2)\n-> END\n");
        let call = s.iter().find(|c| c.callee == "teleport").expect("call");
        assert_eq!(call.groups.len(), 1);
        let g = &call.groups[0];
        assert_eq!(g.param_indices, vec![1, 2]);
        assert_eq!(g.context, vec![("map".to_string(), "harbor".to_string())]);
        assert_eq!(g.context_params, vec![("map".to_string(), 0)]);
    }

    // ── #2359: `argument_widgets_native` — the native (`.brink`) sibling ────

    use super::argument_widgets_native;

    fn native_sites(src: &str) -> Vec<super::CallWidgetSite> {
        let mut session = IdeSession::new();
        session.update_and_analyze("test.brink", src.to_string());
        session.set_host_manifest(manifest());
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax_native::parse(src);
        argument_widgets_native(
            &parsed.syntax(),
            analysis,
            rowan::TextRange::new(0.into(), rowan::TextSize::of(src)),
            None,
        )
    }

    #[test]
    fn native_filled_color_literal_is_edit() {
        // A regression test that fails with `argument_widgets_native` deleted
        // and `argument_widgets_impl` in `crates/brink-web` back on the
        // pre-#2359 `is_native` early-return: this exact call site would
        // silently get zero widget sites instead of one Filled color slot.
        let s =
            native_sites("extern set_tint(color)\nflow main() {\n  ~ set_tint(\"#FF8800\")\n}\n");
        let call = s.iter().find(|c| c.callee == "set_tint").expect("call");
        assert_eq!(call.slots.len(), 1);
        let slot = &call.slots[0];
        assert_eq!(slot.widget.as_deref(), Some("color"));
        assert!(
            matches!(&slot.state, SlotState::Filled { value, .. } if value == "#FF8800"),
            "expected Filled #FF8800"
        );
    }

    #[test]
    fn native_empty_call_is_fill_without_comma() {
        let s = native_sites("extern set_tint(color)\nflow main() {\n  ~ set_tint()\n}\n");
        let call = s.iter().find(|c| c.callee == "set_tint").expect("call");
        assert_eq!(call.slots.len(), 1);
        assert!(
            matches!(
                call.slots[0].state,
                SlotState::Empty {
                    needs_leading_comma: false,
                    ..
                }
            ),
            "expected Empty, first slot, no comma"
        );
    }

    #[test]
    fn native_variable_argument_is_expr() {
        let s = native_sites(
            "extern set_tint(color)\nflow main() {\n  ~ let c = \"#000000\"\n  ~ set_tint(c)\n}\n",
        );
        let call = s.iter().find(|c| c.callee == "set_tint").expect("call");
        assert!(matches!(call.slots[0].state, SlotState::Expr));
    }

    #[test]
    fn native_partial_call_trailing_slot_needs_comma() {
        let s = native_sites("extern place_object(x, y)\nflow main() {\n  ~ place_object(5)\n}\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.slots.len(), 2);
        assert!(matches!(call.slots[0].state, SlotState::Filled { .. }));
        assert!(
            matches!(
                call.slots[1].state,
                SlotState::Empty {
                    needs_leading_comma: true,
                    ..
                }
            ),
            "trailing slot after an arg needs a comma"
        );
    }

    #[test]
    fn native_filled_group_is_edit_over_all_members() {
        let s =
            native_sites("extern place_object(x, y)\nflow main() {\n  ~ place_object(3, 5)\n}\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.groups.len(), 1);
        let g = &call.groups[0];
        assert_eq!(g.ty, "map_point");
        assert_eq!(g.param_indices, vec![0, 1]);
        assert!(
            matches!(
                &g.state,
                GroupState::Filled { spans, values }
                    if spans.len() == 2 && values == &vec!["3".to_string(), "5".to_string()]
            ),
            "expected Filled group over both members",
        );
    }

    #[test]
    fn native_mixed_group_is_not_emitted() {
        let s = native_sites("extern place_object(x, y)\nflow main() {\n  ~ place_object(3)\n}\n");
        let call = s.iter().find(|c| c.callee == "place_object").expect("call");
        assert_eq!(call.groups.len(), 0);
    }
}
