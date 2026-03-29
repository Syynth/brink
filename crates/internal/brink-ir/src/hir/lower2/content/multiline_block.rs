use brink_syntax::ast;

use crate::Stmt;

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::LowerBody;

impl LowerBody for ast::MultilineBlock {
    type Output = Option<Stmt>;

    fn lower_body(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Option<Stmt>> {
        Ok(super::super::conditional::lower_multiline_block(
            self, scope, sink,
        ))
    }
}
