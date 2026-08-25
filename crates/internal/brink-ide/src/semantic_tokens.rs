//! Semantic tokens — re-exported from `brink_ir::semantic_tokens` since
//! #3064 B4 (see that module's doc). The `AnalysisResult`-taking entry
//! points stay here: the IR crate deliberately doesn't know the
//! analyzer's types.

use std::collections::BTreeMap;

use brink_analyzer::AnalysisResult;
use brink_ir::hir::projection::range_key;
use brink_ir::{FileId, SymbolKind};
use brink_syntax::SyntaxNode;

pub use brink_ir::semantic_tokens::*;

/// Range-keyed resolution-kind map for one file — the identity join the
/// classifiers refine `IDENT` tokens with.
#[must_use]
pub fn build_resolution_index(
    analysis: &AnalysisResult,
    file_id: FileId,
) -> BTreeMap<(u32, u32), SymbolKind> {
    let mut map = BTreeMap::new();
    for rref in &analysis.resolutions {
        if rref.file == file_id
            && let Some(info) = analysis.index.symbols.get(&rref.target)
        {
            map.insert(range_key(rref.range), info.kind);
        }
    }
    map
}

/// Compute raw (absolute-position) semantic tokens for the entire file.
pub fn semantic_tokens(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
) -> Vec<RawToken> {
    let resolution_index = build_resolution_index(analysis, file_id);
    tokens_with_kinds(source, root, &resolution_index)
}

/// Compute raw semantic tokens filtered to a line range.
pub fn semantic_tokens_range(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
    start_line: u32,
    end_line: u32,
) -> Vec<RawToken> {
    let raw = semantic_tokens(source, root, analysis, file_id);
    raw.into_iter()
        .filter(|t| t.line >= start_line && t.line <= end_line)
        .collect()
}

/// Compute raw (absolute-position) semantic tokens for a native (`.brink`)
/// file's whole CST (issue #2280) — the native sibling of [`semantic_tokens`].
/// Callers must feed this the file's *native* parse
/// (`ProjectDb::parse_native`/`IdeSession::syntax_root_native`), never the
/// ink one: `ProjectDb::parse`/`IdeSession::syntax_root` always run the ink
/// frontend regardless of the file's extension (that dispatch happens only
/// in `lowered_query`), so calling this with an ink-parsed root would just
/// reproduce the original bug one layer down.
pub fn semantic_tokens_native(
    source: &str,
    root: &brink_syntax_native::SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
) -> Vec<RawToken> {
    let resolution_index = build_resolution_index(analysis, file_id);
    tokens_with_kinds_native(source, root, &resolution_index)
}

