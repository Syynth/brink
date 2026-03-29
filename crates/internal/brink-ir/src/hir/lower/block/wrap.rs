//! `wrap_content_as_block` — wraps content-level children as a single-statement `Block`.

use brink_syntax::ast::{self, AstNode};

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
        stmts.push(Stmt::Content(Content {
            ptr: None,
            parts,
            tags,
        }));
    }
    if let Some(d) = divert_stmt {
        stmts.push(d);
    }
    if stmts.is_empty() {
        return Block::default();
    }
    Block { label: None, stmts }
}
