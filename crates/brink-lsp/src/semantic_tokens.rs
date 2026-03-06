use tower_lsp::lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};

pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,    // knots
    SemanticTokenType::FUNCTION,     // stitches, externals
    SemanticTokenType::VARIABLE,     // variables
    SemanticTokenType::STRING,       // string content
    SemanticTokenType::NUMBER,       // numeric literals
    SemanticTokenType::KEYWORD,      // VAR, CONST, LIST, INCLUDE, etc.
    SemanticTokenType::OPERATOR,     // ->, <-, ~, etc.
    SemanticTokenType::COMMENT,      // // and /* */
    SemanticTokenType::ENUM,         // list names
    SemanticTokenType::ENUM_MEMBER,  // list items
    SemanticTokenType::PARAMETER,    // function/knot params
    SemanticTokenType::DECORATOR,    // tags (#)
    SemanticTokenType::new("label"), // labels, gather names
];

pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,   // CONST
    SemanticTokenModifier::DEPRECATED, // future use
];

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}
