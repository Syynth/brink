use brink_syntax::ast;

use crate::{ContentPart, Stmt};

use super::super::conditional::{lower_inline_logic_into_parts, lower_multiline_block_from_inline};
use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerBody;

/// Output from lowering an `InlineLogic` node in a body context.
pub enum InlineLogicOutput {
    /// Promoted to a block-level statement (multiline conditional/sequence).
    Block(Stmt),
    /// Stayed inline as content parts (interpolation, inline conditional/sequence).
    Inline(Vec<ContentPart>),
}

impl LowerBody for ast::InlineLogic {
    type Output = InlineLogicOutput;

    fn lower_body(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<InlineLogicOutput> {
        // Try block promotion first
        if let Some(stmt) = lower_multiline_block_from_inline(self, scope, sink) {
            return Ok(InlineLogicOutput::Block(stmt));
        }
        // Fallback: inline content parts
        let mut parts = Vec::new();
        lower_inline_logic_into_parts(self, &mut parts, scope, sink);
        Ok(InlineLogicOutput::Inline(parts))
    }
}
