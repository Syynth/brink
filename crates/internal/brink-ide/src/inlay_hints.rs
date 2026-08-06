use brink_analyzer::AnalysisResult;
use brink_db::ProjectDb;
use brink_ir::FileId;
use brink_syntax::SyntaxNode;
use brink_syntax::ast::AstNode;
use rowan::{TextRange, TextSize};

use crate::inferred_types::enclosing_callable;

/// The kind of inlay hint.
pub enum InlayHintKind {
    /// A `name:` / `name: type` label before an argument.
    Parameter,
    /// A host value label after a literal argument (e.g. `5 ⟨HarborGate⟩`) —
    /// the static value source's label for that literal (#174).
    Value,
    /// An inferred type label after an unannotated `temp` declaration
    /// (TM-5, #621) — reads `ProjectDb::infer_body`, the FG-narrowed
    /// per-def seam; never shown when the author already wrote a `: type`
    /// ascription (TM-2) — that's already visible in the source.
    InferredType,
}

/// An inlay hint to display in the editor.
pub struct InlayHint {
    pub offset: TextSize,
    pub label: String,
    pub kind: InlayHintKind,
    pub padding_right: bool,
}

/// Compute inlay hints for the given syntax tree within the requested range.
///
/// `host_values` (Tier 3, #174) supplies labels for `host`-source semantic
/// types from the pushed cache; pass `None` when no host is attached (static
/// value labels still resolve from the manifest). `db`/`file_id` back the
/// TM-5 (#621) inferred-type hints on unannotated `temp` declarations —
/// `db` is the same `ProjectDb` `analysis` was computed from/against.
pub fn inlay_hints(
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    db: &ProjectDb,
    file_id: FileId,
    range: TextRange,
    host_values: Option<&crate::HostValues>,
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
                collect_param_hints(&name, call.arg_list(), analysis, host_values, &mut hints);
            }
        } else if let Some(target) = brink_syntax::ast::DivertTargetWithArgs::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            let name = path_node.full_name();
            collect_param_hints(&name, target.arg_list(), analysis, host_values, &mut hints);
        } else if let Some(temp_decl) = brink_syntax::ast::TempDecl::cast(node.clone()) {
            collect_inferred_type_hint(&temp_decl, analysis, db, file_id, &mut hints);
        }
    }

    hints
}

/// TM-5 (#621): an inferred-type inlay after an *unannotated* `temp`
/// declaration's name (`~ temp x = …` -> `~ temp x: int = …` as a ghost
/// label, never inserted into the source). Silently does nothing when the
/// author already wrote a `: type` ascription, the temp doesn't resolve to
/// its own declaration symbol, or inference can't pin down a concrete type
/// (`Unknown` — showing that would be noise, not information).
fn collect_inferred_type_hint(
    temp_decl: &brink_syntax::ast::TempDecl,
    analysis: &AnalysisResult,
    db: &ProjectDb,
    file_id: FileId,
    hints: &mut Vec<InlayHint>,
) {
    if temp_decl.type_annotation().is_some() {
        return;
    }
    let Some(identifier) = temp_decl.identifier() else {
        return;
    };
    let Some(name) = temp_decl.name() else {
        return;
    };
    let ident_range = identifier.syntax().text_range();

    // Resolve to this temp's own declaration-site `SymbolInfo` so its
    // `Scope` gives us the enclosing knot/stitch to key `infer_body` by.
    let Some(info) = analysis.index.symbols.values().find(|info| {
        info.file == file_id && info.kind == brink_ir::SymbolKind::Temp && info.range == ident_range
    }) else {
        return;
    };

    let Some(ty) = enclosing_callable(analysis, info)
        .and_then(|def| db.infer_body(def))
        .and_then(|body| body.locals.get(&name).cloned())
        .filter(|ty| !ty.is_unknown())
    else {
        return;
    };

    hints.push(InlayHint {
        offset: ident_range.end(),
        label: format!(": {}", ty.display()),
        kind: InlayHintKind::InferredType,
        padding_right: false,
    });
}

