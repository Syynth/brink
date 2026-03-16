use std::collections::HashMap;

use brink_analyzer::AnalysisResult;
use brink_ir::{FileId, SymbolKind};
use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;
use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend,
};

use crate::convert::LineIndex;

// ── Legend ──────────────────────────────────────────────────────────

pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,    // 0  knots
    SemanticTokenType::FUNCTION,     // 1  stitches, externals
    SemanticTokenType::VARIABLE,     // 2  variables
    SemanticTokenType::STRING,       // 3  string content
    SemanticTokenType::NUMBER,       // 4  numeric literals
    SemanticTokenType::KEYWORD,      // 5  VAR, CONST, LIST, INCLUDE, etc.
    SemanticTokenType::OPERATOR,     // 6  ->, <-, ~, etc.
    SemanticTokenType::COMMENT,      // 7  // and /* */
    SemanticTokenType::ENUM,         // 8  list names
    SemanticTokenType::ENUM_MEMBER,  // 9  list items
    SemanticTokenType::PARAMETER,    // 10 function/knot params
    SemanticTokenType::DECORATOR,    // 11 tags (#)
    SemanticTokenType::new("label"), // 12 labels, gather names
];

pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION, // 1 << 0
    SemanticTokenModifier::DEFINITION,  // 1 << 1
    SemanticTokenModifier::READONLY,    // 1 << 2
    SemanticTokenModifier::DEPRECATED,  // 1 << 3 (future use)
];

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}

// ── Token type indices ─────────────────────────────────────────────

const TT_NAMESPACE: u32 = 0;
const TT_FUNCTION: u32 = 1;
const TT_VARIABLE: u32 = 2;
const TT_STRING: u32 = 3;
const TT_NUMBER: u32 = 4;
const TT_KEYWORD: u32 = 5;
const TT_OPERATOR: u32 = 6;
const TT_COMMENT: u32 = 7;
const TT_ENUM: u32 = 8;
const TT_ENUM_MEMBER: u32 = 9;
const TT_PARAMETER: u32 = 10;
const TT_DECORATOR: u32 = 11;
const TT_LABEL: u32 = 12;

// ── Modifier bitmasks ──────────────────────────────────────────────

const MOD_DECLARATION: u32 = 1 << 0;
const MOD_READONLY: u32 = 1 << 2;

// ── Raw token (absolute position) ──────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct RawToken {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub token_type: u32,
    pub modifiers: u32,
}

// ── Classification result ──────────────────────────────────────────

struct Classification {
    token_type: u32,
    modifiers: u32,
}

// ── Resolution index ───────────────────────────────────────────────

fn build_resolution_index(
    analysis: &AnalysisResult,
    file_id: FileId,
) -> HashMap<TextRange, SymbolKind> {
    let mut map = HashMap::new();
    for rref in &analysis.resolutions {
        if rref.file == file_id
            && let Some(info) = analysis.index.symbols.get(&rref.target)
        {
            map.insert(rref.range, info.kind);
        }
    }
    map
}

// ── Token classification ───────────────────────────────────────────

