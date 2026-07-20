//! Lexer and error-resilient CST for the `.brink` native surface.
//!
//! This crate is the **B0.5 grammar skeleton** (`docs/b0-sequencing.md`
//! §B0.5): the token vocabulary and a lossless, error-resilient rowan CST
//! for the ruled native surface subset (`docs/native-surface-charter.md`
//! §5/§6, NF-2's writer-sufficient subset). It performs **no HIR
//! lowering, no resolution, no type-checking** — those are B0.6/B0.7/B0.8.
//!
//! Peer crate to `brink-syntax` (the ink frontend), per the NF-1 ruling: its
//! own `SyntaxKind` space, its own rowan tree, depending on nothing
//! ink-shaped.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod syntax_kind;

pub use lexer::lex;
pub use parser::{Parse, ParseError, parse, parse_with_cache};
pub use syntax_kind::{NativeLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

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
