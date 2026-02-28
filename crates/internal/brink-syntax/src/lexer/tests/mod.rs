mod basic;
mod keywords;
mod numbers;
mod operators;
mod roundtrip;
mod strings;
mod unicode;

use crate::SyntaxKind;
use crate::lexer::lex;

/// Lex and return just the kinds.
fn kinds(src: &str) -> Vec<SyntaxKind> {
    lex(src).into_iter().map(|(k, _)| k).collect()
}

/// Lex and return (kind, text) pairs.
fn tokens(src: &str) -> Vec<(SyntaxKind, &str)> {
    lex(src)
}
