/// All syntactic constructs in the Ink language.
///
/// Tokens (lexer output) and nodes (parser output) share a single flat enum
/// so that `rowan` can store them in one `u16` discriminant. Use [`is_token`]
/// and [`is_node`] to classify at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[expect(non_camel_case_types)]
pub enum SyntaxKind {
    // ── Trivia tokens ─────────────────────────────────────────────
    /// Spaces and tabs (NOT newlines).
    WHITESPACE = 0,
    /// `\n` or `\r\n`.
    NEWLINE,
    /// `// ...` through end-of-line.
    LINE_COMMENT,
    /// `/* ... */` (may span lines).
    BLOCK_COMMENT,

    // ── Keyword tokens ────────────────────────────────────────────
    KW_INCLUDE,
    KW_EXTERNAL,
    KW_VAR,
    KW_CONST,
    KW_LIST,
    KW_TEMP,
    KW_RETURN,
    KW_REF,
    KW_TRUE,
    KW_FALSE,
    KW_NOT,
    KW_AND,
    KW_OR,
    KW_MOD,
    KW_HAS,
    KW_HASNT,
    KW_ELSE,
    KW_FUNCTION,
    KW_STOPPING,
    KW_CYCLE,
    KW_SHUFFLE,
    KW_ONCE,
    KW_DONE,
    KW_END,
    KW_TODO,

    // ── Punctuation / operator tokens ────────────────────────────
    /// `=`
    EQ,
    /// `+=`
    PLUS_EQ,
    /// `-=`
    MINUS_EQ,
    /// `==`
    EQ_EQ,
    /// `!=`
    BANG_EQ,
    /// `<`
    LT,
    /// `>`
    GT,
    /// `<=`
    LT_EQ,
    /// `>=`
    GT_EQ,
    /// `&`
    AMP,
    /// `&&`
    AMP_AMP,
    /// `+`
    PLUS,
    /// `-`
    MINUS,
    /// `*`
    STAR,
    /// `/`
    SLASH,
    /// `%`
    PERCENT,
    /// `^`
    CARET,
    /// `!`
    BANG,
    /// `?`
    QUESTION,
    /// `!?`
    BANG_QUESTION,
    /// `$`
    DOLLAR,
    /// `(`
    L_PAREN,
    /// `)`
    R_PAREN,
    /// `{`
    L_BRACE,
    /// `}`
    R_BRACE,
    /// `[`
    L_BRACKET,
    /// `]`
    R_BRACKET,
    /// `|`
    PIPE,
    /// `,`
    COMMA,
    /// `.`
    DOT,
    /// `:`
    COLON,
    /// `#`
    HASH,
    /// `~`
    TILDE,
    /// `\`
    BACKSLASH,

    // ── Compound tokens ──────────────────────────────────────────
    /// `<>`
    GLUE,
    /// `->`
    DIVERT,
    /// `<-`
    THREAD,
    /// `->->`
    TUNNEL_ONWARDS,

    // ── Content tokens ───────────────────────────────────────────
    /// Integer literal (digits only; no leading sign).
    INTEGER,
    /// Float literal (`digits.digits`).
    FLOAT,
    /// `"` (opening or closing quote).
    QUOTE,
    /// Run of non-special characters inside a string literal.
    STRING_TEXT,
    /// Escape sequence inside a string (`\n`, `\t`, `\\`, `\"`).
    STRING_ESCAPE,
    /// Identifier: `(IDENT_START IDENT_CONTINUE*) | (DIGIT+ IDENT_START IDENT_CONTINUE*)`.
    IDENT,
    /// Any byte the lexer could not classify.
    ERROR_TOKEN,
    /// End of file (synthetic).
    EOF,

