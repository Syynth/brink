//! Choice and gather lowering.
//!
//! Implements [`LowerChoice`] on `ast::Choice` and provides gather lowering.

use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode, AstPtr, SyntaxNodePtr};

use crate::{
    Block, Choice, Content, ContentPart, DiagnosticCode, Divert, Expr, InfixOp, Stmt, SymbolKind,
    Tag,
};

use super::backbone::{BodyChild, classify_body_child};
use super::content::{ContentAccumulator, DirectBackend, lower_content_node_children, lower_tags};
use super::context::{LowerScope, LowerSink, Lowered};
use super::divert::{LowerDivert, lower_divert_target_with_args};
use super::expr::LowerExpr;
use super::helpers::{content_ends_with_glue, name_from_ident};

// ─── LowerChoice trait ──────────────────────────────────────────────

/// Extension trait for lowering a choice AST node.
pub trait LowerChoice {
    fn lower_choice(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Choice>;
}

#[expect(
    clippy::too_many_lines,
    reason = "choice lowering has many CST regions"
)]
impl LowerChoice for ast::Choice {
    fn lower_choice(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Choice> {
        let range = self.syntax().text_range();
        let bullets = self
            .bullets()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E019))?;
        let is_sticky = bullets.is_sticky();

        let label = self.label().and_then(|l| name_from_ident(&l.identifier()?));

        if let Some(ref label_name) = label {
            let qualified = scope.qualify_label(&label_name.text);
            sink.declare(SymbolKind::Label, &qualified, label_name.range);
        }

        let is_fallback = self.start_content().is_none()
            && self.bracket_content().is_none()
            && self.inner_content().is_none();

        let condition = self
            .conditions()
            .filter_map(|c| c.expr().and_then(|e| e.lower_expr(scope, sink).ok()))
            .reduce(|a, b| Expr::Infix(Box::new(a), InfixOp::And, Box::new(b)));

        let mut start_content = self.start_content().map(|sc| {
            let mut parts = lower_content_node_children(sc.syntax(), scope, sink);
            replace_trailing_ws_with_spring(&mut parts);
            Content {
                ptr: None,
                parts,
                tags: Vec::new(),
            }
        });

        let bracket_content = self.bracket_content().map(|bc| {
            let bracket_tags: Vec<Tag> = bc
                .syntax()
                .children()
                .filter_map(ast::Tags::cast)
                .flat_map(|t| lower_tags(Some(t), scope, sink))
                .collect();
            Content {
                ptr: None,
                parts: lower_content_node_children(bc.syntax(), scope, sink),
                tags: bracket_tags,
            }
        });

        let mut inner_content = self.inner_content().map(|ic| Content {
            ptr: None,
            parts: lower_content_node_children(ic.syntax(), scope, sink),
            tags: Vec::new(),
        });

