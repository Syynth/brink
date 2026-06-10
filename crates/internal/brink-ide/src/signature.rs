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
}
