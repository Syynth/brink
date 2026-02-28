use super::*;
use SyntaxKind::*;

#[test]
fn empty_input() {
    assert_eq!(lex(""), vec![]);
}

#[test]
fn whitespace_and_newlines() {
    assert_eq!(
        tokens("  \t\n"),
        vec![(WHITESPACE, "  \t"), (NEWLINE, "\n")]
    );
}

#[test]
fn crlf_newline() {
    assert_eq!(kinds("\r\n"), vec![NEWLINE]);
    assert_eq!(tokens("\r\n"), vec![(NEWLINE, "\r\n")]);
}

#[test]
fn bare_cr_newline() {
    assert_eq!(kinds("\r"), vec![NEWLINE]);
    assert_eq!(tokens("\r"), vec![(NEWLINE, "\r")]);
}

#[test]
fn bom_as_whitespace() {
    let src = "\u{FEFF}hello";
    let toks = tokens(src);
    assert_eq!(toks[0].0, WHITESPACE);
    assert_eq!(toks[1], (IDENT, "hello"));
}

#[test]
fn line_comment() {
    assert_eq!(
        tokens("// hello\n"),
        vec![(LINE_COMMENT, "// hello"), (NEWLINE, "\n")]
    );
}

#[test]
fn line_comment_at_eof() {
    assert_eq!(tokens("// hello"), vec![(LINE_COMMENT, "// hello")]);
}

#[test]
fn block_comment() {
    assert_eq!(
        tokens("/* a\nb */x"),
        vec![(BLOCK_COMMENT, "/* a\nb */"), (IDENT, "x")]
    );
}

#[test]
fn unterminated_block_comment() {
    assert_eq!(kinds("/* never closed"), vec![BLOCK_COMMENT]);
}

#[test]
fn identifiers() {
    assert_eq!(tokens("foo"), vec![(IDENT, "foo")]);
    assert_eq!(tokens("_bar"), vec![(IDENT, "_bar")]);
    assert_eq!(tokens("x2"), vec![(IDENT, "x2")]);
}

#[test]
fn error_token_for_unknown() {
    assert_eq!(kinds("`"), vec![ERROR_TOKEN]);
}

#[test]
fn error_token_preserves_text() {
    assert_eq!(tokens("`"), vec![(ERROR_TOKEN, "`")]);
}

#[test]
fn content_as_tokens() {
    assert_eq!(
        tokens("Hello world."),
        vec![
            (IDENT, "Hello"),
            (WHITESPACE, " "),
            (IDENT, "world"),
            (DOT, "."),
        ]
    );
}

#[test]
fn knot_header_tokens() {
    assert_eq!(
        kinds("=== myKnot ==="),
        vec![EQ_EQ, EQ, WHITESPACE, IDENT, WHITESPACE, EQ_EQ, EQ]
    );
}

#[test]
fn slash_at_eof() {
    // A lone `/` at end of input — not a comment, just a SLASH
    assert_eq!(kinds("/"), vec![SLASH]);
}
