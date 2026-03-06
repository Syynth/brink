//! Syntax types and parser for inkle's ink narrative scripting language.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod syntax_kind;

pub use lexer::lex;
pub use parser::{Parse, ParseError, parse, parse_with_cache};
pub use syntax_kind::{InkLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

impl Parse {
    /// Returns the typed root AST node.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "parse() always produces SOURCE_FILE root"
    )]
    pub fn tree(&self) -> ast::SourceFile {
        use ast::AstNode as _;
        ast::SourceFile::cast(self.syntax()).expect("parse always produces a SOURCE_FILE root")
    }
}
