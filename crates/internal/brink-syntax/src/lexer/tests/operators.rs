use super::*;
use SyntaxKind::*;

#[test]
fn divert_disambiguation() {
    assert_eq!(kinds("->->"), vec![TUNNEL_ONWARDS]);
    assert_eq!(kinds("->"), vec![DIVERT]);
    assert_eq!(kinds("-"), vec![MINUS]);
    assert_eq!(kinds("--"), vec![MINUS, MINUS]);
    assert_eq!(kinds("-="), vec![MINUS_EQ]);
}

#[test]
fn angle_bracket_disambiguation() {
    assert_eq!(kinds("<>"), vec![GLUE]);
    assert_eq!(kinds("<-"), vec![THREAD]);
    assert_eq!(kinds("<="), vec![LT_EQ]);
    assert_eq!(kinds("<"), vec![LT]);
}

#[test]
fn equals_disambiguation() {
    assert_eq!(kinds("=="), vec![EQ_EQ]);
    assert_eq!(kinds("="), vec![EQ]);
    assert_eq!(kinds("==="), vec![EQ_EQ, EQ]);
    assert_eq!(kinds("===="), vec![EQ_EQ, EQ_EQ]);
    assert_eq!(kinds("====="), vec![EQ_EQ, EQ_EQ, EQ]);
}

#[test]
fn bang_disambiguation() {
    assert_eq!(kinds("!="), vec![BANG_EQ]);
    assert_eq!(kinds("!?"), vec![BANG_QUESTION]);
    assert_eq!(kinds("!"), vec![BANG]);
}

#[test]
fn pipe_disambiguation() {
    assert_eq!(kinds("||"), vec![PIPE_PIPE]);
    assert_eq!(kinds("|"), vec![PIPE]);
}

#[test]
fn ampersand_disambiguation() {
    assert_eq!(kinds("&&"), vec![AMP_AMP]);
    assert_eq!(kinds("&"), vec![AMP]);
}

#[test]
fn plus_disambiguation() {
    assert_eq!(kinds("++"), vec![PLUS, PLUS]);
    assert_eq!(kinds("+="), vec![PLUS_EQ]);
    assert_eq!(kinds("+"), vec![PLUS]);
}

#[test]
fn greater_disambiguation() {
    assert_eq!(kinds(">="), vec![GT_EQ]);
    assert_eq!(kinds(">"), vec![GT]);
}

#[test]
fn slash_disambiguation() {
    assert_eq!(kinds("/ "), vec![SLASH, WHITESPACE]);
    assert_eq!(kinds("//x"), vec![LINE_COMMENT]);
    assert_eq!(kinds("/**/"), vec![BLOCK_COMMENT]);
}

#[test]
fn single_char_punctuation() {
    assert_eq!(kinds("*"), vec![STAR]);
    assert_eq!(kinds("%"), vec![PERCENT]);
    assert_eq!(kinds("^"), vec![CARET]);
    assert_eq!(kinds("?"), vec![QUESTION]);
    assert_eq!(kinds("$"), vec![DOLLAR]);
    assert_eq!(kinds("("), vec![L_PAREN]);
    assert_eq!(kinds(")"), vec![R_PAREN]);
    assert_eq!(kinds("{"), vec![L_BRACE]);
    assert_eq!(kinds("}"), vec![R_BRACE]);
    assert_eq!(kinds("["), vec![L_BRACKET]);
    assert_eq!(kinds("]"), vec![R_BRACKET]);
    assert_eq!(kinds(","), vec![COMMA]);
    assert_eq!(kinds("."), vec![DOT]);
    assert_eq!(kinds(":"), vec![COLON]);
    assert_eq!(kinds("#"), vec![HASH]);
    assert_eq!(kinds("~"), vec![TILDE]);
    assert_eq!(kinds("\\"), vec![BACKSLASH]);
}
