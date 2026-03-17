use brink_analyzer::AnalysisResult;
use brink_syntax::SyntaxNode;
use brink_syntax::ast::AstNode;
use rowan::{TextRange, TextSize};

/// The kind of inlay hint.
pub enum InlayHintKind {
    Parameter,
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

    for (arg, param) in args.iter().zip(&info.params) {
        // Skip hint if the argument text already matches the parameter name
        let arg_text = arg.syntax().text().to_string();
        let arg_text = arg_text.trim();
        if arg_text == param.name {
            continue;
        }

        let label = if param.is_ref {
            format!("ref {}:", param.name)
        } else if param.is_divert {
            format!("-> {}:", param.name)
        } else {
            format!("{}:", param.name)
        };

        hints.push(InlayHint {
            offset: arg.syntax().text_range().start(),
            label,
            kind: InlayHintKind::Parameter,
            padding_right: true,
        });
    }
}