fn classify_token(
    token: &brink_syntax::SyntaxToken,
    resolution_index: &HashMap<TextRange, SymbolKind>,
) -> Option<Classification> {
    let kind = token.kind();

    // Skip tokens we never highlight
    if matches!(
        kind,
        SyntaxKind::WHITESPACE
            | SyntaxKind::NEWLINE
            | SyntaxKind::EOF
            | SyntaxKind::ERROR_TOKEN
            | SyntaxKind::L_PAREN
            | SyntaxKind::R_PAREN
            | SyntaxKind::L_BRACE
            | SyntaxKind::R_BRACE
            | SyntaxKind::L_BRACKET
            | SyntaxKind::R_BRACKET
            | SyntaxKind::COMMA
            | SyntaxKind::DOT
            | SyntaxKind::COLON
            | SyntaxKind::PIPE
            | SyntaxKind::BACKSLASH
            | SyntaxKind::DOLLAR
    ) {
        return None;
    }

    // Direct mappings by SyntaxKind
    if kind == SyntaxKind::LINE_COMMENT || kind == SyntaxKind::BLOCK_COMMENT {
        return Some(Classification {
            token_type: TT_COMMENT,
            modifiers: 0,
        });
    }

    if kind.is_keyword() {
        return Some(Classification {
            token_type: TT_KEYWORD,
            modifiers: 0,
        });
    }

    if kind == SyntaxKind::INTEGER || kind == SyntaxKind::FLOAT {
        return Some(Classification {
            token_type: TT_NUMBER,
            modifiers: 0,
        });
    }

    if matches!(
        kind,
        SyntaxKind::STRING_TEXT | SyntaxKind::STRING_ESCAPE | SyntaxKind::QUOTE
    ) {
        return Some(Classification {
            token_type: TT_STRING,
            modifiers: 0,
        });
    }

    if matches!(
        kind,
        SyntaxKind::DIVERT
            | SyntaxKind::THREAD
            | SyntaxKind::TUNNEL_ONWARDS
            | SyntaxKind::GLUE
            | SyntaxKind::TILDE
            | SyntaxKind::EQ
            | SyntaxKind::EQ_EQ
            | SyntaxKind::BANG_EQ
            | SyntaxKind::LT
            | SyntaxKind::GT
            | SyntaxKind::LT_EQ
            | SyntaxKind::GT_EQ
            | SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::PERCENT
            | SyntaxKind::CARET
            | SyntaxKind::BANG
            | SyntaxKind::QUESTION
            | SyntaxKind::BANG_QUESTION
            | SyntaxKind::AMP
            | SyntaxKind::AMP_AMP
            | SyntaxKind::PLUS_EQ
            | SyntaxKind::MINUS_EQ
    ) {
        return Some(Classification {
            token_type: TT_OPERATOR,
            modifiers: 0,
        });
    }

    if kind == SyntaxKind::HASH {
        return Some(Classification {
            token_type: TT_DECORATOR,
            modifiers: 0,
        });
    }

    // IDENT classification — context-dependent
    if kind == SyntaxKind::IDENT {
        return classify_ident(token, resolution_index);
    }

    None
}