    // ── Node kinds (parser) ──────────────────────────────────────
    SOURCE_FILE,
    INCLUDE_STMT,
    FILE_PATH,
    EXTERNAL_DECL,
    KNOT_DEF,
    KNOT_HEADER,
    KNOT_BODY,
    KNOT_PARAMS,
    KNOT_PARAM_DECL,
    STITCH_DEF,
    STITCH_HEADER,
    STITCH_BODY,
    EMPTY_LINE,
    AUTHOR_WARNING,
    LOGIC_LINE,
    CONTENT_LINE,
    TAG_LINE,
    STRAY_CLOSING_BRACE,
    RETURN_STMT,
    TEMP_DECL,
    ASSIGNMENT,
    MIXED_CONTENT,
    TEXT,
    ESCAPE,
    GLUE_NODE,
    CHOICE,
    CHOICE_BULLETS,
    LABEL,
    CHOICE_CONDITION,
    CHOICE_START_CONTENT,
    CHOICE_BRACKET_CONTENT,
    CHOICE_INNER_CONTENT,
    GATHER,
    GATHER_DASHES,
    TAGS,
    TAG,
    INLINE_LOGIC,
    MULTILINE_BLOCK,
    SEQUENCE_WITH_ANNOTATION,
    SEQUENCE_SYMBOL_ANNOTATION,
    SEQUENCE_WORD_ANNOTATION,
    INLINE_BRANCHES_SEQ,
    MULTILINE_BRANCHES_SEQ,
    MULTILINE_BRANCH_SEQ,
    BRANCH_CONTENT,
    CONDITIONAL_WITH_EXPR,
    BRANCHLESS_COND_BODY,
    ELSE_BRANCH,
    INLINE_BRANCHES_COND,
    MULTILINE_BRANCHES_COND,
    MULTILINE_CONDITIONAL,
    MULTILINE_BRANCH_COND,
    MULTILINE_BRANCH_BODY,
    IMPLICIT_SEQUENCE,
    INNER_EXPRESSION,
    PREFIX_EXPR,
    POSTFIX_EXPR,
    INFIX_EXPR,
    PAREN_EXPR,
    FUNCTION_CALL,
    ARG_LIST,
    DIVERT_TARGET_EXPR,
    LIST_EXPR,
    DIVERT_NODE,
    SIMPLE_DIVERT,
    DIVERT_TARGET_WITH_ARGS,
    THREAD_START,
    TUNNEL_ONWARDS_NODE,
    TUNNEL_CALL_NODE,
    IDENTIFIER,
    PATH,
    VAR_DECL,
    CONST_DECL,
    LIST_DECL,
    LIST_DEF,
    LIST_MEMBER,
    LIST_MEMBER_ON,
    LIST_MEMBER_OFF,
    FUNCTION_PARAM_LIST,
    INTEGER_LIT,
    FLOAT_LIT,
    STRING_LIT,
    BOOLEAN_LIT,
    ERROR,

    // Not a real kind — used only for `rowan::Language::kind_to_raw` bounds.
    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    /// Returns `true` for tokens produced by the lexer (leaf nodes in the CST).
    #[must_use]
    pub fn is_token(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE
                | Self::NEWLINE
                | Self::LINE_COMMENT
                | Self::BLOCK_COMMENT
                | Self::KW_INCLUDE
                | Self::KW_EXTERNAL
                | Self::KW_VAR
                | Self::KW_CONST
                | Self::KW_LIST
                | Self::KW_TEMP
                | Self::KW_RETURN
                | Self::KW_REF
                | Self::KW_TRUE
                | Self::KW_FALSE
                | Self::KW_NOT
                | Self::KW_AND
                | Self::KW_OR
                | Self::KW_MOD
                | Self::KW_HAS
                | Self::KW_HASNT
                | Self::KW_ELSE
                | Self::KW_FUNCTION
                | Self::KW_STOPPING
                | Self::KW_CYCLE
                | Self::KW_SHUFFLE
                | Self::KW_ONCE
                | Self::KW_DONE
                | Self::KW_END
                | Self::KW_TODO
                | Self::EQ
                | Self::PLUS_EQ
                | Self::MINUS_EQ
                | Self::EQ_EQ
                | Self::BANG_EQ
                | Self::LT
                | Self::GT
                | Self::LT_EQ
                | Self::GT_EQ
                | Self::AMP
                | Self::AMP_AMP
                | Self::PLUS
                | Self::MINUS
                | Self::STAR
                | Self::SLASH
                | Self::PERCENT
                | Self::CARET
                | Self::BANG
                | Self::QUESTION
                | Self::BANG_QUESTION
                | Self::DOLLAR
                | Self::L_PAREN
                | Self::R_PAREN
                | Self::L_BRACE
                | Self::R_BRACE
                | Self::L_BRACKET
                | Self::R_BRACKET
                | Self::PIPE
                | Self::COMMA
                | Self::DOT
                | Self::COLON
                | Self::HASH
                | Self::TILDE
                | Self::BACKSLASH
                | Self::GLUE
                | Self::DIVERT
                | Self::THREAD
                | Self::TUNNEL_ONWARDS
                | Self::INTEGER
                | Self::FLOAT
                | Self::QUOTE
                | Self::STRING_TEXT
                | Self::STRING_ESCAPE
                | Self::IDENT
                | Self::ERROR_TOKEN
                | Self::EOF
        )
    }

    /// Returns `true` for composite nodes built by the parser.
    #[must_use]
    pub fn is_node(self) -> bool {
        !self.is_token() && self != Self::__LAST
    }

    /// Returns `true` for trivia — tokens the parser may skip over.
    /// `NEWLINE` is **not** trivia; it terminates lines and delimits blocks.
    #[must_use]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE | Self::LINE_COMMENT | Self::BLOCK_COMMENT
        )
    }

    /// Returns `true` for keyword tokens.
    #[must_use]
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::KW_INCLUDE
                | Self::KW_EXTERNAL
                | Self::KW_VAR
                | Self::KW_CONST
                | Self::KW_LIST
                | Self::KW_TEMP
                | Self::KW_RETURN
                | Self::KW_REF
                | Self::KW_TRUE
                | Self::KW_FALSE
                | Self::KW_NOT
                | Self::KW_AND
                | Self::KW_OR
                | Self::KW_MOD
                | Self::KW_HAS
                | Self::KW_HASNT
                | Self::KW_ELSE
                | Self::KW_FUNCTION
                | Self::KW_STOPPING
                | Self::KW_CYCLE
                | Self::KW_SHUFFLE
                | Self::KW_ONCE
                | Self::KW_DONE
                | Self::KW_END
                | Self::KW_TODO
        )
    }
}

