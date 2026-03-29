//! Block lowering — the [`LowerBlock`] trait for body-context AST nodes.
//!
//! Body nodes implement [`LowerBlock`] to produce a [`Block`]. Each impl
//! iterates classified children, delegates shared arms to the accumulator,
//! and only contains its own newline/whitespace logic.

use brink_syntax::ast::{self, AstNode};

use crate::{Block, Content, Stmt};

use super::backbone::{BodyChild, BranchChild, classify_body_child, classify_branch_child};
use super::choice::{LowerChoice, lower_gather_to_block};
use super::content::{
    BodyBackend, ContentAccumulator, DirectBackend, HandleResult, lower_content_node_children,
    lower_tags,
};
use super::context::{LowerScope, LowerSink};
use super::divert::LowerDivert;

use crate::hir::lower::{WeaveItem, fold_weave};

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
                BranchChild::Stop => break,

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

// ─── MultilineBranchBody ────────────────────────────────────────────

impl LowerBlock for ast::MultilineBranchBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block {
        lower_branch_body_from_syntax(self.syntax(), scope, sink)
    }
}

/// Shared branch body logic — used by `MultilineBranchBody::lower_block`
/// and by callers that have a raw `SyntaxNode` (e.g., conditional branches
/// that access `.body().syntax()`).
pub fn lower_branch_body(
    body: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    lower_branch_body_from_syntax(body, scope, sink)
}

fn lower_branch_body_from_syntax(
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

// ─── KnotBody ───────────────────────────────────────────────────────

/// Weave backend that collects `WeaveItem`s and calls `fold_weave` on finish.
struct WeaveBackend {
    items: Vec<WeaveItem>,
}

impl WeaveBackend {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn push_choice(&mut self, choice: crate::Choice, depth: usize) {
        self.items.push(WeaveItem::Choice {
            choice: Box::new(choice),
            depth,
        });
    }

    fn push_gather(&mut self, block: Block, depth: usize) {
        self.items.push(WeaveItem::Continuation { block, depth });
    }
}

impl BodyBackend for WeaveBackend {
    fn push_stmt(&mut self, stmt: Stmt) {
        self.items.push(WeaveItem::Stmt(stmt));
    }

    fn finish(self) -> Block {
        fold_weave(self.items)
    }
}

impl LowerBlock for ast::KnotBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block {
        lower_weave_body(self.syntax(), scope, sink)
    }
}

// ─── StitchBody ─────────────────────────────────────────────────────

impl LowerBlock for ast::StitchBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block {
        lower_weave_body(self.syntax(), scope, sink)
    }
}

// ─── Weave body (shared by KnotBody, StitchBody, SourceFile root) ──

/// Lower body children with full weave folding.
///
/// Used by `KnotBody`, `StitchBody`, and the source file root content.
pub fn lower_weave_body(
    parent: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let mut acc = ContentAccumulator::new(WeaveBackend::new());

    for child in parent.children() {
        match classify_body_child(&child) {
            BodyChild::ContentLine(cl) => {
                acc.handle(&cl, scope, sink);
            }
            BodyChild::LogicLine(ll) => {
                acc.handle(&ll, scope, sink);
            }
            BodyChild::TagLine(tl) => {
                acc.handle(&tl, scope, sink);
            }
            BodyChild::DivertNode(dn) => {
                acc.handle(&dn, scope, sink);
            }
            BodyChild::InlineLogic(il) => {
                acc.handle(&il, scope, sink);
            }
            BodyChild::MultilineBlock(mb) => {
                acc.handle(&mb, scope, sink);
            }

            BodyChild::Choice(c) => {
                acc.flush();
                let depth = c.bullets().map_or(1, |b| b.depth());
                if let Ok(choice) = c.lower_choice(scope, sink) {
                    acc.backend_mut().push_choice(choice, depth);
                }
            }
            BodyChild::Gather(g) => {
                acc.flush();
                let depth = g.dashes().map_or(1, |d| d.depth());
                acc.backend_mut()
                    .push_gather(lower_gather_to_block(&g, scope, sink), depth);
                if let Some(c) = g.choice() {
                    let choice_depth = c.bullets().map_or(1, |b| b.depth());
                    if let Ok(choice) = c.lower_choice(scope, sink) {
                        acc.backend_mut().push_choice(choice, choice_depth);
                    }
                }
            }

            BodyChild::Structural | BodyChild::Trivia => {}
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