fn classify_ident(
    token: &brink_syntax::SyntaxToken,
    resolution_index: &HashMap<TextRange, SymbolKind>,
) -> Option<Classification> {
    let parent = token.parent()?;
    let parent_kind = parent.kind();

    // IDENT directly in LIST_MEMBER_ON / LIST_MEMBER_OFF (no intermediate IDENTIFIER)
    if parent_kind == SyntaxKind::LIST_MEMBER_ON || parent_kind == SyntaxKind::LIST_MEMBER_OFF {
        return Some(Classification {
            token_type: TT_ENUM_MEMBER,
            modifiers: MOD_DECLARATION,
        });
    }

    // IDENT inside IDENTIFIER node — check grandparent
    if parent_kind == SyntaxKind::IDENTIFIER
        && let Some(grandparent) = parent.parent()
    {
        let gp_kind = grandparent.kind();
        return match gp_kind {
            SyntaxKind::KNOT_HEADER => Some(Classification {
                token_type: TT_NAMESPACE,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::STITCH_HEADER | SyntaxKind::EXTERNAL_DECL => Some(Classification {
                token_type: TT_FUNCTION,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::KNOT_PARAM_DECL => Some(Classification {
                token_type: TT_PARAMETER,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::LABEL => Some(Classification {
                token_type: TT_LABEL,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::VAR_DECL | SyntaxKind::TEMP_DECL => Some(Classification {
                token_type: TT_VARIABLE,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::CONST_DECL => Some(Classification {
                token_type: TT_VARIABLE,
                modifiers: MOD_DECLARATION | MOD_READONLY,
            }),
            SyntaxKind::LIST_DECL => Some(Classification {
                token_type: TT_ENUM,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::FUNCTION_CALL => Some(Classification {
                token_type: TT_FUNCTION,
                modifiers: 0,
            }),
            // PATH or other contexts — try resolution index
            _ => Some(classify_ident_by_resolution(token, resolution_index)),
        };
    }

    // Fallback: try resolution index
    Some(classify_ident_by_resolution(token, resolution_index))
}

fn classify_ident_by_resolution(
    token: &brink_syntax::SyntaxToken,
    resolution_index: &HashMap<TextRange, SymbolKind>,
) -> Classification {
    // Try the token's own range first
    if let Some(&sym_kind) = resolution_index.get(&token.text_range()) {
        return symbol_kind_to_classification(sym_kind);
    }

    // Try the parent IDENTIFIER node range (resolutions may use the node range)
    if let Some(parent) = token.parent() {
        if parent.kind() == SyntaxKind::IDENTIFIER
            && let Some(&sym_kind) = resolution_index.get(&parent.text_range())
        {
            return symbol_kind_to_classification(sym_kind);
        }
        // Try the PATH grandparent range too
        if parent.kind() == SyntaxKind::IDENTIFIER
            && let Some(grandparent) = parent.parent()
            && grandparent.kind() == SyntaxKind::PATH
            && let Some(&sym_kind) = resolution_index.get(&grandparent.text_range())
        {
            return symbol_kind_to_classification(sym_kind);
        }
    }

    // Fallback: generic variable
    Classification {
        token_type: TT_VARIABLE,
        modifiers: 0,
    }
}

fn symbol_kind_to_classification(kind: SymbolKind) -> Classification {
    match kind {
        SymbolKind::Knot => Classification {
            token_type: TT_NAMESPACE,
            modifiers: 0,
        },
        SymbolKind::Stitch | SymbolKind::External => Classification {
            token_type: TT_FUNCTION,
            modifiers: 0,
        },
        SymbolKind::Variable | SymbolKind::Temp => Classification {
            token_type: TT_VARIABLE,
            modifiers: 0,
        },
        SymbolKind::Constant => Classification {
            token_type: TT_VARIABLE,
            modifiers: MOD_READONLY,
        },
        SymbolKind::List => Classification {
            token_type: TT_ENUM,
            modifiers: 0,
        },
        SymbolKind::ListItem => Classification {
            token_type: TT_ENUM_MEMBER,
            modifiers: 0,
        },
        SymbolKind::Label => Classification {
            token_type: TT_LABEL,
            modifiers: 0,
        },
        SymbolKind::Param => Classification {
            token_type: TT_PARAMETER,
            modifiers: 0,
        },
    }
}

// ── Multi-line splitting ───────────────────────────────────────────

fn emit_token(
    token: &brink_syntax::SyntaxToken,
    classification: &Classification,
    idx: &LineIndex,
    out: &mut Vec<RawToken>,
) {
    let text = token.text();
    let start_offset = token.text_range().start();

    // Fast path: single-line token
    if !text.contains('\n') {
        let (line, start_char) = idx.line_col(start_offset);
        let length = utf16_len(text);
        out.push(RawToken {
            line,
            start_char,
            length,
            token_type: classification.token_type,
            modifiers: classification.modifiers,
        });
        return;
    }

    // Multi-line: split by newlines
    let segments: Vec<&str> = text.split('\n').collect();
    let num_segments = segments.len();
    let mut byte_offset = u32::from(start_offset);
    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() && i > 0 {
            // Empty line segment after split — skip but advance offset
            byte_offset += 1; // for the \n
            continue;
        }
        let (line, start_char) = idx.line_col(rowan::TextSize::from(byte_offset));
        let length = utf16_len(segment);
        if length > 0 {
            out.push(RawToken {
                line,
                start_char,
                length,
                token_type: classification.token_type,
                modifiers: classification.modifiers,
            });
        }
        // Advance past this segment + the \n separator
        byte_offset += u32::try_from(segment.len()).unwrap_or(u32::MAX);
        if i < num_segments - 1 {
            byte_offset += 1; // \n
        }
    }
}

fn utf16_len(s: &str) -> u32 {
    s.chars()
        .map(|c| u32::try_from(c.len_utf16()).unwrap_or(1))
        .sum()
}

// ── Delta encoding ─────────────────────────────────────────────────

fn delta_encode(raw_tokens: &[RawToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(raw_tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in raw_tokens {
        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            tok.start_char - prev_start
        } else {
            tok.start_char
        };

        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: tok.length,
            token_type: tok.token_type,
            token_modifiers_bitset: tok.modifiers,
        });

        prev_line = tok.line;
        prev_start = tok.start_char;
    }

    result
}

// ── Public API ─────────────────────────────────────────────────────

/// Compute raw (absolute-position) semantic tokens for the entire file.
pub(crate) fn compute_raw_tokens(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
) -> Vec<RawToken> {
    let resolution_index = build_resolution_index(analysis, file_id);
    let idx = LineIndex::new(source);
    let mut raw_tokens = Vec::new();

    for element in root.descendants_with_tokens() {
        let token = match element {
            rowan::NodeOrToken::Token(t) => t,
            rowan::NodeOrToken::Node(_) => continue,
        };

        if let Some(classification) = classify_token(&token, &resolution_index) {
            emit_token(&token, &classification, &idx, &mut raw_tokens);
        }
    }

    raw_tokens
}

/// Compute semantic tokens for the full document.
pub fn compute_semantic_tokens(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
) -> Vec<SemanticToken> {
    let raw = compute_raw_tokens(source, root, analysis, file_id);
    delta_encode(&raw)
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
    let raw = compute_raw_tokens(source, root, analysis, file_id);
    let filtered: Vec<_> = raw
        .into_iter()
        .filter(|t| t.line >= range_start_line && t.line <= range_end_line)
        .collect();
    delta_encode(&filtered)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use brink_analyzer::AnalysisResult;
    use brink_ir::SymbolIndex;
    use brink_syntax::SyntaxNode;

    fn empty_analysis() -> AnalysisResult {
        AnalysisResult {
            index: SymbolIndex::default(),
            resolutions: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn parse_and_tokens(source: &str) -> Vec<RawToken> {
        let parse = brink_syntax::parse(source);
        let root = parse.syntax();
        let analysis = empty_analysis();
        compute_raw_tokens(source, &root, &analysis, FileId(0))
    }

    #[test]
    fn keywords_are_classified() {
        let tokens = parse_and_tokens("VAR x = 5\n");
        let kw = tokens.iter().find(|t| t.token_type == TT_KEYWORD);
        assert!(kw.is_some(), "expected a keyword token for VAR");
    }

    #[test]
    fn comments_are_classified() {
        let tokens = parse_and_tokens("// hello\n");
        let comment = tokens.iter().find(|t| t.token_type == TT_COMMENT);
        assert!(comment.is_some(), "expected a comment token");
    }

    #[test]
    fn numbers_are_classified() {
        let tokens = parse_and_tokens("VAR x = 42\n");
        let num = tokens.iter().find(|t| t.token_type == TT_NUMBER);
        assert!(num.is_some(), "expected a number token for 42");
    }

    #[test]
    fn strings_are_classified() {
        let tokens = parse_and_tokens("VAR x = \"hello\"\n");
        let string_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TT_STRING)
            .collect();
        assert!(
            !string_tokens.is_empty(),
            "expected string tokens for \"hello\""
        );
    }

    #[test]
    fn operators_are_classified() {
        let tokens = parse_and_tokens("-> knot_name\n");
        let op = tokens.iter().find(|t| t.token_type == TT_OPERATOR);
        assert!(op.is_some(), "expected an operator token for ->");
    }

    #[test]
    fn knot_declaration() {
        let tokens = parse_and_tokens("=== my_knot ===\n");
        let ns = tokens
            .iter()
            .find(|t| t.token_type == TT_NAMESPACE && t.modifiers & MOD_DECLARATION != 0);
        assert!(
            ns.is_some(),
            "expected namespace+declaration for knot header"
        );
    }

    #[test]
    fn stitch_declaration() {
        let tokens = parse_and_tokens("=== my_knot ===\n= my_stitch\n");
        let func = tokens
            .iter()
            .find(|t| t.token_type == TT_FUNCTION && t.modifiers & MOD_DECLARATION != 0);
        assert!(
            func.is_some(),
            "expected function+declaration for stitch header"
        );
    }

    #[test]
    fn var_declaration() {
        let tokens = parse_and_tokens("VAR score = 0\n");
        let var = tokens
            .iter()
            .find(|t| t.token_type == TT_VARIABLE && t.modifiers & MOD_DECLARATION != 0);
        assert!(var.is_some(), "expected variable+declaration for VAR decl");
    }

    #[test]
    fn const_declaration_has_readonly() {
        let tokens = parse_and_tokens("CONST MAX = 100\n");
        let var = tokens.iter().find(|t| {
            t.token_type == TT_VARIABLE
                && t.modifiers & MOD_DECLARATION != 0
                && t.modifiers & MOD_READONLY != 0
        });
        assert!(
            var.is_some(),
            "expected variable+declaration+readonly for CONST decl"
        );
    }

    #[test]
    fn multiline_block_comment() {
        let source = "/* line1\nline2 */\n";
        let tokens = parse_and_tokens(source);
        let comments: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TT_COMMENT)
            .collect();
        assert!(
            comments.len() >= 2,
            "expected at least 2 comment entries for multi-line block comment, got {}",
            comments.len()
        );
        // First on line 0, second on line 1
        assert_eq!(comments[0].line, 0);
        assert_eq!(comments[1].line, 1);
    }

    #[test]
    fn delta_encoding_correctness() {
        let raw = vec![
            RawToken {
                line: 0,
                start_char: 0,
                length: 3,
                token_type: TT_KEYWORD,
                modifiers: 0,
            },
            RawToken {
                line: 0,
                start_char: 4,
                length: 1,
                token_type: TT_VARIABLE,
                modifiers: MOD_DECLARATION,
            },
            RawToken {
                line: 1,
                start_char: 2,
                length: 5,
                token_type: TT_COMMENT,
                modifiers: 0,
            },
        ];
        let encoded = delta_encode(&raw);
        assert_eq!(encoded.len(), 3);

        // First token: delta from origin
        assert_eq!(encoded[0].delta_line, 0);
        assert_eq!(encoded[0].delta_start, 0);
        assert_eq!(encoded[0].length, 3);

        // Second token: same line, delta_start from previous
        assert_eq!(encoded[1].delta_line, 0);
        assert_eq!(encoded[1].delta_start, 4);
        assert_eq!(encoded[1].length, 1);

        // Third token: new line, delta_start is absolute
        assert_eq!(encoded[2].delta_line, 1);
        assert_eq!(encoded[2].delta_start, 2);
        assert_eq!(encoded[2].length, 5);
    }

    #[test]
    fn hash_is_decorator() {
        let tokens = parse_and_tokens("Hello world #tag\n");
        let dec = tokens.iter().find(|t| t.token_type == TT_DECORATOR);
        assert!(dec.is_some(), "expected decorator token for #");
    }

    #[test]
    fn list_declaration() {
        let tokens = parse_and_tokens("LIST colors = red, blue\n");
        let enum_tok = tokens
            .iter()
            .find(|t| t.token_type == TT_ENUM && t.modifiers & MOD_DECLARATION != 0);
        assert!(
            enum_tok.is_some(),
            "expected enum+declaration for LIST name"
        );
    }

    #[test]
    fn full_pipeline_produces_semantic_tokens() {
        let source = "=== start ===\nHello world\n-> END\n";
        let parse = brink_syntax::parse(source);
        let root = parse.syntax();
        let analysis = empty_analysis();
        let tokens = compute_semantic_tokens(source, &root, &analysis, FileId(0));
        assert!(!tokens.is_empty(), "expected non-empty semantic tokens");
    }

    #[test]
    fn range_filter_works() {
        let source = "=== start ===\nHello world\n-> END\n";
        let parse = brink_syntax::parse(source);
        let root: SyntaxNode = parse.syntax();
        let analysis = empty_analysis();

        let all = compute_raw_tokens(source, &root, &analysis, FileId(0));
        let range_tokens = compute_semantic_tokens_range(source, &root, &analysis, FileId(0), 2, 2);

        // Only tokens on line 2
        let all_line2: Vec<_> = all.iter().filter(|t| t.line == 2).collect();
        assert_eq!(range_tokens.len(), all_line2.len());
    }
}
