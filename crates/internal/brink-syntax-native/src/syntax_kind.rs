/// All syntactic constructs in the native `.brink` grammar.
///
/// This is a **peer** enum to `brink-syntax`'s ink-shaped `SyntaxKind` — its
/// own discriminant space, sharing no numbering with the ink frontend (NF-1
/// ruling, 2026-07-19: a new crate, not a co-located module, because the
/// only reason to share `SyntaxKind` space was `AstPtr` interop, which the
/// HIR admission contract's opaque `Provenance` already removed). Tokens
/// (lexer output) and nodes (parser output) share one flat enum so `rowan`
/// can store them in a single `u16` discriminant — see [`is_token`] /
/// [`is_node`].
///
/// Scope: B0.5 (`docs/b0-sequencing.md` §B0.5) — the token set and
/// error-resilient CST for the *ruled* native surface subset (NF-2:
/// writer-sufficient, not the full charter). No HIR lowering happens in
/// this crate; that is B0.6/B0.7/B0.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[expect(non_camel_case_types)]
pub enum SyntaxKind {
    // ── Trivia tokens ─────────────────────────────────────────────
    /// Spaces and tabs (NOT newlines). A leading UTF-8 BOM is folded in here
    /// too (lossless-roundtrip requirement — see lexer tests).
    WHITESPACE = 0,
    /// `\n` or `\r\n`.
    NEWLINE,
    /// `// ...` through end-of-line.
    LINE_COMMENT,
    /// `/* ... */` (may span lines; unterminated block comments run to EOF —
    /// recorded as a parse error, never a panic).
    BLOCK_COMMENT,

    // ── Keyword tokens — hard-reserved (Finding #1: if/match/else/as are
    // reserved everywhere, Rust-style, not contextual like ink's T1b
    // keywords — the charter doesn't rule this either way; RustScript's own
    // north star reserves them globally, so this is the coherent default) ──
    /// `flow` — story-time container (Coloring axis, charter §3).
    KW_FLOW,
    /// `fn` — expression-time container (Coloring axis, charter §3).
    KW_FN,
    KW_VAR,
    KW_CONST,
    /// `flags` — renamed LIST (charter §11).
    KW_FLAGS,
    KW_STRUCT,
    KW_EXTERN,
    KW_IMPORT,
    KW_USE,
    KW_MODULE,
    /// `return` — leave this container; also the tunnel-return respelling's
    /// first half (`return -> x`, charter §11).
    KW_RETURN,
    /// `ref` — ref-argument marker (kept from ink).
    KW_REF,
    /// `if` — word-annotated brace family member (charter §6) AND (Finding
    /// #1) reserved as a code-ground keyword everywhere.
    KW_IF,
    /// `match` — word-annotated brace family member (charter §6).
    KW_MATCH,
    /// `else` — conditional else-arm AND a choice point's fallback branch
    /// (charter §11: "a choice point's fallback is its else-branch").
    KW_ELSE,
    /// `as` — import/use aliasing (`use a::b as c`).
    KW_AS,
    KW_TRUE,
    KW_FALSE,
    /// `END` — divert target sentinel (kept verbatim, charter §11).
    KW_END,
    /// `DONE` — divert target sentinel (kept verbatim, charter §11).
    KW_DONE,

