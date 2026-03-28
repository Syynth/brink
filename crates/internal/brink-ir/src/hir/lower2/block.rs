//! Block lowering — the [`LowerBlock`] trait for body-context AST nodes.
//!
//! Body nodes (`BranchlessCondBody`, `MultilineBranchBody`, etc.) implement
//! [`LowerBlock`] to produce a [`Block`]. Each impl configures the appropriate
//! dispatch context (which children are valid, how newlines are tracked).

use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode};

use crate::{Block, Content, ContentPart, Stmt};

use super::conditional::{lower_inline_logic_into_parts, lower_multiline_block_from_inline};
use super::content::{LowerBody, lower_content_node_children, lower_tags};
use super::context::{LowerScope, LowerSink};
use super::divert::LowerDivert;
use super::helpers::content_ends_with_glue;

// ─── LowerBlock trait ───────────────────────────────────────────────

/// "I am a body container — lower my children into a [`Block`]."
///
/// Implemented on body-context AST nodes. Each impl owns its dispatch
/// rules (valid children, newline handling). Callers just get a `Block`.
pub trait LowerBlock {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block;
}

// ─── BranchlessCondBody ─────────────────────────────────────────────

impl LowerBlock for ast::BranchlessCondBody {
    #[expect(clippy::too_many_lines, reason = "match arms are individually simple")]
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Block {
        let mut stmts = Vec::new();
        let mut parts = Vec::new();
        let mut is_multiline = false;

        for child in self.syntax().children_with_tokens() {
            match child.kind() {
                SyntaxKind::ELSE_BRANCH => break,
                SyntaxKind::CONTENT_LINE => {
                    if let Some(cl) = child.into_node().and_then(ast::ContentLine::cast) {
                        let line_parts = cl.mixed_content().map_or_else(Vec::new, |mc| {
                            lower_content_node_children(mc.syntax(), scope, sink)
                        });
                        parts.extend(line_parts);
                        let tags = lower_tags(cl.tags(), scope, sink);
                        if !parts.is_empty() || !tags.is_empty() {
                            stmts.push(Stmt::Content(Content {
                                ptr: None,
                                parts: std::mem::take(&mut parts),
                                tags,
                            }));
                        }
                        if let Some(dn) = cl.divert()
                            && let Ok(s) = dn.lower_divert(scope, sink)
                        {
                            stmts.push(s);
                        }
                    }
                }
                SyntaxKind::LOGIC_LINE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(ll) = child.into_node().and_then(ast::LogicLine::cast)
                        && let Ok(output) = ll.lower_body(scope, sink)
                    {
                        let needs_eol = output.has_call();
                        stmts.push(output.into_stmt());
                        if needs_eol {
                            stmts.push(Stmt::EndOfLine);
                        }
                    }
                }
                SyntaxKind::DIVERT_NODE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(dn) = child.into_node().and_then(ast::DivertNode::cast)
                        && let Ok(stmt) = dn.lower_divert(scope, sink)
                    {
                        stmts.push(stmt);
                    }
                }
                SyntaxKind::INLINE_LOGIC => {
                    if let Some(il) = child.into_node().and_then(ast::InlineLogic::cast) {
                        if let Some(stmt) = lower_multiline_block_from_inline(&il, scope, sink) {
                            flush_content_parts(&mut parts, &mut stmts);
                            stmts.push(stmt);
                        } else {
                            lower_inline_logic_into_parts(&il, &mut parts, scope, sink);
                        }
                    }
                }
                SyntaxKind::TEXT => {
                    let text = child.to_string();
                    if !text.is_empty() {
                        parts.push(ContentPart::Text(text));
                    }
                }
                SyntaxKind::NEWLINE => {
                    let was_multiline = is_multiline;
                    is_multiline = true;
                    if !parts.is_empty() {
                        let ends_glue = content_ends_with_glue(&parts);
                        flush_content_parts(&mut parts, &mut stmts);
                        if !ends_glue {
                            stmts.push(Stmt::EndOfLine);
                        }
                    } else if stmts.last().is_some_and(|s| matches!(s, Stmt::Content(_)))
                        || !was_multiline
                    {
                        stmts.push(Stmt::EndOfLine);
                    }
                }
                SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
                SyntaxKind::ESCAPE => {
                    let text = child.to_string();
                    if text.len() > 1 {
                        parts.push(ContentPart::Text(text[1..].to_string()));
                    }
                }
                SyntaxKind::CHOICE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    // Choice lowering deferred to choice phase
                }
                other if other.is_token() => {}
                other => {
                    debug_assert!(
                        other.is_token(),
                        "unexpected SyntaxKind in BranchlessCondBody::lower_block: {other:?}"
                    );
                }
            }
        }
        flush_content_parts(&mut parts, &mut stmts);

        if is_multiline && stmts.last().is_some_and(|s| matches!(s, Stmt::Content(_))) {
            stmts.push(Stmt::EndOfLine);
        }

        Block { label: None, stmts }
    }
}

// ─── Branch body (free function for SyntaxNode) ─────────────────────

