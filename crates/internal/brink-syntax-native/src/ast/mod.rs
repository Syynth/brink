//! Typed AST wrappers for the native `.brink` CST.
//!
//! Every struct is a zero-cost newtype around [`SyntaxNode`] that implements
//! [`AstNode`], mirroring `brink-syntax`'s pattern exactly (studied from
//! `crates/internal/brink-syntax/src/ast/mod.rs`) but over this crate's own
//! `SyntaxKind`. Use [`crate::Parse::tree()`] to get a [`SourceFile`] from a
//! parse result.

mod nodes;
mod support;

pub use nodes::*;

use crate::SyntaxNode;

/// A typed wrapper around a [`SyntaxNode`].
pub trait AstNode: Sized {
    /// Returns `true` for a node with a `SyntaxKind` this type can wrap.
    fn can_cast(kind: crate::SyntaxKind) -> bool;

    /// Try to cast a generic `SyntaxNode` into this typed wrapper.
    fn cast(node: SyntaxNode) -> Option<Self>;

    /// Access the underlying `SyntaxNode`.
    fn syntax(&self) -> &SyntaxNode;
}

/// Generates a zero-cost newtype struct implementing [`AstNode`].
macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            syntax: $crate::SyntaxNode,
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(&self.syntax, f)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.syntax.text(), f)
            }
        }

        impl $crate::ast::AstNode for $name {
            fn can_cast(kind: $crate::SyntaxKind) -> bool {
                kind == $crate::SyntaxKind::$kind
            }

            fn cast(node: $crate::SyntaxNode) -> Option<Self> {
                if Self::can_cast(node.kind()) {
                    Some(Self { syntax: node })
                } else {
                    None
                }
            }

            fn syntax(&self) -> &$crate::SyntaxNode {
                &self.syntax
            }
        }
    };
}

pub(crate) use ast_node;

#[cfg(test)]
mod tests;
