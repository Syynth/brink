//! `LowerBlock` impl for `ast::MultilineBranchBody` + shared branch body logic.

use brink_syntax::ast::{self, AstNode};

use crate::Block;

use super::super::backbone::{BranchChild, classify_branch_child};
use super::super::content::{ContentAccumulator, DirectBackend, HandleResult};
use super::super::context::{LowerScope, LowerSink};
use super::LowerBlock;

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
