use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::{Block, DiagnosticCode, Sequence, SequenceType, Stmt};

use super::super::block::{lower_branch_body, wrap_content_as_block};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerSequence;

// ── SequenceWithAnnotation (inline or block) ────────────────────────

impl LowerSequence for ast::SequenceWithAnnotation {
    fn lower_sequence(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Sequence> {
        let kind = lower_sequence_type(self);

        let branches = if let Some(inline_branches) = self.inline_branches() {
            inline_branches
                .branches()
                .map(|b| wrap_content_as_block(b.syntax(), scope, sink))
                .collect()
        } else if let Some(ml_branches) = self.multiline_branches() {
            ml_branches
                .branches()
                .map(|b| {
                    b.body().map_or_else(Block::default, |body| {
                        lower_branch_body(body.syntax(), scope, sink)
                    })
                })
                .collect()
        } else {
            return Err(sink.diagnose(self.syntax().text_range(), DiagnosticCode::E021));
        };

        Ok(Sequence {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            kind,
            branches,
            container_id: None,
        })
    }
}

// ── ImplicitSequence ────────────────────────────────────────────────

impl LowerSequence for ast::ImplicitSequence {
    fn lower_sequence(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Sequence> {
        let branches: Vec<Block> = self
            .branches()
            .map(|b| wrap_content_as_block(b.syntax(), scope, sink))
            .collect();
        Ok(Sequence {
            ptr: SyntaxNodePtr::from_node(self.syntax()),
            kind: SequenceType::STOPPING,
            branches,
            container_id: None,
        })
    }
}

// ─── Block-level sequence ───────────────────────────────────────────

/// Lower a `SequenceWithAnnotation` as a block-level sequence (leading
/// `EndOfLine` per branch). Used by multiline block promotion.
pub fn lower_block_sequence(
    seq: &ast::SequenceWithAnnotation,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Sequence {
    let kind = lower_sequence_type(seq);
    let branches = seq.multiline_branches().map_or_else(Vec::new, |ml| {
        ml.branches()
            .map(|b| {
                let mut block = b.body().map_or_else(Block::default, |body| {
                    lower_branch_body(body.syntax(), scope, sink)
                });
                block.stmts.insert(0, Stmt::EndOfLine);
                block
            })
            .collect()
    });
    Sequence {
        ptr: SyntaxNodePtr::from_node(seq.syntax()),
        kind,
        branches,
        container_id: None,
    }
}

pub(super) fn lower_sequence_type(seq: &ast::SequenceWithAnnotation) -> SequenceType {
    let mut kind = SequenceType::empty();

    if let Some(sym) = seq.symbol_annotation() {
        if sym.amp_token().is_some() {
            kind |= SequenceType::CYCLE;
        }
        if sym.bang_token().is_some() {
            kind |= SequenceType::ONCE;
        }
        if sym.tilde_token().is_some() {
            kind |= SequenceType::SHUFFLE;
        }
        if sym.dollar_token().is_some() {
            kind |= SequenceType::STOPPING;
        }
    }

    if let Some(word) = seq.word_annotation() {
        if word.stopping_kw().is_some() {
            kind |= SequenceType::STOPPING;
        }
        if word.cycle_kw().is_some() {
            kind |= SequenceType::CYCLE;
        }
        if word.shuffle_kw().is_some() {
            kind |= SequenceType::SHUFFLE;
        }
        if word.once_kw().is_some() {
            kind |= SequenceType::ONCE;
        }
    }

    if kind.is_empty() {
        SequenceType::STOPPING
    } else {
        kind
    }
}
