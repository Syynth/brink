use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{Content, ContentPart, Stmt};

use super::super::conditional::{lower_inline_logic_into_parts, lower_multiline_block_from_inline};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::divert::LowerDivert;
use super::super::helpers::content_ends_with_glue;
use super::LowerBody;
use super::helpers::{lower_content_node_children, lower_tags};

/// Structured output from lowering a [`ast::ContentLine`].
pub enum ContentLineOutput {
    /// A content statement with optional trailing divert.
    Content {
        content: Content,
        divert: Option<Stmt>,
        ends_with_glue: bool,
    },
    /// A bare divert with no content (e.g., `-> knot`).
    BareDivert(Stmt),
    /// The content line wraps a promoted multiline block.
    /// All trailing content and divert are pre-lowered.
    PromotedBlock {
        stmt: Stmt,
        trailing_content: Option<Content>,
        divert: Option<Stmt>,
        needs_eol: bool,
    },
    /// The line had no content, no divert, no tags.
    Empty,
}

impl LowerBody for ast::ContentLine {
    type Output = ContentLineOutput;

    fn lower_body(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<ContentLineOutput> {
        // Check if this content line wraps a multiline block-level construct.
        if let Some(mc) = self.mixed_content()
            && let Some(il) = mc.inline_logics().next()
            && let Some(stmt) = lower_multiline_block_from_inline(&il, scope, sink)
        {
            let il_syntax = il.syntax().clone();
            let mut past_promoted = false;
            let mut trailing_parts = Vec::new();
            for child in mc.syntax().children_with_tokens() {
                if let rowan::NodeOrToken::Node(ref child_node) = child
                    && *child_node == il_syntax
                {
                    past_promoted = true;
                    continue;
                }
                if !past_promoted {
                    continue;
                }
                if let rowan::NodeOrToken::Node(child_node) = child {
                    match child_node.kind() {
                        brink_syntax::SyntaxKind::TEXT => {
                            let text = child_node.text().to_string();
                            if !text.is_empty() {
                                trailing_parts.push(ContentPart::Text(text));
                            }
                        }
                        brink_syntax::SyntaxKind::GLUE_NODE => {
                            trailing_parts.push(ContentPart::Glue);
                        }
                        brink_syntax::SyntaxKind::ESCAPE => {
                            let text = child_node.text().to_string();
                            if text.len() > 1 {
                                trailing_parts.push(ContentPart::Text(text[1..].to_string()));
                            }
                        }
                        brink_syntax::SyntaxKind::INLINE_LOGIC => {
                            if let Some(inline) = ast::InlineLogic::cast(child_node) {
                                lower_inline_logic_into_parts(
                                    &inline,
                                    &mut trailing_parts,
                                    scope,
                                    sink,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }

            let trailing_content = if trailing_parts.is_empty() {
                None
            } else {
                Some(Content {
                    ptr: None,
                    parts: trailing_parts,
                    tags: Vec::new(),
                })
            };
            let divert = self
                .divert()
                .and_then(|dn| dn.lower_divert(scope, sink).ok());
            let ends_glue = trailing_content
                .as_ref()
                .is_some_and(|c| content_ends_with_glue(&c.parts));
            let needs_eol = (trailing_content.is_some() && !ends_glue && divert.is_none())
                || (trailing_content.is_none() && divert.is_none());

            return Ok(ContentLineOutput::PromotedBlock {
                stmt,
                trailing_content,
                divert,
                needs_eol,
            });
        }

        let parts = self
            .mixed_content()
            .map(|mc| lower_content_node_children(mc.syntax(), scope, sink))
            .unwrap_or_default();
        let tags = lower_tags(self.tags(), scope, sink);

        if parts.is_empty() && tags.is_empty() {
            if let Some(dn) = self.divert()
                && let Ok(stmt) = dn.lower_divert(scope, sink)
            {
                return Ok(ContentLineOutput::BareDivert(stmt));
            }
            return Ok(ContentLineOutput::Empty);
        }

        let ends_with_glue = content_ends_with_glue(&parts);
        let divert = self
            .divert()
            .and_then(|dn| dn.lower_divert(scope, sink).ok());

        Ok(ContentLineOutput::Content {
            content: Content {
                ptr: Some(SyntaxNodePtr::from_node(self.syntax())),
                parts,
                tags,
            },
            divert,
            ends_with_glue,
        })
    }
}
