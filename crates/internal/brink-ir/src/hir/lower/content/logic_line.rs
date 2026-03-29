use brink_syntax::ast::{self, AstNode};

use crate::{AssignOp, Assignment, DiagnosticCode, Expr, Return, Stmt, TempDecl};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::super::helpers::{expr_contains_call, name_from_ident};
use super::LowerBody;

/// Structured output from lowering a [`ast::LogicLine`].
pub enum LogicLineOutput {
    Return(Return),
    TempDecl(TempDecl),
    Assignment(Assignment),
    ExprStmt(Expr),
}

impl LogicLineOutput {
    /// Whether this logic line contains a function call, which requires
    /// an `EndOfLine` after it to match inklecate's behavior.
    pub fn has_call(&self) -> bool {
        match self {
            Self::ExprStmt(expr) => expr_contains_call(expr),
            Self::TempDecl(td) => td.value.as_ref().is_some_and(expr_contains_call),
            Self::Assignment(a) => expr_contains_call(&a.value),
            Self::Return(_) => false,
        }
    }

    /// Convert into a [`Stmt`].
    pub fn into_stmt(self) -> Stmt {
        match self {
            Self::Return(r) => Stmt::Return(r),
            Self::TempDecl(td) => Stmt::TempDecl(td),
            Self::Assignment(a) => Stmt::Assignment(a),
            Self::ExprStmt(e) => Stmt::ExprStmt(e),
        }
    }
}

impl LowerBody for ast::LogicLine {
    type Output = LogicLineOutput;

    fn lower_body(
        &self,
        scope: &LowerScope,
        sink: &mut impl LowerSink,
    ) -> Lowered<LogicLineOutput> {
        let range = self.syntax().text_range();

        if let Some(ret) = self.return_stmt() {
            let value = ret.value().and_then(|e| e.lower_expr(scope, sink).ok());
            return Ok(LogicLineOutput::Return(Return {
                ptr: Some(ast::AstPtr::new(&ret)),
                value,
                onwards_args: Vec::new(),
            }));
        }

        if let Some(temp) = self.temp_decl() {
            let ident = temp
                .identifier()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
            let name = name_from_ident(&ident)
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
            let value = temp.value().and_then(|e| e.lower_expr(scope, sink).ok());
            sink.add_local(crate::symbols::LocalSymbol {
                name: name.text.clone(),
                range: name.range,
                scope: scope.to_scope(),
                kind: crate::SymbolKind::Temp,
                param_detail: None,
            });
            return Ok(LogicLineOutput::TempDecl(TempDecl {
                ptr: ast::AstPtr::new(&temp),
                name,
                value,
            }));
        }

        if let Some(assign) = self.assignment() {
            let target = assign
                .target()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))
                .and_then(|e| e.lower_expr(scope, sink))?;
            let value = assign
                .value()
                .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))
                .and_then(|e| e.lower_expr(scope, sink))?;
            let op = assign
                .op_token()
                .map_or(AssignOp::Set, |tok| match tok.kind() {
                    brink_syntax::SyntaxKind::PLUS_EQ => AssignOp::Add,
                    brink_syntax::SyntaxKind::MINUS_EQ => AssignOp::Sub,
                    _ => AssignOp::Set,
                });
            return Ok(LogicLineOutput::Assignment(Assignment {
                ptr: ast::AstPtr::new(&assign),
                target,
                op,
                value,
            }));
        }

        for child in self.syntax().children() {
            if let Some(expr) = ast::Expr::cast(child) {
                let e = expr.lower_expr(scope, sink)?;
                return Ok(LogicLineOutput::ExprStmt(e));
            }
        }

        Err(sink.diagnose(range, DiagnosticCode::E014))
    }
}
