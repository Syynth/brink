//! Trivia & error recovery — malformed input must never panic or hang, and
//! must still round-trip losslessly. Family for #1199.

use super::*;

#[test]
fn unclosed_flow_body_recovers() {
    let p = assert_lossless("flow greet() {\n  Hello\n");
    assert!(!p.errors().is_empty());
}

#[test]
fn stray_closing_brace_recovers() {
    let p = assert_lossless("}\n");
    assert!(!p.errors().is_empty());
}

#[test]
fn keyword_as_prose_falls_through() {
    // "flow" not followed by an IDENT is not a declaration head (Finding
    // #5) — it's ordinary prose text and must not error.
    let src = "flow through the garden and see what grows.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn decl_keyword_followed_by_ident_and_brace_is_a_decl() {
    // The flip side of the above: `flow name {` unambiguously looks like a
    // declaration head (Finding #5's third-token disambiguator) and is
    // parsed as one.
    let src = "flow gardenfulofdanger {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn decl_keyword_followed_by_ident_alone_is_still_prose() {
    // Even `flow IDENT` alone, with nothing brace/paren-shaped after it,
    // stays prose under the strengthened three-token check — this is
    // exactly the residual ambiguity Finding #5 documents, made as safe as
    // a cheap lookahead reasonably can.
    let src = "flow gardenfulofdanger\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn garbage_tokens_never_panic() {
    let src = "flow {}}}{{{ @[ ( ) -> -> -> :: :: match if if if\n";
    let p = assert_lossless(src);
    // Not asserting on error count — only that it doesn't panic/hang and
    // stays lossless.
    let _ = p.errors();
}

#[test]
fn deeply_nested_interpolation_does_not_overflow_stack() {
    let mut src = String::new();
    for _ in 0..2000 {
        src.push('(');
    }
    src.push('1');
    for _ in 0..2000 {
        src.push(')');
    }
    let wrapped = format!("var x = {src}\n");
    let p = assert_lossless(&wrapped);
    // Must hit the depth limit and recover, not blow the stack.
    let _ = p.errors();
}

// ── Trivia round-trip: WHITESPACE / NEWLINE / LINE_COMMENT / BLOCK_COMMENT ──
//
// `assert_lossless` already asserts byte-for-byte equality for every test
// in this crate, but these cases stress the trivia kinds specifically —
// their content, their edges (EOF, no-trailing-newline, no-space-before-
// code), and their line-ending variants — rather than incidentally
// exercising them as background noise around some other construct.

#[test]
fn line_comment_content_preserved_verbatim() {
    let src = "// a line comment with \"quotes\" and {braces} and -> arrows\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn block_comment_content_preserved_verbatim() {
    let src = "/* a block\n   comment spanning\n   several lines: { } [ ] -> */\nvar x = 1\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn block_comment_with_no_trailing_newline_at_eof() {
    // A comment is the entire (rest of the) file — no NEWLINE token exists
    // to flush it, so it must still land in the tree via `eat`'s
    // unconditional leading-trivia flush (see `Parser::eat`'s doc comment).
    let src = "// trailing comment, no newline";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
}

#[test]
fn block_comment_immediately_before_open_brace_no_space() {
    // Regression-shaped: `brink-syntax/tests/fuzz_repro.rs` records a real
    // crash where a block comment directly abutting `{` (no whitespace
    // between them) looped forever in the ink parser's `mixed_content`.
    // Same adversarial shape, ported to the native grammar.
    let src = "flow g() {\n  text/*c*/{if x { y }}\n}\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn block_comment_adjacent_to_code_no_space() {
    let src = "var x = 1/**/+2\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn crlf_newlines_preserved() {
    let src = "flow g() {\r\n  Hello\r\n}\r\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn bare_cr_newlines_preserved() {
    // A lone `\r` (old Mac line ending) is still one NEWLINE token, not
    // silently dropped or merged with adjacent content.
    let src = "flow g() {\r  Hello\r}\r";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn mixed_crlf_lf_cr_in_one_file_preserved() {
    let src = "flow g() {\n  a\r\n  b\r  c\n}\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn tabs_and_spaces_mixed_whitespace_roundtrip() {
    let src = "flow\tg( )\t{\n \t Hello\t \n}\n";
    let p = assert_lossless(src);
    let _ = p.errors();
}

#[test]
fn utf8_bom_at_start_of_file_is_whitespace() {
    let src = "\u{FEFF}flow g() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let root = p.syntax();
    let first_token = root
        .first_token()
        .expect("non-empty source has a first token");
    assert_eq!(first_token.kind(), SyntaxKind::WHITESPACE);
    assert_eq!(first_token.text(), "\u{FEFF}");
}

#[test]
fn whitespace_only_source_parses() {
    let p = assert_lossless("   \t\t  \n\n  \t\n");
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn comment_only_source_parses() {
    let p = assert_lossless("// just this\n/* and this */\n");
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── ERROR node: forward-progress and structural shape ───────────────────

#[test]
fn error_recover_wraps_exactly_one_token() {
    // A stray `}` with no enclosing block/choice to close is the shape
    // `error_recover` actually fires for (`content_items_until` always
    // lists `R_BRACE` as a stop kind, so a bare `}` at a content position
    // makes zero parsing progress and `source_file`'s loop falls back to
    // `error_recover`) — it must wrap exactly that one token in an ERROR
    // node, not swallow anything around it.
    let src = "}\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    let root = p.syntax();
    let err_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ERROR)
        .expect("an ERROR node was produced");
    // Exactly one non-trivia token swallowed by the ERROR wrapper.
    let non_trivia_tokens: Vec<_> = err_node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|t| !t.kind().is_trivia())
        .collect();
    assert_eq!(non_trivia_tokens.len(), 1, "tokens: {non_trivia_tokens:?}");
    assert_eq!(non_trivia_tokens[0].text(), "}");
}

#[test]
fn multiple_stray_tokens_each_get_their_own_error_node() {
    // Three consecutive unmatched `}`s must recover independently — one
    // ERROR node per stray token, not one ERROR node greedily swallowing
    // all three (which would still be "forward progress" but would lose
    // the per-token diagnostic granularity `error_recover`'s contract
    // implies).
    let src = "} } }\n";
    let p = assert_lossless(src);
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::ERROR), 3);
    assert_eq!(p.errors().len(), 3, "errors: {:?}", p.errors());
}

