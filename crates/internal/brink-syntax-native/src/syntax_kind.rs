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
    /// `// ...` through end-of-line. Also `////+` (four or more slashes) —
    /// Rust precedent: only *exactly* three slashes is a doc comment (see
    /// [`Self::DOC_COMMENT_OUTER`]).
    LINE_COMMENT,
    /// `/* ... */` (may span lines; unterminated block comments run to EOF —
    /// recorded as a parse error, never a panic).
    BLOCK_COMMENT,
    /// `/// ...` through end-of-line — exactly three slashes (a fourth
    /// keeps it a plain [`Self::LINE_COMMENT`]). B0.6b
    /// (`docs/decision-log.md` 2026-07-20): first-class on the native
    /// surface — **not** trivia (see [`Self::is_trivia`]), since the parser
    /// dispatches on this token to decide whether a contiguous run attaches
    /// as a `DOC_COMMENT` CST node to the declaration it immediately
    /// precedes (`parser::doc_comment`).
    DOC_COMMENT_OUTER,
    /// `//! ...` through end-of-line — the inner form (B0.6b, Rust `//!`
    /// precedent, ink had no equivalent). A contiguous run at the very
    /// start of a knot/flow/file body documents the *enclosing* container
    /// rather than a following declaration. Also not trivia.
    DOC_COMMENT_INNER,

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
    /// `let` — a code-ground statement-position binding (B0.8 Wave A,
    /// `docs/decision-log.md` 2026-07-23 "Code-ground sitting"). Distinct
    /// from `var`/`const` (declaration-layer keywords, B0.5): `let`
    /// introduces a `LET_STMT` inside a `STMT_BLOCK`, terminated by `;`
    /// like every other code-ground statement — `var`/`const` keep their
    /// existing terminator-free declaration shape (`parser/decl.rs`'s
    /// `var_decl`/`const_decl` doc comments).
    KW_LET,
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
    /// `while` — code-ground loop statement (B0.8 Wave B,
    /// `docs/decision-log.md` 2026-07-23 "Code-ground sitting"). Hard-
    /// reserved everywhere, mirroring `if`/`match`/`else` (Finding #1) —
    /// unlike the brink-dialect's `~ { … }` T1b grammar, where `while` is a
    /// *contextual* soft keyword (`brink-syntax/src/parser/logic.rs`), the
    /// native surface reserves its `RustScript`-shaped statement keywords
    /// globally.
    KW_WHILE,
    /// `for` — code-ground loop statement (B0.8 Wave B). Hard-reserved,
    /// see [`Self::KW_WHILE`]'s doc.
    KW_FOR,
    /// `in` — the `for name in expr { … }` loop-head separator (B0.8 Wave
    /// B). Hard-reserved, see [`Self::KW_WHILE`]'s doc.
    KW_IN,
    /// `until` — the code-ground condition-park statement (B0.8 Wave B,
    /// decision-log 2026-07-23 item 4): `until <pure-bool-expr>;` parks the
    /// flow until the condition becomes true (reactive), then resumes —
    /// the runtime's existing `FlowSleep` reactive-wake mechanism. Native
    /// **retires** `await` entirely (its future-resolution mental model is
    /// wrong for a condition-park); `until` is the only spelling. Lowers to
    /// the exact same `AwaitStmt` HIR node the brink-dialect's `~ await
    /// cond` produces — a spelling change, not a new construct (NF-2
    /// fence). Hard-reserved, see [`Self::KW_WHILE`]'s doc.
    KW_UNTIL,
    /// `break` — code-ground loop-exit statement (B0.8 Wave B tail, issue
    /// #1322, `docs/decision-log.md` 2026-07-23 "Code-ground sitting").
    /// Hard-reserved, see [`Self::KW_WHILE`]'s doc. No content-ground
    /// counterpart — `break` only has meaning inside a `while`/`for` body.
    KW_BREAK,
    /// `continue` — code-ground loop-skip statement (B0.8 Wave B tail,
    /// issue #1322). Hard-reserved, see [`Self::KW_WHILE`]'s doc. No
    /// content-ground counterpart, same as [`Self::KW_BREAK`].
    KW_CONTINUE,
    /// `as` — import/use aliasing (`use a::b as c`).
    KW_AS,
    /// `or` — B1 `or`-coalescing (`docs/stdlib-spec.md` §1.6a, issue
    /// #1460): `x or default`. A distinct keyword from `||` (boolean
    /// disjunction, still two adjacent `PIPE` tokens — see
    /// `ast::InfixExpr::is_double_pipe`).
    KW_OR,
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
    /// `;` — `use`'s optional statement terminator (charter §13.2's
    /// literal example: `use story::market::{barter, haggle};`, Finding #6:
    /// no *declaration* requires one), **and** the code-ground statement
    /// terminator (B0.8 Wave A, `docs/decision-log.md` 2026-07-23
    /// "Code-ground sitting"): `LET_STMT`/`ASSIGN_STMT`/`EXPR_STMT` inside a
    /// `STMT_BLOCK` each require a trailing `;` — the one thing that
    /// distinguishes a statement from the block's unterminated tail
    /// expression (blocks-as-values).
    SEMICOLON,
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
    /// `@`. A lone `@` outside the `@[` pair is not otherwise meaningful
    /// punctuation in this grammar; it still lexes as its own `AT` token
    /// (not `ERROR_TOKEN`) and folds into plain `TEXT` at parse time, so
    /// prose containing a bare `@` round-trips losslessly and errorlessly
    /// (`docs/directive-annotations-spec.md` §5b: "a lone `@` in prose
    /// stays plain text").
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
    /// Any byte/char the lexer could not classify — unterminated block
    /// comments' unreachable tail (folded into `BLOCK_COMMENT` itself, not
    /// this), and raw prose bytes at declaration scope. Does NOT include a
    /// lone `@`, which lexes as its own `AT` token (see `AT`'s doc
    /// comment) — not this.
    ERROR_TOKEN,
    /// End of file (synthetic).
    EOF,

    // ── Node kinds — doc comments (B0.6b) ────────────────────────
    /// A contiguous run of [`Self::DOC_COMMENT_OUTER`] tokens (the leading
    /// child of the declaration node it documents) or
    /// [`Self::DOC_COMMENT_INNER`] tokens (the leading child of the
    /// enclosing knot/flow/file body it documents) — one node shape for
    /// both variants; `ast::DocComment::is_inner` tells them apart by
    /// inspecting which token kind the node's children carry
    /// (`docs/native-surface-charter.md`'s doc-comment section).
    DOC_COMMENT,

    // ── Node kinds — top level & declarations ───────────────────
    SOURCE_FILE,
    /// `flow name(params) { … }` / nested `flow` = stitch (charter §4).
    FLOW_DECL,
    /// `fn name(params) { … }`.
    FN_DECL,
    /// Shared param-list shape for `FLOW_DECL`/`FN_DECL`.
    PARAM_LIST,
    /// One parameter: `ref`? `IDENT` (`:` type)? (NG-A, #1487). Also the
    /// node lambda parameters use under `LAMBDA_PARAMS` — they used to be
    /// bare `IDENT` tokens directly there, but now each gets its own
    /// `PARAM` so a `: type` annotation attaches to the right one (`ref`
    /// is still not accepted on a lambda parameter).
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
    /// or correct.) The colon-body form's `else:` boundary is recognized
    /// whether it starts its own physical line or trails other content on
    /// the SAME line (#1254 Gap 1, fixed #1261 — `family::colon_body_line`).
    /// A flat `else if <cond> { … }`/`else if <cond>: …` chain (ruled
    /// 2026-07-22, #1258, implemented #1261) lowers to the identical shape
    /// an explicit nested `{if …}` would.
    CONDITIONAL_BLOCK,
    IF_ARM,
    MATCH_ARM,
    /// A `match` arm's pattern (kept intentionally shallow — a bare
    /// expression grammar reused, not a real pattern language; exhaustive
    /// pattern matching is out of B0.5's scope).
    MATCH_PATTERN,

    /// `{~ … }` shuffle / `{& … }` cycle / `{! … }` once / `{| … }`
    /// stopping-sequence — one node shape, `ALTERNATION_MARKER` child
    /// records which. A `{` led by any of these four marker chars is
    /// ALWAYS claimed by this family ahead of bare `{expr}` interpolation
    /// (ruled 2026-07-22, "alternation markers win," #1258/#1261 —
    /// `family::at_alternation`'s doc comment has the full rationale and
    /// the parens escape hatch); a body with zero branches (`{~}`, `{&\n}`)
    /// is a parse error (brink-syntax parity), not silently accepted.
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
    /// `return` — leave this container (content-ground, bare, no value —
    /// `parser/divert.rs::return_stmt`). **Also** reused, unmodified, as
    /// the code-ground `return e?;` value-return statement (B0.8 Wave B
    /// tail, issue #1322, `docs/decision-log.md` 2026-07-23 "Code-ground
    /// sitting" item 1) — `parser/stmt.rs::return_stmt` parses an optional
    /// value expression and a `;` terminator instead. The two grammars
    /// never overlap (dispatched from different parent contexts — content-
    /// ground `BLOCK`/`family.rs` vs. code-ground `STMT_BLOCK`/`stmt.rs`),
    /// so one node shape serves both, mirroring the brink-dialect's own
    /// `RETURN_STMT` (`brink-syntax`), which likewise serves both its bare
    /// container-exit and its valued `~ { … }`-block-return uses.
    /// `ast::ReturnStmt::value()` is a plain "first child expr, if any"
    /// accessor — `None` for the content-ground form (never parses one),
    /// `Some`/`None` for the code-ground form (initializer optional there
    /// too).
    RETURN_STMT,
    /// `return -> x` — the tunnel-return respelling (charter §11):
    /// `RETURN_STMT` immediately followed by a divert to `x`. Content-
    /// ground only — code-ground `return` has no redirect counterpart (a
    /// content-ground/tunnel concept with no code-ground meaning).
    RETURN_REDIRECT,

    // ── Node kinds — paths (charter §13.2) ───────────────────────
    /// A dotted/`::`-separated name path. `::` crosses module walls, `.`
    /// walks everything inside.
    PATH,
    PATH_SEGMENT,

    // ── Node kinds — a minimal expression grammar ────────────────
    // Shared by interpolation content, annotation args, choice guards,
    // divert targets, and conditional/match heads. B0.8 Wave A (below) adds
    // the statement layer (`let`/assignment/expression-statements/blocks-
    // as-values) over this skeleton; B0.8 Wave B (further below) adds
    // `if`/`while`/`for`/`until` control flow, and Wave B tail (issue
    // #1322) adds `return`/`break`/`continue`/compound-assign, as more
    // statement kinds (`docs/b0-sequencing.md` §B0.8, `docs/decision-log.md`
    // 2026-07-23 "Code-ground sitting"). UFCS *resolution* (field-access-
    // wins vs. free-fn desugar) is not a grammar concern at all: the call
    // shape parses and structurally lowers as-is, and the type-directed
    // verdict is `brink-analyzer::ufcs`' job (issue #1482, B3a) — see
    // `brink_ir::hir::lower_native::expr`'s module doc.
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
    /// `|x, y| expr` — lambda pipes. Tokenized and structurally parsed in
    /// B0.5; **lowered** since issue #1685 (`hir::lower_native::lambda` →
    /// `hir::Expr::Lambda`), per the 2026-07-19 ruling.
    LAMBDA_EXPR,
    /// Holds one `PARAM` child per lambda parameter (NG-A, #1487) — each
    /// parameter used to be a bare `IDENT` token directly under this node;
    /// promoting them to `PARAM` lets `: type` attach to the right one.
    LAMBDA_PARAMS,
    /// `[expr, expr, …]` — the array/sequence literal (NG-D, issue #1490,
    /// RULED 2026-07-27: "the everyday collection literal deserves the
    /// lightest spelling"). The B5-symmetric `Array { … }` construction-
    /// registry entry was weighed and rejected in the same ruling —
    /// `L_BRACKET`/`R_BRACKET` were already lexed and idle in expression
    /// position, so this is a new atom, not a `CONSTRUCT_LITERAL` registry
    /// entry. Holds its element expressions as direct children (same shape
    /// as [`Self::ARG_LIST`] — no per-element wrapper node, unlike
    /// [`Self::CONSTRUCT_ENTRY`], since an array element is never a pair).
    /// Distinct from the type-annotation grammar's `<T>` generic-argument
    /// syntax (NG-D's sibling ruling, issue #1552): `[ ]` is for *values*,
    /// `< >` is for *type arguments* — `parser/types.rs` never touches
    /// `L_BRACKET`, so the two spellings cannot collide.
    ARRAY_LITERAL,

    // ── Node kinds — the construction initializer (B5, issue #1464, ─────
    // ── #1103 RULED 2026-07-23, `docs/stdlib-spec.md` §9.6) ─────────────
    /// `TypeName { … }` — the one construction-initializer grammar
    /// (`docs/decision-log.md` 2026-07-23 "Collection/construction
    /// initializer"). The brace *tokens* are fixed surface grammar this
    /// parser produces; **meaning is protocol dispatch**, resolved one
    /// layer up by the `construct` registry
    /// (`brink_ir::hir::construct::ConstructTarget`) against the leading
    /// [`Self::PATH`], never by this grammar. So `Map { "a": 1 }`,
    /// `Flags { Red, Blue }`, `Weighted { 3: "gold" }` and a struct's
    /// `Point { x: 1, y: 2 }` are all one node shape here.
    CONSTRUCT_LITERAL,
    /// One entry of a [`Self::CONSTRUCT_LITERAL`], in whichever of the
    /// three ruled forms the source used: the **element** form (a single
    /// child expression — `Flags { Red }`), or the **pair**/**field** form
    /// (two child expressions around a `COLON` — `Map { k: v }`,
    /// `Point { x: 1 }`). Pair and field are one shape by construction:
    /// they differ only in what the target type makes of the left-hand
    /// expression, which is dispatch, not grammar.
    CONSTRUCT_ENTRY,

    // ── Node kinds — the code-ground statement layer (B0.8 Wave A, ──────
    // ── `docs/decision-log.md` 2026-07-23 "Code-ground sitting") ────────
    // RustScript-shaped statements over the expression skeleton above.
    // Parser only — no HIR lowering yet (that's Wave B, alongside `if`/
    // `while`/`for`/`until`). `parser/stmt.rs` is the dispatcher.
    /// `{ stmt* tail? }` — the code-ground body shape. Blocks-as-values
    /// ruled: an unterminated trailing expression, if present, is the
    /// block's *tail* — a bare (unwrapped) expression child, the last thing
    /// before `R_BRACE`. Reached as an expression atom (`expr::atom`'s
    /// `L_BRACE` case) — a statement-block is itself an expression
    /// (`let x = { … };` is valid), distinct from the content-ground
    /// [`Self::BLOCK`] `flow`/`fn`/`module` bodies still use (that seam is
    /// Wave B's call, not this wave's).
    STMT_BLOCK,
    /// `let name = expr;` (initializer optional). Distinct from
    /// [`Self::VAR_DECL`]/[`Self::CONST_DECL`] — those are declaration-layer
    /// keywords (B0.5, terminator-free); `let` is code-ground, inside a
    /// [`Self::STMT_BLOCK`], and always `;`-terminated.
    LET_STMT,
    /// `x = expr;` / `x.field = expr;` — a read-modify-write place path
    /// (charter's RMW-paths ruling). The place is a dotted [`Self::PATH`]
    /// (no `::` — an assignable place is always local).
    ASSIGN_STMT,
    /// `expr;` — a bare expression statement, `;`-terminated. The one
    /// thing distinguishing this from a [`Self::STMT_BLOCK`]'s unterminated
    /// tail expression.
    EXPR_STMT,

    // ── Node kinds — the code-ground statement tail (B0.8 Wave B tail, ──
    // ── issue #1322, `docs/decision-log.md` 2026-07-23 "Code-ground ────
    // ── sitting") ────────────────────────────────────────────────────
    // `break`/`continue` have no content-ground counterpart (loops are a
    // code-ground-only concept); `return`'s valued form reuses
    // `Self::RETURN_STMT` (see that variant's doc) rather than adding a
    // node here. Lowers to the *existing* `~ { … }` T1b closed statement
    // set (`BlockStmt::{Return,Break,Continue}` in `brink-ir`) — the NF-2
    // fence, no new HIR nodes.
    /// `break;` — loop-exit statement, `;`-terminated like every other
    /// code-ground statement. Legal only inside a `while`/`for` body — an
    /// out-of-loop `break` is `brink-analyzer`'s job to reject (E057), not
    /// this grammar's.
    BREAK_STMT,
    /// `continue;` — loop-skip statement, `;`-terminated. See
    /// [`Self::BREAK_STMT`]'s doc for the same in-loop caveat.
    CONTINUE_STMT,

    // ── Node kinds — the code-ground control-flow layer (B0.8 Wave B, ───
    // ── `docs/decision-log.md` 2026-07-23 "Code-ground sitting") ────────
    // Rides Wave A's `STMT_BLOCK` for every body (`parser/control_flow.rs`
    // reuses `parser/stmt.rs::stmt_block` verbatim — no second block
    // shape). Lowers to the *existing* `~ { … }` T1b closed statement set
    // (`IfStmt`/`WhileStmt`/`ForStmt`/`AwaitStmt` in `brink-ir`) — the NF-2
    // fence, no new HIR nodes.
    /// `if cond { … } (else if cond { … } | else { … })?`. No case for a
    /// bare `{` opener here — that's [`Self::CONDITIONAL_BLOCK`]'s
    /// annotated-brace family, a different (content-ground) construct this
    /// one does not replace.
    IF_STMT,
    /// The `else` arm of an [`Self::IF_STMT`]: either another [`Self::IF_STMT`]
    /// (an `else if` chain) or a plain [`Self::STMT_BLOCK`].
    ELSE_CLAUSE,
    /// `while cond { … }`. Always a plain loop on the native surface — no
    /// `while await cond { … }` persistent-await form (that's the
    /// brink-dialect T1b grammar's own concern; native retired `await`
    /// entirely in favor of [`Self::UNTIL_STMT`], decision-log item 4).
    WHILE_STMT,
    /// `for name in expr { … }` — single-binding iteration (charter's
    /// existing `ForStmt` HIR shape; no destructuring).
    FOR_STMT,
    /// `until <pure-bool-expr>;` — the condition-park statement
    /// (decision-log 2026-07-23 item 4): native's sole flow-suspension
    /// spelling, replacing `await`. Lowers to the same `AwaitStmt` HIR node
    /// the brink-dialect's `~ await cond` produces.
    UNTIL_STMT,

    // ── Node kinds — the type-annotation grammar (NG-A/B/C, issues ──────
    // ── #1487/#1488/#1489; `docs/decision-log.md` 2026-07-26 "NG-C ─────
    // ── ruled: `: type` returns everywhere") ────────────────────────────
    // One `: type` spelling in every position: `fn f(g: Guest): float`,
    // `flow f(): Quest`, `let x: int = 1;`, `var hp: int = 10`,
    // `|g: Guest|: bool { … }`. Structurally mirrors the brink dialect's
    // own TM-2 grammar (`brink-syntax/src/parser/types.rs`) so both
    // frontends lower to the same `brink_ir::hir::TypeExpr` shape.
    /// `: type_expr` — the annotation clause itself (the `:` token plus
    /// exactly one [`Self::TYPE_EXPR`] child).
    TYPE_ANNOTATION,
    /// A type expression: wraps exactly one of [`Self::TYPE_NAME`],
    /// [`Self::TYPE_GENERIC`], or [`Self::TYPE_FN`].
    TYPE_EXPR,
    /// A bare nominal type name — `int`, `string`, a struct name. The
    /// grammar accepts any `IDENT`; recognizing the fixed set is a semantic
    /// check (`brink-analyzer`), never this parser's concern.
    TYPE_NAME,
    /// `name<arg, …>` — `list<L>`, `map<K, V>`, or any unrecognized
    /// generic head.
    TYPE_GENERIC,
    /// `fn(type, …): type` — a function type. Parses here; the checker
    /// decides what it means.
    TYPE_FN,

    // ── Node kind — the `as` binding (B1b, issue #1475, ruled ──────────
    // ── `docs/decision-log.md` 2026-07-26 "The `as` binding") ──────────
    /// `as NAME` — the condition-position Option binding, in BOTH of the
    /// language's condition positions: the statement forms
    /// ([`Self::IF_STMT`], [`Self::WHILE_STMT`]) and the template form
    /// ([`Self::CONDITIONAL_BLOCK`]'s `{if …: … else: …}`). One construct,
    /// one node kind — the ruling explicitly refused a second binding
    /// grammar. Always the LAST child node of the construct it binds in,
    /// following the head expression, so every existing "first child node
    /// that isn't a body/arm" condition accessor keeps working.
    ///
    /// Parsed (but never lowered) inside a [`Self::CHOICE_GUARD`] too:
    /// guard-`as` is admitted by the language but **implemented** only
    /// once the `.inkb` v6 Choice record grows a captured environment, so
    /// `brink-ir` diagnoses it as not-yet-supported (`E146`) rather than
    /// letting it half-work.
    AS_BINDING,

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
                | Self::DOC_COMMENT_OUTER
                | Self::DOC_COMMENT_INNER
                | Self::KW_FLOW
                | Self::KW_FN
                | Self::KW_VAR
                | Self::KW_CONST
                | Self::KW_LET
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
                | Self::KW_WHILE
                | Self::KW_FOR
                | Self::KW_IN
                | Self::KW_UNTIL
                | Self::KW_BREAK
                | Self::KW_CONTINUE
                | Self::KW_AS
                | Self::KW_OR
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
                | Self::SEMICOLON
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
                | Self::KW_LET
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
                | Self::KW_WHILE
                | Self::KW_FOR
                | Self::KW_IN
                | Self::KW_UNTIL
                | Self::KW_BREAK
                | Self::KW_CONTINUE
                | Self::KW_AS
                | Self::KW_OR
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
            SyntaxKind::KW_LET,
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
            SyntaxKind::KW_WHILE,
            SyntaxKind::KW_FOR,
            SyntaxKind::KW_IN,
            SyntaxKind::KW_UNTIL,
            SyntaxKind::KW_BREAK,
            SyntaxKind::KW_CONTINUE,
            SyntaxKind::KW_AS,
            SyntaxKind::KW_OR,
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
