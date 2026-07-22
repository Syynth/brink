use super::*;
use crate::SyntaxKind::*;

/// Every lexer test's baseline invariant: concatenating the token slices
/// reproduces the exact source, byte-for-byte.
fn assert_lossless(source: &str) -> Vec<(SyntaxKind, &str)> {
    let tokens = lex(source);
    let rebuilt: String = tokens.iter().map(|(_, text)| *text).collect();
    assert_eq!(rebuilt, source, "lexer is not lossless for {source:?}");
    tokens
}

#[test]
fn empty_source_is_empty() {
    assert_eq!(lex(""), vec![]);
}

#[test]
fn keywords_classify() {
    let toks = assert_lossless(
        "flow fn var const flags struct extern import use module return ref if match else as true false END DONE",
    );
    let kinds: Vec<_> = toks
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !k.is_trivia())
        .collect();
    assert_eq!(
        kinds,
        vec![
            KW_FLOW, KW_FN, KW_VAR, KW_CONST, KW_FLAGS, KW_STRUCT, KW_EXTERN, KW_IMPORT, KW_USE,
            KW_MODULE, KW_RETURN, KW_REF, KW_IF, KW_MATCH, KW_ELSE, KW_AS, KW_TRUE, KW_FALSE,
            KW_END, KW_DONE,
        ]
    );
}

#[test]
fn plain_ident_is_not_a_keyword() {
    let toks = assert_lossless("flowchart");
    assert_eq!(toks, vec![(IDENT, "flowchart")]);
}

#[test]
fn case_sensitive_keywords() {
    // `Flow`/`FLOW` are not the `flow` keyword — matches native's
    // lowercase-decl-keyword ruling (b0-sequencing §B0.5).
    let toks = assert_lossless("Flow FLOW");
    assert_eq!(toks[0].0, IDENT);
    assert_eq!(toks[2].0, IDENT);
}

#[test]
fn compound_tokens() {
    let toks = assert_lossless("@[ <> -> <- => :: ==");
    let kinds: Vec<_> = toks
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !k.is_trivia())
        .collect();
    assert_eq!(
        kinds,
        vec![
            AT_L_BRACKET,
            GLUE,
            DIVERT,
            THREAD,
            FAT_ARROW,
            COLON_COLON,
            EQ_EQ
        ]
    );
}

#[test]
fn lone_at_is_error_token_not_swallowed() {
    let toks = assert_lossless("@name");
    assert_eq!(toks[0], (AT, "@"));
    assert_eq!(toks[1], (IDENT, "name"));
}

#[test]
fn double_pipe_is_two_tokens_not_compound() {
    let toks = assert_lossless("||");
    assert_eq!(toks, vec![(PIPE, "|"), (PIPE, "|")]);
}

#[test]
fn integers_and_floats() {
    let toks = assert_lossless("42 3.14 0");
    let kinds: Vec<_> = toks
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !k.is_trivia())
        .collect();
    assert_eq!(kinds, vec![INTEGER, FLOAT, INTEGER]);
}

#[test]
fn digit_start_ident_stays_ident() {
    // Not valid in most languages, but the lexer must not panic or lose
    // bytes — the parser rejects it later, the lexer just classifies.
    let toks = assert_lossless("3d6");
    assert_eq!(toks, vec![(IDENT, "3d6")]);
}

#[test]
fn dot_after_int_without_digit_is_separate_dot() {
    let toks = assert_lossless("42.foo");
    let kinds: Vec<_> = toks.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, vec![INTEGER, DOT, IDENT]);
}

#[test]
fn line_comment_runs_to_newline() {
    let toks = assert_lossless("// hello\nvar");
    assert_eq!(toks[0], (LINE_COMMENT, "// hello"));
    assert_eq!(toks[1], (NEWLINE, "\n"));
}

#[test]
fn block_comment_spans_lines() {
    let toks = assert_lossless("/* a\nb */var");
    assert_eq!(toks[0], (BLOCK_COMMENT, "/* a\nb */"));
    assert_eq!(toks[1], (KW_VAR, "var"));
}

#[test]
fn unterminated_block_comment_runs_to_eof_lossless() {
    assert_lossless("/* never closes");
}

#[test]
fn string_literal_round_trips() {
    let toks = assert_lossless("\"hello world\"");
    assert_eq!(toks[0], (QUOTE, "\""));
    assert_eq!(toks[1], (STRING_TEXT, "hello world"));
    assert_eq!(toks[2], (QUOTE, "\""));
}

#[test]
fn string_escape_sequences() {
    let toks = assert_lossless(r#""a\nb\t\\\"c""#);
    let kinds: Vec<_> = toks.iter().map(|(k, _)| *k).collect();
    assert!(kinds.contains(&STRING_ESCAPE));
}

#[test]
fn interpolation_inside_string_reenters_code_mode() {
    let toks = assert_lossless("\"hp is {hp}\"");
    let kinds: Vec<_> = toks.iter().map(|(k, _)| *k).collect();
    // No STRING_TEXT between `}` and the closing `"` — the string ends
    // immediately after the interpolation with no trailing literal text.
    assert_eq!(
        kinds,
        vec![QUOTE, STRING_TEXT, L_BRACE, IDENT, R_BRACE, QUOTE]
    );
}

#[test]
fn interpolation_inside_string_with_trailing_text() {
    let toks = assert_lossless("\"hp is {hp} now\"");
    let kinds: Vec<_> = toks.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        vec![
            QUOTE,
            STRING_TEXT,
            L_BRACE,
            IDENT,
            R_BRACE,
            STRING_TEXT,
            QUOTE
        ]
    );
}

