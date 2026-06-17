use std::fmt::Write as _;

use brink_analyzer::AnalysisResult;

use crate::find_call_context;

/// A parameter label.
pub struct ParamLabel {
    pub label: String,
}

/// Signature help information for a function call.
pub struct SignatureInfo {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<ParamLabel>,
    pub active_parameter: u32,
}

/// Compute signature help at the given byte offset.
pub fn signature_help(
    analysis: &AnalysisResult,
    source: &str,
    byte_offset: usize,
) -> Option<SignatureInfo> {
    let (func_name, active_param) = find_call_context(source, byte_offset)?;

    // Look up the function in the symbol index
    let info = analysis.index.symbols.values().find(|info| {
        matches!(
            info.kind,
            brink_ir::SymbolKind::Knot
                | brink_ir::SymbolKind::Stitch
                | brink_ir::SymbolKind::External
        ) && info.name == func_name
            && !info.params.is_empty()
    })?;

    // Host-manifest enrichment: typed params / return / doc for externals.
    let meta = analysis.symbol_meta.get(&info.id);

    let param_labels: Vec<ParamLabel> = info
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut label = if p.is_ref {
                format!("ref {}", p.name)
            } else if p.is_divert {
                format!("-> {}", p.name)
            } else {
                p.name.clone()
            };
            if let Some(ty) = meta
                .and_then(|m| m.params.get(i))
                .and_then(|rp| rp.ty.as_ref())
            {
                let _ = write!(label, ": {}", ty.name);
            }
            ParamLabel { label }
        })
        .collect();

    let ret = meta
        .and_then(|m| m.returns.as_ref())
        .map_or(String::new(), |t| format!(" -> {}", t.name));

    let signature_label = format!(
        "{}({}){ret}",
        func_name,
        param_labels
            .iter()
            .map(|p| p.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    #[expect(
        clippy::cast_possible_truncation,
        reason = "active param index fits in u32"
    )]
    let active = active_param.min(info.params.len().saturating_sub(1)) as u32;

    Some(SignatureInfo {
        label: signature_label,
        documentation: meta
            .and_then(|m| m.doc.clone())
            .or_else(|| info.detail.clone()),
        parameters: param_labels,
        active_parameter: active,
    })
}

/// One pickable value for an argument (Tier 3 static value source, #174).
pub struct ArgumentValueCompletion {
    /// The literal inserted into source (e.g. `"5"`).
    pub value: String,
    /// The display label (e.g. `"HarborGate"`).
    pub label: String,
    /// Optional secondary text (e.g. `"Switch #5"`).
    pub detail: Option<String>,
}

