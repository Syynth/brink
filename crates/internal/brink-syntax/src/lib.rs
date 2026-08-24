//! Syntax types and parser for inkle's ink narrative scripting language.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod segment;
pub mod syntax_kind;

pub use lexer::lex;
pub use parser::{Parse, ParseError, parse, parse_with_cache};
pub use segment::{Segment, SegmentKind, segment_file};
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

/// Extract the file paths of all `INCLUDE` directives from an ink source.
///
/// Performs a full parse and walks the resulting AST for `IncludeStmt`
/// nodes, returning each include's raw filename (whitespace-trimmed).
/// Includes nested in commented-out blocks are correctly excluded by
/// the parser.
///
/// Useful for tools that need to discover an ink project's transitive
/// file graph without doing full HIR lowering — for example, the bevy
/// `.ink` asset loader walks the include graph asynchronously and feeds
/// the resulting source cache into the synchronous compiler.
#[must_use]
pub fn extract_includes(source: &str) -> Vec<String> {
    parse(source)
        .tree()
        .includes()
        .filter_map(|inc| inc.file_path())
        .map(|fp| fp.text().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod extract_includes_tests {
    use super::*;

    #[test]
    fn extracts_top_level_includes() {
        let src = "INCLUDE helper.ink\nINCLUDE other.ink\n";
        assert_eq!(extract_includes(src), vec!["helper.ink", "other.ink"]);
    }

    #[test]
    fn empty_source_returns_empty() {
        assert!(extract_includes("").is_empty());
    }

    #[test]
    fn no_includes_returns_empty() {
        assert!(extract_includes("=== knot ===\nhello\n").is_empty());
    }

    #[test]
    fn ignores_commented_out_include() {
        let src = "// INCLUDE commented.ink\nINCLUDE real.ink\n";
        assert_eq!(extract_includes(src), vec!["real.ink"]);
    }
}
