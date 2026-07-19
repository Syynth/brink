//! The ink frontend's side of the provenance seam (contract Q1(b)).
//!
//! [`ink_provenance`] stamps [`Provenance`] from a live ink syntax node
//! during lowering; [`InkProvenanceResolver`] resolves it back for IDE
//! consumers (rename/extract), preserving the exact `SyntaxKind + range`
//! matching the retired `AstPtr::resolve` performed. The ink `SyntaxKind`
//! lives only inside [`KindToken::raw`] here — nothing outside this module
//! and the ink lowering may interpret it (D1).

use brink_syntax::SyntaxNode;
use brink_syntax::ast::AstNode;
use rowan::TextRange;

use crate::provenance::{KindToken, NodeClass, Provenance, ProvenanceResolver};

use super::types::FileId;

/// Stamp ink-frontend provenance for a lowered HIR node.
///
/// `class` is the frontend-agnostic node class the pipeline may interpret;
/// the node's own `SyntaxKind` is preserved (privately) in
/// [`KindToken::raw`] so [`InkProvenanceResolver`] can resolve back with
/// the same exactness the retired `AstPtr` had.
#[must_use]
pub fn ink_provenance(file: FileId, class: NodeClass, syntax: &SyntaxNode) -> Provenance {
    Provenance {
        file,
        range: syntax.text_range(),
        kind: KindToken {
            class,
            raw: syntax.kind() as u16,
        },
    }
}

/// The ink frontend's [`ProvenanceResolver`].
///
/// Wraps a parse root (plus the [`FileId`] it was parsed from) and resolves
/// ink-stamped provenance back to live nodes. Provenance from another file,
/// another frontend, or a synthetic stamp resolves to `None`.
pub struct InkProvenanceResolver<'a> {
    file: FileId,
    root: &'a SyntaxNode,
}

impl<'a> InkProvenanceResolver<'a> {
    /// A resolver over `root`, the parse tree of `file`.
    #[must_use]
    pub fn new(file: FileId, root: &'a SyntaxNode) -> Self {
        Self { file, root }
    }

    /// Resolve to a *typed* AST node — the common IDE-consumer shape
    /// (e.g. `resolve_ast::<ast::IncludeStmt>`).
    #[must_use]
    pub fn resolve_ast<N: AstNode>(&self, provenance: Provenance) -> Option<N> {
        self.resolve(provenance).and_then(N::cast)
    }
}

impl ProvenanceResolver for InkProvenanceResolver<'_> {
    type Node = SyntaxNode;

    fn resolve(&self, provenance: Provenance) -> Option<SyntaxNode> {
        if provenance.file != self.file {
            return None;
        }
        resolve_by_raw_kind(self.root, provenance.kind.raw, provenance.range)
    }
}

/// Walk up from the covering element to the node matching `raw` kind and
/// `range` exactly — byte-for-byte the retired `SyntaxNodePtr::resolve`
/// algorithm, with the kind comparison done in `u16` space (a raw value
/// outside ink's kind space, e.g. [`KindToken::SYNTHETIC_RAW`], simply
/// never matches).
fn resolve_by_raw_kind(root: &SyntaxNode, raw: u16, range: TextRange) -> Option<SyntaxNode> {
    if !root.text_range().contains_range(range) {
        return None;
    }
    let mut node = root.covering_element(range);
    loop {
        match &node {
            rowan::NodeOrToken::Node(n) => {
                if n.text_range() == range && n.kind() as u16 == raw {
                    return Some(n.clone());
                }
                if n.text_range().start() < range.start() {
                    return None;
                }
                let parent = n.parent()?;
                node = rowan::NodeOrToken::Node(parent);
            }
            rowan::NodeOrToken::Token(t) => {
                let parent = t.parent()?;
                node = rowan::NodeOrToken::Node(parent);
            }
        }
    }
}
