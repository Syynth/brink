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

    let param_labels: Vec<ParamLabel> = info
        .params
        .iter()
        .map(|p| {
            let label = if p.is_ref {
                format!("ref {}", p.name)
            } else if p.is_divert {
                format!("-> {}", p.name)
            } else {
                p.name.clone()
            };
            ParamLabel { label }
        })
        .collect();

    let signature_label = format!(
        "{}({})",
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
        documentation: info.detail.clone(),
        parameters: param_labels,
        active_parameter: active,
    })
}