        // Distribute choice-level tags to the appropriate content region.
        {
            let mut last_region = "start";
            for child in self.syntax().children() {
                match child.kind() {
                    SyntaxKind::CHOICE_START_CONTENT => last_region = "start",
                    SyntaxKind::CHOICE_BRACKET_CONTENT => last_region = "bracket",
                    SyntaxKind::CHOICE_INNER_CONTENT => last_region = "inner",
                    SyntaxKind::TAGS => {
                        let tags_node = ast::Tags::cast(child);
                        let lowered = lower_tags(tags_node, scope, sink);
                        match last_region {
                            "start" => {
                                if let Some(ref mut sc) = start_content {
                                    sc.tags.extend(lowered);
                                }
                            }
                            _ => {
                                if let Some(ref mut ic) = inner_content {
                                    ic.tags.extend(lowered);
                                } else if let Some(ref mut sc) = start_content {
                                    sc.tags.extend(lowered);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let inline_divert = self.divert().and_then(|d| {
            let target = d
                .simple_divert()?
                .targets()
                .next()
                .and_then(|t| lower_divert_target_with_args(&t, scope, sink))?;
            Some(Divert {
                ptr: Some(SyntaxNodePtr::from_node(d.syntax())),
                target,
            })
        });

        let tags = Vec::new();
        let has_empty_simple_divert = self.divert().is_some_and(|d| {
            d.simple_divert()
                .is_some_and(|sd| sd.targets().next().is_none())
        });
        let skip_divert = inline_divert.is_some() || has_empty_simple_divert;
        let mut body = lower_choice_body(self, skip_divert, scope, sink);

        let mut preamble = Vec::new();
        if let Some(d) = inline_divert {
            preamble.push(Stmt::Divert(d));
        }
        preamble.push(Stmt::EndOfLine);
        preamble.append(&mut body.stmts);
        body.stmts = preamble;

        Ok(Choice {
            ptr: AstPtr::new(self),
            is_sticky,
            is_fallback,
            label,
            condition,
            start_content,
            bracket_content,
            inner_content,
            tags,
            body,
            container_id: None,
        })
    }
}

// ─── Choice body ────────────────────────────────────────────────────

/// Lower the body of a choice using the classifier + accumulator pattern.
///
/// The choice's structural children (bullets, label, content regions, tags)
/// are skipped by the classifier (they're `Structural`/`Trivia`). Only
/// body-level children (content lines, logic lines, diverts, inline logic)
/// are dispatched through the accumulator.
fn lower_choice_body(
    choice: &ast::Choice,
    skip_divert: bool,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let choice_divert_range = if skip_divert {
        choice.divert().map(|d| d.syntax().text_range())
    } else {
        None
    };

    let mut acc = ContentAccumulator::new(DirectBackend::new());

    for child in choice.syntax().children() {
        // Skip the inline divert if it was already captured.
        if choice_divert_range.is_some_and(|r| r == child.text_range()) {
            continue;
        }

        match classify_body_child(&child) {
            BodyChild::ContentLine(cl) => {
                acc.handle(&cl, scope, sink);
            }
            BodyChild::LogicLine(ll) => {
                acc.handle(&ll, scope, sink);
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
            BodyChild::TagLine(tl) => {
                acc.handle(&tl, scope, sink);
            }
            // Choice structural parts + weave items are skipped.
            BodyChild::Choice(_)
            | BodyChild::Gather(_)
            | BodyChild::Structural
            | BodyChild::Trivia => {}
        }
    }

    acc.finish()
}

// ─── Gather ─────────────────────────────────────────────────────────

/// Lower an AST gather into a continuation `Block`.
pub fn lower_gather_to_block(
    gather: &ast::Gather,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Block {
    let label = gather
        .label()
        .and_then(|l| name_from_ident(&l.identifier()?));

    if let Some(ref label_name) = label {
        let qualified = scope.qualify_label(&label_name.text);
        sink.declare(SymbolKind::Label, &qualified, label_name.range);
    }

    let content = gather.mixed_content().map(|mc| Content {
        ptr: None,
        parts: lower_content_node_children(mc.syntax(), scope, sink),
        tags: Vec::new(),
    });

    let divert_stmt = gather
        .divert()
        .and_then(|d| d.lower_divert(scope, sink).ok());
    let tags = lower_tags(gather.tags(), scope, sink);

    let mut stmts = Vec::new();
    let has_content = content
        .as_ref()
        .is_some_and(|c| !c.parts.is_empty() || !tags.is_empty());
    let ends_glue = content
        .as_ref()
        .is_some_and(|c| content_ends_with_glue(&c.parts));
    if let Some(c) = content
        && has_content
    {
        stmts.push(Stmt::Content(Content {
            ptr: None,
            parts: c.parts,
            tags,
        }));
    }
    if let Some(d) = divert_stmt {
        stmts.push(d);
    } else if has_content && !ends_glue {
        stmts.push(Stmt::EndOfLine);
    }

    Block {
        label,
        stmts,
        container_id: None,
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn replace_trailing_ws_with_spring(parts: &mut Vec<ContentPart>) {
    if let Some(ContentPart::Text(t)) = parts.last_mut()
        && t.ends_with(char::is_whitespace)
    {
        let trimmed = t.trim_end().to_string();
        if trimmed.is_empty() {
            parts.pop();
        } else {
            *t = trimmed;
        }
        parts.push(ContentPart::Spring);
    }
}
