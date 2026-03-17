use brink_analyzer::AnalysisResult;
use brink_ir::FileId;
use brink_syntax::SyntaxNode;
use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend,
};

// ── Legend ──────────────────────────────────────────────────────────

pub fn legend() -> SemanticTokensLegend {
    let token_types: Vec<SemanticTokenType> = brink_ide::semantic_tokens::token_type_names()
        .iter()
        .map(|name| SemanticTokenType::new(name))
        .collect();

    let token_modifiers: Vec<SemanticTokenModifier> =
        brink_ide::semantic_tokens::token_modifier_names()
            .iter()
            .map(|name| SemanticTokenModifier::new(name))
            .collect();

    SemanticTokensLegend {
        token_types,
        token_modifiers,
    }
}

// ── Public API (thin adapters) ──────────────────────────────────────

/// Compute semantic tokens for the full document.
pub fn compute_semantic_tokens(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
) -> Vec<SemanticToken> {
    let raw = brink_ide::semantic_tokens::semantic_tokens(source, root, analysis, file_id);
    let deltas = brink_ide::semantic_tokens::delta_encode(&raw);
    deltas
        .into_iter()
        .map(|d| SemanticToken {
            delta_line: d.delta_line,
            delta_start: d.delta_start,
            length: d.length,
            token_type: d.token_type,
            token_modifiers_bitset: d.token_modifiers,
        })
        .collect()
}

/// Compute semantic tokens for a range of the document.
pub fn compute_semantic_tokens_range(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
    range_start_line: u32,
    range_end_line: u32,
) -> Vec<SemanticToken> {
    let raw = brink_ide::semantic_tokens::semantic_tokens_range(
        source,
        root,
        analysis,
        file_id,
        range_start_line,
        range_end_line,
    );
    let deltas = brink_ide::semantic_tokens::delta_encode(&raw);
    deltas
        .into_iter()
        .map(|d| SemanticToken {
            delta_line: d.delta_line,
            delta_start: d.delta_start,
            length: d.length,
            token_type: d.token_type,
            token_modifiers_bitset: d.token_modifiers,
        })
        .collect()
}
