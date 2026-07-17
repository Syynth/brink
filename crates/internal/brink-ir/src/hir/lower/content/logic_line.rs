use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::hir::types::{AwaitStmt, LogicBlock};
use crate::{AssignOp, Assignment, DiagnosticCode, Expr, Return, Stmt, TempDecl};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::super::helpers::{expr_contains_call, name_from_ident};
use super::super::types::lower_type_annotation;
use super::LowerBody;
use super::logic_block::{lower_await_stmt, lower_stmt_block};

/// Structured output from lowering a [`ast::LogicLine`].
pub enum LogicLineOutput {
    Return(Return),
    TempDecl(TempDecl),
    Assignment(Assignment),
    ExprStmt(Expr),
    /// `~ { … }` — a T1b multi-line logic block (docs/t1b-surface-spec.md
    /// §2, brink extension). Never lowers past HIR — see
    /// `brink_ir::hir::types::Stmt::LogicBlock`.
    Block(LogicBlock),
    /// `~ await <cond>` — a FlowFrame suspension point
    /// (docs/flow-suspension-spec.md §3, brink extension). Fenced at LIR
    /// lowering (E052) until FS-3.
    Await(AwaitStmt),
}

impl LogicLineOutput {
    /// Whether this logic line contains a function call, which requires
    /// an `EndOfLine` after it to match inklecate's behavior.
    pub fn has_call(&self) -> bool {
        match self {
            Self::ExprStmt(expr) => expr_contains_call(expr),
            Self::TempDecl(td) => td.value.as_ref().is_some_and(expr_contains_call),
            Self::Assignment(a) => expr_contains_call(&a.value),
            // An `await` condition may itself contain a call, but the await is
            // a suspension point, not a content-emitting logic line — it never
            // needs the trailing `EndOfLine` inklecate emits after a call on a
            // content line.
            Self::Return(_) | Self::Block(_) | Self::Await(_) => false,
        }
    }

    /// Convert into a [`Stmt`].
    pub fn into_stmt(self) -> Stmt {
        match self {
            Self::Return(r) => Stmt::Return(r),
            Self::TempDecl(td) => Stmt::TempDecl(td),
            Self::Assignment(a) => Stmt::Assignment(a),
            Self::ExprStmt(e) => Stmt::ExprStmt(e),
            Self::Block(lb) => Stmt::LogicBlock(lb),
            Self::Await(a) => Stmt::Await(a),
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

        if let Some(block) = self.stmt_block() {
            let stmts = lower_stmt_block(&block, scope, sink);
            return Ok(LogicLineOutput::Block(LogicBlock {
                ptr: SyntaxNodePtr::from_node(block.syntax()),
                stmts,
            }));
        }

        if let Some(await_stmt) = self.await_stmt() {
            return Ok(LogicLineOutput::Await(lower_await_stmt(
                &await_stmt,
                scope,
                sink,
            )));
        }

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
            let annotation = temp
                .type_annotation()
                .and_then(|ta| lower_type_annotation(&ta));
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
                annotation,
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
