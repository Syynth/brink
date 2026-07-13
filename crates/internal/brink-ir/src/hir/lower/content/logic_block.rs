//! T1b `~ { … }` block-statement lowering (docs/t1b-surface-spec.md §2).
//!
//! Structural AST→HIR lowering only, shared by both dialects — see
//! `hir/lower/expr/sigils.rs`'s module doc for the same rationale. A
//! malformed individual statement is skipped (its own sub-lowerer already
//! emitted a diagnostic) rather than aborting the whole block, mirroring how
//! weave-block lowering treats individual statements.

use brink_syntax::ast::{self, AstNode, SyntaxNodePtr};

use crate::hir::types::{BlockStmt, ElseBranch, ForStmt, IfStmt, WhileStmt};
use crate::symbols::LocalSymbol;
use crate::{AssignOp, Assignment, DiagnosticCode, Return, SymbolKind, TempDecl};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::expr::LowerExpr;
use super::super::helpers::name_from_ident;
use super::super::types::lower_type_annotation;

/// Lower a `~ { … }` block body's statements.
pub(crate) fn lower_stmt_block(
    block: &ast::StmtBlock,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Vec<BlockStmt> {
    block
        .stmts()
        .filter_map(|stmt| lower_block_stmt(&stmt, scope, sink).ok())
        .collect()
}

fn lower_block_stmt(
    stmt: &ast::BlockStmt,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<BlockStmt> {
    match stmt {
        ast::BlockStmt::TempDecl(temp) => lower_block_temp_decl(temp, scope, sink),
        ast::BlockStmt::Assignment(assign) => lower_block_assignment(assign, scope, sink),
        ast::BlockStmt::Return(ret) => Ok(lower_block_return(ret, scope, sink)),
        ast::BlockStmt::If(if_stmt) => lower_if_stmt(if_stmt, scope, sink).map(BlockStmt::If),
        ast::BlockStmt::While(w) => lower_while_stmt(w, scope, sink),
        ast::BlockStmt::For(f) => lower_for_stmt(f, scope, sink),
        ast::BlockStmt::Break(b) => Ok(BlockStmt::Break(SyntaxNodePtr::from_node(b.syntax()))),
        ast::BlockStmt::Continue(c) => {
            Ok(BlockStmt::Continue(SyntaxNodePtr::from_node(c.syntax())))
        }
        ast::BlockStmt::ExprStmt(stmt) => lower_block_expr_stmt(stmt, scope, sink),
    }
}

fn lower_block_temp_decl(
    temp: &ast::TempDecl,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<BlockStmt> {
    let range = temp.syntax().text_range();
    let ident = temp
        .identifier()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
    let name = name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
    let value = temp.value().and_then(|e| e.lower_expr(scope, sink).ok());
    let annotation = temp
        .type_annotation()
        .and_then(|ta| lower_type_annotation(&ta));
    sink.add_local(LocalSymbol {
        name: name.text.clone(),
        range: name.range,
        scope: scope.to_scope(),
        kind: SymbolKind::Temp,
        param_detail: None,
    });
    Ok(BlockStmt::TempDecl(TempDecl {
        ptr: ast::AstPtr::new(temp),
        name,
        value,
        annotation,
    }))
}

fn lower_block_assignment(
    assign: &ast::Assignment,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<BlockStmt> {
    let range = assign.syntax().text_range();
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
    Ok(BlockStmt::Assignment(Assignment {
        ptr: ast::AstPtr::new(assign),
        target,
        op,
        value,
    }))
}

fn lower_block_return(
    ret: &ast::ReturnStmt,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> BlockStmt {
    let value = ret.value().and_then(|e| e.lower_expr(scope, sink).ok());
    BlockStmt::Return(Return {
        ptr: Some(ast::AstPtr::new(ret)),
        value,
        onwards_args: Vec::new(),
    })
}

fn lower_while_stmt(
    w: &ast::WhileStmt,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<BlockStmt> {
    let range = w.syntax().text_range();
    let condition = w
        .condition()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
        .and_then(|e| e.lower_expr(scope, sink))?;
    let body = w
        .body()
        .map(|b| lower_stmt_block(&b, scope, sink))
        .unwrap_or_default();
    Ok(BlockStmt::While(WhileStmt {
        ptr: SyntaxNodePtr::from_node(w.syntax()),
        condition,
        body,
    }))
}

fn lower_for_stmt(
    f: &ast::ForStmt,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<BlockStmt> {
    let range = f.syntax().text_range();
    let ident = f
        .identifier()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
    let var_name =
        name_from_ident(&ident).ok_or_else(|| sink.diagnose(range, DiagnosticCode::E014))?;
    let iterable = f
        .iterable()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
        .and_then(|e| e.lower_expr(scope, sink))?;
    sink.add_local(LocalSymbol {
        name: var_name.text.clone(),
        range: var_name.range,
        scope: scope.to_scope(),
        kind: SymbolKind::Temp,
        param_detail: None,
    });
    let body = f
        .body()
        .map(|b| lower_stmt_block(&b, scope, sink))
        .unwrap_or_default();
    Ok(BlockStmt::For(ForStmt {
        ptr: SyntaxNodePtr::from_node(f.syntax()),
        var_name,
        iterable,
        body,
    }))
}

fn lower_block_expr_stmt(
    stmt: &ast::ExprStmt,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<BlockStmt> {
    let range = stmt.syntax().text_range();
    let expr = stmt
        .expr()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
        .and_then(|e| e.lower_expr(scope, sink))?;
    Ok(BlockStmt::ExprStmt(expr))
}

fn lower_if_stmt(
    if_stmt: &ast::IfStmt,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<IfStmt> {
    let range = if_stmt.syntax().text_range();
    let condition = if_stmt
        .condition()
        .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E015))
        .and_then(|e| e.lower_expr(scope, sink))?;
    let body = if_stmt
        .body()
        .map(|b| lower_stmt_block(&b, scope, sink))
        .unwrap_or_default();
    let else_branch = if_stmt
        .else_clause()
        .map(|clause| lower_else_clause(&clause, scope, sink))
        .transpose()?;
    Ok(IfStmt {
        ptr: SyntaxNodePtr::from_node(if_stmt.syntax()),
        condition,
        body,
        else_branch,
    })
}

fn lower_else_clause(
    clause: &ast::ElseClause,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Lowered<ElseBranch> {
    if let Some(nested_if) = clause.if_stmt() {
        let inner = lower_if_stmt(&nested_if, scope, sink)?;
        Ok(ElseBranch::ElseIf(Box::new(inner)))
    } else {
        let body = clause
            .body()
            .map(|b| lower_stmt_block(&b, scope, sink))
            .unwrap_or_default();
        Ok(ElseBranch::Else(body))
    }
}
