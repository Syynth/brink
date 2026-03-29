//! Expression lowering phase.
//!
//! The [`LowerExpr`] trait is implemented on AST expression nodes. Each node
//! knows how to lower itself to an HIR [`Expr`], emitting diagnostics via
//! the [`LowerSink`] and registering unresolved references.

mod literals;
mod operators;
mod references;

use brink_syntax::ast;

use crate::Expr;

use super::context::{LowerScope, LowerSink, Lowered};

// ─── Trait definition ───────────────────────────────────────────────

/// Extension trait for lowering AST expressions to HIR expressions.
///
/// Implemented on `brink_syntax::ast` expression types. Each impl is
/// self-contained: it lowers its own properties and recursively lowers
/// child expressions via the same trait.
pub trait LowerExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr>;
}

// ─── Dispatch hub ───────────────────────────────────────────────────

impl LowerExpr for ast::Expr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        match self {
            ast::Expr::IntegerLit(lit) => lit.lower_expr(scope, sink),
            ast::Expr::FloatLit(lit) => lit.lower_expr(scope, sink),
            ast::Expr::BooleanLit(lit) => lit.lower_expr(scope, sink),
            ast::Expr::StringLit(lit) => lit.lower_expr(scope, sink),
            ast::Expr::Path(path) => path.lower_expr(scope, sink),
            ast::Expr::Prefix(pe) => pe.lower_expr(scope, sink),
            ast::Expr::Infix(ie) => ie.lower_expr(scope, sink),
            ast::Expr::Postfix(pe) => pe.lower_expr(scope, sink),
            ast::Expr::Paren(pe) => pe.lower_expr(scope, sink),
            ast::Expr::FunctionCall(fc) => fc.lower_expr(scope, sink),
            ast::Expr::DivertTarget(dt) => dt.lower_expr(scope, sink),
            ast::Expr::ListExpr(le) => le.lower_expr(scope, sink),
        }
    }
}