    // ── Punctuation / operator tokens ────────────────────────────
    /// `=`
    EQ,
    /// `+=`
    PLUS_EQ,
    /// `-=`
    MINUS_EQ,
    /// `*=`
    STAR_EQ,
    /// `/=`
    SLASH_EQ,
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
    /// `-`. Also the entry-marker sigil (charter §6) and the choice-list
    /// once-bullet-adjacent dash; the parser, not the lexer, decides which
    /// role a given `-` plays from structural position.
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
    /// `?`. Also half of the `{?` choice-point opener — the parser
    /// recognizes the *adjacent* `L_BRACE QUESTION` pair, no compound token
    /// (mirrors how `{if`/`{match`/`{~` are recognized: `{` is always plain
    /// `L_BRACE`, disambiguation is a parser lookahead, not a lexer job).
    QUESTION,
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
    /// `|`. Two adjacent `PIPE`s are NOT compounded into a logical-or token
    /// (mirrors brink-syntax precedent for `||`/`++`/`--`) — the parser
    /// disambiguates `a || b` (logical or) from `|x|` (lambda params) by
    /// expression-vs-lambda-head position, not lexical shape.
    PIPE,
    /// `,`
    COMMA,
    /// `.` — the intra-module separator (containers, fields, variants,
    /// UFCS; charter §13.2).
    DOT,
    /// `:`
    COLON,
    /// `::` — the module-wall separator (charter §13.2). Lexed as one
    /// compound token so a bare `:` (used in inline `{if cond: …}` bodies)
    /// never gets swallowed by a stray adjacent colon.
    COLON_COLON,
    /// `#` — tag opener (charter §11: tags kept).
    HASH,
    /// `~`. Also an alternation-family opener (`{~ }` shuffle, charter §6).
    TILDE,
    /// `\`
    BACKSLASH,
    /// `@`. A lone `@` outside the `@[` pair is not punctuation in this
    /// grammar (mirrors the ink `AT_L_BRACKET` precedent) and is emitted as
    /// `ERROR_TOKEN` so prose containing a bare `@` still round-trips
    /// losslessly instead of being silently absorbed.
    AT,

    // ── Compound tokens ──────────────────────────────────────────
    /// `@[` — annotation-line opener (charter §11 / NS-A2 lineage,
    /// `docs/directive-annotations-spec.md` §5b). Only the *adjacent* pair
    /// opens an annotation line.
    AT_L_BRACKET,
    /// `<>` — glue (kept, charter §11).
    GLUE,
    /// `->` — divert (kept verbatim, charter §11).
    DIVERT,
    /// `<-` — splice, valid only inside a choice point (charter §5).
    THREAD,
    /// `=>` — match-arm separator.
    FAT_ARROW,

    // ── Content tokens ───────────────────────────────────────────
    /// Integer literal (digits only; no leading sign — unary `-` is a
    /// separate `PREFIX_EXPR`).
    INTEGER,
    /// Float literal (`digits.digits`).
    FLOAT,
    /// `"` (opening or closing quote).
    QUOTE,
    /// Run of non-special characters inside a string literal.
    STRING_TEXT,
    /// Escape sequence inside a string (`\n`, `\t`, `\\`, `\"`).
    STRING_ESCAPE,
    /// Identifier: ASCII `[A-Za-z_][A-Za-z0-9_]*` (Finding #2: native
    /// identifiers are ASCII-only in this skeleton — the charter's S4
    /// casing partition (`snake_case`/`UpperCamel`) is an ASCII-shaped rule
    /// and the ink Unicode identifier range table is ink-specific baggage
    /// with no native ruling to inherit it from; widening later is
    /// additive, not breaking).
    IDENT,
    /// Any byte/char the lexer could not classify — including a lone `@`,
    /// unterminated block comments' unreachable tail (folded into
    /// `BLOCK_COMMENT` itself, not this), and raw prose bytes at
    /// declaration scope.
    ERROR_TOKEN,
    /// End of file (synthetic).
    EOF,

