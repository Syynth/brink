//! Expression lowering phase.
//!
//! The [`LowerExpr`] trait is implemented on AST expression nodes. Each node
//! knows how to lower itself to an HIR [`Expr`], emitting diagnostics via
//! the [`LowerSink`] and registering unresolved references.

use brink_syntax::ast::{self, AstNode};

use crate::{DiagnosticCode, Expr, FloatBits, Path, RefKind};

use super::context::{LowerScope, LowerSink, Lowered};
use super::helpers::{
    lower_infix_op, lower_path, lower_postfix_op, lower_prefix_op, lower_string_lit, make_name,
    path_full_name,
};

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

// ─── Path / variable reference ──────────────────────────────────────

impl LowerExpr for ast::Path {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let p = lower_path(self);
        let full = path_full_name(&p);
        sink.add_unresolved(
            &full,
            self.syntax().text_range(),
            RefKind::Variable,
            &scope.to_scope(),
            None,
        );
        Ok(Expr::Path(p))
    }
}

// ─── Operators ──────────────────────────────────────────────────────

impl LowerExpr for ast::PrefixExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let op = lower_prefix_op(self).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E016))?;
        let operand = self
            .operand()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        Ok(Expr::Prefix(op, Box::new(operand)))
    }
}

impl LowerExpr for ast::InfixExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let lhs = self
            .lhs()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let op = lower_infix_op(self).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E016))?;
        let rhs = self
            .rhs()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        Ok(Expr::Infix(Box::new(lhs), op, Box::new(rhs)))
    }
}

impl LowerExpr for ast::PostfixExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let operand = self
            .operand()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))?;
        let op =
            lower_postfix_op(self).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E016))?;
        Ok(Expr::Postfix(Box::new(operand), op))
    }
}

impl LowerExpr for ast::ParenExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        self.inner()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
            .and_then(|e| e.lower_expr(scope, sink))
    }
}

// ─── Function calls ─────────────────────────────────────────────────

impl LowerExpr for ast::FunctionCall {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let name_text = ident
            .name()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let ident_range = ident.syntax().text_range();
        let path = Path {
            segments: vec![make_name(name_text.clone(), ident_range)],
            range: ident_range,
        };
        let args: Vec<Expr> = self
            .arg_list()
            .map(|al| {
                al.args()
                    .filter_map(|a| a.lower_expr(scope, sink).ok())
                    .collect()
            })
            .unwrap_or_default();
        sink.add_unresolved(
            &name_text,
            ident_range,
            RefKind::Function,
            &scope.to_scope(),
            Some(args.len()),
        );
        Ok(Expr::Call(path, args))
    }
}

// ─── Divert targets and list literals ───────────────────────────────

impl LowerExpr for ast::DivertTargetExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let ast_path = self
            .target()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E018))?;
        let path = lower_path(&ast_path);
        let full = path_full_name(&path);
        sink.add_unresolved(
            &full,
            ast_path.syntax().text_range(),
            RefKind::Divert,
            &scope.to_scope(),
            None,
        );
        Ok(Expr::DivertTarget(path))
    }
}

impl LowerExpr for ast::ListExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let items: Vec<Path> = self.items().map(|p| lower_path(&p)).collect();
        for item in &items {
            let full = path_full_name(item);
            sink.add_unresolved(&full, item.range, RefKind::List, &scope.to_scope(), None);
        }
        Ok(Expr::ListLiteral(items))
    }
}
