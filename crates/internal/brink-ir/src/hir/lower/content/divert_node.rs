use brink_syntax::ast;

use crate::Stmt;

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::divert::LowerDivert;
use super::LowerBody;

impl LowerBody for ast::DivertNode {
    type Output = Stmt;

    fn lower_body(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Stmt> {
        self.lower_divert(scope, sink)
    }
}