    // ── Node kinds — top level & declarations ───────────────────
    SOURCE_FILE,
    /// `flow name(params) { … }` / nested `flow` = stitch (charter §4).
    FLOW_DECL,
    /// `fn name(params) { … }`.
    FN_DECL,
    /// Shared param-list shape for `FLOW_DECL`/`FN_DECL`.
    PARAM_LIST,
    /// One parameter: `ref`? `IDENT`.
    PARAM,
    /// `var name = expr`.
    VAR_DECL,
    /// `const name = expr`.
    CONST_DECL,
    /// `flags Name = (member), member, …` (charter §11).
    FLAGS_DECL,
    FLAGS_MEMBER_LIST,
    /// One flag member; a parenthesized member is the default-on entry.
    FLAGS_MEMBER,
    /// `struct Name { field: type, … }` (charter §13.1's sibling; concrete
    /// grammar shape here, no field-type semantics checked).
    STRUCT_DECL,
    STRUCT_FIELD,
    /// `extern name(params)`.
    EXTERN_DECL,
    /// `use path::{a, b as c};` (Rust `use` lifted verbatim, charter
    /// §13.2).
    USE_DECL,
    /// One `use` tree: a path optionally followed by `{ … }` (nested
    /// group), `as alias`, or bare.
    USE_TREE,
    USE_TREE_LIST,
    /// `import name;` (Finding #3: the charter doesn't separately spell an
    /// `import` grammar distinct from `use` — b0-sequencing's token-set
    /// bullet lists `import` as its own decl keyword alongside `use`
    /// regardless, so this skeleton gives it the minimal reasonable shape:
    /// a single bare path statement, distinct node from `USE_DECL`. Real
    /// semantics (whole-module import vs name-import) are B0.6's call).
    IMPORT_DECL,
    /// `module name { … }` — a nested module block (charter §13.2: files
    /// hold declared `module` blocks nesting within them).
    MODULE_DECL,

    // ── Node kinds — bodies & content ───────────────────────────
    /// A brace-delimited body: `{ BodyItem* }`. Universal body delimiter
    /// (charter §4) for flow/fn/module bodies and nested block content.
    BLOCK,
    /// A single line of prose content, generic text interspersed with
    /// interpolation/glue, terminated by `NEWLINE` or EOF.
    CONTENT_LINE,
    /// A run of literal text inside a `CONTENT_LINE` (no escapes, no
    /// interpolation — those break the run).
    TEXT,
    /// `{expr}` — bare-brace interpolation, and nothing else, ever (charter
    /// §6).
    INTERPOLATION,
    /// `<>` glue, in content position.
    GLUE_NODE,
    /// `# tag text` — a tag line (charter §11: tags kept).
    TAG_LINE,
    /// One `#`-prefixed tag inside a `TAG_LINE` or a `CONTENT_LINE`'s
    /// trailing-tags tail.
    TAG,

    // ── Node kinds — choice points (charter §5) ─────────────────
    /// `{? … }` — an explicit choice point.
    CHOICE_POINT,
    /// One `*`/`+` choice line inside a `CHOICE_POINT`.
    CHOICE,
    /// `*` (once) or `+` (sticky) bullet token wrapper.
    CHOICE_BULLET,
    /// `(name)` — a choice label (kept, charter §11).
    LABEL,
    /// `{if cond}` — a choice guard.
    CHOICE_GUARD,
    /// The `text[bracket]inner` display-split anatomy of a choice line
    /// (kept as-is, charter §5).
    CHOICE_START_CONTENT,
    CHOICE_BRACKET_CONTENT,
    CHOICE_INNER_CONTENT,
    /// A choice's braced nested-content body (charter §5: "choice bodies
    /// take braces when they have nested content").
    CHOICE_BODY,
    /// `else { … }` — a choice point's fallback branch (charter §11).
    ELSE_BRANCH,
    /// `<- flow(args)` — a splice inside a choice point (charter §5).
    SPLICE,

    // ── Node kinds — the annotated-brace family (charter §6) ────
    /// `{if cond { … } else { … }}` / `{if cond: … else: …}` (Finding #4:
    /// this skeleton accepts BOTH an inline colon-body form and a braced
    /// multiline-arm form for `if`/`match` rather than the entry-marker-`-`
    /// form charter §6 documents for the *alternation* family — the
    /// charter itself flags entry-marker anatomy as "under-understood even
    /// by the implementer," and nothing in the charter says `-` arms apply
    /// to `if`/`match` specifically, so branches use the brace delimiter
    /// charter §4 already declares universal, and dashes are reserved for
    /// alternation blocks below. Flagged for the Track-B queue to confirm
    /// or correct.)
    CONDITIONAL_BLOCK,
    IF_ARM,
    MATCH_ARM,
    /// A `match` arm's pattern (kept intentionally shallow — a bare
    /// expression grammar reused, not a real pattern language; exhaustive
    /// pattern matching is out of B0.5's scope).
    MATCH_PATTERN,

