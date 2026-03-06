use std::marker::PhantomData;

use rowan::TextRange;

use crate::{SyntaxKind, SyntaxNode};

use super::AstNode;

// ─── SyntaxNodePtr (untyped) ────────────────────────────────────────

/// An untyped lightweight pointer to a syntax node, resolvable against a tree.
///
/// Stores `SyntaxKind + TextRange` — the same data as [`AstPtr`] but without
/// a type parameter, so it can point at nodes of heterogeneous kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxNodePtr {
    kind: SyntaxKind,
    range: TextRange,
}

impl SyntaxNodePtr {
    /// Create a pointer from a live syntax node.
    pub fn from_node(node: &SyntaxNode) -> Self {
        Self {
            kind: node.kind(),
            range: node.text_range(),
        }
    }

    /// The text range this pointer points to.
    pub fn text_range(&self) -> TextRange {
        self.range
    }

    /// The syntax kind of the pointed-to node.
    pub fn syntax_kind(&self) -> SyntaxKind {
        self.kind
    }

    /// Resolve this pointer back to a live syntax node.
    pub fn resolve(&self, root: &SyntaxNode) -> Option<SyntaxNode> {
        let mut node = root.covering_element(self.range);
        loop {
            match &node {
                rowan::NodeOrToken::Node(n) => {
                    if n.text_range() == self.range && n.kind() == self.kind {
                        return Some(n.clone());
                    }
                    if n.text_range().start() < self.range.start() {
                        return None;
                    }
                    match n.parent() {
                        Some(parent) => node = rowan::NodeOrToken::Node(parent),
                        None => return None,
                    }
                }
                rowan::NodeOrToken::Token(t) => match t.parent() {
                    Some(parent) => node = rowan::NodeOrToken::Node(parent),
                    None => return None,
                },
            }
        }
    }
}

impl<N: AstNode> From<AstPtr<N>> for SyntaxNodePtr {
    fn from(ptr: AstPtr<N>) -> Self {
        Self {
            kind: ptr.syntax_kind(),
            range: ptr.text_range(),
        }
    }
}

// ─── AstPtr (typed) ─────────────────────────────────────────────────

/// A lightweight pointer to an AST node, resolvable against a syntax tree.
///
/// Stores the node's `SyntaxKind` and `TextRange` — enough to find it again
/// given the tree root, without holding an `Arc` reference to the green tree.
///
/// Follows the pattern used by rust-analyzer's `AstPtr`.
pub struct AstPtr<N: AstNode> {
    kind: SyntaxKind,
    range: TextRange,
    _phantom: PhantomData<fn() -> N>,
}

// Manual impls to avoid requiring `N: Clone/PartialEq/Eq/Hash` bounds.
// The PhantomData<fn() -> N> is always Copy regardless of N.

impl<N: AstNode> Clone for AstPtr<N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<N: AstNode> PartialEq for AstPtr<N> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.range == other.range
    }
}

impl<N: AstNode> Eq for AstPtr<N> {}

impl<N: AstNode> std::hash::Hash for AstPtr<N> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
        self.range.hash(state);
    }
}

impl<N: AstNode> AstPtr<N> {
    /// Create a pointer from a live AST node.
    pub fn new(node: &N) -> Self {
        let syntax = node.syntax();
        Self {
            kind: syntax.kind(),
            range: syntax.text_range(),
            _phantom: PhantomData,
        }
    }

    /// The text range this pointer points to.
    pub fn text_range(&self) -> TextRange {
        self.range
    }

    /// The syntax kind of the pointed-to node.
    pub fn syntax_kind(&self) -> SyntaxKind {
        self.kind
    }

    /// Resolve this pointer back to a live AST node.
    ///
    /// Walks the tree from `root` to find a node with matching kind and range.
    /// Returns `None` if the tree has been reparsed and no matching node exists
    /// (stale pointer).
    pub fn resolve(&self, root: &SyntaxNode) -> Option<N> {
        // Find the node that covers this range, then walk ancestors to find
        // the one with the right kind.
        self.resolve_syntax(root).and_then(N::cast)
    }

    fn resolve_syntax(&self, root: &SyntaxNode) -> Option<SyntaxNode> {
        // Start with the covering element at this range
        let mut node = root.covering_element(self.range);

        // Walk up to find our exact match
        loop {
            match &node {
                rowan::NodeOrToken::Node(n) => {
                    if n.text_range() == self.range && n.kind() == self.kind {
                        return Some(n.clone());
                    }
                    // If we've gone past our range, bail
                    if n.text_range().start() < self.range.start() {
                        return None;
                    }
                    match n.parent() {
                        Some(parent) => node = rowan::NodeOrToken::Node(parent),
                        None => return None,
                    }
                }
                rowan::NodeOrToken::Token(t) => match t.parent() {
                    Some(parent) => node = rowan::NodeOrToken::Node(parent),
                    None => return None,
                },
            }
        }
    }
}

impl<N: AstNode> std::fmt::Debug for AstPtr<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AstPtr")
            .field("kind", &self.kind)
            .field("range", &self.range)
            .finish()
    }
}

impl<N: AstNode> Copy for AstPtr<N> {}
