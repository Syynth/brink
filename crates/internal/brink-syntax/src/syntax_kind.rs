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
    /// `IMPORT` — module import statement (brink extension, M-2,
    /// docs/modules-spec.md §2). `FROM`/`AS` stay contextual `IDENT`s.
    KW_IMPORT,

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
    /// `IMPORT { a, b AS c } FROM mod` or `IMPORT mod` (M-2).
    IMPORT_STMT,
    /// The `{ … }` name list of a bare-form import.
    IMPORT_LIST,
    /// One `name` or `name AS alias` entry in an import list.
    IMPORT_ITEM,
    /// The module name of an import (both forms).
    IMPORT_MODULE,
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

    // ── T1b superset grammar (docs/t1b-surface-spec.md) ────────────
    // Multi-line `~ { … }` logic blocks (§2). Parse-only in T1b-1 — every
    // node below is dialect-gated at analysis and never reaches LIR.
    /// `{ stmt* }` — a braced statement list. Used for the top-level
    /// `~ { … }` block body and every nested `if`/`while`/`for` body.
    STMT_BLOCK,
    /// `if cond { … } (else …)?`. `if`/`else if` are contextual keywords
    /// (plain `IDENT` tokens) — see `parser::logic`.
    IF_STMT,
    /// The `else` arm of an `IF_STMT`: either a nested `IF_STMT` (else-if)
    /// or a bare `STMT_BLOCK` (else).
    ELSE_CLAUSE,
    /// `while cond { … }`.
    WHILE_STMT,
    /// `for name in expr { … }`.
    FOR_STMT,
    /// `break`.
    BREAK_STMT,
    /// `continue`.
    CONTINUE_STMT,
    /// A bare expression statement inside a block (function/external calls).
    EXPR_STMT,
    /// `#[expr, …]` — array sigil literal (§3). Expression position only.
    ARRAY_LITERAL,
    /// `#{key: expr, …}` — map sigil literal (§3). Expression position only.
    MAP_LITERAL,
    /// One `key: expr` pair inside a `MAP_LITERAL`.
    MAP_ENTRY,
    /// `base[index]` — postfix indexing, chainable (§4).
    INDEX_EXPR,

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──
    // `name: type` after params/VAR/temp declarations, `): type ===` return
    // position. Superset grammar — always parses; dialect-gated (E051 under
    // strict-ink) at analysis, same pattern as T1b.
    /// `: type_expr` — one annotation, attached after an identifier (param,
    /// `VAR`/`temp` name) or a knot header's params (return position).
    TYPE_ANNOTATION,
    /// A type expression: wraps exactly one of `TYPE_NAME`, `TYPE_GENERIC`,
    /// or `TYPE_FN`.
    TYPE_EXPR,
    /// A bare nominal type name (`int`, `float`, `bool`, `string`, `divert`,
    /// `void`, or an unrecognized identifier — semantic validity is an
    /// analyzer concern, not a grammar one).
    TYPE_NAME,
    /// `name<type_expr, …>` — `list<L>`, `array<T>`, `map<K, V>`.
    TYPE_GENERIC,
    /// `fn(type_expr, …): type_expr` — function type. Parses in T1b/TM-2;
    /// types as reserved until T1c.
    TYPE_FN,

    // ── TM-4b structs (docs/typed-mode-spec.md §6) ─────────────────
    // `STRUCT Name = #{ field: type, … }` declaration, `Name#{field: expr, …}`
    // construction literal, postfix `.field` access. Superset grammar —
    // always parses (dialect-gated at analysis, same T1b/TM-2 pattern);
    // `STRUCT` is a contextual (soft) keyword recognized only at top-level
    // declaration-start position (`STRUCT` `IDENT` `=` `#` `{`), so it never
    // reserves the word elsewhere. LIR lowering rejects every construct
    // below — codegen lands with TM-4c (#666).
    /// `STRUCT Name = #{ field: type, … }`. Top-level only.
    STRUCT_DECL,
    /// One `field: type` pair inside a `STRUCT_DECL`'s body.
    STRUCT_FIELD_DECL,
    /// `Name#{field: expr, …}` — struct construction literal. Expression
    /// position only, like `ARRAY_LITERAL`/`MAP_LITERAL`.
    STRUCT_LITERAL,
    /// One `field: expr` pair inside a `STRUCT_LITERAL`.
    STRUCT_FIELD_INIT,
    /// `base.field` — postfix field access. Only used where the existing
    /// dotted-`PATH` grammar doesn't already cover the shape (e.g. after a
    /// `STRUCT_LITERAL`, an `INDEX_EXPR`, or a parenthesized expression); a
    /// bare `ident.ident` chain still parses as one `PATH` node and the
    /// static-path-vs-field-access ambiguity is resolved by
    /// `brink-analyzer`'s resolution fallback (typed-mode-spec §6), not here.
    FIELD_ACCESS_EXPR,

    // ── T1c function values (docs/t1c-spec.md §2) ──────────────────
    /// `#fn(target, args…)` — function-value creation (partial application
    /// over a named function). Joins the `#[…]`/`#{…}`/`Name#{…}` sigil
    /// family: expression position only — in prose position `#` still opens
    /// a tag, unchanged. Superset grammar — always parses; dialect-gated
    /// (E051 under strict-ink) at analysis, same pattern as T1b/TM-4b.
    FN_LITERAL,

    // ── T1e path projections (docs/t1e-spec.md §2) ──────────────────
    // `ref lvalue-path` — path-projection creation in ref-argument position
    // (`heal(ref npc.hp, 5)`, `#fn(heal, ref party[leader].hp)`,
    // `bind(f, ref inventory[idx])`). Superset grammar — always parses in
    // expression position (mirrors `FN_LITERAL`'s dialect-gate pattern);
    // whether the position is legal (ref-argument only, never standalone)
    // and whether the root is a durable cell is `brink-analyzer`'s job.
    /// `ref` followed by a single lvalue-shaped operand — a plain path, a
    /// dotted field chain, `[…]` indexing, or a mix of the two.
    REF_EXPR,

    // ── Computed-callee call attempt (docs/t1c-spec.md §3/§10, issue #869) ──
    // `expr(args…)` where `expr` isn't a bare identifier immediately
    // followed by `(` (that shape is `FUNCTION_CALL`, consumed at `atom()`).
    // Direct-call syntax is RULED (t1c-spec §3) to a bare variable/temp/param
    // callee only; "method-call syntax" (dispatch through an indexed/field/
    // call-result callee via bare-call sugar) is explicitly out of T1c
    // (§10). Superset grammar — always parses, so the author's `(args…)`
    // is captured instead of silently reinterpreted as trailing prose text
    // (the pre-existing behavior, and the exact silent-no-op class #869
    // reports); `brink-ir`'s HIR lowering always rejects it (E100), pointing
    // at the ratified `call(f, args…)` form.
    /// A postfix call applied to a callee that isn't a bare name — always
    /// rejected at HIR lowering (E100).
    CALL_EXPR,

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
                | Self::KW_IMPORT
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
                | Self::KW_IMPORT
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
