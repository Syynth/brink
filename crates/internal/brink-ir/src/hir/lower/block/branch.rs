//! `LowerBlock` impl for `ast::MultilineBranchBody` + shared branch body logic.

use brink_syntax::ast::{self, AstNode};
use rowan::TextRange;

use crate::Block;

use super::super::backbone::{BranchChild, classify_branch_child};
use super::super::choice::LowerChoice;
use super::super::content::{BodyBackend, ContentAccumulator, HandleResult};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerBlock;
use super::weave::WeaveBackend;

// ─── MultilineBranchBody ────────────────────────────────────────────

impl LowerBlock for ast::MultilineBranchBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Block> {
        Ok(lower_branch_body_from_syntax(self.syntax(), scope, sink))
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

#[expect(
    clippy::too_many_lines,
    reason = "one arm per BranchChild variant — grows with the classifier \
              (NS-A2 added the annotation-line arm)"
)]
fn lower_branch_body_from_syntax(
    body: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let mut acc = ContentAccumulator::new(WeaveBackend::new(), scope.file_id);
    // Deferred inter-token whitespace, carried alongside its own source
    // range (issue #981) — flushed as a `push_text` just before whichever
    // content token follows it, same as the text itself.
    let mut pending_ws: Option<(String, TextRange)> = None;
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
            BranchChild::TagLine(tl) => {
                pending_ws = None;
                acc.handle(&tl, scope, sink);
            }
            BranchChild::AnnotationLine(al) => {
                // NS-A2: never a recognized placement in branch context.
                super::super::directive::handle_annotation_line(&al, sink);
            }
            BranchChild::DivertNode(dn) => {
                pending_ws = None;
                acc.handle(&dn, scope, sink);
            }
            BranchChild::InlineLogic(il) => {
                let range = il.syntax().text_range();
                match acc.handle(&il, scope, sink) {
                    HandleResult::Block => {
                        pending_ws = None;
                        after_content_block = true;
                    }
                    HandleResult::Inline => {
                        flush_pending_ws(&mut acc, &mut pending_ws);
                        acc.note_range(range);
                        seen_content = true;
                    }
                }
            }
            BranchChild::Text(t) => {
                flush_pending_ws(&mut acc, &mut pending_ws);
                seen_content = true;
                let range = child.text_range();
                acc.push_text(t, range);
            }
            BranchChild::Glue => {
                // Issue #3507: after an inline construct, the deferred
                // whitespace becomes a `Spring` rather than `Text(" ")` —
                // the same lowering as `lower_content_node_children`, so a
                // lifted arm never ends up as a whitespace-only line
                // (`emit_line " "; glue`), which the runtime's glue scan
                // would take for content.
                if pending_ws.is_some() && acc.last_part_is_inline_construct() {
                    pending_ws = None;
                    acc.push_glue_after(child.text_range(), true);
                } else {
                    flush_pending_ws(&mut acc, &mut pending_ws);
                    acc.push_glue(child.text_range());
                }
                seen_content = true;
            }
            BranchChild::Escape(t) => {
                flush_pending_ws(&mut acc, &mut pending_ws);
                seen_content = true;
                let range = child.text_range();
                acc.push_escape(&t, range);
            }
            BranchChild::Choice(c) => {
                pending_ws = None;
                acc.flush();
                let depth = c.bullets().map_or(1, |b| b.depth());
                if let Ok(choice) = c.lower_choice(scope, sink) {
                    acc.backend_mut().push_choice(choice, depth);
                }
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
                    let range = child.text_range();
                    if let Some((ref mut existing, ref mut existing_range)) = pending_ws {
                        existing.push_str(&ws);
                        *existing_range = existing_range.cover(range);
                    } else {
                        pending_ws = Some((ws, range));
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

/// Flush deferred whitespace (if any) into the accumulator as text.
fn flush_pending_ws<B: BodyBackend>(
    acc: &mut ContentAccumulator<B>,
    pending_ws: &mut Option<(String, TextRange)>,
) {
    if let Some((ws, range)) = pending_ws.take() {
        acc.push_text(ws, range);
    }
}
