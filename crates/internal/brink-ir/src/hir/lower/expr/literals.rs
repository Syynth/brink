//! Literal expression lowering: integers, floats, booleans, strings.

use brink_syntax::ast::{self, AstNode};

use crate::{DiagnosticCode, Expr, FloatBits};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::lower_string_lit;
use super::LowerExpr;

// ─── Literals ───────────────────────────────────────────────────────

impl LowerExpr for ast::IntegerLit {
    fn lower_expr(&self, _scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        #[expect(clippy::cast_possible_truncation, reason = "ink integers are 32-bit")]
        match self.value() {
            Some(v) => Ok(Expr::Int(v as i32)),
            None => Err(sink.diagnose(range, DiagnosticCode::E015)),
        }
    }
}

impl LowerExpr for ast::FloatLit {
    fn lower_expr(&self, _scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        match self.value() {
            Some(v) => Ok(Expr::Float(FloatBits::from_f64(v))),
            None => Err(sink.diagnose(range, DiagnosticCode::E015)),
        }
    }
}

impl LowerExpr for ast::BooleanLit {
    fn lower_expr(&self, _scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        match self.value() {
            Some(v) => Ok(Expr::Bool(v)),
            None => Err(sink.diagnose(range, DiagnosticCode::E015)),
        }
    }
}

impl LowerExpr for ast::StringLit {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        Ok(Expr::String(lower_string_lit(self, scope, sink)))
    }
}
