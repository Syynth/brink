//! T1b `~ { … }` block/loop/indexed-assignment lowering
//! (`docs/t1b-surface-spec.md` §2, §4).
//!
//! Block bodies are pure logic — no weave concepts (content, choices,
//! diverts, gathers, threads) ever appear in `hir::BlockStmt`, enforced by
//! construction in the HIR shape (§2's seam rule) — so every construct here
//! lowers to a **flat** `lir::Stmt` sequence within the enclosing
//! container's own body. No child containers are ever created:
//!
//! - `if`/`else if`/`else` reuse `lir::Conditional` (`CondKind::IfElse`) —
//!   the exact same shape a weave multiline conditional's branches use,
//!   minus the container wrapping weave conditionals need for choice
//!   isolation (block bodies never contain choices).
//! - `while` lowers to `lir::LogicWhile`, compiled by the bytecode backend
//!   to a flat backward-jump loop in the same container.
//! - `for x in arr` / `for k in map` desugars entirely to a `LogicWhile`
//!   (index-based iteration over `CollectionKeys(iterable)` — see that
//!   opcode's doc for why one opcode covers both cases) with the index
//!   increment in `LogicWhile::post` so `continue` still advances the loop.
//! - Indexed assignment (`a[i] = v`, chained `grid[y][x] = v`) desugars to
//!   the ratified RMW discipline exactly: take → `make_mut` → write-back on
//!   the root cell, chains as nested RMW via synthetic temps — never
//!   interior references. Every sub-expression (root, each index, the
//!   value) is evaluated exactly once, in source order.
//!
//! Block-scoped `temp` declarations (including `for` loop variables and the
//! synthetic temps this module allocates for desugaring) get fresh slots via
//! [`LowerCtx::alloc_block_slot`] and are visible only within their
//! `push_block_scope`/`pop_block_scope` bracket — shadowing an
//! already-visible temp (an outer classic `~ temp` or an enclosing block's
//! temp) is legal but produces an E054 warning (§2).

use crate::hir;
use crate::{AssignOp, Diagnostic, DiagnosticCode, InfixOp};

use super::context::LowerCtx;
use super::expr::lower_expr;
use super::lir;