/// Lower a branch body `SyntaxNode` to a [`Block`].
///
/// Used for `MultilineBranchBody` and other contexts where the body is
/// accessed as a raw `SyntaxNode` via `.body().syntax()`.
#[expect(clippy::too_many_lines, reason = "match arms are individually simple")]
pub fn lower_branch_body(
    body: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let mut stmts = Vec::new();
    let mut parts = Vec::new();
    let mut pending_ws: Option<String> = None;
    let mut seen_content = false;
    let mut after_content_block = false;

    for child in body.children_with_tokens() {
        if let rowan::NodeOrToken::Token(ref token) = child {
            if token.kind() == SyntaxKind::NEWLINE {
                if !parts.is_empty() {
                    let ends_glue = content_ends_with_glue(&parts);
                    flush_content_parts(&mut parts, &mut stmts);
                    if !ends_glue {
                        stmts.push(Stmt::EndOfLine);
                    }
                } else if after_content_block {
                    stmts.push(Stmt::EndOfLine);
                }
                seen_content = false;
                pending_ws = None;
                after_content_block = false;
            } else if seen_content && token.kind() == SyntaxKind::WHITESPACE {
                let text = token.text().to_string();
                if let Some(ref mut ws) = pending_ws {
                    ws.push_str(&text);
                } else {
                    pending_ws = Some(text);
                }
            }
            continue;
        }
        let rowan::NodeOrToken::Node(child) = child else {
            continue;
        };
        if matches!(
            child.kind(),
            SyntaxKind::TEXT | SyntaxKind::GLUE_NODE | SyntaxKind::ESCAPE
        ) {
            if let Some(ws) = pending_ws.take() {
                parts.push(ContentPart::Text(ws));
            }
            seen_content = true;
        } else if child.kind() != SyntaxKind::INLINE_LOGIC {
            pending_ws = None;
        }
        match child.kind() {
            SyntaxKind::CONTENT_LINE => {
                if let Some(cl) = ast::ContentLine::cast(child) {
                    if let Some(mc) = cl.mixed_content()
                        && let Some(il) = mc.inline_logics().next()
                        && let Some(stmt) = lower_multiline_block_from_inline(&il, scope, sink)
                    {
                        flush_content_parts(&mut parts, &mut stmts);
                        stmts.push(stmt);
                        continue;
                    }
                    let line_parts = cl.mixed_content().map_or_else(Vec::new, |mc| {
                        lower_content_node_children(mc.syntax(), scope, sink)
                    });
                    parts.extend(line_parts);
                    let tags = lower_tags(cl.tags(), scope, sink);
                    let has_divert = cl.divert().is_some();
                    let ends_glue = content_ends_with_glue(&parts);
                    if !parts.is_empty() || !tags.is_empty() {
                        stmts.push(Stmt::Content(Content {
                            ptr: None,
                            parts: std::mem::take(&mut parts),
                            tags,
                        }));
                    }
                    if let Some(dn) = cl.divert()
                        && let Ok(s) = dn.lower_divert(scope, sink)
                    {
                        stmts.push(s);
                    }
                    if !has_divert && !ends_glue {
                        stmts.push(Stmt::EndOfLine);
                    }
                }
            }
            SyntaxKind::LOGIC_LINE => {
                flush_content_parts(&mut parts, &mut stmts);
                if let Some(ll) = ast::LogicLine::cast(child)
                    && let Ok(output) = ll.lower_body(scope, sink)
                {
                    let needs_eol = output.has_call();
                    stmts.push(output.into_stmt());
                    if needs_eol {
                        stmts.push(Stmt::EndOfLine);
                    }
                }
            }
            SyntaxKind::DIVERT_NODE => {
                flush_content_parts(&mut parts, &mut stmts);
                if let Some(dn) = ast::DivertNode::cast(child)
                    && let Ok(stmt) = dn.lower_divert(scope, sink)
                {
                    stmts.push(stmt);
                }
            }
            SyntaxKind::INLINE_LOGIC => {
                if let Some(il) = ast::InlineLogic::cast(child) {
                    if let Some(stmt) = lower_multiline_block_from_inline(&il, scope, sink) {
                        pending_ws = None;
                        flush_content_parts(&mut parts, &mut stmts);
                        stmts.push(stmt);
                        after_content_block = true;
                    } else {
                        if let Some(ws) = pending_ws.take() {
                            parts.push(ContentPart::Text(ws));
                        }
                        seen_content = true;
                        lower_inline_logic_into_parts(&il, &mut parts, scope, sink);
                    }
                }
            }
            SyntaxKind::TEXT => {
                let text = child.text().to_string();
                if !text.is_empty() {
                    parts.push(ContentPart::Text(text));
                }
            }
            SyntaxKind::NEWLINE => {
                if !parts.is_empty() {
                    let ends_glue = content_ends_with_glue(&parts);
                    flush_content_parts(&mut parts, &mut stmts);
                    if !ends_glue {
                        stmts.push(Stmt::EndOfLine);
                    }
                }
                seen_content = false;
                pending_ws = None;
            }
            SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
            SyntaxKind::ESCAPE => {
                let text = child.text().to_string();
                if text.len() > 1 {
                    parts.push(ContentPart::Text(text[1..].to_string()));
                }
            }
            SyntaxKind::CHOICE => {
                flush_content_parts(&mut parts, &mut stmts);
                // Choice lowering deferred to choice phase
            }
            other if other.is_token() => {}
            other => {
                debug_assert!(
                    other.is_token(),
                    "unexpected SyntaxKind in lower_branch_body: {other:?}"
                );
            }
        }
    }
    if !parts.is_empty() {
        let ends_glue = content_ends_with_glue(&parts);
        flush_content_parts(&mut parts, &mut stmts);
        if !ends_glue {
            stmts.push(Stmt::EndOfLine);
        }
    }

    Block { label: None, stmts }
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

// ─── Shared helper ──────────────────────────────────────────────────

fn flush_content_parts(parts: &mut Vec<ContentPart>, stmts: &mut Vec<Stmt>) {
    if !parts.is_empty() {
        stmts.push(Stmt::Content(Content {
            ptr: None,
            parts: std::mem::take(parts),
            tags: Vec::new(),
        }));
    }
}
