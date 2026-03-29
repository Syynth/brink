//! Block lowering — the [`LowerBlock`] trait for body-context AST nodes.
//!
//! Body nodes implement [`LowerBlock`] to produce a [`Block`]. Each impl
//! iterates classified children, delegates shared arms to the accumulator,
//! and only contains its own newline/whitespace logic.

mod branch;
mod branchless;
mod weave;
mod wrap;

pub use branch::lower_branch_body;
pub use weave::lower_weave_body;
pub use wrap::wrap_content_as_block;

use crate::Block;

use super::context::{LowerScope, LowerSink, Lowered};

// ─── LowerBlock trait ───────────────────────────────────────────────

/// "I am a body container — lower my children into a [`Block`]."
pub trait LowerBlock {
    fn lower_block(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Block>;
}
