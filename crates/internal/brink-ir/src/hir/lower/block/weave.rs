//! Weave body lowering — `WeaveBackend`, `lower_weave_body`, and
//! `LowerBlock` impls for `KnotBody` / `StitchBody`.

use brink_syntax::ast::{self, AstNode};

use crate::{Block, Choice, ChoiceSet, ChoiceSetContext, Name, Stmt};

use super::super::backbone::{BodyChild, classify_body_child};
use super::super::choice::{LowerChoice, lower_gather_to_block};
use super::super::content::{BodyBackend, ContentAccumulator};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerBlock;

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

// ─── WeaveItem + Weave folding ─────────────────────────────────────

pub enum WeaveItem {
    Choice { choice: Box<Choice>, depth: usize },
    Continuation { block: Block, depth: usize },
    Stmt(Stmt),
}

/// Fold a flat stream of `WeaveItem`s into a recursively nested `Block`.
///
/// Matches the reference ink compiler's `ConstructWeaveHierarchyFromIndentation`:
/// items at deeper depths are recursively folded and inserted into the preceding
/// weave point's body.
pub fn fold_weave(items: Vec<WeaveItem>) -> Block {
    let base_depth = determine_base_depth(&items);
    fold_weave_at_depth(items, base_depth)
}

/// Determine the base depth from the first choice or gather in the list.
fn determine_base_depth(items: &[WeaveItem]) -> usize {
    for item in items {
        match item {
            WeaveItem::Choice { depth, .. } | WeaveItem::Continuation { depth, .. } => {
                return *depth;
            }
            WeaveItem::Stmt(_) => {}
        }
    }
    1
}

/// Fold items at a given base depth. Items at deeper depths are collected
/// and recursively folded into the preceding weave point's body.
fn fold_weave_at_depth(items: Vec<WeaveItem>, base_depth: usize) -> Block {
    // Phase 1: Group nested items into sub-weaves (matching ConstructWeaveHierarchyFromIndentation)
    let items = nest_deeper_items(items, base_depth);

    // Phase 2: Build choice sets from the now-single-depth stream.
    //
    // Key invariant: everything after a gather nests *inside* the gather's
    // continuation block. When we encounter a Continuation after accumulated
    // choices, we recursively fold all remaining items into the continuation
    // and stop — producing a nested tree, not flat siblings.
    let mut stmts = Vec::new();
    let mut choice_acc: Vec<Choice> = Vec::new();
    let mut last_standalone_label: Option<Name> = None;
    // Tracks where in `stmts` a standalone labeled gather's content begins,
    // so we can retroactively wrap it in a LabeledBlock if no choices follow.
    let mut gather_stmts_start: Option<usize> = None;

    let mut iter = items.into_iter();
    while let Some(item) = iter.next() {
        match item {
            WeaveItem::Stmt(stmt) => {
                if choice_acc.is_empty() {
                    stmts.push(stmt);
                } else {
                    // Content between choices belongs to the previous choice's body
                    // (matches reference ink's addContentToPreviousWeavePoint)
                    if let Some(c) = choice_acc.last_mut() {
                        c.body.stmts.push(stmt);
                    }
                }
            }
            WeaveItem::Choice { choice, .. } => {
                choice_acc.push(*choice);
            }
            WeaveItem::Continuation { block, depth } => {
                if choice_acc.is_empty() {
                    // When a new labeled gather arrives while a previous
                    // labeled gather is pending, nest the new gather (and
                    // everything after it) inside the previous one.  This
                    // mirrors inklecate's tail-nesting: `-> opts` loops
                    // back to opts, and because test is nested inside opts,
                    // test is naturally re-entered.
                    if let Some(start) = gather_stmts_start.take()
                        && let Some(prev_label) = last_standalone_label.take()
                        && block.label.is_some()
                    {
                        let mut gather_stmts = stmts.split_off(start);
                        // Recurse: fold the new gather + remaining items.
                        let mut remaining = vec![WeaveItem::Continuation { block, depth }];
                        remaining.extend(iter);
                        let nested = fold_weave_at_depth(remaining, base_depth);
                        gather_stmts.extend(nested.stmts);

                        stmts.push(Stmt::LabeledBlock(Box::new(Block {
                            label: Some(prev_label),
                            stmts: gather_stmts,
                        })));
                        return Block { label: None, stmts };
                    }
                    // Standalone gather — emit content as stmts, save label
                    gather_stmts_start = block.label.as_ref().map(|_| stmts.len());
                    emit_standalone_gather(&mut stmts, &block);
                    last_standalone_label = block.label;
                } else {
                    // Gather after choices — label was consumed as opening label.
                    // Collect remaining items, fold them recursively, and nest
                    // everything into the continuation.
                    let mut continuation = block;
                    let remaining: Vec<WeaveItem> = iter.collect();
                    if !remaining.is_empty() {
                        let nested = fold_weave_at_depth(remaining, base_depth);
                        continuation.stmts.extend(nested.stmts);
                    }
                    flush_choices(
                        &mut stmts,
                        &mut choice_acc,
                        continuation,
                        last_standalone_label.take(),
                        gather_stmts_start.take(),
                        base_depth,
                    );
                    // All remaining items consumed — we're done
                    return Block { label: None, stmts };
                }
            }
        }
    }

    // If a standalone labeled gather was never consumed by a choice set,
    // retroactively wrap its content in a LabeledBlock so the planning phase
    // allocates a container for it (making it a valid divert target).
    if choice_acc.is_empty()
        && let Some(start) = gather_stmts_start
        && let Some(label) = last_standalone_label.take()
    {
        let gather_stmts = stmts.split_off(start);
        stmts.push(Stmt::LabeledBlock(Box::new(Block {
            label: Some(label),
            stmts: gather_stmts,
        })));
    }

    flush_choices(
        &mut stmts,
        &mut choice_acc,
        Block::default(),
        last_standalone_label.take(),
        gather_stmts_start,
        base_depth,
    );
    Block { label: None, stmts }
}