/// Lower a `~ { … }` block's statements into the enclosing body's flat
/// statement sequence. Opens a new T1b lexical scope for the block's own
/// `temp` declarations (and any nested scopes it opens), popped on return.
pub(super) fn lower_logic_block(
    stmts: &[hir::BlockStmt],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::Stmt> {
    ctx.push_block_scope();
    let out = lower_block_stmt_list(stmts, ctx);
    ctx.pop_block_scope();
    out
}

fn lower_block_stmt_list(stmts: &[hir::BlockStmt], ctx: &mut LowerCtx<'_>) -> Vec<lir::Stmt> {
    let mut out = Vec::new();
    for stmt in stmts {
        lower_block_stmt(stmt, ctx, &mut out);
    }
    out
}

fn lower_block_stmt(stmt: &hir::BlockStmt, ctx: &mut LowerCtx<'_>, out: &mut Vec<lir::Stmt>) {
    match stmt {
        hir::BlockStmt::TempDecl(decl) => lower_block_temp_decl(decl, ctx, out),
        hir::BlockStmt::Assignment(assign) => lower_block_assignment(assign, ctx, out),
        hir::BlockStmt::Return(ret) => {
            let value = ret.value.as_ref().map(|e| lower_expr(e, ctx));
            out.push(lir::Stmt::Return {
                value,
                is_tunnel: false,
                args: Vec::new(),
            });
        }
        hir::BlockStmt::If(if_stmt) => {
            let mut branches = Vec::new();
            lower_if_branch(if_stmt, ctx, &mut branches);
            out.push(lir::Stmt::Conditional(lir::Conditional {
                kind: lir::CondKind::IfElse,
                branches,
            }));
        }
        hir::BlockStmt::While(w) => {
            let condition = lower_expr(&w.condition, ctx);
            ctx.push_block_scope();
            let body = lower_block_stmt_list(&w.body, ctx);
            ctx.pop_block_scope();
            out.push(lir::Stmt::LogicWhile(lir::LogicWhile {
                condition,
                body,
                post: Vec::new(),
            }));
        }
        hir::BlockStmt::For(f) => lower_for_stmt(f, ctx, out),
        hir::BlockStmt::Break(_) => out.push(lir::Stmt::LogicBreak),
        hir::BlockStmt::Continue(_) => out.push(lir::Stmt::LogicContinue),
        hir::BlockStmt::ExprStmt(expr) => {
            out.push(lir::Stmt::ExprStmt(lower_expr(expr, ctx)));
        }
    }
}

fn lower_if_branch(
    if_stmt: &hir::IfStmt,
    ctx: &mut LowerCtx<'_>,
    branches: &mut Vec<lir::CondBranch>,
) {
    let condition = Some(lower_expr(&if_stmt.condition, ctx));
    ctx.push_block_scope();
    let body = lower_block_stmt_list(&if_stmt.body, ctx);
    ctx.pop_block_scope();
    branches.push(lir::CondBranch { condition, body });

    match &if_stmt.else_branch {
        Some(hir::ElseBranch::ElseIf(inner)) => lower_if_branch(inner, ctx, branches),
        Some(hir::ElseBranch::Else(else_body)) => {
            ctx.push_block_scope();
            let body = lower_block_stmt_list(else_body, ctx);
            ctx.pop_block_scope();
            branches.push(lir::CondBranch {
                condition: None,
                body,
            });
        }
        None => {}
    }
}

/// Declare a block-scoped `temp`, emitting the E054 shadow warning if `name`
/// is already visible (an outer classic temp/param or an enclosing block
/// scope). Returns the allocated slot and interned name.
fn declare_shadow_checked(
    name: &str,
    range: rowan::TextRange,
    ctx: &mut LowerCtx<'_>,
) -> (u16, brink_format::NameId) {
    if ctx.is_name_visible(name) {
        ctx.diagnostics.push(Diagnostic {
            file: ctx.file,
            range,
            message: format!(
                "`{name}` shadows an already-visible temp — block-scoped `temp` \
                 declarations may shadow outer temps (docs/t1b-surface-spec.md §2), \
                 but double-check this is intentional"
            ),
            code: DiagnosticCode::E054,
        });
    }
    let slot = ctx.alloc_block_slot();
    let name_id = ctx.names.intern(name);
    ctx.declare_block_local(name.to_string(), slot);
    (slot, name_id)
}

fn lower_block_temp_decl(decl: &hir::TempDecl, ctx: &mut LowerCtx<'_>, out: &mut Vec<lir::Stmt>) {
    // Evaluate the initializer BEFORE the new name becomes visible — matches
    // classic (non-block) `TempDecl` lowering, so `temp x = x` reads the
    // outer `x`, not itself.
    let value = decl.value.as_ref().map(|e| lower_expr(e, ctx));
    let (slot, name) = declare_shadow_checked(&decl.name.text, decl.name.range, ctx);
    out.push(lir::Stmt::DeclareTemp { slot, name, value });
}

fn lower_block_assignment(
    assign: &hir::Assignment,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    if let hir::Expr::Index(idx) = &assign.target {
        lower_indexed_assignment(idx, assign.op, &assign.value, ctx, out);
        return;
    }
    // Plain variable target — `LowerCtx::temp_slot` (used internally by
    // `lower_assign_target`) already checks open T1b block scopes first, so
    // this correctly resolves a block-scoped-shadowed name to its own slot.
    if let Some(target) = super::stmts::lower_assign_target(&assign.target, ctx) {
        let value = lower_expr(&assign.value, ctx);
        out.push(lir::Stmt::Assign {
            target,
            op: assign.op,
            value,
        });
    }
    // An unresolvable target (e.g. a genuinely undeclared name) silently
    // drops the statement — the same behavior classic `~ x = …` assignment
    // lowering already has (`lower_stmt`'s `Assignment` arm); the analyzer's
    // E025 unresolved-variable diagnostic is what surfaces this to authors,
    // not LIR lowering.
}

/// Unwind a (possibly chained) `IndexExpr` into its root expression and the
/// index expressions in left-to-right (written) order — e.g. `grid[y][x]`
/// unwinds to `(Path(grid), [y, x])`.
fn flatten_index_chain(idx: &hir::IndexExpr) -> (&hir::Expr, Vec<&hir::Expr>) {
    let mut indices_outer_first: Vec<&hir::Expr> = vec![&idx.index];
    let mut cur_base = idx.base.as_ref();
    while let hir::Expr::Index(inner) = cur_base {
        indices_outer_first.push(&inner.index);
        cur_base = inner.base.as_ref();
    }
    indices_outer_first.reverse();
    (cur_base, indices_outer_first)
}

fn get_expr_for_target(target: &lir::AssignTarget) -> lir::Expr {
    match target {
        lir::AssignTarget::Global(id) => lir::Expr::GetGlobal(*id),
        lir::AssignTarget::Temp(slot, name) => lir::Expr::GetTemp(*slot, *name),
    }
}

fn declare_synthetic(
    prefix: &str,
    value: lir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) -> (u16, brink_format::NameId) {
    let slot = ctx.alloc_block_slot();
    let name = ctx.names.intern(prefix);
    out.push(lir::Stmt::DeclareTemp {
        slot,
        name,
        value: Some(value),
    });
    (slot, name)
}

/// Lower `base[i0][i1]...[iN-1] OP= value` (§4). Follows the ratified RMW
/// discipline exactly: take → `make_mut` → write-back on the root cell;
/// chains lower to nested RMW via synthetic temps — never interior
/// references (no projections in T1b). Every sub-expression is evaluated
/// exactly once: the root once, each index left-to-right once, the value
/// once.
fn lower_indexed_assignment(
    idx: &hir::IndexExpr,
    op: AssignOp,
    value_expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    let (root_expr, indices_hir) = flatten_index_chain(idx);
    let Some(root_target) = super::stmts::lower_assign_target(root_expr, ctx) else {
        // Unresolvable root — same silent-skip discipline as plain
        // assignment (analyzer's E025 is the author-facing signal).
        return;
    };
    let n = indices_hir.len();
    if n == 0 {
        // Structurally unreachable (an `IndexExpr` always has an `index`),
        // guarded rather than asserted so a future grammar change can't
        // corrupt output instead of just doing nothing.
        return;
    }

    // 1. Materialize every index value into its own temp, evaluated once,
    //    left to right, before any container read.
    let idx_slots: Vec<(u16, brink_format::NameId)> = indices_hir
        .iter()
        .map(|e| {
            let v = lower_expr(e, ctx);
            declare_synthetic("__idx", v, ctx, out)
        })
        .collect();

    // 2. Read the root container once, then walk down the chain reading
    //    each level once: c[0] = root; c[k+1] = c[k][idx[k]].
    let mut c_slots: Vec<(u16, brink_format::NameId)> = vec![declare_synthetic(
        "__c",
        get_expr_for_target(&root_target),
        ctx,
        out,
    )];
    for k in 0..n - 1 {
        let base = lir::Expr::GetTemp(c_slots[k].0, c_slots[k].1);
        let index = lir::Expr::GetTemp(idx_slots[k].0, idx_slots[k].1);
        let read = lir::Expr::Index {
            base: Box::new(base),
            index: Box::new(index),
        };
        c_slots.push(declare_synthetic("__c", read, ctx, out));
    }

    // 3. Evaluate the RHS once.
    let mut rhs = lower_expr(value_expr, ctx);

    // 3b. Compound assignment (`+=`/`-=`): rhs = current OP rhs. `current`
    //     re-reads the target path via the already-materialized temps (no
    //     re-evaluation of the root/index expressions).
    if op != AssignOp::Set {
        let last_base = lir::Expr::GetTemp(c_slots[n - 1].0, c_slots[n - 1].1);
        let last_index = lir::Expr::GetTemp(idx_slots[n - 1].0, idx_slots[n - 1].1);
        let current = lir::Expr::Index {
            base: Box::new(last_base),
            index: Box::new(last_index),
        };
        // `op == AssignOp::Set` is excluded by the guard above, so only
        // `Add`/`Sub` ever reach here.
        let infix_op = if op == AssignOp::Sub {
            InfixOp::Sub
        } else {
            InfixOp::Add
        };
        rhs = lir::Expr::Infix(Box::new(current), infix_op, Box::new(rhs));
    }

    // 4. Mutate the deepest level in place: c[N-1] = IndexSet(c[N-1],
    //    idx[N-1], rhs). Turn-terminating fault on OOB/missing-key (§6).
    {
        let (slot, name) = c_slots[n - 1];
        let base = lir::Expr::GetTemp(slot, name);
        let index = lir::Expr::GetTemp(idx_slots[n - 1].0, idx_slots[n - 1].1);
        out.push(lir::Stmt::Assign {
            target: lir::AssignTarget::Temp(slot, name),
            op: AssignOp::Set,
            value: lir::Expr::IndexSet {
                base: Box::new(base),
                index: Box::new(index),
                value: Box::new(rhs),
            },
        });
    }

    // 5. Cascade the write-back upward: c[k] = IndexSet(c[k], idx[k],
    //    c[k+1]) for k = N-2 down to 0.
    for k in (0..n - 1).rev() {
        let (slot, name) = c_slots[k];
        let base = lir::Expr::GetTemp(slot, name);
        let index = lir::Expr::GetTemp(idx_slots[k].0, idx_slots[k].1);
        let inner = lir::Expr::GetTemp(c_slots[k + 1].0, c_slots[k + 1].1);
        out.push(lir::Stmt::Assign {
            target: lir::AssignTarget::Temp(slot, name),
            op: AssignOp::Set,
            value: lir::Expr::IndexSet {
                base: Box::new(base),
                index: Box::new(index),
                value: Box::new(inner),
            },
        });
    }

    // 6. Write the final root container back into the root variable.
    let (root_slot, root_name) = c_slots[0];
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: lir::Expr::GetTemp(root_slot, root_name),
    });
}

