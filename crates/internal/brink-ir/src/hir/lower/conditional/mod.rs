//! Conditional and sequence normalization traits.
//!
//! Defines [`LowerConditional`] and [`LowerSequence`] — normalization traits
//! that collapse multiple AST representations into their common HIR types.

mod conditional_with_expr;
mod multiline;
mod promotion;
mod sequence;

use crate::{Conditional, Sequence};

use super::context::{LowerScope, LowerSink, Lowered};

// ─── LowerConditional ──────────────────────────────────────────────

/// Normalization trait: multiple AST representations → [`Conditional`].
pub trait LowerConditional {
    fn lower_conditional(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Conditional>;
}

// ─── LowerSequence ─────────────────────────────────────────────────

/// Normalization trait: multiple AST representations → [`Sequence`].
pub trait LowerSequence {
    fn lower_sequence(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Sequence>;
}

// Re-exports.
pub use promotion::{
    lower_inline_logic_into_parts, lower_multiline_block, lower_multiline_block_from_inline,
};