/// Pickable values for the argument at `byte_offset` (#174): if the cursor is
/// inside a call whose active parameter has a value source, return its labelled
/// items — `static` items from the manifest, or `host` items from the pushed
/// cache (`host_values`, empty/`None` when no host is attached). Empty for a
/// non-value param. Reuses the same call-site → semantic-type join point as
/// [`signature_help`].
#[must_use]
pub fn argument_value_completions(
    analysis: &AnalysisResult,
    source: &str,
    byte_offset: usize,
    host_values: Option<&crate::HostValues>,
) -> Vec<ArgumentValueCompletion> {
    let Some((func_name, active_param)) = find_call_context(source, byte_offset) else {
        return Vec::new();
    };
    let Some(info) = analysis.index.symbols.values().find(|info| {
        matches!(
            info.kind,
            brink_ir::SymbolKind::Knot
                | brink_ir::SymbolKind::Stitch
                | brink_ir::SymbolKind::External
        ) && info.name == func_name
            && !info.params.is_empty()
    }) else {
        return Vec::new();
    };
    let Some(rt) = analysis
        .symbol_meta
        .get(&info.id)
        .and_then(|m| m.params.get(active_param))
        .and_then(|rp| rp.ty.as_ref())
    else {
        return Vec::new();
    };
    let to_completion = |it: &brink_ir::ValueItem| ArgumentValueCompletion {
        value: it.value.clone(),
        label: it.label.clone(),
        detail: it.detail.clone(),
    };
    match rt.values.as_ref() {
        Some(brink_ir::ValueSource::Static { items }) => items.iter().map(to_completion).collect(),
        Some(brink_ir::ValueSource::Host) => host_values
            .and_then(|hv| hv.get(&rt.name))
            .map_or_else(Vec::new, |items| items.iter().map(to_completion).collect()),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::session::IdeSession;

    use super::signature_help;

    #[test]
    fn signature_help_shows_inline_types() {
        let src = "/// @param item {bool}\n/// @returns {bool}\nEXTERNAL holds(item)\n~ temp x = holds(true)\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let offset = src.find("holds(true)").expect("call present") + 6; // inside parens
        let sig = signature_help(analysis, src, offset).expect("signature");
        assert!(sig.label.contains("item: bool"), "label: {}", sig.label);
        assert!(sig.label.contains("-> bool"), "label: {}", sig.label);
    }

    #[test]
    fn signature_help_shows_function_knot_doc_and_types() {
        let src = "\
/// Damage roll for an attack.
/// @param weapon {int}
/// @returns {int}
== function damage(weapon) ==
~ return weapon
== main ==
~ temp x = damage(3)
-> END
";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let offset = src.find("damage(3)").expect("call present") + 7; // inside parens
        let sig = signature_help(analysis, src, offset).expect("signature");
        assert!(sig.label.contains("weapon: int"), "label: {}", sig.label);
        assert!(sig.label.contains("-> int"), "label: {}", sig.label);
        assert_eq!(
            sig.documentation.as_deref(),
            Some("Damage roll for an attack.")
        );
    }

    #[test]
    fn argument_value_completions_for_static_source() {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, ValueItem, ValueSource,
        };

        let src = "EXTERNAL set_switch(id, on)\n~ set_switch(5, true)\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
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

                widgets: vec![],
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "switch_id".into(),
                base: BaseType::Int,
                constraint: None,
                values: Some(ValueSource::Static {
                    items: vec![
                        ValueItem {
                            value: "5".into(),
                            label: "HarborGate".into(),
                            detail: Some("Switch #5".into()),
                        },
                        ValueItem {
                            value: "9".into(),
                            label: "Vault".into(),
                            detail: None,
                        },
                    ],
                }),
                widget: None,
            }],
        });
        let analysis = session.analysis().expect("analysis");

        // Cursor in the first arg slot (id: switch_id) → the labelled values.
        let id_offset = src.find("set_switch(").expect("call") + "set_switch(".len();
        let values = super::argument_value_completions(analysis, src, id_offset, None);
        assert_eq!(values.len(), 2, "two switch values");
        assert_eq!(values[0].label, "HarborGate");
        assert_eq!(values[0].value, "5");
        assert_eq!(values[0].detail.as_deref(), Some("Switch #5"));

        // Cursor in the second arg slot (on: bool — no value source) → empty.
        let on_offset = src.find("true").expect("second arg");
        assert!(super::argument_value_completions(analysis, src, on_offset, None).is_empty());
    }

    #[test]
    fn argument_value_completions_from_host_cache() {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, ValueItem, ValueSource,
        };

        let src = "EXTERNAL give_item(id)\n~ give_item(0)\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        // `item_id` is a HOST-source type — its values come from the cache, not
        // the manifest.
        session.set_host_manifest(HostManifest {
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

        let id_offset = src.find("give_item(").expect("call") + "give_item(".len();

        // With no host values pushed, the host source yields nothing.
        {
            let analysis = session.analysis().expect("analysis");
            assert!(
                super::argument_value_completions(
                    analysis,
                    src,
                    id_offset,
                    Some(session.host_values())
                )
                .is_empty(),
                "no host values pushed yet",
            );
        }

        // After the host pushes a snapshot, the picker serves it.
        session.set_host_values(crate::HostValues::from([(
            "item_id".to_string(),
            vec![
                ValueItem {
                    value: "0".into(),
                    label: "Potion".into(),
                    detail: None,
                },
                ValueItem {
                    value: "1".into(),
                    label: "Ether".into(),
                    detail: Some("MP".into()),
                },
            ],
        )]));
        let analysis = session.analysis().expect("analysis");
        let values = super::argument_value_completions(
            analysis,
            src,
            id_offset,
            Some(session.host_values()),
        );
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].label, "Potion");
        assert_eq!(values[1].value, "1");
    }
}