/// Collect parameter name inlay hints for a call with the given callee name.
fn collect_param_hints(
    callee_name: &str,
    arg_list: Option<brink_syntax::ast::ArgList>,
    analysis: &AnalysisResult,
    host_values: Option<&crate::HostValues>,
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
        // A param whose type has a studio-builtin widget shows just `name:` —
        // the widget (e.g. the color swatch) conveys the type, so repeating it
        // is noise: `set_tint(color: ▮"#FF8800")`, not `color: hex_color`.
        //
        // The type portion goes through `honest_type_display` (#1027/#1053) —
        // an unregistered semantic type renders with the same warning marker
        // and E040 cross-reference hover/signature-help use, instead of a
        // bare, confident name.
        let label = match ty {
            Some(ty) if ty.widget.is_none() => {
                format!(
                    "{prefix}{}: {}",
                    param.name,
                    crate::hover::honest_type_display(ty)
                )
            }
            _ => format!("{prefix}{}:", param.name),
        };

        hints.push(InlayHint {
            offset: arg.syntax().text_range().start(),
            label,
            kind: InlayHintKind::Parameter,
            padding_right: true,
        });

        // Value-label hint (#174): if the param's semantic type carries a value
        // set (static manifest items, or `host` items from the pushed cache) and
        // this literal matches one, show its label after the argument
        // (`set_switch(5 ⟨HarborGate⟩, …)`). Advisory — a non-matching literal
        // gets no label (the host's set may have changed; the game is truth).
        let value_items: Option<&[brink_ir::ValueItem]> =
            ty.and_then(|rt| match rt.values.as_ref() {
                Some(brink_ir::ValueSource::Static { items }) => Some(items.as_slice()),
                Some(brink_ir::ValueSource::Host) => host_values
                    .and_then(|hv| hv.get(&rt.name))
                    .map(Vec::as_slice),
                None => None,
            });
        if let Some(items) = value_items {
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
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        let labels: Vec<_> = hints.iter().map(|h| h.label.as_str()).collect();
        assert!(labels.contains(&"weapon: int"), "{labels:?}");
        assert!(labels.contains(&"amount:"), "{labels:?}");
    }

    #[test]
    fn builtin_widget_param_drops_the_type_label() {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, WidgetDecl,
        };

        // `color: hex_color` carries the built-in color widget — the swatch
        // conveys the type, so the inlay shows just `color:` (spec §9).
        let src = "EXTERNAL set_tint(color)\n~ set_tint(\"#FF8800\")\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
            externals: vec![ManifestExternal {
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
            }],
            types: vec![SemanticTypeDef {
                name: "hex_color".into(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: Some(WidgetDecl {
                    kind: "color".into(),
                }),
            }],
        });
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        let labels: Vec<_> = hints.iter().map(|h| h.label.as_str()).collect();
        assert!(labels.contains(&"color:"), "{labels:?}");
        assert!(!labels.contains(&"color: hex_color"), "{labels:?}");
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
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        // set_switch(id: switch_id, on: bool); switch_id maps "5" -> "HarborGate".
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
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

                widgets: vec![],
                path: Vec::new(),
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
                widget: None,
            }],
        });
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
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

    #[test]
    fn host_value_source_labels_from_pushed_cache() {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, ValueItem, ValueSource,
        };

        let src = "EXTERNAL give_item(id)\n~ give_item(1)\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
            externals: vec![ManifestExternal {
                name: "give_item".into(),
                params: vec![ManifestParam {
                    name: "id".into(),
                    ty: TypeRef("item_id".into()),
                }],
                returns: TypeRef::default(),
                kind: ExternalKind::Effect,
                doc: None,

                widgets: vec![],
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "item_id".into(),
                base: BaseType::Int,
                constraint: None,
                values: Some(ValueSource::Host),
                widget: None,
            }],
        });
        session.set_host_values(crate::HostValues::from([(
            "item_id".to_string(),
            vec![ValueItem {
                value: "1".into(),
                label: "Ether".into(),
                detail: None,
            }],
        )]));
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let range = TextRange::new(TextSize::new(0), TextSize::of(src));

        // The pushed cache resolves the host literal's label.
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            range,
            Some(session.host_values()),
        );
        assert!(
            hints
                .iter()
                .any(|h| matches!(h.kind, InlayHintKind::Value) && h.label.contains("Ether")),
            "host value label from cache",
        );

        // Without the cache (no host attached), no host label.
        let bare = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            range,
            None,
        );
        assert!(
            !bare.iter().any(|h| matches!(h.kind, InlayHintKind::Value)),
            "no host label without the cache",
        );
    }

    // ── Issue #1053: parameter-type inlay hints must be honest about an
    // unregistered semantic type, not render it with the same bare
    // confidence as a registered one (extends #1027's hover/signature-help
    // fix to this surface). ──────────────────────────────────────────────

    #[test]
    fn unregistered_semantic_type_param_hint_carries_the_warning_marker() {
        use brink_ir::{BaseType, HostManifest, SemanticTypeDef};

        // `var_id` is named in the inline `@param` doc, but the registered
        // manifest only defines a *sibling* type (`actor_id`) — the
        // vocabulary reached the analyzer, `var_id` just isn't in it (the
        // #1004/#1027 case).
        let src = "/// @param id {var_id}\nEXTERNAL get_variable(id)\n~ get_variable(5)\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
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
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        let param_label = hints
            .iter()
            .find(|h| matches!(h.kind, InlayHintKind::Parameter))
            .map(|h| h.label.as_str())
            .expect("a parameter hint for `id`");

        assert_ne!(
            param_label, "id: var_id",
            "must not render var_id with bare, unqualified confidence"
        );
        assert!(
            param_label.contains("var_id"),
            "still shows the written name: {param_label}"
        );
        assert!(
            param_label.contains('\u{26A0}'),
            "must carry the explicit warning marker: {param_label}"
        );
        assert!(
            param_label.contains("E040"),
            "must cross-reference the E040 diagnostic code: {param_label}"
        );
    }

    #[test]
    fn registered_semantic_type_param_hint_has_no_warning() {
        use brink_ir::{BaseType, HostManifest, SemanticTypeDef};

        let src =
            "/// @param id {actor_id}\nEXTERNAL get_variable(id)\n~ get_variable(5)\n-> END\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
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
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        let param_label = hints
            .iter()
            .find(|h| matches!(h.kind, InlayHintKind::Parameter))
            .map(|h| h.label.as_str())
            .expect("a parameter hint for `id`");

        assert_eq!(param_label, "id: actor_id");
        assert!(!param_label.contains('\u{26A0}'), "{param_label}");
    }

    // ── TM-5 (#621): inferred-type inlay hints on unannotated `temp` decls ──

    #[test]
    fn inferred_type_hint_after_unannotated_temp_in_a_function_body() {
        let src = "=== function heal(hp) ===\n~ temp bonus = hp + 1\n~ return bonus\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        let inferred: Vec<_> = hints
            .iter()
            .filter(|h| matches!(h.kind, InlayHintKind::InferredType))
            .collect();
        assert_eq!(
            inferred.len(),
            1,
            "{:?}",
            hints.iter().map(|h| &h.label).collect::<Vec<_>>()
        );
        assert_eq!(inferred[0].label, ": int");
        // Placed right after `bonus`, before ` = hp + 1`.
        let bonus_end = TextSize::try_from(src.find("bonus").expect("present") + "bonus".len())
            .expect("offset");
        assert_eq!(inferred[0].offset, bonus_end);
    }

    #[test]
    fn no_inferred_type_hint_when_temp_already_has_an_annotation() {
        let src = "=== function heal(hp) ===\n~ temp bonus: int = hp + 1\n~ return bonus\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        assert!(
            !hints
                .iter()
                .any(|h| matches!(h.kind, InlayHintKind::InferredType)),
            "an explicit ascription already shows the type in-source: {:?}",
            hints.iter().map(|h| &h.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_inferred_type_hint_when_inference_cannot_resolve_one() {
        // `unused` is a temp that's never read — inference leaves it
        // `Unknown`, and showing that would be noise, not information.
        let src =
            "=== function heal(hp) ===\n~ temp unused = hp\n~ temp other = 1\n~ return other\n";
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let parsed = brink_syntax::parse(src);
        let hints = inlay_hints(
            &parsed.syntax(),
            analysis,
            session.db(),
            file_id,
            TextRange::new(TextSize::new(0), TextSize::of(src)),
            None,
        );
        let labels: Vec<_> = hints
            .iter()
            .filter(|h| matches!(h.kind, InlayHintKind::InferredType))
            .map(|h| h.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![": int"],
            "only `other` resolves, `unused` stays Unknown"
        );
    }
}