    /// `{~ … }` shuffle / `{& … }` cycle / `{! … }` once / `{| … }`
    /// stopping-sequence — one node shape, `ALTERNATION_MARKER` child
    /// records which.
    ALTERNATION_BLOCK,
    /// The `~`/`&`/`!`/`|` token that opened an `ALTERNATION_BLOCK`.
    ALTERNATION_MARKER,
    /// One `-`-prefixed entry/arm inside a multiline `ALTERNATION_BLOCK`
    /// (charter §6). Runs until the next `-` or the closing `}`.
    ENTRY,

    /// `{? … }`'s sibling annotation-position dispatch already lives under
    /// `CHOICE_POINT` above; this marker exists only so the family's
    /// dispatch site has one name to log against in doc comments — not a
    /// real node, never emitted. (Kept out of `is_node`/`is_token` via the
    /// `__LAST` sentinel below being the true boundary; this variant is
    /// unused and reserved as a documentation anchor only.)
    // (intentionally no variant here — CHOICE_POINT already covers it)

    // ── Node kinds — annotations (charter §11, `@[…]`) ──────────
    /// `@[name(args)]` (directive-annotations-spec.md §5b's paren-clause
    /// grammar, e.g. `@[effects(pure, silent, reads(gold, hp))]`).
    ANNOTATION_LINE,
    /// The parenthesized, comma-separated argument list of an annotation
    /// or nested paren-clause.
    ANNOTATION_ARGS,
    /// One argument: a bare `IDENT`, or `IDENT(ANNOTATION_ARGS)` (the
    /// nested paren-clause form, e.g. `reads(gold, hp)` nested inside
    /// `effects(…)`).
    ANNOTATION_ARG,

    // ── Node kinds — diverts, tunnels, return (charter §11) ─────
    /// `-> target` — kept verbatim.
    DIVERT_STMT,
    /// `-> place ->` — a tunnel call (kept, charter §11): divert, target,
    /// divert, with nothing else before the line ends.
    TUNNEL_CALL,
    /// A divert target: `END` / `DONE` / a `PATH`.
    DIVERT_TARGET,
    /// `return` — leave this container.
    RETURN_STMT,
    /// `return -> x` — the tunnel-return respelling (charter §11):
    /// `RETURN_STMT` immediately followed by a divert to `x`.
    RETURN_REDIRECT,

    // ── Node kinds — paths (charter §13.2) ───────────────────────
    /// A dotted/`::`-separated name path. `::` crosses module walls, `.`
    /// walks everything inside.
    PATH,
    PATH_SEGMENT,

    // ── Node kinds — a minimal expression grammar ────────────────
    // Shared by interpolation content, annotation args, choice guards,
    // divert targets, and conditional/match heads. Real code-dialect
    // statement grammar (let/assign/if-stmt/while/for/UFCS-calls/etc.) is
    // explicitly B0.8 (`docs/b0-sequencing.md` §B0.8) — this is the
    // expression *skeleton* B0.5 needs to give the constructs above a real
    // (not just balanced-token) internal shape.
    INTEGER_LIT,
    FLOAT_LIT,
    STRING_LIT,
    BOOLEAN_LIT,
    PATH_EXPR,
    PAREN_EXPR,
    PREFIX_EXPR,
    INFIX_EXPR,
    CALL_EXPR,
    ARG_LIST,
    /// `|x, y| expr` — lambda pipes, tokenized and structurally parsed;
    /// lowering is explicitly deferred (charter §7/§8: "B0.5 tokenizes
    /// pipes; B0.8 does not lower them").
    LAMBDA_EXPR,
    LAMBDA_PARAMS,