#[test]
fn error_recovery_lets_well_formed_content_after_it_still_parse() {
    // The whole point of bounded ERROR-wrapping: bad tokens must not
    // poison the rest of the file. A well-formed FLOW_DECL after the
    // garbage line must still show up as a real FLOW_DECL node.
    let src = "} } }\nflow greet() {\n  Hello\n}\n";
    let p = assert_lossless(src);
    assert!(has_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL));
    assert!(p.errors().len() >= 3, "errors: {:?}", p.errors());
}

#[test]
fn error_node_range_points_at_the_offending_token() {
    let src = "}\n";
    let p = assert_lossless(src);
    let err = p.errors().first().expect("one error recorded");
    let offending = &src[usize::from(err.range.start())..usize::from(err.range.end())];
    assert_eq!(offending, "}");
}

// ── Unterminated / truncated constructs at brace-family boundaries ──────

#[test]
fn unterminated_block_comment_runs_to_eof_and_recovers() {
    let src = "var x = 1\n/* never closed, swallows the rest of the file";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    // The lexer folds everything from `/*` to EOF into one BLOCK_COMMENT
    // token (see `lexer::Lexer::try_lex_comment`'s unterminated branch) —
    // confirm that token exists and its text runs all the way to EOF.
    let root = p.syntax();
    let comment = root
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::BLOCK_COMMENT)
        .expect("an unterminated BLOCK_COMMENT token exists");
    assert!(comment.text().ends_with("swallows the rest of the file"));
}

#[test]
fn unterminated_string_terminated_by_newline_recovers() {
    let src = "var x = \"never closed\nvar y = 2\n";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    // Recovery must not swallow the next declaration.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::VAR_DECL), 2);
}

#[test]
fn unclosed_nested_braces_all_recover() {
    // Three levels of unclosed `{` — flow body, conditional, match — all
    // truncated at EOF with no closing braces anywhere.
    let src = "flow g() {\n  {if x {\n    {match y {\n      z => {\n";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(!p.errors().is_empty());
}

#[test]
fn unclosed_paren_in_expression_recovers() {
    let src = "var x = (1 + 2\nvar y = 3\n";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(!p.errors().is_empty());
    // Recovery must not swallow the next declaration.
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::VAR_DECL), 2);
}

#[test]
fn unclosed_paren_in_call_args_recovers() {
    let src = "flow g() {\n  var x = foo(1, 2\n}\n";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(!p.errors().is_empty());
}

#[test]
fn unclosed_annotation_bracket_and_paren_recover() {
    // Neither the arg-list `)` nor the annotation's own `]` are ever
    // closed.
    let src = "@[foo(bar, baz\nflow g() {\n}\n";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(!p.errors().is_empty());
}

#[test]
fn unclosed_choice_text_bracket_recovers() {
    let src = "flow g() {\n  {?\n    * [unterminated bracket text\n  }\n}\n";
    let p = assert_lossless(src);
    assert_eq!(p.syntax().kind(), SyntaxKind::SOURCE_FILE);
    assert!(!p.errors().is_empty());
}

// ── Recovery position: SOURCE_FILE root survives every truncation point ─

#[test]
fn every_truncation_point_of_a_well_formed_source_still_roots_at_source_file() {
    // Truncate a well-formed, multi-construct source at every 10% step
    // and confirm the parser never panics, never hangs, always still
    // roots the tree at SOURCE_FILE, and stays lossless — the exact
    // "recovery-position" property the issue calls for, swept densely
    // rather than at one or two hand-picked cut points.
    let src = concat!(
        "// leading comment\n",
        "flow greet(name) {\n",
        "  Hello, {name}! <>\n",
        "  {?\n",
        "    * [Hi.] -> reply\n",
        "    * [Bye.] -> END\n",
        "  }\n",
        "}\n",
        "\n",
        "flow reply() {\n",
        "  var x = (1 + 2) * 3\n",
        "  -> END\n",
        "}\n",
    );
    for pct in 0..=100u32 {
        let cut = (src.len() as u64 * u64::from(pct) / 100) as usize;
        let mut end = cut.min(src.len());
        while end > 0 && !src.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = &src[..end];
        let p = parse(truncated);
        assert_eq!(
            p.syntax().text().to_string(),
            truncated,
            "lossless roundtrip failed at cut {pct}%"
        );
        assert_eq!(
            p.syntax().kind(),
            SyntaxKind::SOURCE_FILE,
            "root kind changed at cut {pct}%"
        );
    }
}