#[test]
fn unterminated_string_ends_at_newline() {
    let toks = assert_lossless("\"never closes\nvar x");
    assert_eq!(toks[0].0, QUOTE);
    assert_eq!(toks[1].0, STRING_TEXT);
    assert_eq!(toks[2].0, NEWLINE);
    assert_eq!(toks[3].0, KW_VAR);
}

#[test]
fn brackets_recognized_inside_string_mode() {
    let toks = assert_lossless("\"tired[.\"]\"");
    let kinds: Vec<_> = toks.iter().map(|(k, _)| *k).collect();
    assert!(kinds.contains(&L_BRACKET));
    assert!(kinds.contains(&R_BRACKET));
}

#[test]
fn glue_breaks_string_text() {
    let toks = assert_lossless("\"a<>b\"");
    let kinds: Vec<_> = toks.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, vec![QUOTE, STRING_TEXT, GLUE, STRING_TEXT, QUOTE]);
}

#[test]
fn bom_is_whitespace_trivia() {
    let src = "\u{FEFF}flow x() {}";
    let toks = assert_lossless(src);
    assert_eq!(toks[0].0, WHITESPACE);
    assert_eq!(toks[0].1, "\u{FEFF}");
}

#[test]
fn crlf_and_lf_both_single_newline_tokens() {
    let toks = assert_lossless("a\r\nb\nc\rd");
    let newlines: Vec<_> = toks.iter().filter(|(k, _)| *k == NEWLINE).collect();
    assert_eq!(newlines.len(), 3);
    assert_eq!(newlines[0].1, "\r\n");
    assert_eq!(newlines[1].1, "\n");
    assert_eq!(newlines[2].1, "\r");
}

#[test]
fn unicode_prose_text_is_lossless_error_tokens() {
    // Outside string/ident position, non-ASCII bytes fall through to
    // ERROR_TOKEN (one codepoint at a time) — the parser is responsible
    // for folding runs of these into prose TEXT nodes. The lexer's only
    // job is never losing or mis-splitting bytes.
    assert_lossless("héllo wörld 日本語 emoji: 🎉");
}

#[test]
fn mixed_content_never_panics_or_drops_bytes() {
    let src = "flow greet(name) {\n    Hello, {name}! <>\n    {? * [Hi] Hi back! }\n}\n";
    assert_lossless(src);
}

// ── B0.6b: doc-comment classification ────────────────────────────────

#[test]
fn plain_double_slash_is_line_comment() {
    let toks = assert_lossless("// just a comment");
    assert_eq!(toks, vec![(LINE_COMMENT, "// just a comment")]);
}

#[test]
fn triple_slash_is_doc_comment_outer() {
    let toks = assert_lossless("/// a doc comment");
    assert_eq!(toks, vec![(DOC_COMMENT_OUTER, "/// a doc comment")]);
}

#[test]
fn bang_slash_slash_is_doc_comment_inner() {
    let toks = assert_lossless("//! module-level doc");
    assert_eq!(toks, vec![(DOC_COMMENT_INNER, "//! module-level doc")]);
}

#[test]
fn quadruple_slash_stays_line_comment() {
    // Rust precedent: a fourth slash falls back to a plain (non-doc)
    // comment, not a doc comment with a literal leading `/`.
    let toks = assert_lossless("//// separator");
    assert_eq!(toks, vec![(LINE_COMMENT, "//// separator")]);
}

#[test]
fn five_or_more_slashes_stay_line_comment() {
    let toks = assert_lossless("///////");
    assert_eq!(toks, vec![(LINE_COMMENT, "///////")]);
}

#[test]
fn bare_triple_slash_at_eof_is_doc_comment_outer() {
    // Exactly three slashes, nothing after — still classifies as the doc
    // form (the "not followed by a fourth" check must not panic reading
    // past EOF).
    let toks = assert_lossless("///");
    assert_eq!(toks, vec![(DOC_COMMENT_OUTER, "///")]);
}

#[test]
fn doc_comments_are_not_trivia() {
    assert!(!DOC_COMMENT_OUTER.is_trivia());
    assert!(!DOC_COMMENT_INNER.is_trivia());
    assert!(DOC_COMMENT_OUTER.is_token());
    assert!(DOC_COMMENT_INNER.is_token());
}

#[test]
fn doc_comment_lines_roundtrip_losslessly() {
    let src = "/// line one\n/// line two\n//! inner doc\nflow x() {}\n";
    assert_lossless(src);
}

#[test]
fn every_byte_is_covered_by_exactly_one_token() {
    let src = "flow x(a, ref b) {\n  @[effects(pure, silent, reads(gold, hp))]\n  var y = 1 + 2 * (3 - 4)\n  -> END\n}\n";
    let tokens = assert_lossless(src);
    let mut cursor = 0usize;
    for (_, text) in &tokens {
        assert_eq!(&src[cursor..cursor + text.len()], *text);
        cursor += text.len();
    }
    assert_eq!(cursor, src.len());
}