/// Compute native raw semantic tokens filtered to a line range — the native
/// sibling of [`semantic_tokens_range`].
pub fn semantic_tokens_range_native(
    source: &str,
    root: &brink_syntax_native::SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
    start_line: u32,
    end_line: u32,
) -> Vec<RawToken> {
    let raw = semantic_tokens_native(source, root, analysis, file_id);
    raw.into_iter()
        .filter(|t| t.line >= start_line && t.line <= end_line)
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use brink_analyzer::AnalysisResult;
    use brink_ir::SymbolIndex;

    fn empty_analysis() -> AnalysisResult {
        AnalysisResult {
            index: std::sync::Arc::new(SymbolIndex::default()),
            resolutions: Vec::new(),
            diagnostics: Vec::new(),
            symbol_meta: std::collections::BTreeMap::new(),
        }
    }

    fn parse_and_tokens(source: &str) -> Vec<RawToken> {
        let parse = brink_syntax::parse(source);
        let root = parse.syntax();
        let analysis = empty_analysis();
        semantic_tokens(source, &root, &analysis, FileId(0))
    }

    /// Tokens produced against a *real* analysis (resolutions included),
    /// rather than [`empty_analysis`]'s resolution-free stand-in — the
    /// resolution index is what drives dotted-path classification.
    fn analyzed_tokens(source: &str) -> Vec<RawToken> {
        let mut session = crate::session::IdeSession::new();
        let file_id = session.update_and_analyze("t.ink", source.to_string());
        let root = session.syntax_root(file_id).expect("syntax root");
        let analysis = session.analysis().expect("analysis");
        semantic_tokens(source, &root, analysis, file_id)
    }

    /// The native (`.brink`) sibling of [`analyzed_tokens`] (issue #2280) —
    /// routed through `syntax_root_native`/`semantic_tokens_native`, the
    /// same path `brink-web`'s `semantic_tokens_impl` now dispatches to for
    /// a native file.
    fn analyzed_native_tokens(source: &str) -> Vec<RawToken> {
        let mut session = crate::session::IdeSession::new();
        let file_id = session.update_and_analyze("t.brink", source.to_string());
        let root = session
            .syntax_root_native(file_id)
            .expect("native syntax root");
        let analysis = session.analysis().expect("analysis");
        semantic_tokens_native(source, &root, analysis, file_id)
    }

    /// Decode a token's `(line, start_char, length)` back onto `source`,
    /// returning the exact substring it covers — issue #2280's mandate is to
    /// verify by decoding tokens onto source text, never by asserting a
    /// count or non-emptiness (both would pass against the pre-fix output
    /// too). Every source these tests use is ASCII, so a UTF-16 column is
    /// also a byte column.
    fn token_text<'a>(source: &'a str, tok: &RawToken) -> &'a str {
        let line_text = source.split('\n').nth(tok.line as usize).unwrap_or("");
        let start = (tok.start_char as usize).min(line_text.len());
        let end = (start + tok.length as usize).min(line_text.len());
        &line_text[start..end]
    }

    #[test]
    fn content_logic_delimiters_classify_as_operator() {
        // Author feedback (2026-08-25): the `{`/`}` around alternatives,
        // conditionals, and interpolations — and the `|` between
        // alternative branches — carried no token, so they rendered in the
        // prose color and visually merged into dialogue/action text.
        let src = "VAR mood = 1\nThe lamp {flickers|dims} and glows {mood} bright.\n";
        let tokens = parse_and_tokens(src);
        let delimiters: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| matches!(token_text(src, t), "{" | "}" | "|"))
            .collect();
        assert_eq!(
            delimiters.len(),
            5,
            "expected {{ | }} {{ }} as tokens: {tokens:?}"
        );
        assert!(
            delimiters.iter().all(|t| t.token_type == TT_OPERATOR),
            "content-logic delimiters must classify as operator: {delimiters:?}"
        );
    }

    #[test]
    fn native_content_logic_delimiters_classify_as_operator() {
        // The native mirror of the ink case above: interpolation braces in
        // a flow's prose line get the operator classification too.
        let src =
            "var mood: int = 1\nflow main() {\n    The lamp glows {mood} bright.\n    -> DONE\n}\n";
        let tokens = analyzed_native_tokens(src);
        let prose_line = 2u32;
        let braces: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| t.line == prose_line && matches!(token_text(src, t), "{" | "}"))
            .collect();
        assert_eq!(
            braces.len(),
            2,
            "expected the interpolation braces: {tokens:?}"
        );
        assert!(
            braces.iter().all(|t| t.token_type == TT_OPERATOR),
            "native interpolation braces must classify as operator: {braces:?}"
        );
    }

    #[test]
    fn field_access_segments_are_not_coloured_as_the_head_variable() {
        // Issue #1571: the `ResolvedRef` for `p.x` is recorded against the
        // *whole* path (a load-bearing contract pinned by #1561), so the
        // PATH-grandparent fallback used to hand every segment of the path
        // the head variable's own classification — `p.x` rendered as one
        // flat `variable` run. Only the head names the variable; the rest
        // are struct field names.
        let src = "VAR p = 0\n=== main ===\n~ y = p.x\n-> DONE\n";
        let tokens = analyzed_tokens(src);

        // Line 2 is `~ y = p.x`: `p` at column 6, `x` at column 8.
        let at = |line: u32, col: u32| {
            tokens
                .iter()
                .find(|t| t.line == line && t.start_char == col)
                .expect("a semantic token at the requested line/column")
        };
        assert_eq!(
            at(2, 6).token_type,
            TT_VARIABLE,
            "the head segment still names the resolved variable"
        );
        assert_eq!(
            at(2, 8).token_type,
            TT_PROPERTY,
            "the trailing field segment must not reuse the head variable's colour"
        );
    }

    #[test]
    fn a_qualified_list_item_reference_colours_its_tail_segment() {
        // The kind gate's other side: `Colors.Red` resolves to a LIST ITEM,
        // which the path's *last* segment names — so `Red` gets the
        // enumMember colour and the `Colors.` qualifier (a symbol this pass
        // resolves nothing for on its own) keeps the plain fallback, rather
        // than the whole path being painted one colour.
        let src = "LIST Colors = Red, Green\n=== main ===\n~ c = Colors.Red\n-> DONE\n";
        let tokens = analyzed_tokens(src);

        // Line 2 is `~ c = Colors.Red`: `Colors` at column 6, `Red` at 13.
        let at = |line: u32, col: u32| {
            tokens
                .iter()
                .find(|t| t.line == line && t.start_char == col)
                .expect("a semantic token at the requested line/column")
        };
        assert_eq!(at(2, 13).token_type, TT_ENUM_MEMBER, "`Red` names the item");
        assert_eq!(
            at(2, 6).token_type,
            TT_VARIABLE,
            "the `Colors` qualifier must not be painted as the item itself"
        );
    }

    #[test]
    fn keywords_are_classified() {
        let tokens = parse_and_tokens("VAR x = 5\n");
        let kw = tokens.iter().find(|t| t.token_type == TT_KEYWORD);
        assert!(kw.is_some(), "expected a keyword token for VAR");
    }

    #[test]
    fn prose_words_matching_keywords_are_not_highlighted() {
        // "and"/"or"/"not" are real English words; in narrative text they must
        // not be coloured like ink keywords (#275).
        let tokens = parse_and_tokens("Cats and dogs, rain or shine, why not.\n");
        let kw = tokens.iter().find(|t| t.token_type == TT_KEYWORD);
        assert!(
            kw.is_none(),
            "prose words matching keywords should not be highlighted, got {kw:?}"
        );
    }

    #[test]
    fn keywords_in_inline_logic_are_still_highlighted() {
        // The same words ARE keywords inside an expression — `{ ... and ... }`.
        let tokens = parse_and_tokens("{ x > 1 and x < 9 }\n");
        let kw = tokens.iter().find(|t| t.token_type == TT_KEYWORD);
        assert!(
            kw.is_some(),
            "`and` inside inline logic must still be highlighted as a keyword"
        );
    }

    #[test]
    fn prose_words_are_not_classified_as_variables() {
        // #2293: #275/#2286 only carved the "absorbed into prose" guard out
        // for keywords. A plain narrative word (`IDENT` with no wrapping
        // `IDENTIFIER` node — `text_content` bumps `TEXT` children flat) fell
        // through `classify_ident`'s every declaration-shaped arm to the
        // resolution fallback, which had nothing to resolve and defaulted to
        // `GENERIC_VARIABLE` — so ordinary dialogue painted the same colour
        // as a local variable.
        let src = "Cats and dogs run.\n";
        let tokens = parse_and_tokens(src);
        for word in ["Cats", "dogs", "run"] {
            assert!(
                tokens.iter().all(|t| token_text(src, t) != word),
                "prose word {word:?} must not get its own semantic token: {tokens:?}"
            );
        }
    }

    #[test]
    fn prose_punctuation_is_not_classified_as_operator_or_string() {
        // #2293's named remainder beyond #2280/#2286: a hyphen inside a
        // hyphenated word, and other punctuation `text_content`'s stop set
        // doesn't exclude (`!`, `?`), still reached the unconditional
        // operator arm; a literal quote mark in dialogue (also not a
        // `text_content` stop character) still reached the unconditional
        // string arm.
        //
        // Review finding on #2293: the quoted line alone does not exercise
        // `!`/`?` at all — inside `"Wait! Really?"` the lexer emits
        // QUOTE + STRING_TEXT("Wait! Really?") + QUOTE, all as direct `TEXT`
        // children, so `!`/`?` never surface as their own BANG/QUESTION
        // tokens there; only the `-` in "well-known" was actually covered.
        // An *unquoted* `Wait! Really?` line is required to reach the
        // BANG/QUESTION operator arm at all (unquoted prose does produce
        // them: `TEXT > IDENT, BANG, WHITESPACE, IDENT, QUESTION`).
        let src = "The well-known hero shouted, \"Wait! Really?\"\nWait! Really?\n";
        let tokens = parse_and_tokens(src);
        assert!(
            tokens.iter().all(|t| t.token_type != TT_OPERATOR),
            "no prose punctuation should classify as an operator: {tokens:?}"
        );
        assert!(
            tokens.iter().all(|t| t.token_type != TT_STRING),
            "a literal quote mark in dialogue must not read as a string delimiter: {tokens:?}"
        );
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
    fn full_pipeline_produces_tokens() {
        let source = "=== start ===\nHello world\n-> END\n";
        let parse = brink_syntax::parse(source);
        let root = parse.syntax();
        let analysis = empty_analysis();
        let tokens = semantic_tokens(source, &root, &analysis, FileId(0));
        assert!(!tokens.is_empty(), "expected non-empty semantic tokens");
    }

    #[test]
    fn range_filter_works() {
        let source = "=== start ===\nHello world\n-> END\n";
        let parse = brink_syntax::parse(source);
        let root = parse.syntax();
        let analysis = empty_analysis();

        let all = semantic_tokens(source, &root, &analysis, FileId(0));
        let range_tokens = semantic_tokens_range(source, &root, &analysis, FileId(0), 2, 2);

        // Only tokens on line 2
        let all_line2: Vec<_> = all.iter().filter(|t| t.line == 2).collect();
        assert_eq!(range_tokens.len(), all_line2.len());
    }

    // ── Native (`.brink`) classification (issue #2280) ──────────────
    //
    // Every assertion below decodes a token's `(line, start_char, length)`
    // back onto the source text via `token_text` and checks the resulting
    // substring, per the issue's mandate — a token-count or
    // non-emptiness assertion would have passed against the pre-fix
    // behaviour (every one of these identifiers was already emitted, just
    // uniformly misclassified as `variable`).

    #[test]
    fn native_struct_decl_and_field_are_not_variable() {
        let src = "struct Cue {\n    speaker: string,\n}\n";
        let tokens = analyzed_native_tokens(src);

        let find = |text: &str| {
            let found = tokens.iter().find(|t| token_text(src, t) == text);
            assert!(
                found.is_some(),
                "no semantic token decodes to {text:?}: {tokens:?}"
            );
            found.expect("checked above")
        };

        assert_eq!(
            find("struct").token_type,
            TT_KEYWORD,
            "`struct` is a keyword"
        );
        assert_eq!(
            find("Cue").token_type,
            TT_STRUCT,
            "the struct's own name must not read as a plain `variable`"
        );
        assert_ne!(
            find("Cue").token_type,
            TT_VARIABLE,
            "the pre-fix bug: every native identifier fell back to `variable`"
        );
        assert_eq!(
            find("speaker").token_type,
            TT_PROPERTY,
            "a struct field name is a property, not a `variable`"
        );
        assert_ne!(find("speaker").token_type, TT_VARIABLE);
        assert_ne!(
            find("string").token_type,
            TT_VARIABLE,
            "a field's type reference must not read as a plain `variable`"
        );
    }

    #[test]
    fn native_annotation_name_and_arg_name_are_not_variable() {
        let src = "@[convention(claims = \"x\")]\nflow main() {\n    -> DONE\n}\n";
        let tokens = analyzed_native_tokens(src);

        let find = |text: &str| {
            let found = tokens.iter().find(|t| token_text(src, t) == text);
            assert!(
                found.is_some(),
                "no semantic token decodes to {text:?}: {tokens:?}"
            );
            found.expect("checked above")
        };

        assert_eq!(
            find("convention").token_type,
            TT_DECORATOR,
            "the annotation's own name reads as a decorator, not `variable`"
        );
        assert_eq!(
            find("claims").token_type,
            TT_PARAMETER,
            "an annotation argument name reads as a parameter, not `variable`"
        );
    }

    #[test]
    fn native_regex_string_literal_is_not_fragmented() {
        // The issue's own repro: a character class inside a quoted string
        // (`[A-Z]`) — native's shared lexer emits `[`/`]` as their own
        // punctuation kind even in string mode (`lexer::lex_string_token`'s
        // doc), so a naive per-leaf-kind classifier paints the brackets a
        // different colour than the surrounding string content. Every token
        // whose range falls inside the quotes must decode as `string`.
        let src = "@[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\")]\nflow main() {\n    -> DONE\n}\n";
        let tokens = analyzed_native_tokens(src);

        let quote_start = src.find('"').expect("opening quote");
        let quote_end = src.rfind('"').expect("closing quote");

        let inside_string: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| {
                // Line 0 only — every token in this fixture's string sits on
                // the first line, so start_char doubles as a byte offset.
                t.line == 0
                    && (t.start_char as usize) > quote_start
                    && (t.start_char as usize) < quote_end
            })
            .collect();

        assert!(
            inside_string.len() > 1,
            "expected the string to still decode as multiple leaf tokens \
             (the lexer's bracket-always-breaks-out behaviour is unchanged \
             by this fix — only its *colour* is): {inside_string:?}"
        );
        for tok in &inside_string {
            assert_eq!(
                tok.token_type,
                TT_STRING,
                "every fragment inside the string literal must read as `string`, \
                 got {:?} decoding to {:?}",
                tok.token_type,
                token_text(src, tok)
            );
        }
        // Full-coverage check: a classifier that just *skips* the bracket
        // tokens (rather than recolouring them) would pass the per-token
        // loop above vacuously — every remaining fragment is still `string`,
        // there's just a silent gap where the brackets used to be. Assert
        // the fragments concatenate back to the literal's exact interior
        // text, so a dropped token is caught too.
        let mut by_start: Vec<&&RawToken> = inside_string.iter().collect();
        by_start.sort_by_key(|t| t.start_char);
        let decoded: String = by_start.iter().map(|t| token_text(src, t)).collect();
        let expected = &src[quote_start + 1..quote_end];
        assert_eq!(
            decoded, expected,
            "the string's tokens must cover its full interior with no gaps"
        );

        // And the pre-`{`/post-`}` interpolation guard: a bracket outside
        // the string must NOT be forced to `string` just because it's a
        // bracket — sanity check that the fix is scoped to STRING_LIT's
        // direct children.
        let flow_kw = tokens
            .iter()
            .find(|t| token_text(src, t) == "flow")
            .expect("flow keyword token");
        assert_eq!(flow_kw.token_type, TT_KEYWORD);
    }

    #[test]
    fn native_dotted_field_access_head_and_property_are_distinguished() {
        // Mirrors `field_access_segments_are_not_coloured_as_the_head_variable`
        // (#1571) for native's PATH/PATH_SEGMENT shape (no ink-style
        // IDENTIFIER wrapper) — confirms the whole-path resolution-range
        // contract holds identically here.
        let src = "struct P {\n    x: int,\n}\nvar p: P = P { x: 0 }\nflow main() {\n    ~ let y = p.x\n    -> DONE\n}\n";
        let tokens = analyzed_native_tokens(src);

        let line = "    ~ let y = p.x";
        let p_col =
            u32::try_from(line.find('p').expect("`p` on the assignment line")).expect("fits u32");
        let x_col =
            u32::try_from(line.rfind('x').expect("`x` on the assignment line")).expect("fits u32");
        let line_no = u32::try_from(
            src.lines()
                .position(|l| l == line)
                .expect("the assignment line"),
        )
        .expect("fits u32");

        let at = |col: u32| {
            let found = tokens
                .iter()
                .find(|t| t.line == line_no && t.start_char == col);
            assert!(
                found.is_some(),
                "no token at line {line_no} col {col}: {tokens:?}"
            );
            found.expect("checked above")
        };

        assert_eq!(
            at(p_col).token_type,
            TT_VARIABLE,
            "the head segment names the resolved variable"
        );
        assert_eq!(
            at(x_col).token_type,
            TT_PROPERTY,
            "the trailing field segment must not reuse the head variable's colour"
        );
    }

    #[test]
    fn native_scene_heading_title_and_slug_are_not_variable() {
        // #2280's own worked example: `INT. COFFEE SHOP - DAY` had no arm
        // for `SCENE_TITLE`/`SCENE_SLUG` in `classify_native_ident`'s match,
        // so every word of the display name (and the slug's stitch name)
        // fell through to `classify_native_ident_by_resolution` ->
        // `GENERIC_VARIABLE` — the exact defect the issue names. This is
        // inside the fence the PR drew: it already covers the sibling
        // `CUE`/`CUE_NAME`/`COMPACT_CUE` constructs from the same parser
        // module (`element.rs`).
        let src = "INT. COFFEE SHOP - DAY [coffee_shop]\nThe morning rush.\n";
        let tokens = analyzed_native_tokens(src);

        // The title is narrative display text, not a symbol reference — no
        // token at all should decode to one of its words (the pre-fix bug
        // *did* emit a token here, just misclassified as `variable`).
        for word in ["COFFEE", "SHOP", "DAY"] {
            assert!(
                tokens.iter().all(|t| token_text(src, t) != word),
                "scene-title word {word:?} must not get its own semantic token: {tokens:?}"
            );
        }

        // The title's `-` is literal text, not an operator.
        assert!(
            tokens
                .iter()
                .all(|t| !(token_text(src, t) == "-" && t.token_type == TT_OPERATOR)),
            "the title's `-` must not read as an operator: {tokens:?}"
        );

        // The slug names the stitch this heading declares.
        let slug = tokens
            .iter()
            .find(|t| token_text(src, t) == "coffee_shop")
            .expect("a semantic token for the scene slug");
        assert_eq!(
            slug.token_type, TT_FUNCTION,
            "the slug declares a stitch, like any other stitch header"
        );
        assert_ne!(
            slug.modifiers & MOD_DECLARATION,
            0,
            "the slug is this stitch's declaration site"
        );
    }

    #[test]
    fn native_cue_name_and_tag_do_not_leak_keyword_or_operator_colours() {
        // The "absorbed into prose" carve-out (both the keyword guard and
        // the `-`/digit guard) only checked `NK::TEXT`, but `CUE_NAME`
        // (`element::cue_name`) and `TAG` (`content::tag`) raw-bump source
        // text the exact same way, and native hard-reserves keywords
        // everywhere at the lexer (`lexer/ident.rs`: `"END" => KW_END`) —
        // so `@THE END:` lexed `END` as `KW_END` with parent `CUE_NAME` and
        // rendered in keyword colour inside a character name, and `#if
        // only` did the same for `if` under `TAG`. Same shape as `@JEAN-LUC`
        // rendering its `-` as an operator.
        let src = "@THE END: Says who?\n@JEAN-LUC\nHello.\n#if only\n";
        let tokens = analyzed_native_tokens(src);

        let keyword_texts: Vec<&str> = tokens
            .iter()
            .filter(|t| t.token_type == TT_KEYWORD)
            .map(|t| token_text(src, t))
            .collect();
        assert!(
            !keyword_texts.contains(&"END"),
            "`END` inside a cue name must not read as a keyword: {keyword_texts:?}"
        );
        assert!(
            !keyword_texts.contains(&"if"),
            "`if` inside a tag must not read as a keyword: {keyword_texts:?}"
        );

        assert!(
            tokens
                .iter()
                .all(|t| !(token_text(src, t) == "-" && t.token_type == TT_OPERATOR)),
            "`-` inside a cue name must not read as an operator: {tokens:?}"
        );
    }

    #[test]
    fn native_text_and_tag_prose_words_are_not_variable() {
        // #2293: #2286 closed this exact gap for `SCENE_TITLE` alone
        // (`native_scene_heading_title_and_slug_are_not_variable`) — an
        // ordinary word of plain dialogue under `TEXT`, or a tag's own free
        // text under `TAG`, had no matching arm in `classify_native_ident`
        // and still fell through to `classify_native_ident_by_resolution`'s
        // `GENERIC_VARIABLE` default.
        let src = "The well-known hero shouted.\n#a tag about cats\n";
        let tokens = analyzed_native_tokens(src);

        for word in [
            "The", "well", "known", "hero", "shouted", "tag", "about", "cats",
        ] {
            assert!(
                tokens.iter().all(|t| token_text(src, t) != word),
                "prose/tag word {word:?} must not get its own semantic token: {tokens:?}"
            );
        }
    }

    #[test]
    fn native_prose_operators_beyond_minus_are_not_operator() {
        // #2293: #2286's parent-aware punctuation guard only widened to
        // `MINUS | INTEGER | FLOAT`. `text_run_until`'s stop set doesn't
        // exclude `!`/`?`, so dialogue emphasis still hit the ordinary
        // operator arm; `tag()`'s raw-bump loop doesn't stop on
        // `DIVERT`/`GLUE` either, so a tag body containing `->`/`<>` did
        // too.
        let src = "Wait! Really?\n#a -> b <> c\n";
        let tokens = analyzed_native_tokens(src);
        assert!(
            tokens.iter().all(|t| t.token_type != TT_OPERATOR),
            "no prose/tag punctuation should classify as an operator: {tokens:?}"
        );
    }

    #[test]
    fn native_tag_hash_is_still_a_decorator() {
        // Regression for the review finding on #2293's own widened prose
        // guard: `tag()` (`content.rs`) makes `HASH` a direct child of
        // `TAG`, which `is_prose_run_container` treats as a prose
        // container — so the blanket "punctuation inside a prose container
        // is text" guard swallowed the tag sigil itself along with the tag
        // body, losing the `decorator` colour every `#tag` line had before
        // this PR. Mirrors ink's `hash_is_decorator`.
        let tokens = analyzed_native_tokens("#a tag about cats\n");
        let dec = tokens.iter().find(|t| t.token_type == TT_DECORATOR);
        assert!(
            dec.is_some(),
            "expected a decorator token for `#`: {tokens:?}"
        );
    }

    #[test]
    fn native_range_filter_works() {
        // The native sibling of `range_filter_works` — review finding on
        // #2280/#2286: `semantic_tokens_range_native` had zero callers and
        // zero tests, unlike its ink sibling (covered by this same test
        // shape).
        let source = "flow main() {\n    Hello world\n    -> DONE\n}\n";
        let mut session = crate::session::IdeSession::new();
        let file_id = session.update_and_analyze("t.brink", source.to_string());
        let root = session
            .syntax_root_native(file_id)
            .expect("native syntax root");
        let analysis = session.analysis().expect("analysis");

        let all = semantic_tokens_native(source, &root, analysis, file_id);
        let range_tokens = semantic_tokens_range_native(source, &root, analysis, file_id, 2, 2);

        // Only tokens on line 2 (`    -> DONE`).
        let all_line2: Vec<_> = all.iter().filter(|t| t.line == 2).collect();
        assert_eq!(range_tokens.len(), all_line2.len());
        assert!(
            !range_tokens.is_empty(),
            "line 2 has real tokens to filter to"
        );
    }
}
