use crate::hir;
use crate::symbols::SymbolKind;

use super::content::lower_content;
use super::context::LowerCtx;
use super::expr::{lower_expr, path_to_string};
use super::lir;

/// Lower a single HIR statement to a LIR statement.
///
/// `ChoiceSet`, `LabeledBlock`, `Conditional`, and `Sequence` are handled
/// by the caller (`lower_block_with_children`) since they may produce child
/// containers. This function handles all remaining statement types.
pub(super) fn lower_stmt(stmt: &hir::Stmt, ctx: &mut LowerCtx<'_>) -> Option<lir::Stmt> {
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

        // ChoiceSet, LabeledBlock, Conditional, and Sequence are dispatched
        // by lower_block_with_children before reaching lower_stmt. If they
        // reach here, it indicates a dispatch bug.
        hir::Stmt::ChoiceSet(_)
        | hir::Stmt::LabeledBlock(_)
        | hir::Stmt::Conditional(_)
        | hir::Stmt::Sequence(_) => {
            debug_assert!(
                false,
                "ChoiceSet/LabeledBlock/Conditional/Sequence should not reach lower_stmt"
            );
            None
        }

        hir::Stmt::EndOfLine => Some(lir::Stmt::EndOfLine),
    }
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
