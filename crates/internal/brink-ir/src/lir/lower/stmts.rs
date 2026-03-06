use brink_format::DefinitionId;

use crate::hir;
use crate::symbols::SymbolKind;

use super::content::lower_content;
use super::context::LowerCtx;
use super::expr::{lower_expr, path_to_string};
use super::lir;
use super::plan::{ChoiceKey, ContainerPlan};

/// Lower a HIR block to a sequence of LIR statements.
///
/// When a `ChoiceSet` with a gather is encountered, remaining statements in the
/// block are NOT lowered here — they belong in the gather container instead.
pub fn lower_block(
    block: &hir::Block,
    ctx: &mut LowerCtx<'_>,
    plan: &ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> Vec<lir::Stmt> {
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        let has_gather = matches!(stmt, hir::Stmt::ChoiceSet(cs) if cs.gather.is_some());
        if let Some(s) = lower_stmt(stmt, ctx, plan, choice_counter, gather_counter) {
            stmts.push(s);
        }
        // Statements after a ChoiceSet with a gather belong in the gather container
        if has_gather {
            break;
        }
    }
    stmts
}

/// Lower a suffix of a HIR block starting after `skip` statements.
///
/// Used by gather containers to lower the trailing statements that follow
/// the `ChoiceSet` in the parent block.
pub fn lower_block_from(
    block: &hir::Block,
    skip: usize,
    ctx: &mut LowerCtx<'_>,
    plan: &ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
) -> Vec<lir::Stmt> {
    let mut stmts = Vec::new();
    for stmt in block.stmts.iter().skip(skip) {
        let has_gather = matches!(stmt, hir::Stmt::ChoiceSet(cs) if cs.gather.is_some());
        if let Some(s) = lower_stmt(stmt, ctx, plan, choice_counter, gather_counter) {
            stmts.push(s);
        }
        if has_gather {
            break;
        }
    }
    stmts
}

fn lower_stmt(
    stmt: &hir::Stmt,
    ctx: &mut LowerCtx<'_>,
    plan: &ContainerPlan,
    choice_counter: &mut usize,
    gather_counter: &mut usize,
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
            let value = decl.value.as_ref().map(|e| lower_expr(e, ctx));
            Some(lir::Stmt::DeclareTemp { slot, value })
        }

        hir::Stmt::Assignment(assign) => {
            let target = lower_assign_target(&assign.target, ctx)?;
            // AssignOp is a shared type — pass through directly
            let value = lower_expr(&assign.value, ctx);
            Some(lir::Stmt::Assign {
                target,
                op: assign.op,
                value,
            })
        }

        hir::Stmt::Return(ret) => {
            let value = ret.value.as_ref().map(|e| lower_expr(e, ctx));
            Some(lir::Stmt::Return(value))
        }

        hir::Stmt::ExprStmt(expr) => Some(lir::Stmt::ExprStmt(lower_expr(expr, ctx))),

        hir::Stmt::ChoiceSet(cs) => {
            let gather_target = if cs.gather.is_some() {
                find_gather_target(ctx, plan, gather_counter)
            } else {
                None
            };

            let choices: Vec<lir::Choice> = cs
                .choices
                .iter()
                .map(|choice| lower_choice(choice, ctx, plan, choice_counter, gather_target))
                .collect();

            Some(lir::Stmt::ChoiceSet(lir::ChoiceSet {
                choices,
                gather_target,
            }))
        }

        hir::Stmt::Conditional(cond) => {
            let branches = cond
                .branches
                .iter()
                .map(|b| {
                    let condition = b.condition.as_ref().map(|e| lower_expr(e, ctx));
                    let mut bc = 0;
                    let mut bg = 0;
                    let body = lower_block(&b.body, ctx, plan, &mut bc, &mut bg);
                    lir::CondBranch { condition, body }
                })
                .collect();
            Some(lir::Stmt::Conditional(lir::Conditional { branches }))
        }

        hir::Stmt::Sequence(seq) => {
            let branches = seq
                .branches
                .iter()
                .map(|b| {
                    let mut bc = 0;
                    let mut bg = 0;
                    lower_block(b, ctx, plan, &mut bc, &mut bg)
                })
                .collect();
            Some(lir::Stmt::Sequence(lir::Sequence {
                kind: seq.kind,
                branches,
            }))
        }
    }
}

fn lower_choice(
    choice: &hir::Choice,
    ctx: &mut LowerCtx<'_>,
    plan: &ContainerPlan,
    choice_counter: &mut usize,
    _gather_target: Option<DefinitionId>,
) -> lir::Choice {
    let key = ChoiceKey {
        file: ctx.file,
        scope: ctx.scope_path.clone(),
        index: *choice_counter,
    };
    *choice_counter += 1;

    let target = plan
        .choice_targets
        .get(&key)
        .copied()
        .unwrap_or(plan.root_id);

    // Combine display content: start + bracket
    let display = combine_content(
        choice.start_content.as_ref(),
        choice.bracket_content.as_ref(),
        ctx,
    );

    // Combine output content: start + inner
    let output = combine_content(
        choice.start_content.as_ref(),
        choice.inner_content.as_ref(),
        ctx,
    );

    let condition = choice.condition.as_ref().map(|e| lower_expr(e, ctx));
    let tags = choice.tags.iter().map(|t| t.text.clone()).collect();

    lir::Choice {
        is_sticky: choice.is_sticky,
        is_fallback: choice.is_fallback,
        condition,
        display,
        output,
        target,
        tags,
    }
}

fn combine_content(
    a: Option<&hir::Content>,
    b: Option<&hir::Content>,
    ctx: &mut LowerCtx<'_>,
) -> Option<lir::Content> {
    match (a, b) {
        (None, None) => None,
        (Some(content), None) | (None, Some(content)) => Some(lower_content(content, ctx)),
        (Some(a_content), Some(b_content)) => {
            let mut parts = Vec::new();
            for p in &a_content.parts {
                parts.push(super::content::lower_content_part_pub(p, ctx));
            }
            for p in &b_content.parts {
                parts.push(super::content::lower_content_part_pub(p, ctx));
            }
            let mut tags: Vec<String> = a_content.tags.iter().map(|t| t.text.clone()).collect();
            tags.extend(b_content.tags.iter().map(|t| t.text.clone()));
            Some(lir::Content { parts, tags })
        }
    }
}

fn find_gather_target(
    ctx: &LowerCtx<'_>,
    plan: &ContainerPlan,
    gather_counter: &mut usize,
) -> Option<DefinitionId> {
    use super::plan::GatherKey;
    let key = GatherKey {
        file: ctx.file,
        scope: ctx.scope_path.clone(),
        index: *gather_counter,
    };
    *gather_counter += 1;
    plan.gather_targets.get(&key).copied()
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
                return Some(lir::AssignTarget::Temp(slot));
            }
            if let Some(id) = ctx.resolve_id(path.range) {
                return Some(lir::AssignTarget::Global(id));
            }
            None
        }
        _ => None,
    }
}
