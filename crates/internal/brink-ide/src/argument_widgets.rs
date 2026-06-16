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
    /// Arg-group widgets (spec §2) — a widget spanning several params. Grouped
    /// params still appear in `slots` (for the Form); the studio renders the
    /// group inline and skips those slots.
    pub groups: Vec<GroupWidgetSite>,
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
    /// Resolved inter-arg context: key → the sibling arg's literal value.
    pub context: Vec<(String, String)>,
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
    /// The semantic-type name, if the param is typed.
    pub type_name: Option<String>,
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
                    &node,
                    call.arg_list(),
                    analysis,
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
                &node,
                target.arg_list(),
                analysis,
            ) {
                sites.push(site);
            }
        }
    }
    sites
}

fn collect(
    callee_name: &str,
    name_range: rowan::TextRange,
    node: &SyntaxNode,
    arg_list: Option<brink_syntax::ast::ArgList>,
    analysis: &AnalysisResult,
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

    // `()` can yield a single empty/whitespace arg node — drop it so an empty
    // call reads as zero args (an Empty slot, not a phantom Expr).
    let (args, arg_list_inner): (Vec<_>, Option<TextSize>) = match arg_list {
        Some(al) => {
            let inner = al.syntax().text_range().start() + TextSize::from(1);
            let args = al
                .args()
                .filter(|a| !a.syntax().text().to_string().trim().is_empty())
                .collect();
            (args, Some(inner))
        }
        None => (Vec::new(), None),
    };

    // The append point for trailing-empty slots: after the last arg, or just
    // inside `(` when the call has no arguments yet. Empty parens produce no
    // `ArgList` node, so fall back to scanning the call node for `(`.
    let append_at = if let Some(last) = args.last() {
        Some(last.syntax().text_range().end())
    } else {
        arg_list_inner.or_else(|| open_paren_inside(node))
    };

    // The authoring state of param `i` (a literal → Filled, no arg → Empty, a
    // non-literal → Expr).
    let state_for = |i: usize| -> SlotState {
        match args.get(i) {
            Some(arg) => match literal_value(arg) {
                Some(value) => SlotState::Filled {
                    start: arg.syntax().text_range().start(),
                    end: arg.syntax().text_range().end(),
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

    let mut slots = Vec::with_capacity(info.params.len());
    for (i, param) in info.params.iter().enumerate() {
        let ty = meta
            .and_then(|m| m.params.get(i))
            .and_then(|rp| rp.ty.as_ref());
        let widget = ty.and_then(|t| t.widget.as_ref()).map(|w| w.kind.clone());
        let type_name = ty.map(|t| t.name.clone());
        slots.push(SlotWidget {
            param_name: param.name.clone(),
            widget,
            type_name,
            state: state_for(i),
        });
    }

    // Arg-group widgets (spec §2): emit one per declared group, but only when
    // every member is uniformly Filled (Edit) or Empty (Fill).
    let mut groups = Vec::new();
    if let Some(meta) = meta {
        for gw in &meta.group_widgets {
            if let Some(site) = build_group(gw, info, &args, append_at, &state_for) {
                groups.push(site);
            }
        }
    }

    Some(CallWidgetSite {
        callee: callee_name.to_string(),
        name_start: name_range.start(),
        name_end: name_range.end(),
        slots,
        groups,
    })
}

/// Build a [`GroupWidgetSite`] for one declared arg-group, or `None` when the
/// group is out of range or its members are mixed/non-literal (degrade to the
/// per-slot widgets + the Form).
fn build_group(
    gw: &brink_ir::ArgGroupWidget,
    info: &brink_ir::SymbolInfo,
    args: &[brink_syntax::ast::Expr],
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

    // Inter-arg context: each key → the sibling arg's literal value.
    let mut context = Vec::new();
    for (key, &arg_idx) in &gw.context {
        if let Some(arg) = args.get(arg_idx as usize)
            && let Some(value) = literal_value(arg)
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

/// The literal value of an argument (quotes stripped for strings), or `None`
/// when the argument is a non-literal expression.
fn literal_value(arg: &brink_syntax::ast::Expr) -> Option<String> {
    let text = arg.syntax().text().to_string();
    let t = text.trim();
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
        SemanticTypeDef, TypeRef, WidgetDecl,
    };
    use std::collections::BTreeMap;

    fn manifest() -> HostManifest {
        HostManifest {
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
                },
            ],
            types: vec![SemanticTypeDef {
                name: "hex_color".into(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: Some(WidgetDecl {
                    kind: "color".into(),
                }),
            }],
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
}
