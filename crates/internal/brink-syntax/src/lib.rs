//! Syntax types and parser for inkle's ink narrative scripting language.

pub mod lexer;
pub mod parser;
pub mod syntax_kind;

pub use lexer::lex;
pub use parser::{Parse, ParseError, parse};
pub use syntax_kind::{InkLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