    /// A parse-error wrapper node — swallows one unexpected token so error
    /// recovery always makes forward progress.
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
                | Self::KW_FLOW
                | Self::KW_FN
                | Self::KW_VAR
                | Self::KW_CONST
                | Self::KW_FLAGS
                | Self::KW_STRUCT
                | Self::KW_EXTERN
                | Self::KW_IMPORT
                | Self::KW_USE
                | Self::KW_MODULE
                | Self::KW_RETURN
                | Self::KW_REF
                | Self::KW_IF
                | Self::KW_MATCH
                | Self::KW_ELSE
                | Self::KW_AS
                | Self::KW_TRUE
                | Self::KW_FALSE
                | Self::KW_END
                | Self::KW_DONE
                | Self::EQ
                | Self::PLUS_EQ
                | Self::MINUS_EQ
                | Self::STAR_EQ
                | Self::SLASH_EQ
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
                | Self::COLON_COLON
                | Self::HASH
                | Self::TILDE
                | Self::BACKSLASH
                | Self::AT
                | Self::AT_L_BRACKET
                | Self::GLUE
                | Self::DIVERT
                | Self::THREAD
                | Self::FAT_ARROW
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
            Self::KW_FLOW
                | Self::KW_FN
                | Self::KW_VAR
                | Self::KW_CONST
                | Self::KW_FLAGS
                | Self::KW_STRUCT
                | Self::KW_EXTERN
                | Self::KW_IMPORT
                | Self::KW_USE
                | Self::KW_MODULE
                | Self::KW_RETURN
                | Self::KW_REF
                | Self::KW_IF
                | Self::KW_MATCH
                | Self::KW_ELSE
                | Self::KW_AS
                | Self::KW_TRUE
                | Self::KW_FALSE
                | Self::KW_END
                | Self::KW_DONE
        )
    }
}

/// Rowan language tag for the native `.brink` grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeLanguage {}

impl rowan::Language for NativeLanguage {
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

/// A rowan `SyntaxNode` parameterized by [`NativeLanguage`].
pub type SyntaxNode = rowan::SyntaxNode<NativeLanguage>;
/// A rowan `SyntaxToken` parameterized by [`NativeLanguage`].
pub type SyntaxToken = rowan::SyntaxToken<NativeLanguage>;
/// A rowan `SyntaxElement` parameterized by [`NativeLanguage`].
pub type SyntaxElement = rowan::SyntaxElement<NativeLanguage>;

#[cfg(test)]
mod tests {
    use super::*;
    use rowan::Language;

    #[test]
    fn roundtrip_through_rowan() {
        let mut i = 0u16;
        loop {
            if i == SyntaxKind::__LAST as u16 {
                break;
            }
            let raw = rowan::SyntaxKind(i);
            let kind = NativeLanguage::kind_from_raw(raw);
            let back = NativeLanguage::kind_to_raw(kind);
            assert_eq!(raw, back, "roundtrip failed for discriminant {i}");
            i += 1;
        }
    }

    #[test]
    fn token_node_partition() {
        let mut i = 0u16;
        loop {
            if i == SyntaxKind::__LAST as u16 {
                break;
            }
            let kind = NativeLanguage::kind_from_raw(rowan::SyntaxKind(i));
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
            let kind = NativeLanguage::kind_from_raw(rowan::SyntaxKind(i));
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
            SyntaxKind::KW_FLOW,
            SyntaxKind::KW_FN,
            SyntaxKind::KW_VAR,
            SyntaxKind::KW_CONST,
            SyntaxKind::KW_FLAGS,
            SyntaxKind::KW_STRUCT,
            SyntaxKind::KW_EXTERN,
            SyntaxKind::KW_IMPORT,
            SyntaxKind::KW_USE,
            SyntaxKind::KW_MODULE,
            SyntaxKind::KW_RETURN,
            SyntaxKind::KW_REF,
            SyntaxKind::KW_IF,
            SyntaxKind::KW_MATCH,
            SyntaxKind::KW_ELSE,
            SyntaxKind::KW_AS,
            SyntaxKind::KW_TRUE,
            SyntaxKind::KW_FALSE,
            SyntaxKind::KW_END,
            SyntaxKind::KW_DONE,
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
