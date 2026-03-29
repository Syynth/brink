//! Weave body lowering — `WeaveBackend`, `lower_weave_body`, and
//! `LowerBlock` impls for `KnotBody` / `StitchBody`.

use brink_syntax::ast::{self, AstNode};

use crate::{Block, Stmt};

use super::super::backbone::{BodyChild, classify_body_child};
use super::super::choice::{LowerChoice, lower_gather_to_block};
use super::super::content::{BodyBackend, ContentAccumulator};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerBlock;

use crate::hir::lower::{WeaveItem, fold_weave};

// ─── WeaveBackend ───────────────────────────────────────────────────

/// Weave backend that collects `WeaveItem`s and calls `fold_weave` on finish.
pub(super) struct WeaveBackend {
    items: Vec<WeaveItem>,
}

impl WeaveBackend {
    pub(super) fn new() -> Self {
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

// ─── KnotBody ───────────────────────────────────────────────────────

impl LowerBlock for ast::KnotBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Block> {
        Ok(lower_weave_body(self.syntax(), scope, sink))
    }
}

// ─── StitchBody ─────────────────────────────────────────────────────

impl LowerBlock for ast::StitchBody {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Block> {
        Ok(lower_weave_body(self.syntax(), scope, sink))
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
