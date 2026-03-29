//! Block lowering — the [`LowerBlock`] trait for body-context AST nodes.
//!
//! Body nodes implement [`LowerBlock`] to produce a [`Block`]. Each impl
//! iterates classified children, delegates shared arms to the accumulator,
//! and only contains its own newline/whitespace logic.

use brink_syntax::ast::{self, AstNode};

use crate::{Block, Content, Stmt};

use super::backbone::{BranchChild, classify_branch_child};
use super::content::{
    ContentAccumulator, DirectBackend, HandleResult, lower_content_node_children, lower_tags,
};
use super::context::{LowerScope, LowerSink};
use super::divert::LowerDivert;

// ─── LowerBlock trait ───────────────────────────────────────────────

/// "I am a body container — lower my children into a [`Block`]."
pub trait LowerBlock {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block;
}

// ─── BranchlessCondBody ─────────────────────────────────────────────

impl LowerBlock for ast::BranchlessCondBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block {
        let mut acc = ContentAccumulator::new(DirectBackend::new());
        let mut is_multiline = false;

        for child in self.syntax().children_with_tokens() {
            match classify_branch_child(&child) {
                // Shared: delegate to accumulator via generic handle
                BranchChild::ContentLine(cl) => {
                    acc.handle(&cl, scope, sink);
                }
                BranchChild::LogicLine(ll) => {
                    acc.handle(&ll, scope, sink);
                }
                BranchChild::DivertNode(dn) => {
                    acc.handle(&dn, scope, sink);
                }
                BranchChild::InlineLogic(il) => {
                    acc.handle(&il, scope, sink);
                }
                BranchChild::Text(t) => acc.push_text(t),
                BranchChild::Glue => acc.push_glue(),
                BranchChild::Escape(t) => acc.push_escape(&t),
                BranchChild::Choice(_) | BranchChild::Whitespace(_) | BranchChild::Trivia => {}

                // Stop at else branch
                BranchChild::Stop => break,

                // Branchless-specific newline logic
                BranchChild::Newline => {
                    let was_multiline = is_multiline;
                    is_multiline = true;
                    if acc.has_buffered_parts() {
                        let ends_glue = acc.ends_with_glue();
                        acc.flush();
                        if !ends_glue {
                            acc.push_eol();
                        }
                    } else if acc.last_was_content() || !was_multiline {
                        acc.push_eol();
                    }
                }
            }
        }

        acc.flush();
        if is_multiline && acc.last_was_content() {
            acc.push_eol();
        }
        acc.finish()
    }
}

// ─── Branch body ────────────────────────────────────────────────────

/// Lower a multiline branch body syntax node to a [`Block`].
pub fn lower_branch_body(
    body: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let mut acc = ContentAccumulator::new(DirectBackend::new());
    let mut pending_ws: Option<String> = None;
    let mut seen_content = false;
    let mut after_content_block = false;

    for child in body.children_with_tokens() {
        match classify_branch_child(&child) {
            // Shared: delegate to accumulator via generic handle
            BranchChild::ContentLine(cl) => {
                pending_ws = None;
                acc.handle(&cl, scope, sink);
            }
            BranchChild::LogicLine(ll) => {
                pending_ws = None;
                acc.handle(&ll, scope, sink);
            }
            BranchChild::DivertNode(dn) => {
                pending_ws = None;
                acc.handle(&dn, scope, sink);
            }
            BranchChild::InlineLogic(il) => match acc.handle(&il, scope, sink) {
                HandleResult::Block => {
                    pending_ws = None;
                    after_content_block = true;
                }
                HandleResult::Inline => {
                    if let Some(ws) = pending_ws.take() {
                        acc.push_text(ws);
                    }
                    seen_content = true;
                }
            },
            BranchChild::Text(t) => {
                if let Some(ws) = pending_ws.take() {
                    acc.push_text(ws);
                }
                seen_content = true;
                acc.push_text(t);
            }
            BranchChild::Glue => {
                if let Some(ws) = pending_ws.take() {
                    acc.push_text(ws);
                }
                seen_content = true;
                acc.push_glue();
            }
            BranchChild::Escape(t) => {
                if let Some(ws) = pending_ws.take() {
                    acc.push_text(ws);
                }
                seen_content = true;
                acc.push_escape(&t);
            }
            BranchChild::Choice(_) => {
                pending_ws = None;
                acc.flush();
            }
            BranchChild::Trivia => {}
            BranchChild::Stop => break,

            // Branch-specific newline logic
            BranchChild::Newline => {
                if acc.has_buffered_parts() {
                    let ends_glue = acc.ends_with_glue();
                    acc.flush();
                    if !ends_glue {
                        acc.push_eol();
                    }
                } else if after_content_block {
                    acc.push_eol();
                }
                seen_content = false;
                pending_ws = None;
                after_content_block = false;
            }

            // Branch-specific: preserve whitespace between content nodes
            BranchChild::Whitespace(ws) => {
                if seen_content {
                    if let Some(ref mut existing) = pending_ws {
                        existing.push_str(&ws);
                    } else {
                        pending_ws = Some(ws);
                    }
                }
            }
        }
    }

    if acc.has_buffered_parts() {
        let ends_glue = acc.ends_with_glue();
        acc.flush();
        if !ends_glue {
            acc.push_eol();
        }
    }

    acc.finish()
}

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
