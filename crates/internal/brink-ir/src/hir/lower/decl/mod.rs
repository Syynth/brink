//! Declaration lowering phase.
//!
//! The [`DeclareSymbols`] trait is implemented on AST declaration nodes.
//! Each impl registers the declared symbol in the [`LowerSink`] and
//! produces the corresponding HIR declaration node.

mod constant;
mod external;
mod list;
mod var;

use super::context::{LowerScope, LowerSink, Lowered};

/// Extension trait for AST declaration nodes that register symbols and
/// produce HIR declaration types.
pub trait DeclareSymbols {
    type Output;

    fn declare_and_lower(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<Self::Output>;
}
