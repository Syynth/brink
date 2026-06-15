use brink_analyzer::AnalysisResult;
use brink_syntax::SyntaxNode;
use brink_syntax::ast::AstNode;
use rowan::{TextRange, TextSize};

/// The kind of inlay hint.
pub enum InlayHintKind {
    /// A `name:` / `name: type` label before an argument.
    Parameter,
    /// A host value label after a literal argument (e.g. `5 ⟨HarborGate⟩`) —
    /// the static value source's label for that literal (#174).
    Value,
}

/// An inlay hint to display in the editor.
pub struct InlayHint {
    pub offset: TextSize,
    pub label: String,
    pub kind: InlayHintKind,
    pub padding_right: bool,
}

/// Compute inlay hints for the given syntax tree within the requested range.
pub fn inlay_hints(
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    range: TextRange,
) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    for node in root.descendants() {
        let node_range = node.text_range();
        // Skip nodes entirely outside the requested range
        if node_range.end() < range.start() || node_range.start() > range.end() {
            continue;
        }

        if let Some(call) = brink_syntax::ast::FunctionCall::cast(node.clone()) {
            if let Some(name) = call.name() {
                collect_param_hints(&name, call.arg_list(), analysis, &mut hints);
            }
        } else if let Some(target) = brink_syntax::ast::DivertTargetWithArgs::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            let name = path_node.full_name();
            collect_param_hints(&name, target.arg_list(), analysis, &mut hints);
        }
    }

    hints
}

/// Collect parameter name inlay hints for a call with the given callee name.
fn collect_param_hints(
    callee_name: &str,
    arg_list: Option<brink_syntax::ast::ArgList>,
    analysis: &AnalysisResult,
    hints: &mut Vec<InlayHint>,
) {
    let Some(arg_list) = arg_list else { return };
    let args: Vec<_> = arg_list.args().collect();
    if args.is_empty() {
        return;
    }

    // Look up the callee in the symbol index
    let Some(ids) = analysis.index.by_name.get(callee_name) else {
        return;
    };

    // Find a matching symbol with params. Prefer one whose param count matches.
    let info = ids
        .iter()
        .filter_map(|id| analysis.index.symbols.get(id))
        .find(|info| {
            matches!(
                info.kind,
                brink_ir::SymbolKind::Knot
                    | brink_ir::SymbolKind::Stitch
                    | brink_ir::SymbolKind::External
            ) && info.params.len() == args.len()
        })
        .or_else(|| {
            // Fallback: any callable with params
            ids.iter()
                .filter_map(|id| analysis.index.symbols.get(id))
                .find(|info| {
                    matches!(
                        info.kind,
                        brink_ir::SymbolKind::Knot
                            | brink_ir::SymbolKind::Stitch
                            | brink_ir::SymbolKind::External
                    ) && !info.params.is_empty()
                })
        });

    let Some(info) = info else { return };

    // Typed params (from `///` doc tags or the host manifest) render as
    // `name: type`; untyped params keep the bare `name:` form.
    let meta = analysis.symbol_meta.get(&info.id);

    for (i, (arg, param)) in args.iter().zip(&info.params).enumerate() {
        // Skip hint if the argument text already matches the parameter name
        let arg_text = arg.syntax().text().to_string();
        let arg_text = arg_text.trim();
        if arg_text == param.name {
            continue;
        }

        let prefix = if param.is_ref {
            "ref "
        } else if param.is_divert {
            "-> "
        } else {
            ""
        };
        let ty = meta
            .and_then(|m| m.params.get(i))
            .and_then(|rp| rp.ty.as_ref());
        let label = match ty {
            Some(ty) => format!("{prefix}{}: {}", param.name, ty.name),
            None => format!("{prefix}{}:", param.name),
        };

        hints.push(InlayHint {
            offset: arg.syntax().text_range().start(),
            label,
            kind: InlayHintKind::Parameter,
            padding_right: true,
        });

        // Value-label hint (#174): if the param's semantic type declares a
        // static labelled value set and this literal matches an item, show its
        // label after the argument (`set_switch(5 ⟨HarborGate⟩, …)`). Advisory —
        // a non-matching literal simply gets no label (the host's set may have
        // changed; the running game is source of truth).
        if let Some(brink_ir::ValueSource::Static { items }) = ty.and_then(|rt| rt.values.as_ref())
        {
            let literal = arg_text.trim_matches('"');
            if let Some(item) = items.iter().find(|it| it.value == literal) {
                hints.push(InlayHint {
                    offset: arg.syntax().text_range().end(),
                    // Leading space separates it from the literal (no padding_left).
                    label: format!(" \u{27e8}{}\u{27e9}", item.label),
                    kind: InlayHintKind::Value,
                    padding_right: false,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rowan::{TextRange, TextSize};

    use super::{InlayHintKind, inlay_hints};
    use crate::session::IdeSession;

    #[test]
    fn typed_param_hints_include_type_untyped_keep_colon() {
        let src = "\
/// @param weapon {int}
== function damage(weapon) ==
~ return weapon
== function heal(amount) ==
~ return amount
== main ==
~ temp x = damage(3)
~ temp y = heal(4)
-> END
";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
        );
        let labels: Vec<_> = hints.iter().map(|h| h.label.as_str()).collect();
        assert!(labels.contains(&"weapon: int"), "{labels:?}");
        assert!(labels.contains(&"amount:"), "{labels:?}");
    }

    #[test]
    fn static_value_source_labels_matching_literal() {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, ValueItem, ValueSource,
        };

        let src = "\
EXTERNAL set_switch(id, on)
== main ==
~ set_switch(5, true)
~ set_switch(9, false)
-> END
";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        // set_switch(id: switch_id, on: bool); switch_id maps "5" -> "HarborGate".
        session.set_host_manifest(HostManifest {
            externals: vec![ManifestExternal {
                name: "set_switch".into(),
                params: vec![
                    ManifestParam {
                        name: "id".into(),
                        ty: TypeRef("switch_id".into()),
                    },
                    ManifestParam {
                        name: "on".into(),
                        ty: TypeRef("bool".into()),
                    },
                ],
                returns: TypeRef::default(),
                kind: ExternalKind::Effect,
                doc: None,
            }],
            types: vec![SemanticTypeDef {
                name: "switch_id".into(),
                base: BaseType::Int,
                constraint: None,
                values: Some(ValueSource::Static {
                    items: vec![ValueItem {
                        value: "5".into(),
                        label: "HarborGate".into(),
                        detail: None,
                    }],
                }),
            }],
        });
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
        );

        // The matching literal `5` gets a value label; `9` (not in the set) does not.
        let value_labels: Vec<_> = hints
            .iter()
            .filter(|h| matches!(h.kind, InlayHintKind::Value))
            .map(|h| h.label.as_str())
            .collect();
        assert_eq!(
            value_labels.len(),
            1,
            "only the matching literal: {value_labels:?}"
        );
        assert!(value_labels[0].contains("HarborGate"), "{value_labels:?}");
    }
}