/// Extract runs of deeper-depth items and recursively fold them into nested blocks,
/// inserting the result into the preceding weave point's body.
fn nest_deeper_items(items: Vec<WeaveItem>, base_depth: usize) -> Vec<WeaveItem> {
    let mut result = Vec::new();
    let mut iter = items.into_iter().peekable();

    while let Some(item) = iter.next() {
        let depth = item_depth(&item);

        if let Some(d) = depth
            && d > base_depth
        {
            // Collect all consecutive items at this deeper depth or beyond
            let inner_depth = d;
            let mut nested_items = vec![item];
            loop {
                let Some(peeked) = iter.peek() else {
                    break;
                };
                if let Some(d) = item_depth(peeked)
                    && d <= base_depth
                {
                    break;
                }
                // Safe: we just peeked successfully
                if let Some(next) = iter.next() {
                    nested_items.push(next);
                }
            }
            let nested_block = fold_weave_at_depth(nested_items, inner_depth);

            // Attach the nested block to the previous weave point's body
            if let Some(WeaveItem::Choice { choice, .. }) = result.last_mut() {
                choice.body.stmts.extend(nested_block.stmts);
            } else {
                // No preceding choice — emit as standalone stmts
                for stmt in nested_block.stmts {
                    result.push(WeaveItem::Stmt(stmt));
                }
            }
        } else {
            result.push(item);
        }
    }

    result
}

fn item_depth(item: &WeaveItem) -> Option<usize> {
    match item {
        WeaveItem::Choice { depth, .. } | WeaveItem::Continuation { depth, .. } => Some(*depth),
        WeaveItem::Stmt(_) => None,
    }
}

#[expect(clippy::cast_possible_truncation)]
fn flush_choices(
    stmts: &mut Vec<Stmt>,
    choice_acc: &mut Vec<Choice>,
    continuation: Block,
    opening_label: Option<Name>,
    gather_stmts_start: Option<usize>,
    base_depth: usize,
) {
    if choice_acc.is_empty() {
        return;
    }
    let choices = std::mem::take(choice_acc);
    let cs = Stmt::ChoiceSet(Box::new(ChoiceSet {
        choices,
        continuation,
        context: ChoiceSetContext::Weave,
        depth: base_depth as u32,
    }));
    if let Some(label) = opening_label {
        // Move statements emitted after the standalone gather into the
        // labeled block so they live inside the gather container.  This
        // ensures thread calls and other code between the gather label
        // and the first choice are re-executed when looping back.
        let mut labeled_stmts = gather_stmts_start
            .map(|start| stmts.split_off(start))
            .unwrap_or_default();
        labeled_stmts.push(cs);
        stmts.push(Stmt::LabeledBlock(Box::new(Block {
            label: Some(label),
            stmts: labeled_stmts,
        })));
    } else {
        stmts.push(cs);
    }
}

/// Emit a standalone gather's content as statements.
///
/// The label is preserved by the caller for potential use as an opening label
/// on a subsequent choice set.
fn emit_standalone_gather(stmts: &mut Vec<Stmt>, block: &Block) {
    for stmt in &block.stmts {
        stmts.push(stmt.clone());
    }
}
