use crate::hir;
use crate::symbols::SymbolKind;

use super::content::lower_content;
use super::context::LowerCtx;
use super::expr::{lower_expr, path_to_string};
use super::lir;
use super::plan::ContainerPlan;

/// Lower a single HIR statement to a LIR statement.
///
/// `ChoiceSet` is handled by the caller (`lower_block_with_children`)
/// since it produces child containers. This function handles all other
/// statement types.
#[expect(clippy::too_many_lines)]
pub(super) fn lower_stmt(
    stmt: &hir::Stmt,
    ctx: &mut LowerCtx<'_>,
    plan: &ContainerPlan,
    _choice_counter: &mut usize,
    _gather_counter: &mut usize,
) -> Option<lir::Stmt> {
    match stmt {
        hir::Stmt::Content(content) => Some(lir::Stmt::EmitContent(lower_content(content, ctx))),

        hir::Stmt::Divert(divert) => {
            Some(lir::Stmt::Divert(lower_divert_target(&divert.target, ctx)))
        }

        hir::Stmt::TunnelCall(tunnel) => {
            let targets = tunnel
                .targets
                .iter()
                .map(|t| {
                    let d = lower_divert_target(t, ctx);
                    lir::TunnelTarget {
                        target: d.target,
                        args: d.args,
                    }
                })
                .collect();
            Some(lir::Stmt::TunnelCall(lir::TunnelCall { targets }))
        }

        hir::Stmt::ThreadStart(thread) => {
            let d = lower_divert_target(&thread.target, ctx);
            Some(lir::Stmt::ThreadStart(lir::ThreadStart {
                target: d.target,
                args: d.args,
            }))
        }

        hir::Stmt::TempDecl(decl) => {
            let slot = ctx.temp_slot(&decl.name.text)?;
            let name = ctx.names.intern(&decl.name.text);
            let value = decl.value.as_ref().map(|e| lower_expr(e, ctx));
            Some(lir::Stmt::DeclareTemp { slot, name, value })
        }

        hir::Stmt::Assignment(assign) => {
            let target = lower_assign_target(&assign.target, ctx)?;
            let value = lower_expr(&assign.value, ctx);
            Some(lir::Stmt::Assign {
                target,
                op: assign.op,
                value,
            })
        }

        hir::Stmt::Return(ret) => {
            let value = ret.value.as_ref().map(|e| lower_expr(e, ctx));
            // `->->` (tunnel return) has ptr: None in the HIR;
            // `~ return expr` has ptr: Some(...).
            let is_tunnel = ret.ptr.is_none();
            Some(lir::Stmt::Return { value, is_tunnel })
        }

        hir::Stmt::ExprStmt(expr) => {
            // Convert x++ / x-- into Assign { target: x, op: Add/Sub, value: 1 }
            if let hir::Expr::Postfix(inner, op) = expr
                && let Some(target) = lower_assign_target(inner, ctx)
            {
                let assign_op = match op {
                    crate::PostfixOp::Increment => crate::AssignOp::Add,
                    crate::PostfixOp::Decrement => crate::AssignOp::Sub,
                };
                return Some(lir::Stmt::Assign {
                    target,
                    op: assign_op,
                    value: lir::Expr::Int(1),
                });
            }
            Some(lir::Stmt::ExprStmt(lower_expr(expr, ctx)))
        }

        hir::Stmt::ChoiceSet(_) => {
            // ChoiceSet is handled by lower_block_with_children in mod.rs
            None
        }

        hir::Stmt::Conditional(cond) => {
            let branches = cond
                .branches
                .iter()
                .map(|b| {
                    let condition = b.condition.as_ref().map(|e| lower_expr(e, ctx));
                    let mut bc = 0;
                    let mut bg = 0;
                    let body = lower_block_stmts_only(&b.body, ctx, plan, &mut bc, &mut bg);
                    lir::CondBranch { condition, body }
                })
                .collect();
            let kind = match &cond.kind {
                hir::CondKind::InitialCondition => lir::CondKind::InitialCondition,
                hir::CondKind::IfElse => lir::CondKind::IfElse,
                hir::CondKind::Switch(expr) => lir::CondKind::Switch(lower_expr(expr, ctx)),
            };
            Some(lir::Stmt::Conditional(lir::Conditional { kind, branches }))
        }

        hir::Stmt::Sequence(seq) => {
            let branches = seq
                .branches
                .iter()
                .map(|b| {
                    let mut bc = 0;
                    let mut bg = 0;
                    lower_block_stmts_only(b, ctx, plan, &mut bc, &mut bg)
                })
                .collect();
            Some(lir::Stmt::Sequence(lir::Sequence {
                kind: seq.kind,
                branches,
            }))
        }

        hir::Stmt::EndOfLine => Some(lir::Stmt::EndOfLine),
    }
}

/// Lower a block returning only statements (no child containers).
/// Used for conditional/sequence branches where children aren't expected.
fn lower_block_stmts_only(
    block: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    plan: &ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> Vec<lir::Stmt> {
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        if let Some(s) = lower_stmt(stmt, ctx, plan, choice_counter, gather_counter) {
            stmts.push(s);
        }
    }
    stmts
}

fn lower_divert_target(target: &hir::DivertTarget, ctx: &mut LowerCtx<'_>) -> lir::Divert {
    let args = target
        .args
        .iter()
        .map(|a| lir::CallArg::Value(lower_expr(a, ctx)))
        .collect();

    let lir_target = match &target.path {
        hir::DivertPath::Done => lir::DivertTarget::Done,
        hir::DivertPath::End => lir::DivertTarget::End,
        hir::DivertPath::Path(path) => {
            if let Some(info) = ctx.resolve_path(path.range) {
                match info.kind {
                    SymbolKind::Variable | SymbolKind::Constant => {
                        lir::DivertTarget::Variable(info.id)
                    }
                    _ => lir::DivertTarget::Container(info.id),
                }
            } else {
                lir::DivertTarget::Done
            }
        }
    };

    lir::Divert {
        target: lir_target,
        args,
    }
}

fn lower_assign_target(expr: &hir::Expr, ctx: &mut LowerCtx<'_>) -> Option<lir::AssignTarget> {
    match expr {
        hir::Expr::Path(path) => {
            let name = path_to_string(path);
            if let Some(slot) = ctx.temp_slot(&name) {
                let name_id = ctx.names.intern(&name);
                return Some(lir::AssignTarget::Temp(slot, name_id));
            }
            if let Some(id) = ctx.resolve_id(path.range) {
                return Some(lir::AssignTarget::Global(id));
            }
            None
        }
        _ => None,
    }
}