/// Rowan language tag for Ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InkLanguage {}

impl rowan::Language for InkLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(raw.0 < SyntaxKind::__LAST as u16);
        // SAFETY: `SyntaxKind` is `#[repr(u16)]` with contiguous discriminants,
        // and we just checked bounds.
        #[expect(unsafe_code, reason = "repr(u16) transmute with bounds check")]
        unsafe {
            std::mem::transmute::<u16, SyntaxKind>(raw.0)
        }
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

/// A rowan `SyntaxNode` parameterized by [`InkLanguage`].
pub type SyntaxNode = rowan::SyntaxNode<InkLanguage>;
/// A rowan `SyntaxToken` parameterized by [`InkLanguage`].
pub type SyntaxToken = rowan::SyntaxToken<InkLanguage>;
/// A rowan `SyntaxElement` parameterized by [`InkLanguage`].
pub type SyntaxElement = rowan::SyntaxElement<InkLanguage>;

#[cfg(test)]
mod tests {
    use super::*;
    use rowan::Language;

    #[test]
    fn roundtrip_through_rowan() {
        // Every SyntaxKind (except __LAST) should survive raw → kind → raw.
        let mut i = 0u16;
        loop {
            if i == SyntaxKind::__LAST as u16 {
                break;
            }
            let raw = rowan::SyntaxKind(i);
            let kind = InkLanguage::kind_from_raw(raw);
            let back = InkLanguage::kind_to_raw(kind);
            assert_eq!(raw, back, "roundtrip failed for discriminant {i}");
            i += 1;
        }
    }

    #[test]
    fn token_node_partition() {
        // Every kind (except __LAST) is either a token or a node, never both.
        let mut i = 0u16;
        loop {
            if i == SyntaxKind::__LAST as u16 {
                break;
            }
            let kind = InkLanguage::kind_from_raw(rowan::SyntaxKind(i));
            assert!(
                kind.is_token() ^ kind.is_node(),
                "{kind:?} is neither token nor node (or both)"
            );
            i += 1;
        }
    }

    #[test]
    fn trivia_is_subset_of_tokens() {
        let mut i = 0u16;
        loop {
            if i == SyntaxKind::__LAST as u16 {
                break;
            }
            let kind = InkLanguage::kind_from_raw(rowan::SyntaxKind(i));
            if kind.is_trivia() {
                assert!(kind.is_token(), "{kind:?} is trivia but not a token");
            }
            i += 1;
        }
    }

    #[test]
    fn newline_is_not_trivia() {
        assert!(!SyntaxKind::NEWLINE.is_trivia());
        assert!(SyntaxKind::NEWLINE.is_token());
    }

    #[test]
    fn keywords_are_tokens() {
        let keywords = [
            SyntaxKind::KW_INCLUDE,
            SyntaxKind::KW_EXTERNAL,
            SyntaxKind::KW_VAR,
            SyntaxKind::KW_CONST,
            SyntaxKind::KW_LIST,
            SyntaxKind::KW_TEMP,
            SyntaxKind::KW_RETURN,
            SyntaxKind::KW_REF,
            SyntaxKind::KW_TRUE,
            SyntaxKind::KW_FALSE,
            SyntaxKind::KW_NOT,
            SyntaxKind::KW_AND,
            SyntaxKind::KW_OR,
            SyntaxKind::KW_MOD,
            SyntaxKind::KW_HAS,
            SyntaxKind::KW_HASNT,
            SyntaxKind::KW_ELSE,
            SyntaxKind::KW_FUNCTION,
            SyntaxKind::KW_STOPPING,
            SyntaxKind::KW_CYCLE,
            SyntaxKind::KW_SHUFFLE,
            SyntaxKind::KW_ONCE,
            SyntaxKind::KW_DONE,
            SyntaxKind::KW_END,
            SyntaxKind::KW_TODO,
        ];
        for kw in keywords {
            assert!(kw.is_token(), "{kw:?} should be a token");
            assert!(kw.is_keyword(), "{kw:?} should be a keyword");
        }
    }

    #[test]
    fn non_keywords_are_not_keywords() {
        assert!(!SyntaxKind::IDENT.is_keyword());
        assert!(!SyntaxKind::PLUS.is_keyword());
        assert!(!SyntaxKind::SOURCE_FILE.is_keyword());
    }
}
