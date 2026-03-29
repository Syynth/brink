//! Body dispatchers and child classifiers.
//!
//! Defines child classification enums for different body contexts and
//! the dispatcher functions that wire classifiers to the accumulator.

use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode};

use crate::{Block, Content, Stmt};

use super::conditional::lower_multiline_block;
use super::content::{ContentAccumulator, DirectBackend, lower_tags};
use super::context::{LowerScope, LowerSink};

// ─── Weave-context child classification ─────────────────────────────

/// Children in a weave body context (knot, stitch, source file root).
pub enum BodyChild {
    ContentLine(ast::ContentLine),
    LogicLine(ast::LogicLine),
    TagLine(ast::TagLine),
    DivertNode(ast::DivertNode),
    InlineLogic(ast::InlineLogic),
    MultilineBlock(ast::MultilineBlock),
    Choice(ast::Choice),
    Gather(ast::Gather),
    Structural,
    Trivia,
}

/// Classify a CST child in weave body context.
pub fn classify_body_child(node: &brink_syntax::SyntaxNode) -> BodyChild {
    match node.kind() {
        SyntaxKind::CONTENT_LINE => {
            ast::ContentLine::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::ContentLine)
        }
        SyntaxKind::LOGIC_LINE => {
            ast::LogicLine::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::LogicLine)
        }
        SyntaxKind::TAG_LINE => {
            ast::TagLine::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::TagLine)
        }
        SyntaxKind::DIVERT_NODE => {
            ast::DivertNode::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::DivertNode)
        }
        SyntaxKind::INLINE_LOGIC => {
            ast::InlineLogic::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::InlineLogic)
        }
        SyntaxKind::MULTILINE_BLOCK => ast::MultilineBlock::cast(node.clone())
            .map_or(BodyChild::Trivia, BodyChild::MultilineBlock),
        SyntaxKind::CHOICE => {
            ast::Choice::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::Choice)
        }
        SyntaxKind::GATHER => {
            ast::Gather::cast(node.clone()).map_or(BodyChild::Trivia, BodyChild::Gather)
        }
        SyntaxKind::KNOT_DEF
        | SyntaxKind::KNOT_HEADER
        | SyntaxKind::STITCH_DEF
        | SyntaxKind::STITCH_HEADER
        | SyntaxKind::VAR_DECL
        | SyntaxKind::CONST_DECL
        | SyntaxKind::LIST_DECL
        | SyntaxKind::EXTERNAL_DECL
        | SyntaxKind::INCLUDE_STMT
        | SyntaxKind::STRAY_CLOSING_BRACE
        | SyntaxKind::AUTHOR_WARNING => BodyChild::Structural,
        SyntaxKind::EMPTY_LINE => BodyChild::Trivia,
        other => {
            debug_assert!(
                other.is_trivia(),
                "unexpected SyntaxKind in classify_body_child: {other:?}"
            );
            BodyChild::Trivia
        }
    }
}

// ─── Branch-context child classification ────────────────────────────

/// Children in a branch body context (branchless conditional, multiline
/// branch body). Includes raw token-level children (text, newline, etc.)
/// that don't appear in weave context.
pub enum BranchChild {
    ContentLine(ast::ContentLine),
    LogicLine(ast::LogicLine),
    DivertNode(ast::DivertNode),
    InlineLogic(ast::InlineLogic),
    Choice(ast::Choice),
    Text(String),
    Glue,
    Escape(String),
    Newline,
    Whitespace(String),
    /// `ELSE_BRANCH` — signals stop for branchless bodies.
    Stop,
    Trivia,
}

/// Classify children of a branch body (both tokens and nodes).
///
/// Yields `BranchChild` for each child in the syntax node. Call
/// `.children_with_tokens()` externally and pass each element through this.
pub fn classify_branch_child(
    child: &rowan::NodeOrToken<brink_syntax::SyntaxNode, brink_syntax::SyntaxToken>,
) -> BranchChild {
    match child {
        rowan::NodeOrToken::Token(token) => match token.kind() {
            SyntaxKind::NEWLINE => BranchChild::Newline,
            SyntaxKind::WHITESPACE => BranchChild::Whitespace(token.text().to_string()),
            _ => BranchChild::Trivia,
        },
        rowan::NodeOrToken::Node(node) => match node.kind() {
            SyntaxKind::CONTENT_LINE => ast::ContentLine::cast(node.clone())
                .map_or(BranchChild::Trivia, BranchChild::ContentLine),
            SyntaxKind::LOGIC_LINE => ast::LogicLine::cast(node.clone())
                .map_or(BranchChild::Trivia, BranchChild::LogicLine),
            SyntaxKind::DIVERT_NODE => ast::DivertNode::cast(node.clone())
                .map_or(BranchChild::Trivia, BranchChild::DivertNode),
            SyntaxKind::INLINE_LOGIC => ast::InlineLogic::cast(node.clone())
                .map_or(BranchChild::Trivia, BranchChild::InlineLogic),
            SyntaxKind::CHOICE => {
                ast::Choice::cast(node.clone()).map_or(BranchChild::Trivia, BranchChild::Choice)
            }
            SyntaxKind::ELSE_BRANCH => BranchChild::Stop,
            SyntaxKind::TEXT => {
                let text = node.text().to_string();
                if text.is_empty() {
                    BranchChild::Trivia
                } else {
                    BranchChild::Text(text)
                }
            }
            SyntaxKind::GLUE_NODE => BranchChild::Glue,
            SyntaxKind::ESCAPE => {
                let text = node.text().to_string();
                if text.len() > 1 {
                    BranchChild::Escape(text)
                } else {
                    BranchChild::Trivia
                }
            }
            other if other.is_token() => BranchChild::Trivia,
            other => {
                debug_assert!(
                    other.is_trivia(),
                    "unexpected SyntaxKind in classify_branch_child: {other:?}"
                );
                BranchChild::Trivia
            }
        },
    }
}

// ─── Simple body dispatcher (for backbone tests) ────────────────────

/// Lower a simple body (content lines + logic lines, no weave) to a [`Block`].
pub fn lower_simple_body(
    parent: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let mut acc = ContentAccumulator::new(DirectBackend::new());

    for child in parent.children() {
        match classify_body_child(&child) {
            BodyChild::ContentLine(cl) => acc.handle_content_line(&cl, scope, sink),
            BodyChild::LogicLine(ll) => acc.handle_logic_line(&ll, scope, sink),
            BodyChild::TagLine(tl) => {
                let tags = lower_tags(tl.tags(), scope, sink);
                if !tags.is_empty() {
                    acc.flush();
                    acc.push_stmt(Stmt::Content(Content {
                        ptr: None,
                        parts: Vec::new(),
                        tags,
                    }));
                    acc.push_eol();
                }
            }
            BodyChild::DivertNode(dn) => acc.handle_divert(&dn, scope, sink),
            BodyChild::InlineLogic(il) => {
                acc.handle_inline_logic(&il, scope, sink);
            }
            BodyChild::MultilineBlock(mb) => {
                if let Some(stmt) = lower_multiline_block(&mb, scope, sink) {
                    acc.flush();
                    acc.push_stmt(stmt);
                }
            }
            BodyChild::Choice(_)
            | BodyChild::Gather(_)
            | BodyChild::Structural
            | BodyChild::Trivia => {}
        }
    }

    acc.finish()
}
