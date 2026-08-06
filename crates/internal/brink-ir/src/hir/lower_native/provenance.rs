//! The native frontend's side of the provenance seam (contract Q1(b)).
//!
//! Mirrors [`crate::hir::ink_provenance`] exactly (same seam, same
//! algorithm) but stamps/resolves against `brink-syntax-native`'s own
//! `SyntaxKind` space instead of ink's — this is the point of the opaque
//! `Provenance` design: two frontends implementing the same trait, neither
//! reaching into the other's tree (`docs/hir-admission-contract.md` D1).
//! The native `SyntaxKind` lives only inside [`KindToken::raw`] here —
//! nothing outside this module and [`super`]'s lowering may interpret it.

use brink_syntax_native::SyntaxNode;
use brink_syntax_native::ast::AstNode;
use rowan::TextRange;

use crate::hir::FileId;
use crate::provenance::{KindToken, NodeClass, Provenance, ProvenanceResolver};

/// Stamp native-frontend provenance for a lowered HIR node.
///
/// `class` is the frontend-agnostic node class the pipeline may interpret;
/// the node's own native `SyntaxKind` is preserved (privately) in
/// [`KindToken::raw`] so [`NativeProvenanceResolver`] can resolve back to a
/// live native syntax node.
#[must_use]
pub fn native_provenance(file: FileId, class: NodeClass, syntax: &SyntaxNode) -> Provenance {
    Provenance {
        file,
        range: syntax.text_range(),
        kind: KindToken {
            class,
            raw: syntax.kind() as u16,
        },
    }
}

/// The native frontend's [`ProvenanceResolver`].
///
/// Wraps a parse root (plus the [`FileId`] it was parsed from) and resolves
/// native-stamped provenance back to live nodes. Provenance from another
/// file, another frontend (e.g. ink's), or a synthetic stamp resolves to
/// `None` — the same contract [`crate::hir::InkProvenanceResolver`] upholds.
pub struct NativeProvenanceResolver<'a> {
    file: FileId,
    root: &'a SyntaxNode,
}

impl<'a> NativeProvenanceResolver<'a> {
    /// A resolver over `root`, the parse tree of `file`.
    #[must_use]
    pub fn new(file: FileId, root: &'a SyntaxNode) -> Self {
        Self { file, root }
    }

    /// Resolve to a *typed* AST node — the common IDE-consumer shape.
    #[must_use]
    pub fn resolve_ast<N: AstNode>(&self, provenance: Provenance) -> Option<N> {
        self.resolve(provenance).and_then(N::cast)
    }
}

impl ProvenanceResolver for NativeProvenanceResolver<'_> {
    type Node = SyntaxNode;

    fn resolve(&self, provenance: Provenance) -> Option<SyntaxNode> {
        if provenance.file != self.file {
            return None;
        }
        resolve_by_raw_kind(self.root, provenance.kind.raw, provenance.range)
    }
}

/// Walk up from the covering element to the node matching `raw` kind and
/// `range` exactly — byte-for-byte
/// [`crate::hir::ink_provenance::resolve_by_raw_kind`]'s algorithm (private
/// there), reproduced here because each frontend's resolver is self-
/// contained by design (no shared-tree-walk coupling between frontends).
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

#[cfg(test)]
mod tests {
    use super::*;
    use brink_syntax_native::ast;

    #[test]
    fn resolver_round_trips_a_native_node() {
        let parse = brink_syntax_native::parse("flow greet() {\n  Hi!\n}\n");
        let root = parse.syntax();
        let file = FileId(0);
        let source_file = ast::SourceFile::cast(root.clone()).expect("SOURCE_FILE root");
        let flow = source_file.flows().next().expect("one flow");
        let prov = native_provenance(file, NodeClass::Knot, flow.syntax());

        let resolver = NativeProvenanceResolver::new(file, &root);
        let live_node = resolver.resolve(prov).expect("resolves back");
        assert_eq!(live_node.text_range(), flow.syntax().text_range());
        let typed: ast::FlowDecl = resolver.resolve_ast(prov).expect("resolves to FlowDecl");
        assert_eq!(typed.name_token().unwrap().text(), "greet");
    }

    #[test]
    fn foreign_file_id_never_resolves() {
        let parse = brink_syntax_native::parse("flow greet() {}\n");
        let root = parse.syntax();
        let source_file = ast::SourceFile::cast(root.clone()).expect("SOURCE_FILE root");
        let flow = source_file.flows().next().expect("one flow");
        let prov = native_provenance(FileId(0), NodeClass::Knot, flow.syntax());

        // Resolver bound to a *different* file id — must never resolve,
        // even though the range/raw kind would otherwise match.
        let resolver = NativeProvenanceResolver::new(FileId(1), &root);
        assert!(resolver.resolve(prov).is_none());
    }

    #[test]
    fn synthetic_provenance_never_resolves() {
        let parse = brink_syntax_native::parse("flow greet() {}\n");
        let root = parse.syntax();
        let file = FileId(0);
        let synthetic = Provenance::synthetic(NodeClass::Knot, root.text_range());
        let resolver = NativeProvenanceResolver::new(file, &root);
        assert!(resolver.resolve(synthetic).is_none());
    }
}
