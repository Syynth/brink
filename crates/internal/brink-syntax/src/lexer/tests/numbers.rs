use super::*;
use SyntaxKind::*;

#[test]
fn integers() {
    assert_eq!(tokens("42"), vec![(INTEGER, "42")]);
    assert_eq!(tokens("0"), vec![(INTEGER, "0")]);
}

#[test]
fn floats() {
    assert_eq!(tokens("3.14"), vec![(FLOAT, "3.14")]);
    assert_eq!(tokens("0.5"), vec![(FLOAT, "0.5")]);
}

#[test]
fn float_vs_integer_dot() {
    assert_eq!(kinds("42."), vec![INTEGER, DOT]);
    assert_eq!(kinds("42.x"), vec![INTEGER, DOT, IDENT]);
}

#[test]
fn float_followed_by_ident_is_ident() {
    assert_eq!(tokens("3.14e"), vec![(IDENT, "3.14e")]);
}

#[test]
fn digit_start_identifier() {
    assert_eq!(tokens("512x2"), vec![(IDENT, "512x2")]);
    assert_eq!(tokens("3abc"), vec![(IDENT, "3abc")]);
}

#[test]
fn digit_start_unicode_identifier() {
    // Digits followed by a Unicode ident char → IDENT
    assert_eq!(tokens("42café"), vec![(IDENT, "42café")]);
}
