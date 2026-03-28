//! Body dispatcher — the backbone of the lowering pass.
//!
//! Defines the [`BodyChild`] enum for exhaustive dispatch over CST children,
//! the [`classify_body_child`] function as the single classification point,
//! and the body dispatcher that ties everything together.

use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode};

use crate::{Block, Content, Stmt};

use super::conditional::{lower_multiline_block, lower_multiline_block_from_inline};
use super::content::{ContentAccumulator, Integrate, LowerBody, lower_tags};
use super::context::{LowerScope, LowerSink};
use super::divert::LowerDivert;

// ─── Body child classification ──────────────────────────────────────

/// Exhaustive classification of children within a body context.
///
/// The backbone matches on this enum, never on raw [`SyntaxKind`].
/// Adding a new variant forces all match sites to update — the compiler
/// catches missing arms.
pub enum BodyChild {
    ContentLine(ast::ContentLine),
    LogicLine(ast::LogicLine),
    TagLine(ast::TagLine),
    DivertNode(ast::DivertNode),
    InlineLogic(ast::InlineLogic),
    MultilineBlock(ast::MultilineBlock),
    Choice(ast::Choice),
    Gather(ast::Gather),
    /// Structural children (knot headers, declarations, etc.) that the
    /// body dispatcher explicitly skips. Listed for exhaustiveness.
    Structural,
    /// Trivia (whitespace, comments, empty lines).
    Trivia,
}

/// Classify a CST child node into a [`BodyChild`] variant.
///
/// This is the **single point of truth** for what kinds of children
/// appear in a body context. If a new `SyntaxKind` is added to the
/// parser, it must be handled here — the `debug_assert` catches
/// unrecognized node kinds in debug builds.
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
        // Structural children handled by the parent (source file, knot body).
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
        // Trivia and empty lines.
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

// ─── Body dispatcher ────────────────────────────────────────────────

/// Lower a simple body (no weave folding) to a [`Block`].
///
/// Handles content lines, logic lines, tag lines, diverts, inline logic,
/// and multiline blocks. Choices and gathers are present in the match for
/// exhaustiveness but not yet wired into weave folding.
pub fn lower_simple_body(
    parent: &brink_syntax::SyntaxNode,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let mut acc = ContentAccumulator::new();

    for child in parent.children() {
        match classify_body_child(&child) {
            BodyChild::ContentLine(cl) => {
                if let Ok(output) = cl.lower_body(scope, sink) {
                    acc.integrate(output);
                }
            }
            BodyChild::LogicLine(ll) => {
                if let Ok(output) = ll.lower_body(scope, sink) {
                    acc.integrate(output);
                }
            }
            BodyChild::TagLine(tl) => {
                let tags = lower_tags(tl.tags(), scope, sink);
                if !tags.is_empty() {
                    acc.push_stmt(Stmt::Content(Content {
                        ptr: None,
                        parts: Vec::new(),
                        tags,
                    }));
                    acc.push_stmt(Stmt::EndOfLine);
                }
            }
            BodyChild::DivertNode(dn) => {
                if let Ok(stmt) = dn.lower_divert(scope, sink) {
                    acc.push_stmt(stmt);
                }
            }
            BodyChild::InlineLogic(il) => {
                if let Some(stmt) = lower_multiline_block_from_inline(&il, scope, sink) {
                    acc.push_stmt(stmt);
                }
            }
            BodyChild::MultilineBlock(mb) => {
                if let Some(stmt) = lower_multiline_block(&mb, scope, sink) {
                    acc.push_stmt(stmt);
                }
            }
            // Choices/gathers (not yet wired) + structural + trivia.
            BodyChild::Choice(_)
            | BodyChild::Gather(_)
            | BodyChild::Structural
            | BodyChild::Trivia => {}
        }
    }

    Block {
        label: None,
        stmts: acc.finish(),
    }
}