/// Lower `for x in arr { … }` / `for k in map { … }` (§2). Desugars to an
/// index-based `LogicWhile` — dedicated iterator opcodes are deliberately
/// not part of the T1b surface (`docs/format-v4-rfc.md` §3 note); the
/// iterable is snapshotted once via `CollectionKeys` (which returns the
/// array unchanged for an array input — see that opcode's doc — so one
/// snapshot expression correctly covers both "iterate values" and "iterate
/// keys" without a static array/map type distinction).
fn lower_for_stmt(f: &hir::ForStmt, ctx: &mut LowerCtx<'_>, out: &mut Vec<lir::Stmt>) {
    let iterable = lower_expr(&f.iterable, ctx);
    let (snap_slot, snap_name) = declare_synthetic(
        "__for_snapshot",
        lir::Expr::CollectionKeys(Box::new(iterable)),
        ctx,
        out,
    );
    let (idx_slot, idx_name) = declare_synthetic("__for_idx", lir::Expr::Int(0), ctx, out);

    let condition = lir::Expr::Infix(
        Box::new(lir::Expr::GetTemp(idx_slot, idx_name)),
        InfixOp::Lt,
        Box::new(lir::Expr::CollectionLen(Box::new(lir::Expr::GetTemp(
            snap_slot, snap_name,
        )))),
    );

    ctx.push_block_scope();
    let (var_slot, var_name) = declare_shadow_checked(&f.var_name.text, f.var_name.range, ctx);
    let mut body = vec![lir::Stmt::DeclareTemp {
        slot: var_slot,
        name: var_name,
        value: Some(lir::Expr::Index {
            base: Box::new(lir::Expr::GetTemp(snap_slot, snap_name)),
            index: Box::new(lir::Expr::GetTemp(idx_slot, idx_name)),
        }),
    }];
    body.extend(lower_block_stmt_list(&f.body, ctx));
    ctx.pop_block_scope();

    let post = vec![lir::Stmt::Assign {
        target: lir::AssignTarget::Temp(idx_slot, idx_name),
        op: AssignOp::Add,
        value: lir::Expr::Int(1),
    }];

    out.push(lir::Stmt::LogicWhile(lir::LogicWhile {
        condition,
        body,
        post,
    }));
}
