//! Trait-based HIR lowering (v2).
//!
//! A parallel implementation of HIR lowering that uses extension traits
//! on AST nodes, a read-only scope / write-only sink split, and typed
//! output enums with the [`Diagnosed`] proof token to prevent silent drops.
//!
//! This module coexists with the original `lower` module. Both produce
//! the same HIR types; the intent is to eventually swap.

mod backbone;
mod block;
mod choice;
mod conditional;
mod content;
mod context;
mod decl;
mod divert;
mod expr;
mod helpers;
mod structure;

#[cfg(test)]
mod tests;

// Re-export core infrastructure.
pub use context::{Diagnosed, EffectSink, LowerScope, LowerSink, Lowered};

// Re-export phase traits.
pub use block::LowerBlock;
pub use choice::LowerChoice;
pub use conditional::{LowerConditional, LowerSequence};
pub use content::{BodyBackend, LowerBody};
pub use decl::DeclareSymbols;
pub use divert::LowerDivert;
pub use expr::LowerExpr;

// Re-export backbone.
pub use backbone::{
    BodyChild, BranchChild, classify_body_child, classify_branch_child, lower_simple_body,
};

// Re-export accumulator types.
pub use content::{
    ContentAccumulator, ContentLineOutput, DirectBackend, HandleResult, Integrate, LogicLineOutput,
};

// Re-export public API (same signatures as crate::hir::lower).
pub use structure::{lower, lower_single_knot, lower_top_level};
