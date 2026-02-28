use super::*;
use SyntaxKind::*;

#[test]
fn simple_string() {
    assert_eq!(
        tokens(r#""hello""#),
        vec![(QUOTE, "\""), (STRING_TEXT, "hello"), (QUOTE, "\"")]
    );
}

#[test]
fn string_with_escapes() {
    assert_eq!(
        tokens(r#""a\nb""#),
        vec![
            (QUOTE, "\""),
            (STRING_TEXT, "a"),
            (STRING_ESCAPE, "\\n"),
            (STRING_TEXT, "b"),
            (QUOTE, "\""),
        ]
    );
}

#[test]
fn all_escape_sequences() {
    assert_eq!(
        tokens(r#""\\\"\n\t""#),
        vec![
            (QUOTE, "\""),
            (STRING_ESCAPE, r"\\"),
            (STRING_ESCAPE, r#"\""#),
            (STRING_ESCAPE, r"\n"),
            (STRING_ESCAPE, r"\t"),
            (QUOTE, "\""),
        ]
    );
}

#[test]
fn invalid_escape_is_string_text() {
    // `\x` is not a recognized escape — backslash becomes part of STRING_TEXT
    let toks = tokens(r#""\x""#);
    assert_eq!(
        toks,
        vec![(QUOTE, "\""), (STRING_TEXT, "\\x"), (QUOTE, "\""),]
    );
}

#[test]
fn backslash_at_end_of_string() {
    // `"\` then closing quote — backslash has no valid next char for escape
    // The `\` followed by `"` is actually a STRING_ESCAPE (\" is valid)
    let toks = tokens(r#""\""#);
    assert_eq!(toks, vec![(QUOTE, "\""), (STRING_ESCAPE, r#"\""#),]);
}

#[test]
fn backslash_before_eof_in_string() {
    // Unterminated string with trailing backslash: `"hello\`
    // `hello` is consumed as STRING_TEXT, then `\` with no valid next byte
    // is consumed as a separate STRING_TEXT on the next iteration
    let toks = tokens("\"hello\\");
    assert_eq!(
        toks,
        vec![(QUOTE, "\""), (STRING_TEXT, "hello"), (STRING_TEXT, "\\"),]
    );
}

#[test]
fn string_with_interpolation() {
    let toks = tokens(r#""J{"o"}e""#);
    let expected = vec![
        (QUOTE, "\""),
        (STRING_TEXT, "J"),
        (L_BRACE, "{"),
        (QUOTE, "\""),
        (STRING_TEXT, "o"),
        (QUOTE, "\""),
        (R_BRACE, "}"),
        (STRING_TEXT, "e"),
        (QUOTE, "\""),
    ];
    assert_eq!(toks, expected);
}

#[test]
fn string_interpolation_with_expression() {
    // `"x is {x + 1}"` — expression inside interpolation
    let toks = tokens(r#""x is {x + 1}""#);
    let expected = vec![
        (QUOTE, "\""),
        (STRING_TEXT, "x is "),
        (L_BRACE, "{"),
        (IDENT, "x"),
        (WHITESPACE, " "),
        (PLUS, "+"),
        (WHITESPACE, " "),
        (INTEGER, "1"),
        (R_BRACE, "}"),
        (QUOTE, "\""),
    ];
    assert_eq!(toks, expected);
}

#[test]
fn nested_string_interpolation() {
    let toks = tokens(r#""a{"b{"c"}d"}e""#);
    let expected = vec![
        (QUOTE, "\""), // depth 0→1
        (STRING_TEXT, "a"),
        (L_BRACE, "{"), // depth 1→2
        (QUOTE, "\""),  // depth 2→3
        (STRING_TEXT, "b"),
        (L_BRACE, "{"), // depth 3→4
        (QUOTE, "\""),  // depth 4→5
        (STRING_TEXT, "c"),
        (QUOTE, "\""),  // depth 5→4
        (R_BRACE, "}"), // depth 4→3
        (STRING_TEXT, "d"),
        (QUOTE, "\""),  // depth 3→2
        (R_BRACE, "}"), // depth 2→1
        (STRING_TEXT, "e"),
        (QUOTE, "\""), // depth 1→0
    ];
    assert_eq!(toks, expected);
}

#[test]
fn empty_string() {
    assert_eq!(tokens(r#""""#), vec![(QUOTE, "\""), (QUOTE, "\"")]);
}

#[test]
fn unterminated_string_at_newline() {
    // Newline terminates the string and pops string depth
    let toks = tokens("\"hello\nworld");
    assert_eq!(
        toks,
        vec![
            (QUOTE, "\""),
            (STRING_TEXT, "hello"),
            (NEWLINE, "\n"),
            (IDENT, "world"),
        ]
    );
}

#[test]
fn unterminated_string_at_crlf() {
    let toks = tokens("\"hello\r\nworld");
    assert_eq!(
        toks,
        vec![
            (QUOTE, "\""),
            (STRING_TEXT, "hello"),
            (NEWLINE, "\r\n"),
            (IDENT, "world"),
        ]
    );
}

#[test]
fn closing_brace_outside_string() {
    // `}` in normal code (string_depth == 0) should be R_BRACE via punctuation
    assert_eq!(kinds("}"), vec![R_BRACE]);
}
