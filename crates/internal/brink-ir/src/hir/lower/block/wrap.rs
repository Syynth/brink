//! `wrap_content_as_block` — wraps content-level children as a single-statement `Block`.

use brink_syntax::ast::{self, AstNode};

use crate::provenance::NodeClass;
use crate::{Block, Content, Stmt};

use super::super::content::{lower_content_node_children, lower_tags};
use super::super::context::{LowerScope, LowerSink};
use super::super::divert::LowerDivert;

// ─── Wrap content as block ──────────────────────────────────────────

/// Wrap content-level children as a single-statement `Block` (for inline branches).
pub fn wrap_content_as_block(
    node: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let parts = lower_content_node_children(node, scope, sink);

    let divert_stmt = node
        .children()
        .find_map(ast::DivertNode::cast)
        .and_then(|dn| dn.lower_divert(scope, sink).ok());

    let tags = lower_tags(node.children().find_map(ast::Tags::cast), scope, sink);

    let mut stmts = Vec::new();
    if !parts.is_empty() || !tags.is_empty() {
        // `ptr: Some(...)`, not `None` (issue #3181) — `node`'s own range
        // (an inline conditional/sequence branch's content) was available
        // right here all along; same fix shape as `hir::lower::choice`'s
        // choice-region `ptr`. Guarded against an empty range the same way
        // (review finding, #3181): `parts`/`tags` non-empty here already
        // implies `node`'s range is non-empty (its children carry the
        // range), but stay defensive rather than relying on that
        // invariant — B0.3 admission's E124 rejects an empty range
        // unconditionally, and `None` is the honest fallback.
        let ptr = if node.text_range().is_empty() {
            None
        } else {
            Some(scope.prov(NodeClass::Content, node))
        };
        stmts.push(Stmt::Content(Content { ptr, parts, tags }));
    }
    if let Some(d) = divert_stmt {
        stmts.push(d);
    }
    if stmts.is_empty() {
        return Block::default();
    }
    let tail = crate::tail_from_stmts(&stmts);
    Block {
        label: None,
        stmts,
        container_id: None,
        tail,
    }
}
