//! Generic child/token traversal helpers for typed AST nodes.

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::AstNode;

/// Returns the first child node that can be cast to `N`.
pub(super) fn child<N: AstNode>(parent: &SyntaxNode) -> Option<N> {
    parent.children().find_map(N::cast)
}

/// Returns an iterator over all children that can be cast to `N`.
pub(super) fn children<N: AstNode>(parent: &SyntaxNode) -> impl Iterator<Item = N> {
    parent.children().filter_map(N::cast)
}

/// Returns the first direct-child token matching `kind`.
pub(super) fn token(parent: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|tok| tok.kind() == kind)
}

/// Returns the first direct-child token that is `IDENT` or a keyword.
///
/// Ink keywords are contextual — in positions like list member names,
/// keywords such as `or`, `and`, `not` act as plain identifiers.
pub(super) fn ident_or_keyword_token(parent: &SyntaxNode) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|tok| tok.kind() == SyntaxKind::IDENT || tok.kind().is_keyword())
}

/// Returns an iterator over all direct-child tokens matching `kind`.
pub(super) fn tokens(parent: &SyntaxNode, kind: SyntaxKind) -> impl Iterator<Item = SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(move |tok| tok.kind() == kind)
}
