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

/// A call site with a per-parameter widget slot. (The call-name span that
/// anchors the form affordance arrives with the stage-3 form work.)
pub struct CallWidgetSite {
    /// The callee name as written.
    pub callee: String,
    /// One slot per declared parameter.
    pub slots: Vec<SlotWidget>,
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
                && let Some(site) = collect(&name, &node, call.arg_list(), analysis)
            {
                sites.push(site);
            }
        } else if let Some(target) = brink_syntax::ast::DivertTargetWithArgs::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            let full = path_node.full_name();
            if let Some(site) = collect(&full, &node, target.arg_list(), analysis) {
                sites.push(site);
            }
        }
    }
    sites
}

fn collect(
    callee_name: &str,
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

    let mut slots = Vec::with_capacity(info.params.len());
    for (i, param) in info.params.iter().enumerate() {
        let ty = meta
            .and_then(|m| m.params.get(i))
            .and_then(|rp| rp.ty.as_ref());
        let widget = ty.and_then(|t| t.widget.as_ref()).map(|w| w.kind.clone());
        let type_name = ty.map(|t| t.name.clone());

        let state = match args.get(i) {
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
                // No parens to insert into — can't offer Fill.
                None => SlotState::Expr,
            },
        };

        slots.push(SlotWidget {
            param_name: param.name.clone(),
            widget,
            type_name,
            state,
        });
    }

    Some(CallWidgetSite {
        callee: callee_name.to_string(),
        slots,
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
    use super::{SlotState, argument_widgets};
    use crate::session::IdeSession;
    use brink_ir::{
        BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
        TypeRef, WidgetDecl,
    };

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
}
