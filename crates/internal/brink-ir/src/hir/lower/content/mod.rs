//! Content and body lowering phase.
//!
//! Defines rich output types for content lines and logic lines, the
//! [`LowerBody`] trait, the [`BodyBackend`] trait, and the
//! [`ContentAccumulator`] that ties everything together.

mod accumulator;
mod content_line;
mod divert_node;
mod helpers;
mod inline_logic;
mod logic_line;
mod multiline_block;
mod tag_line;

pub use accumulator::ContentAccumulator;
pub use content_line::ContentLineOutput;
pub use helpers::{lower_content_node_children, lower_tags};
pub use logic_line::LogicLineOutput;

use super::context::{LowerScope, LowerSink, Lowered};

// ─── LowerBody trait ────────────────────────────────────────────────

/// Extension trait for AST nodes that contribute statements to a body.
pub trait LowerBody {
    type Output;
    fn lower_body(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Self::Output>;
}

// ─── BodyBackend trait ──────────────────────────────────────────────

/// Backend for the [`ContentAccumulator`]. Determines where flushed
/// statements go — directly into a `Vec<Stmt>`, or into weave items.
pub trait BodyBackend {
    fn push_stmt(&mut self, stmt: crate::Stmt);
    fn finish(self) -> crate::Block;
}

/// Direct backend: collects statements into a `Block`. For branch bodies.
#[derive(Default)]
pub struct DirectBackend {
    stmts: Vec<crate::Stmt>,
}

impl DirectBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BodyBackend for DirectBackend {
    fn push_stmt(&mut self, stmt: crate::Stmt) {
        self.stmts.push(stmt);
    }

    fn finish(self) -> crate::Block {
        crate::Block {
            label: None,
            stmts: self.stmts,
        }
    }
}

// ─── HandleResult ───────────────────────────────────────────────────

/// Indicates whether a handled node produced block-level output or inline
/// content. Used by branch bodies for whitespace tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleResult {
    /// Output produced block-level statement(s).
    Block,
    /// Output produced inline content parts (or nothing).
    Inline,
}

// ─── Integrate trait ────────────────────────────────────────────────

/// Tells the [`ContentAccumulator`] how to consume a typed output from
/// [`LowerBody`]. Returns [`HandleResult`] indicating the nature of the output.
pub trait Integrate<T> {
    fn integrate(&mut self, output: T) -> HandleResult;
}
