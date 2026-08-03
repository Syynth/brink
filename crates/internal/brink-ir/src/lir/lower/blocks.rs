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
use crate::symbols::SymbolKind;
use crate::{AssignOp, Diagnostic, DiagnosticCode, InfixOp};

use super::context::LowerCtx;
use super::context::TypeMode;
use super::context::UfcsVerdict;
use super::expr::lower_expr;
use super::lir;

/// Lower a `~ { … }` block's statements into the enclosing body's flat
/// statement sequence, honoring [`hir::LogicBlock::scope`] — almost always
/// [`hir::LogicBlockScope::Standalone`] (push a new T1b lexical scope for
/// the block's own `temp` declarations on entry, pop it on return), except
/// when a code-ground body's `> text` prose-line escape has split it into
/// several sibling `LogicBlock`s (issue #1992 review finding F1,
/// `hir::lower_native::body::mark_split_logic_block_scopes`'s doc): those
/// siblings share one scope, so only the first (`Opens`) pushes; a
/// `Continues` sibling neither pushes nor pops. **Neither `Opens` nor
/// `Continues` pops here** — the matching pop is the enclosing block's
/// responsibility ([`super::lower_block_with_children`]), since a
/// `Stmt::Content` sibling produced by a trailing `> text` line can
/// legally come after the last split run and still needs the scope open.
pub(super) fn lower_logic_block(lb: &hir::LogicBlock, ctx: &mut LowerCtx<'_>) -> Vec<lir::Stmt> {
    use hir::LogicBlockScope as Scope;
    match lb.scope {
        Scope::Standalone => {
            ctx.push_block_scope();
            let out = lower_block_stmt_list(&lb.stmts, ctx);
            ctx.pop_block_scope();
            out
        }
        Scope::Opens => {
            ctx.push_block_scope();
            lower_block_stmt_list(&lb.stmts, ctx)
        }
        Scope::Continues => lower_block_stmt_list(&lb.stmts, ctx),
    }
}

pub(super) fn lower_block_stmt_list(
    stmts: &[hir::BlockStmt],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::Stmt> {
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
                is_tunnel: ret.kind == hir::ReturnKind::TunnelRedirect,
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
            // `while await cond { … }` (docs/flow-suspension-spec.md §3): the
            // persistent-await loop is a suspension point, fenced at lowering
            // (E052) exactly like a bare `await` until FS-3. A plain `while`
            // loop lowers as usual.
            if w.is_await {
                super::stmts::emit_await_lowering_fence(ctx, w.ptr.text_range());
                return;
            }
            // Same bracket shape as `lower_if_branch`: the scope opens
            // before the condition so an `as` binding declares into it.
            // `LogicWhile` re-evaluates `condition` each pass, so the
            // binding rebinds per iteration with no extra machinery (B1b,
            // issue #1475).
            ctx.push_block_scope();
            let condition = match &w.binding {
                Some(binding) => lower_bound_condition(&w.condition, binding, ctx),
                None => lower_expr(&w.condition, ctx),
            };
            ctx.loop_depth += 1;
            let body = lower_block_stmt_list(&w.body, ctx);
            ctx.loop_depth -= 1;
            ctx.pop_block_scope();
            out.push(lir::Stmt::LogicWhile(lir::LogicWhile {
                condition,
                body,
                post: Vec::new(),
            }));
        }
        hir::BlockStmt::For(f) => lower_for_stmt(f, ctx, out),
        hir::BlockStmt::Break(ptr) => {
            lower_loop_control(ptr, "break", lir::Stmt::LogicBreak, ctx, out);
        }
        hir::BlockStmt::Continue(ptr) => {
            lower_loop_control(ptr, "continue", lir::Stmt::LogicContinue, ctx, out);
        }
        hir::BlockStmt::ExprStmt(expr) => {
            if !try_lower_mutator_stmt(expr, ctx, out)
                && !try_lower_frame_local_auto_ref_stmt(expr, ctx, out)
            {
                out.push(lir::Stmt::ExprStmt(lower_expr(expr, ctx)));
            }
        }
        // `await <cond>` inside a `~ { … }` block (docs/flow-suspension-spec.md
        // §3) — fenced at lowering (E052) until FS-3, same as the top-level
        // `~ await` and `while await` forms.
        hir::BlockStmt::Await(a) => {
            super::stmts::emit_await_lowering_fence(ctx, a.ptr.text_range());
        }
    }
}

/// Lower a `break`/`continue` statement, rejecting it with E057 when it's
/// not nested inside any `while`/`for` loop (`ctx.loop_depth == 0`) instead
/// of emitting an unguarded `LogicBreak`/`LogicContinue` — codegen's
/// `loop_stack` has no jump target for one and previously degraded it to a
/// silent `Nop` (#577 review). The malformed statement is skipped (not
/// pushed to `out`), matching how an unresolvable assignment target is
/// already skipped elsewhere in this module — the diagnostic is what
/// surfaces this to authors, and it's Error-severity (unlike the E054
/// shadow warning above), so `brink-db`'s `lir_query` refuses to hand back
/// a `Program` at all, independent of and non-suppressible relative to any
/// analysis-phase diagnostic covering the same construct.
fn lower_loop_control(
    ptr: &crate::Provenance,
    keyword: &str,
    stmt: lir::Stmt,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    if ctx.loop_depth == 0 {
        ctx.diagnostics.push(Diagnostic {
            file: ctx.file,
            range: ptr.text_range(),
            message: format!(
                "{}: `{keyword}` used outside any enclosing while/for loop",
                DiagnosticCode::E057.title(),
            ),
            code: DiagnosticCode::E057,
        });
        return;
    }
    out.push(stmt);
}

fn lower_if_branch(
    if_stmt: &hir::IfStmt,
    ctx: &mut LowerCtx<'_>,
    branches: &mut Vec<lir::CondBranch>,
) {
    // The scope opens BEFORE the condition so an `as` binding (B1b, issue
    // #1475) can declare into it; it closes after the success arm, which is
    // exactly the binding's ruled scope — the `else`/`else if` arms below
    // are lowered outside the bracket and never see the name.
    ctx.push_block_scope();
    let condition = Some(match &if_stmt.binding {
        Some(binding) => lower_bound_condition(&if_stmt.condition, binding, ctx),
        None => lower_expr(&if_stmt.condition, ctx),
    });
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

/// Lower a condition that carries an `as` binding (B1b, issue #1475) into
/// the `OptionBind` condition expression, with the binding's scope already
/// open.
///
/// The caller **must** be inside its own `push_block_scope` bracket when it
/// calls this and must pop it after lowering the success arm — that bracket
/// IS the "scoped strictly to the success arm" rule (an `else`/`else if`
/// arm is lowered outside it, so the name is invisible there).
///
/// The binding shares `declare_shadow_checked`'s slot allocation and E054
/// shadow warning with an ordinary block `let`: an `as` binding is a
/// block-scoped immutable local, so shadowing an outer temp is legal and
/// warned about in exactly the same way.
pub(super) fn lower_bound_condition(
    condition: &hir::Expr,
    binding: &crate::Name,
    ctx: &mut LowerCtx<'_>,
) -> lir::Expr {
    // The condition is evaluated before the name becomes visible, so
    // `if find(s) as s { … }` reads the OUTER `s` in its own condition —
    // the same rule `lower_block_temp_decl` applies to `let x = x`.
    let value = lower_expr(condition, ctx);
    let (slot, name) = declare_shadow_checked(&binding.text, binding.range, ctx);
    // The binding is immutable by ruling — record the slot so every write
    // path refuses it (`stmts::lower_assign_target`, E148).
    ctx.as_binding_slots.insert(slot);
    lir::Expr::OptionBind {
        value: Box::new(value),
        slot,
        name,
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
    ctx.record_temp_annotation(slot, decl.annotation.as_ref());
    out.push(lir::Stmt::DeclareTemp { slot, name, value });
}

fn lower_block_assignment(
    assign: &hir::Assignment,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    if try_lower_field_assignment(assign, ctx, out) {
        return;
    }
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

/// Attempt to lower `assign` as a TM-4c struct field write (`p.field =
/// expr`/`p.field op= expr`, `docs/typed-mode-spec.md` §6) — single level
/// only, mirroring [`lower_indexed_assignment`]'s `n == 1` fast path (take →
/// `make_mut` → write-back on the root cell). Returns `false` (nothing
/// lowered or diagnosed) when `assign.target` isn't this shape at all, so
/// the caller falls through to ordinary assignment/indexed-assignment
/// handling.
///
/// A bare `ident.ident` (or longer) chain always parses as one multi-segment
/// `hir::Expr::Path` (see `expr::lower_ambiguous_dotted_path`'s doc) — that
/// is the *only* shape a genuine `p.field = v` target ever takes. A
/// **chained** write (`p.a.b = v`, 3+ segments) or a **mixed** chain
/// (`arr[i].field = v`/`foo().field = v`, the "unambiguous" `FieldAccessExpr`
/// target grammar, whose base is never a plain `Path`) is recognized here
/// too, but rejected with a real, non-suppressible `E074` diagnostic (the
/// T1e boundary the issue fences off) rather than silently miscompiled —
/// this still returns `true` (handled: the diagnostic *is* the handling),
/// so the caller doesn't fall through to a different lowering path that
/// might mishandle the same target shape.
pub(super) fn try_lower_field_assignment(
    assign: &hir::Assignment,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) -> bool {
    if let hir::Expr::Path(path) = &assign.target
        && path.segments.len() > 1
        && let Some(info) = ctx.resolve_path(path.range)
        && matches!(
            info.kind,
            SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Param | SymbolKind::Temp
        )
    {
        if path.segments.len() > 2 {
            emit_chained_field_write_diagnostic(path.range, ctx);
        } else {
            lower_single_level_field_write(path, info, assign.op, &assign.value, ctx, out);
        }
        return true;
    }

    if let hir::Expr::FieldAccess(fa) = &assign.target {
        emit_chained_field_write_diagnostic(fa.ptr.text_range(), ctx);
        return true;
    }

    false
}

fn emit_chained_field_write_diagnostic(range: rowan::TextRange, ctx: &mut LowerCtx<'_>) {
    ctx.diagnostics.push(Diagnostic {
        file: ctx.file,
        range,
        message: DiagnosticCode::E074.title().to_string(),
        code: DiagnosticCode::E074,
    });
}

/// The single-level case (`path.segments.len() == 2`) — `p.field = v`/`p.field
/// op= v` on a resolvable root. Follows the identical take → `make_mut` →
/// write-back RMW discipline [`lower_flat_indexed_assignment`] uses,
/// substituting a `RecordGet`/`RecordSet` field op for that function's
/// `Index`/`IndexSet`: the RHS is evaluated once (root still intact), then
/// `current = root.field` is *always* computed via a non-taking read (the
/// fault pre-check — see `lower_flat_indexed_assignment`'s doc for why this
/// matters: it forces the exact same missing-field validation the mutate
/// step would hit, before the root is ever taken, so a fault never leaves
/// the root holding `Value::Null`), then the root is taken and mutated in
/// place.
fn lower_single_level_field_write(
    path: &hir::Path,
    head_info: &crate::symbols::SymbolInfo,
    op: AssignOp,
    value_expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    #[expect(
        clippy::indexing_slicing,
        reason = "caller already proved path.segments.len() == 2"
    )]
    let field_name = path.segments[1].text.clone();
    #[expect(
        clippy::indexing_slicing,
        reason = "caller already proved path.segments.len() == 2"
    )]
    let head_name = path.segments[0].text.clone();

    let (root_target, root_shape) = match head_info.kind {
        SymbolKind::Variable | SymbolKind::Constant => (
            lir::AssignTarget::Global(head_info.id),
            ctx.global_shape(head_info.id).map(str::to_string),
        ),
        SymbolKind::Param | SymbolKind::Temp => {
            let Some(slot) = ctx.temp_slot(&head_name) else {
                return;
            };
            let name_id = ctx.names.intern(&head_name);
            (
                lir::AssignTarget::Temp(slot, name_id),
                ctx.temp_shape(slot).map(str::to_string),
            )
        }
        // `try_lower_field_assignment` only reaches here for these four kinds.
        _ => return,
    };

    let static_offset = if ctx.structs.type_mode == TypeMode::Strict {
        root_shape
            .as_deref()
            .and_then(|s| ctx.structs.shapes.get(s))
            .and_then(|shape| shape.field(&field_name))
            .map(|(offset, _)| offset)
    } else {
        None
    };
    let field = ctx.names.intern(&field_name);

    // 1. RHS value, evaluated once — root still intact (mirrors
    //    `lower_flat_indexed_assignment` step 2).
    let rhs_value = lower_expr(value_expr, ctx);
    let (rhs_slot, rhs_name) = declare_synthetic("__rhs", rhs_value, ctx, out);

    // 2. Pre-mutation `current = root.field`, ALWAYS computed (fault
    //    pre-check + compound assignment's operand), via an ordinary
    //    (non-taking) read of the still-intact root.
    let current = lir::Expr::RecordGet {
        base: Box::new(get_expr_for_target(&root_target)),
        field,
        static_offset,
    };
    let (current_slot, current_name) = declare_synthetic("__current", current, ctx, out);

    let rhs = if op == AssignOp::Set {
        lir::Expr::GetTemp(rhs_slot, rhs_name)
    } else {
        // `op == AssignOp::Set` is excluded above, so only `Add`/`Sub` ever
        // reach here.
        let infix_op = if op == AssignOp::Sub {
            InfixOp::Sub
        } else {
            InfixOp::Add
        };
        lir::Expr::Infix(
            Box::new(lir::Expr::GetTemp(current_slot, current_name)),
            infix_op,
            Box::new(lir::Expr::GetTemp(rhs_slot, rhs_name)),
        )
    };

    // 3. Take the root — step 2 already proved this exact field is valid
    //    against this exact record value (nothing mutated in between), so
    //    nothing from here on can fault; the root is never left holding
    //    `Value::Null` on this path.
    let (c_slot, c_name) = declare_synthetic("__c", take_expr_for_target(&root_target), ctx, out);
    out.push(lir::Stmt::Assign {
        target: lir::AssignTarget::Temp(c_slot, c_name),
        op: AssignOp::Set,
        value: lir::Expr::RecordSet {
            base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            field,
            static_offset,
            value: Box::new(rhs),
        },
    });

    // 4. Write the mutated record back into the root — takes the (now
    //    dead) synthetic temp too, avoiding one final wasted `Arc` clone.
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: lir::Expr::TakeTemp(c_slot, c_name),
    });
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

/// `get_expr_for_target`'s move-semantics counterpart (issue #576,
/// `docs/value-model-spec.md` §5): moves the target's current value out,
/// leaving `Value::Null` behind, instead of cloning (`Arc`-bumping) it.
/// Only safe to use where nothing else needs `target`'s old value again
/// before it's written back — see [`lower_flat_indexed_assignment`] and
/// [`lower_bare_mutator`], the two call sites that establish this.
fn take_expr_for_target(target: &lir::AssignTarget) -> lir::Expr {
    match target {
        lir::AssignTarget::Global(id) => lir::Expr::TakeGlobal(*id),
        lir::AssignTarget::Temp(slot, name) => lir::Expr::TakeTemp(*slot, *name),
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
///
/// `n == 1` (`a[i] = v`/`a[i] op= v` on a bare variable — the loop-append
/// case value-model-spec §5's "one cliff" targets) dispatches to
/// [`lower_flat_indexed_assignment`], which closes the COW cliff via
/// `TakeGlobal`/`TakeTemp` (issue #576). `n > 1` (chained, e.g.
/// `grid[y][x] = v`) keeps the clone-based RMW below unchanged: a nested
/// container's element is necessarily read out via a structural clone
/// before it can be walked further (it's *still referenced from inside its
/// parent* until that parent's own write-back cascade completes), so a
/// take at any level but the root buys nothing there — this is the
/// sanctioned §7 fallback ("per-write path-walking RMW"), not a regression.
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

    if n == 1 {
        #[expect(
            clippy::indexing_slicing,
            reason = "n == 1 just proved indices_hir has exactly one element"
        )]
        let index_hir = indices_hir[0];
        lower_flat_indexed_assignment(root_target, index_hir, op, value_expr, ctx, out);
        return;
    }

    lower_chained_indexed_assignment(root_target, &indices_hir, op, value_expr, ctx, out);
}

/// Fast path for `n == 1` (`a[i] = v`/`a[i] op= v`, `a` a bare variable):
/// closes the indexed-write COW cliff (issue #576, value-model-spec §5) by
/// using `TakeGlobal`/`TakeTemp` (move semantics — `docs/format-v4-rfc.md`
/// §3 "Sharing discipline") for the root read and the mutate step's
/// container operand, so `array_make_mut`/`map_make_mut` sees a unique
/// `Arc` (refcount 1) whenever nothing else aliases the container — O(1)
/// amortized in-place mutation instead of an O(n) COW copy on every write.
///
/// **Evaluation order** (correctness-critical): the index and the RHS value
/// are fully evaluated — via a non-taking, ordinary read of the still-intact
/// root — *before* the root is taken. This matters because either
/// expression may reference the root variable by name (e.g. `a[0] = a[1] +
/// 1`) and must see its pre-mutation value, not the `Value::Null` a take
/// would leave behind if it happened first.
///
/// **Compound assignment** (`+=`/`-=`) additionally computes the
/// pre-mutation `current = a[idx]` read (needed as the operand) *before*
/// the take, via the same non-taking `Index` read — unchanged by issue
/// #856. As a side effect this still catches out-of-bounds/missing-key/
/// non-collection faults before anything is taken, leaving the root
/// completely untouched on a compound-assign fault, exactly like the
/// pre-#576 clone-based RMW.
///
/// **Plain assignment** (`a[idx] = v`) does **not** compute `current` —
/// nothing needs its value, and (issue #856, ruled 2026-07-15) `IndexSet`'s
/// map branch is now insert-on-absent, so there's no missing-key fault left
/// to pre-empt there. The remaining fault causes on this path
/// (out-of-bounds array index, an invalid-domain map key, a non-collection
/// root) are still turn-terminating faults, but now surface *inside* the
/// take-based mutate step rather than before it, so the root can be left
/// `Value::Null` on one of those — the same documented, deliberate
/// no-precheck trade-off `fault_during_insert_leaves_root_null`/
/// `fault_during_remove_at_leaves_root_null` (runtime crate) already accept for
/// `insert`/`remove`/`remove_at`'s author-supplied keys ("a fault anywhere mid-turn
/// already leaves earlier same-turn mutations applied"). See
/// `fault_during_flat_index_assignment_leaves_root_null` (runtime crate,
/// renamed by #856 from `..._leaves_root_unchanged`) for the property test.
fn lower_flat_indexed_assignment(
    root_target: lir::AssignTarget,
    index_hir: &hir::Expr,
    op: AssignOp,
    value_expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    // 1. Index, evaluated once — root is still intact.
    let index_value = lower_expr(index_hir, ctx);
    let (idx_slot, idx_name) = declare_synthetic("__idx", index_value, ctx, out);

    // 2. RHS value, materialized into its own temp *before* the take (see
    //    doc above) — also still reading the intact root if it references
    //    it.
    let rhs_value = lower_expr(value_expr, ctx);
    let (rhs_slot, rhs_name) = declare_synthetic("__rhs", rhs_value, ctx, out);

    // 3. Compound assignment only (`+=`/`-=`): pre-mutation `current =
    //    a[idx]`, needed as the operand, via an ordinary (non-taking) read
    //    of the still-intact root. As a side effect this also validates the
    //    index/key before anything is taken (see doc above) — plain `=`
    //    skips this entirely (issue #856): it doesn't need `current`'s
    //    value, and `IndexSet`'s map branch no longer faults on a missing
    //    key, so there's nothing left to pre-empt for maps; the remaining
    //    fault causes (array OOB, invalid-domain map key, non-collection
    //    root) are still faults, just inside the take-based mutate step.
    let rhs = if op == AssignOp::Set {
        lir::Expr::GetTemp(rhs_slot, rhs_name)
    } else {
        let current = lir::Expr::Index {
            base: Box::new(get_expr_for_target(&root_target)),
            index: Box::new(lir::Expr::GetTemp(idx_slot, idx_name)),
        };
        let (current_slot, current_name) = declare_synthetic("__current", current, ctx, out);
        // `op == AssignOp::Set` is excluded above, so only `Add`/`Sub`
        // ever reach here.
        let infix_op = if op == AssignOp::Sub {
            InfixOp::Sub
        } else {
            InfixOp::Add
        };
        lir::Expr::Infix(
            Box::new(lir::Expr::GetTemp(current_slot, current_name)),
            infix_op,
            Box::new(lir::Expr::GetTemp(rhs_slot, rhs_name)),
        )
    };

    // 4. Take the root. For compound assignment, step 3 already proved this
    //    exact index is valid against this exact container value (nothing
    //    mutated in between), so nothing from here on can fault; the root
    //    is never left `Value::Null` on that path. For plain `=`, step 5's
    //    `IndexSet` can still fault (array OOB, invalid-domain map key,
    //    non-collection root) — the documented, deliberate trade-off
    //    `fault_during_insert_leaves_root_null` already accepts for
    //    `insert`/`remove`/`remove_at`'s author-supplied keys applies here too.
    let (c_slot, c_name) = declare_synthetic("__c", take_expr_for_target(&root_target), ctx, out);

    // 5. Mutate in place: base is a *take* from `c_slot` too — `c_slot`'s
    //    old value is never read again (this statement's own result
    //    overwrites it), so by the time `array_make_mut`/`map_make_mut`
    //    runs, the only live reference to the container is the one this
    //    statement is about to consume — refcount 1 whenever nothing else
    //    aliases it.
    out.push(lir::Stmt::Assign {
        target: lir::AssignTarget::Temp(c_slot, c_name),
        op: AssignOp::Set,
        value: lir::Expr::IndexSet {
            base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            index: Box::new(lir::Expr::GetTemp(idx_slot, idx_name)),
            value: Box::new(rhs),
        },
    });

    // 6. Write the mutated container back into the root — takes the (now
    //    dead) synthetic temp too, avoiding one final wasted `Arc` clone.
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: lir::Expr::TakeTemp(c_slot, c_name),
    });
}

/// `n > 1` (chained, e.g. `grid[y][x] = v`) — the pre-#576 clone-based RMW,
/// unchanged. See [`lower_indexed_assignment`]'s doc for why the take-based
/// optimization doesn't extend here.
fn lower_chained_indexed_assignment(
    root_target: lir::AssignTarget,
    indices_hir: &[&hir::Expr],
    op: AssignOp,
    value_expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    let n = indices_hir.len();
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

/// Lower `for x in arr { … }` / `for k in map { … }` — or, on the native
/// surface, `for k, v in map { … }` (§2, plus B2 issue #1461 for the
/// two-binding form). Desugars to an index-based `LogicWhile` — dedicated
/// iterator opcodes are deliberately not part of the T1b surface
/// (`docs/format-v4-rfc.md` §3 note); the iterable is snapshotted once via
/// `CollectionKeys` (which returns the array unchanged for an array input
/// — see that opcode's doc — so one snapshot expression correctly covers
/// both "iterate values" and "iterate keys" without a static array/map
/// type distinction).
///
/// The two-binding form (`f.val_name.is_some()`) is exactly the F10-ruled
/// desugar (`docs/stdlib-spec.md` §5/§9: "`for k, v in m` ... desugars to
/// key-iteration + `let v = m[k]`, total by construction, no pair shape
/// ever materializes") — an extra `DeclareTemp` reading `container[key]`
/// at the top of the body, right after `key` itself is declared. The
/// container is evaluated exactly once, into its own synthetic temp,
/// *before* the keys snapshot — it's read twice (once to snapshot its
/// keys, once per-iteration to index it), and `f.iterable` may be an
/// arbitrary expression (e.g. a call) that must not run twice. The
/// single-binding form keeps the original one-snapshot shape byte-for-byte
/// unchanged (no synthetic container temp) since it only ever reads the
/// snapshot.
#[expect(
    clippy::similar_names,
    reason = "var_name/val_name are the ForStmt field names (k/v's HIR spelling, B2 #1461) — \
              not a pair a rename would clarify"
)]
fn lower_for_stmt(f: &hir::ForStmt, ctx: &mut LowerCtx<'_>, out: &mut Vec<lir::Stmt>) {
    let iterable = lower_expr(&f.iterable, ctx);
    let snapshot_source = if f.val_name.is_some() {
        let (container_slot, container_name) =
            declare_synthetic("__for_container", iterable, ctx, out);
        lir::Expr::GetTemp(container_slot, container_name)
    } else {
        iterable
    };
    let (snap_slot, snap_name) = declare_synthetic(
        "__for_snapshot",
        lir::Expr::CollectionKeys(Box::new(snapshot_source.clone())),
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
    if let Some(val_name) = &f.val_name {
        let (val_slot, val_name_id) = declare_shadow_checked(&val_name.text, val_name.range, ctx);
        body.push(lir::Stmt::DeclareTemp {
            slot: val_slot,
            name: val_name_id,
            value: Some(lir::Expr::Index {
                base: Box::new(snapshot_source),
                index: Box::new(lir::Expr::GetTemp(var_slot, var_name)),
            }),
        });
    }
    ctx.loop_depth += 1;
    body.extend(lower_block_stmt_list(&f.body, ctx));
    ctx.loop_depth -= 1;
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

// ─── T1b stdlib slice 1 mutators (§5) ──────────────────────────────────
//
// `push(a, v)` / `insert(x, k_or_i, v)` / `remove(m, k)` / `remove_at(a, i)`
// require an lvalue first argument and lower through the same take →
// `make_mut` → write-back RMW discipline as indexed assignment (§4) —
// desugaring to a chain of synthetic-temp `Assign`s exactly like
// `lower_indexed_assignment` above, just with a
// `CollectionInsert`/`CollectionRemove`/`SeqRemoveAt` mutate step instead
// of the deepest level's `IndexSet`.

/// The chain state [`lower_lvalue_container_chain`] returns: the root
/// assign target, the materialized index temps, and the materialized
/// container-read temps. See that function's doc for the shape.
type LvalueContainerChain = (
    lir::AssignTarget,
    Vec<(u16, brink_format::NameId)>,
    Vec<(u16, brink_format::NameId)>,
);

/// A collection mutator recognized from a call expression
/// (`docs/t1b-surface-spec.md` §5).
#[derive(Clone, Copy)]
enum MutatorKind {
    /// `push(a, v)` — 2 args; desugars to `insert(a, len(a), v)`.
    Push,
    /// `insert(x, k_or_i, v)` — 3 args.
    Insert,
    /// `remove(m, k)` — 2 args. Map-only as of issue #1484: `remove`
    /// uniformly names identity-based, idempotent-total removal (map keys,
    /// flags values). The array-index leg lives at `RemoveAt` now.
    Remove,
    /// `remove_at(a, i)` — 2 args (issue #1484, joining the `_at`
    /// faulting-index family with `char_at`): removes the array element at
    /// `i`, faulting out of bounds.
    RemoveAt,
    /// `clear(m)` — 1 arg (NS-A1, `docs/stdlib-spec.md` §5): empty the map
    /// in place, total.
    Clear,
    /// `shuffle(a)` — 1 arg (NS-A6, `docs/stdlib-spec.md` §7): Fisher-Yates
    /// shuffle of the array in place; every element swap draws through the
    /// one RNG cell. `shuffled(a)` is the functional twin (ordinary
    /// expression lowering).
    Shuffle,
    /// `sort(a)` — 1 arg (NS-A4, `docs/stdlib-spec.md` §4b): sort the
    /// array in place by the doctrine order (dev NaN-fault / prod pinned
    /// placement at the runtime knob). `sorted(a)` is the functional twin.
    Sort,
    /// `sort_by(a, cmp)` — 2 args (NS-A4, F0 ruled 2026-07-19): sort the
    /// array in place by a comparator function value `fn(T, T): int`.
    /// `sorted_by(a, cmp)` is the functional twin.
    SortBy,
    /// `heap_push(a, x)` — 2 args (NS-A7, `docs/stdlib-spec.md` §8): sift
    /// `x` into the min-heap maintained over the array, in place (§4b
    /// entry check: dev NaN-fault / prod pinned placement at the runtime
    /// knob). `heap_pop`/`heap_peek` are not mutator-statement shapes —
    /// `heap_pop` is the `pop` expression/bracket shape, `heap_peek` a
    /// pure expression.
    HeapPush,
}

impl MutatorKind {
    /// The mutator names are a subset of `super::expr::is_t1b_stdlib_name`
    /// (which also covers the pure functions) — kept as an explicit
    /// `matches!` here rather than depending on that function so this
    /// module doesn't need to filter out the pure names on every call.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "push" => Some(Self::Push),
            "insert" => Some(Self::Insert),
            "remove" => Some(Self::Remove),
            "remove_at" => Some(Self::RemoveAt),
            "clear" => Some(Self::Clear),
            "shuffle" => Some(Self::Shuffle),
            "sort" => Some(Self::Sort),
            "sort_by" => Some(Self::SortBy),
            "heap_push" => Some(Self::HeapPush),
            _ => None,
        }
    }

    fn expected_argc(self) -> usize {
        match self {
            Self::Clear | Self::Shuffle | Self::Sort => 1,
            Self::Push | Self::Remove | Self::RemoveAt | Self::SortBy | Self::HeapPush => 2,
            Self::Insert => 3,
        }
    }

    /// The mutator's documented signature (§5), for a targeted E058 message
    /// naming exactly what was expected.
    fn signature(self) -> &'static str {
        match self {
            Self::Push => "push(container, value)",
            Self::Insert => "insert(container, key_or_index, value)",
            Self::Remove => "remove(map, key)",
            Self::RemoveAt => "remove_at(array, index)",
            Self::Clear => "clear(map)",
            Self::Shuffle => "shuffle(array)",
            Self::Sort => "sort(array)",
            Self::SortBy => "sort_by(array, comparator)",
            Self::HeapPush => "heap_push(array, value)",
        }
    }
}

/// Whether `expr` is a valid mutator lvalue (§5: "a variable, temp, or
/// indexed path") — a bare path, or an (arbitrarily chained) indexed path
/// rooted in one. Anything else (a call, literal, operator, collection
/// literal, …) is an rvalue.
fn is_lvalue_expr(expr: &hir::Expr) -> bool {
    match expr {
        hir::Expr::Path(_) => true,
        hir::Expr::Index(idx) => is_lvalue_expr(&idx.base),
        _ => false,
    }
}

/// Recognize and fully lower a `push`/`insert`/`remove`/`remove_at` call
/// statement (§5), splicing its RMW expansion into `out`. Returns `false`
/// (nothing pushed) when `expr` isn't one of these mutator calls, or
/// resolves to a real user symbol — a temp/param holding a divert target,
/// or a resolved
/// knot/external/list/variable (shadowed; the caller falls through to
/// ordinary call lowering, and `brink-analyzer`'s symbol-declaration pass
/// separately emits the E035 shadow warning at the declaration site).
///
/// Called from both `~ { … }` block statements (`lower_block_stmt` above)
/// and classic non-block `~ push(...)` logic lines (`lower::mod`'s
/// `lower_block_with_children`) — a function call used as a statement for
/// its side effect is not a T1b-only concept (ordinary knots/externals are
/// already callable that way outside any block).
/// `seed(n)` (NS-A6, `docs/stdlib-spec.md` §7): statement-only like the
/// mutators, but its argument is an ordinary value, not an lvalue
/// receiver — it writes the RNG cell, not its argument — so it takes its
/// own path rather than joining `MutatorKind`'s lvalue/RMW machinery. It
/// lowers to the frozen `SEED_RANDOM` builtin (one RNG cell, two
/// surfaces, no drift); `ExprStmt` discards the op's `Null`. Same shadow
/// discipline as the mutators: a resolvable user symbol of the same name
/// falls through to ordinary call lowering.
fn try_lower_seed_stmt(
    path: &hir::Path,
    args: &[hir::Expr],
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) -> bool {
    if ctx.temp_slot("seed").is_some() || ctx.resolve_path(path.range).is_some() {
        return false;
    }
    if args.len() != 1 {
        ctx.diagnostics.push(Diagnostic {
            file: ctx.file,
            range: path.range,
            message: format!(
                "{}: `seed` expects 1 argument(s), got {} — expected signature: `seed(n)`",
                DiagnosticCode::E058.title(),
                args.len(),
            ),
            code: DiagnosticCode::E058,
        });
        return true;
    }
    let arg = lower_expr(&args[0], ctx);
    out.push(lir::Stmt::ExprStmt(lir::Expr::CallBuiltin {
        builtin: lir::BuiltinFn::SeedRandom,
        args: vec![arg],
    }));
    true
}

pub(super) fn try_lower_mutator_stmt(
    expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) -> bool {
    let hir::Expr::Call(path, args) = expr else {
        return false;
    };
    let name = super::expr::path_to_string(path);

    if name == "seed" && try_lower_seed_stmt(path, args, ctx, out) {
        return true;
    }
    if name == "seed" {
        return false;
    }

    // B3a UFCS (issue #1506): `m.insert(k, v)` reaches here as a
    // multi-segment `Call` path. `path_to_string` on a multi-segment path
    // yields the dotted string ("m.insert"), which never matches
    // `MutatorKind::from_name` (it only recognizes the bare verb) — so
    // without this arm, every mutator verb spelled as method-call syntax
    // fell through to `lower_call`'s UFCS dispatch
    // (`lower_ufcs_prelude_desugar` → `lower_t1b_stdlib_call`), which
    // unconditionally refuses every mutator name with E056 ("used in
    // expression position") even from statement position. A UFCS call site
    // always carries a resolution at `path.range` naming the *receiver*
    // (see `expr::lower_ufcs_call`'s own doc), so the ordinary
    // `ctx.resolve_path(path.range).is_some()` shadow check below would
    // always bail out for one of these — this arm runs first, reading the
    // analyzer's verdict directly instead of that resolution, and splices
    // the receiver in as the mutator's first argument before running the
    // same RMW expansion a bare `insert(m, k, v)` statement gets.
    if path.segments.len() > 1
        && let Some(UfcsVerdict::PreludeDesugar { name: verb }) =
            ctx.tables.ufcs.get(ctx.file, path.range).cloned()
        && let Some(kind) = MutatorKind::from_name(&verb)
    {
        let receiver = super::expr::ufcs_receiver_path(path);
        let mut desugared_args = Vec::with_capacity(args.len() + 1);
        desugared_args.push(hir::Expr::Path(receiver));
        desugared_args.extend(args.iter().cloned());
        lower_mutator_call(kind, &verb, path, &desugared_args, ctx, out);
        return true;
    }

    let Some(kind) = MutatorKind::from_name(&name) else {
        return false;
    };
    if ctx.temp_slot(&name).is_some() || ctx.resolve_path(path.range).is_some() {
        return false;
    }

    lower_mutator_call(kind, &name, path, args, ctx, out);
    true
}

/// Frame-local projection auto-ref (issue #1531, RULED 2026-07-27 —
/// `docs/decision-log.md`): `g.hp.heal(5)` where `g` is a temp/param and
/// `heal`'s first parameter is `ref`. `brink-analyzer::ufcs::
/// auto_ref_fault` now accepts a frame-local, single-field-deep receiver
/// like this one as a legal `FreeFnAutoRef` verdict — a frame-local cell is
/// a valid projection root, and the mutation needs no effect row because it
/// is unobservable outside the frame.
///
/// There is still no *expression*-shaped lowering for it, though:
/// [`lir::CallArg::RefProjection`]'s root is a durable global
/// [`brink_format::DefinitionId`] only (`docs/format-v4-rfc.md` §1) — using
/// a frame-local's `LocalVar`-tagged id there would fault at runtime as
/// `UnresolvedGlobal` with no compile diagnostic (the same hazard
/// `expr::lower_ref_path_call_arg`'s block-scoped-temp guard already
/// documents for the bare-receiver case). So this recognizes the shape at
/// **statement** position only and expands it as the same RMW discipline
/// `try_lower_field_assignment` already established for `g.hp = v`: read
/// the field into a synthetic temp, call the target passing that temp by
/// `ref` (an ordinary bare [`lir::CallArg::RefTemp`], never a projection),
/// then write the temp back into the field. `expr::lower_ref_projection_arg`
/// carries the matching defense-in-depth refusal for the same verdict
/// reached from *expression* position (nested inside a larger expression,
/// where this recognizer never gets a chance to run) — see that function's
/// own frame-local guard.
///
/// Returns `false` (nothing lowered) for every other call shape, so the
/// caller falls through to ordinary call lowering — including a
/// `FreeFnAutoRef` verdict whose receiver is a durable global or a bare
/// (non-projection) frame-local, both of which
/// `expr::lower_ufcs_desugared_call` already handles correctly, and a
/// receiver more than one field deep, which `brink-analyzer`'s own gate
/// still refuses with `E143` before lowering ever sees it.
pub(super) fn try_lower_frame_local_auto_ref_stmt(
    expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) -> bool {
    let hir::Expr::Call(path, args) = expr else {
        return false;
    };
    let Some(UfcsVerdict::FreeFnAutoRef { target }) =
        ctx.tables.ufcs.get(ctx.file, path.range).cloned()
    else {
        return false;
    };
    let receiver = super::expr::ufcs_receiver_path(path);
    if receiver.segments.len() != 2 {
        return false;
    }
    #[expect(
        clippy::indexing_slicing,
        reason = "just proved receiver.segments.len() == 2"
    )]
    let head_name = receiver.segments[0].text.clone();
    #[expect(
        clippy::indexing_slicing,
        reason = "just proved receiver.segments.len() == 2"
    )]
    let field_name = receiver.segments[1].text.clone();
    // A durable global (or an unresolved name) falls through to the
    // ordinary `RefProjection` desugar — only a genuine frame-local takes
    // this path.
    let Some(root_slot) = ctx.temp_slot(&head_name) else {
        return false;
    };
    // B1b (issue #1475): the same `ref`-bypasses-immutability hole
    // `lower_ref_path_call_arg` and `lower_ref_projection_arg` both guard —
    // this recognizer writes the receiver back into `root_slot` too (step
    // 3 below), so an `as` binding must be refused here as well. Return
    // `true` (handled) rather than `false`: falling through would let this
    // same call reach `expr::lower_ref_projection_arg`'s frame-local guard
    // instead, which emits the misleading "must be its own statement"
    // `E143` — this call *is* its own statement; the real problem is the
    // `as` binding's immutability.
    if ctx.as_binding_slots.contains(&root_slot) {
        ctx.diagnostics.push(Diagnostic {
            file: ctx.file,
            range: path.range,
            message: format!(
                "{}: `{head_name}` is an `as` binding — it is immutable and cannot be passed \
                 by `ref`",
                DiagnosticCode::E148.title(),
            ),
            code: DiagnosticCode::E148,
        });
        return true;
    }
    let Some(target_info) = ctx.index.symbols.get(&target) else {
        // Structurally unreachable — `target` came from the analyzer's own
        // resolution against this same project index, exactly like
        // `expr::lower_ufcs_desugared_call`'s identical guard. Falling
        // through lets that function's own copy of this guard handle it.
        return false;
    };

    let head_name_id = ctx.names.intern(&head_name);
    let field = ctx.names.intern(&field_name);
    let root_target = lir::AssignTarget::Temp(root_slot, head_name_id);

    let static_offset = if ctx.structs.type_mode == TypeMode::Strict {
        ctx.temp_shape(root_slot)
            .and_then(|s| ctx.structs.shapes.get(s))
            .and_then(|shape| shape.field(&field_name))
            .map(|(offset, _)| offset)
    } else {
        None
    };

    // 1. Read the field into a synthetic temp — the call's receiver
    //    argument. A non-mutating read, so it can't itself trigger a COW.
    let current = lir::Expr::RecordGet {
        base: Box::new(get_expr_for_target(&root_target)),
        field,
        static_offset,
    };
    let (recv_slot, recv_name) = declare_synthetic("__recv", current, ctx, out);

    // 2. The call: `target(ref __recv, args…)` — a bare receiver, so it
    //    rides the ordinary `RefTemp` write-through (`Opcode::
    //    PushTempPointer`/`SetTemp`'s `Value::TempPointer` arm), never a
    //    projection.
    let rest_params = target_info.params.get(1..).unwrap_or(&[]);
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(lir::CallArg::RefTemp(recv_slot, recv_name));
    call_args.extend(super::expr::lower_call_args(args, rest_params, ctx));
    let call_expr = if target_info.kind == SymbolKind::External {
        lir::Expr::CallExternal {
            target,
            #[expect(
                clippy::cast_possible_truncation,
                reason = "ink externals have <=255 params"
            )]
            arg_count: target_info.params.len() as u8,
            args: call_args,
        }
    } else {
        lir::Expr::Call {
            target,
            args: call_args,
        }
    };
    out.push(lir::Stmt::ExprStmt(call_expr));

    // 3. Write the (possibly mutated) receiver back into the field. No
    //    fault pre-check is needed here — step 1's read already proved this
    //    exact field is valid on this exact root, and nothing between then
    //    and now could have invalidated that.
    let write_back = lir::Expr::RecordSet {
        base: Box::new(take_expr_for_target(&root_target)),
        field,
        static_offset,
        value: Box::new(lir::Expr::GetTemp(recv_slot, recv_name)),
    };
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: write_back,
    });

    true
}

/// The arity check / lvalue check / RMW-expansion body shared by
/// [`try_lower_mutator_stmt`]'s two call shapes: the direct call
/// (`insert(m, k, v)`) and the UFCS desugar (`m.insert(k, v)`, issue
/// #1506) — in the UFCS case the caller has already spliced the receiver
/// into `args[0]`, so from here on both shapes are identical. `name` is the
/// bare mutator verb (never the dotted UFCS spelling) — used only for
/// diagnostic messages.
fn lower_mutator_call(
    kind: MutatorKind,
    name: &str,
    path: &hir::Path,
    args: &[hir::Expr],
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    // RULED 2026-07-12 (#581, docs/decision-log.md): a mutator arity
    // mismatch is a targeted compile error naming the expected signature
    // (E058), replacing the generic E031 warning this used to share with
    // ordinary function-call arity checking. E031 only ever warned — it
    // never blocked compilation — so the malformed statement fell through
    // to "return true, push nothing," silently dropping the RMW lowering
    // (the mutator call vanished from the bytecode with no compile
    // failure). E058 is Error-severity, so `brink-db`'s `lir_query` now
    // refuses to hand back a `Program` for it, exactly like E055/E056.
    // Pure-function arity checking (ordinary knot/external calls) is
    // untouched — this only covers the mutator names.
    let expected = kind.expected_argc();
    if args.len() != expected {
        ctx.diagnostics.push(Diagnostic {
            file: ctx.file,
            range: path.range,
            message: format!(
                "{}: `{}` expects {expected} argument(s), got {} — expected signature: `{}`",
                DiagnosticCode::E058.title(),
                name,
                args.len(),
                kind.signature(),
            ),
            code: DiagnosticCode::E058,
        });
        return;
    }

    let lvalue_expr = &args[0];
    if !is_lvalue_expr(lvalue_expr) {
        ctx.diagnostics.push(Diagnostic {
            file: ctx.file,
            range: path.range,
            message: format!(
                "{}: `{name}` mutates its first argument — bind it to a variable first",
                DiagnosticCode::E055.title(),
            ),
            code: DiagnosticCode::E055,
        });
        return;
    }

    // Bare-variable lvalue (`push(a, v)`, not `push(grid[y], v)`) — the
    // loop-append benchmark's shape — dispatches to the take-based fast
    // path (issue #576). A chained lvalue keeps the clone-based fallback
    // below unchanged, for the same reason `lower_indexed_assignment`
    // scopes its own fast path to `n == 1`: a nested element is still
    // referenced from inside its parent until the write-back cascade
    // completes, so Take buys nothing at any level but the root.
    //
    // A bare `ident.ident` chain (`push(a.items, v)`) always parses as one
    // multi-segment `hir::Expr::Path` too — never `hir::Expr::FieldAccess`
    // (see `try_lower_field_assignment`'s doc) — so it lands in this same
    // arm. Issue #1495: without this split, `path.segments.len() > 1`
    // silently fell into the bare-variable path below, whose
    // `lower_assign_target` resolves the *whole path's range* to the
    // **root** variable (the TM-4b resolution-fallback shape) — routing
    // the mutator onto `a` (a `Record`) instead of `a.items` (the `Array`),
    // a silent misroute that only surfaced as a runtime `NotIndexable`
    // fault. Mirrors `try_lower_field_assignment`'s own split exactly: a
    // single-segment path (or one that doesn't resolve to a struct-field
    // root) keeps the bare-variable fast path; a struct-field projection
    // routes through `lower_field_mutator`; a chained projection (3+
    // segments) is rejected with the same non-suppressible `E074` that
    // function's chained-write case already raises, rather than silently
    // miscompiled.
    if let hir::Expr::Path(path) = lvalue_expr {
        if path.segments.len() > 1
            && let Some(info) = ctx.resolve_path(path.range)
            && matches!(
                info.kind,
                SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Param | SymbolKind::Temp
            )
        {
            if path.segments.len() > 2 {
                emit_chained_field_write_diagnostic(path.range, ctx);
            } else {
                lower_field_mutator(kind, path, info, args, ctx, out);
            }
            return;
        }
        lower_bare_mutator(kind, lvalue_expr, args, ctx, out);
        return;
    }

    let Some((root_target, idx_slots, c_slots)) =
        lower_lvalue_container_chain(lvalue_expr, ctx, out)
    else {
        // Structurally unreachable given the `is_lvalue_expr` guard above —
        // guarded rather than asserted so a future grammar change can't
        // corrupt output instead of doing nothing (same discipline as
        // `lower_indexed_assignment`'s `n == 0` guard).
        return;
    };
    // `lower_lvalue_container_chain` always pushes the root as `c_slots[0]`
    // before ever returning `Some`, so `c_slots` is never empty.
    let Some(&(last_slot, last_name)) = c_slots.last() else {
        return;
    };
    let container = || lir::Expr::GetTemp(last_slot, last_name);

    let new_container = match kind {
        MutatorKind::Push => {
            let value = lower_expr(&args[1], ctx);
            lir::Expr::CollectionInsert {
                base: Box::new(container()),
                key: Box::new(lir::Expr::CollectionLen(Box::new(container()))),
                value: Box::new(value),
            }
        }
        MutatorKind::Insert => {
            let key = lower_expr(&args[1], ctx);
            let value = lower_expr(&args[2], ctx);
            lir::Expr::CollectionInsert {
                base: Box::new(container()),
                key: Box::new(key),
                value: Box::new(value),
            }
        }
        MutatorKind::Remove => {
            let key = lower_expr(&args[1], ctx);
            lir::Expr::CollectionRemove {
                base: Box::new(container()),
                key: Box::new(key),
            }
        }
        MutatorKind::RemoveAt => {
            let index = lower_expr(&args[1], ctx);
            lir::Expr::SeqRemoveAt {
                base: Box::new(container()),
                index: Box::new(index),
            }
        }
        MutatorKind::Clear => lir::Expr::MapClear(Box::new(container())),
        MutatorKind::Shuffle => lir::Expr::RandShuffle(Box::new(container())),
        MutatorKind::Sort => lir::Expr::SeqSorted(Box::new(container())),
        MutatorKind::SortBy => lir::Expr::SeqSortedBy {
            seq: Box::new(container()),
            cmp: Box::new(lower_expr(&args[1], ctx)),
        },
        MutatorKind::HeapPush => lir::Expr::HeapPush {
            seq: Box::new(container()),
            value: Box::new(lower_expr(&args[1], ctx)),
        },
    };

    out.push(lir::Stmt::Assign {
        target: lir::AssignTarget::Temp(last_slot, last_name),
        op: AssignOp::Set,
        value: new_container,
    });
    writeback_lvalue_container_chain(root_target, &idx_slots, &c_slots, out);
}

/// Fast path for a mutator (`push`/`insert`/`remove`/`remove_at`) whose
/// lvalue is a bare variable — mirrors [`lower_flat_indexed_assignment`]'s
/// Take-based RMW (issue #576): the root is taken (not cloned) only *after*
/// every mutator argument is fully evaluated into its own synthetic temp,
/// since any of them may reference the root by name (e.g. `insert(a, 0,
/// a[0])`); the container fed to
/// `CollectionInsert`/`CollectionRemove`/`SeqRemoveAt` is itself a take
/// from the synthetic root temp, so `array_make_mut`/`map_make_mut` sees a
/// unique `Arc` whenever nothing else aliases the container.
///
/// **Fault-during-RMW slot state**: `push`'s key is always `len(container)`
/// — by construction always a valid insert index — so `push` can only ever
/// fault via `NotIndexable` (the root isn't an array/map at runtime), and
/// this path pre-checks exactly that (the `CollectionLen` read below is
/// non-mutating, so it can't itself trigger a COW) *before* taking the
/// root, giving `push` the same "root is never lost to a fault" guarantee
/// `lower_flat_indexed_assignment` has — and for free, since that same
/// `CollectionLen` read also IS the value `push`'s key needs. `insert`/
/// `remove`/`remove_at` at an arbitrary author-supplied key don't get an
/// equivalent cheap pre-check (validating an arbitrary key/index without mutating
/// would need a dedicated "is this key valid" primitive this issue doesn't
/// add — see the PR's scope notes): a fault there leaves the root holding
/// `Value::Null`, a deliberate, documented, and tested trade-off consistent
/// with this VM's pre-existing no-rollback-on-fault model (a fault
/// anywhere mid-turn already leaves earlier same-turn mutations applied;
/// this extends that same contract to the RMW's own target). See
/// `fault_during_push_leaves_root_unchanged` and
/// `fault_during_insert_leaves_root_null` (runtime crate) for the property
/// tests.
fn lower_bare_mutator(
    kind: MutatorKind,
    root_expr: &hir::Expr,
    args: &[hir::Expr],
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    let Some(root_target) = super::stmts::lower_assign_target(root_expr, ctx) else {
        // Structurally unreachable: `root_expr` is a `hir::Expr::Path`
        // already validated as a resolvable lvalue by `is_lvalue_expr` and
        // (indirectly) `try_lower_mutator_stmt`'s shadow check above —
        // guarded rather than asserted per this module's usual discipline.
        return;
    };

    // Evaluate the mutator's own args (key/value) before touching root —
    // any of them may reference the root by name.
    let arg_slots: Vec<(u16, brink_format::NameId)> = args[1..]
        .iter()
        .map(|a| {
            let v = lower_expr(a, ctx);
            declare_synthetic("__arg", v, ctx, out)
        })
        .collect();

    // `push`'s fault pre-check doubles as its key (see doc above) — read
    // while root is still intact.
    let push_len = matches!(kind, MutatorKind::Push).then(|| {
        declare_synthetic(
            "__len",
            lir::Expr::CollectionLen(Box::new(get_expr_for_target(&root_target))),
            ctx,
            out,
        )
    });

    // Take the root.
    let (c_slot, c_name) = declare_synthetic("__c", take_expr_for_target(&root_target), ctx, out);

    let new_container = match kind {
        MutatorKind::Push => {
            let Some((len_slot, len_name)) = push_len else {
                // Structurally unreachable: `push_len` is always `Some` for
                // `MutatorKind::Push` by the `matches!` guard above.
                return;
            };
            lir::Expr::CollectionInsert {
                base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
                key: Box::new(lir::Expr::GetTemp(len_slot, len_name)),
                value: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
            }
        }
        MutatorKind::Insert => lir::Expr::CollectionInsert {
            base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            key: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
            value: Box::new(lir::Expr::GetTemp(arg_slots[1].0, arg_slots[1].1)),
        },
        MutatorKind::Remove => lir::Expr::CollectionRemove {
            base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            key: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
        MutatorKind::RemoveAt => lir::Expr::SeqRemoveAt {
            base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            index: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
        MutatorKind::Clear => lir::Expr::MapClear(Box::new(lir::Expr::TakeTemp(c_slot, c_name))),
        MutatorKind::Shuffle => {
            lir::Expr::RandShuffle(Box::new(lir::Expr::TakeTemp(c_slot, c_name)))
        }
        MutatorKind::Sort => lir::Expr::SeqSorted(Box::new(lir::Expr::TakeTemp(c_slot, c_name))),
        MutatorKind::SortBy => lir::Expr::SeqSortedBy {
            seq: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            cmp: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
        MutatorKind::HeapPush => lir::Expr::HeapPush {
            seq: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            value: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
    };

    out.push(lir::Stmt::Assign {
        target: lir::AssignTarget::Temp(c_slot, c_name),
        op: AssignOp::Set,
        value: new_container,
    });
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: lir::Expr::TakeTemp(c_slot, c_name),
    });
}

/// Struct-field-projection sibling of [`lower_bare_mutator`] (issue #1495):
/// `push`/`insert`/`remove`/… whose lvalue is a single-level struct-field
/// projection (`push(a.items, v)`, `a: Bag`, `Bag.items: Array<int>`) — a
/// bare `ident.ident` chain, which always parses as one multi-segment
/// `hir::Expr::Path`, never a `hir::Expr::FieldAccess` (see
/// `try_lower_field_assignment`'s doc for why). Follows the identical take →
/// `make_mut` → write-back discipline [`lower_single_level_field_write`]
/// established for `p.field = v`, substituting the mutator's own RMW
/// expansion for a plain value write:
///
/// 1. The mutator's own args (key/value), evaluated once each — mirrors
///    `lower_bare_mutator`'s step 1, since any of them may reference the
///    root or the field by name and both are still intact.
/// 2. `current = root.field`, ALWAYS computed via a non-taking `RecordGet`
///    (the fault pre-check, exactly like `lower_single_level_field_write`'s
///    `current`) — also `push`'s key (`CollectionLen(current)`), read here
///    while the field's current value is still cheaply at hand.
/// 3. The mutator's RMW expansion (`CollectionInsert`/`CollectionRemove`/…)
///    runs against `current`'s temp — an ordinary (cloning) read, not a
///    take: there is no field-level take primitive, only the whole record's.
///    This mirrors `lower_indexed_assignment`'s documented `n > 1` fallback
///    (a nested container is still referenced from inside its parent until
///    the parent's own write-back completes, so a per-field take would buy
///    nothing here either) — the sanctioned §7 fallback, not a regression.
/// 4. The root record is taken, the mutated field written back via
///    `RecordSet`, and the result written back into `root_target` — step 2
///    already proved the field exists on this exact root value, so nothing
///    from here on can fault.
fn lower_field_mutator(
    kind: MutatorKind,
    path: &hir::Path,
    head_info: &crate::symbols::SymbolInfo,
    args: &[hir::Expr],
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) {
    #[expect(
        clippy::indexing_slicing,
        reason = "caller already proved path.segments.len() == 2"
    )]
    let field_name = path.segments[1].text.clone();
    #[expect(
        clippy::indexing_slicing,
        reason = "caller already proved path.segments.len() == 2"
    )]
    let head_name = path.segments[0].text.clone();

    let (root_target, root_shape) = match head_info.kind {
        SymbolKind::Variable | SymbolKind::Constant => (
            lir::AssignTarget::Global(head_info.id),
            ctx.global_shape(head_info.id).map(str::to_string),
        ),
        SymbolKind::Param | SymbolKind::Temp => {
            let Some(slot) = ctx.temp_slot(&head_name) else {
                return;
            };
            let name_id = ctx.names.intern(&head_name);
            (
                lir::AssignTarget::Temp(slot, name_id),
                ctx.temp_shape(slot).map(str::to_string),
            )
        }
        // The caller only reaches here for these four kinds (mirrors
        // `lower_single_level_field_write`'s identical match).
        _ => return,
    };

    let static_offset = if ctx.structs.type_mode == TypeMode::Strict {
        root_shape
            .as_deref()
            .and_then(|s| ctx.structs.shapes.get(s))
            .and_then(|shape| shape.field(&field_name))
            .map(|(offset, _)| offset)
    } else {
        None
    };
    let field = ctx.names.intern(&field_name);

    // 1. The mutator's own args (key/value), evaluated once each — root and
    //    field both still intact.
    let arg_slots: Vec<(u16, brink_format::NameId)> = args[1..]
        .iter()
        .map(|a| {
            let v = lower_expr(a, ctx);
            declare_synthetic("__arg", v, ctx, out)
        })
        .collect();

    // 2. `current = root.field`, ALWAYS computed via a non-taking read (the
    //    fault pre-check, exactly like `lower_single_level_field_write`).
    let current = lir::Expr::RecordGet {
        base: Box::new(get_expr_for_target(&root_target)),
        field,
        static_offset,
    };
    let (current_slot, current_name) = declare_synthetic("__current", current, ctx, out);
    let container = || lir::Expr::GetTemp(current_slot, current_name);

    // `push`'s fault pre-check doubles as its key (see `lower_bare_mutator`'s
    // doc) — read from the field's current value, before anything is taken.
    let push_len = matches!(kind, MutatorKind::Push).then(|| {
        declare_synthetic(
            "__len",
            lir::Expr::CollectionLen(Box::new(container())),
            ctx,
            out,
        )
    });

    let new_field = match kind {
        MutatorKind::Push => {
            let Some((len_slot, len_name)) = push_len else {
                // Structurally unreachable: `push_len` is always `Some` for
                // `MutatorKind::Push` by the `matches!` guard above.
                return;
            };
            lir::Expr::CollectionInsert {
                base: Box::new(container()),
                key: Box::new(lir::Expr::GetTemp(len_slot, len_name)),
                value: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
            }
        }
        MutatorKind::Insert => lir::Expr::CollectionInsert {
            base: Box::new(container()),
            key: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
            value: Box::new(lir::Expr::GetTemp(arg_slots[1].0, arg_slots[1].1)),
        },
        MutatorKind::Remove => lir::Expr::CollectionRemove {
            base: Box::new(container()),
            key: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
        MutatorKind::RemoveAt => lir::Expr::SeqRemoveAt {
            base: Box::new(container()),
            index: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
        MutatorKind::Clear => lir::Expr::MapClear(Box::new(container())),
        MutatorKind::Shuffle => lir::Expr::RandShuffle(Box::new(container())),
        MutatorKind::Sort => lir::Expr::SeqSorted(Box::new(container())),
        MutatorKind::SortBy => lir::Expr::SeqSortedBy {
            seq: Box::new(container()),
            cmp: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
        MutatorKind::HeapPush => lir::Expr::HeapPush {
            seq: Box::new(container()),
            value: Box::new(lir::Expr::GetTemp(arg_slots[0].0, arg_slots[0].1)),
        },
    };

    // 3. Take the root, mutate the field via `RecordSet`, write the
    //    resulting record back.
    let (c_slot, c_name) = declare_synthetic("__c", take_expr_for_target(&root_target), ctx, out);
    out.push(lir::Stmt::Assign {
        target: lir::AssignTarget::Temp(c_slot, c_name),
        op: AssignOp::Set,
        value: lir::Expr::RecordSet {
            base: Box::new(lir::Expr::TakeTemp(c_slot, c_name)),
            field,
            static_offset,
            value: Box::new(new_field),
        },
    });
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: lir::Expr::TakeTemp(c_slot, c_name),
    });
}

/// Resolve an lvalue expression (§5 — "a variable, temp, or indexed path")
/// for a collection mutator's first argument, materializing the same
/// take-chain shape indexed-assignment lowering uses (§4): every index
/// sub-expression evaluated once, left to right, then the container read
/// once at each chain level.
///
/// Returns `root_target` (the ultimate variable to write the mutated
/// container back into), the index temps (empty for a bare variable), and
/// the container-read temps: `c_slots[0]` is the root's current value,
/// `c_slots[k]` is the root indexed by `idx_slots[0..k]`, and
/// `c_slots.last()` — after all `idx_slots.len()` index levels — is the
/// container the mutator itself reads and replaces.
///
/// Contrast [`lower_indexed_assignment`]'s `c_slots`, which stops one level
/// short: an indexed *assignment* only ever needs to read as far as the
/// second-to-last level, since the deepest write is expressed via
/// `IndexSet` directly on that level. A mutator instead needs to read all
/// the way to the fully-indexed value, because that value — not one level
/// up — is the collection being mutated (`push(grid[y], v)` pushes onto the
/// array *at* `grid[y]`, not to some slot of `grid` itself).
///
/// Returns `None` only if `lvalue` isn't a `Path`/`Index` shape, or its root
/// doesn't resolve to an assignable target — both structurally unreachable
/// once the caller has checked [`is_lvalue_expr`] (a genuinely undeclared
/// root is already rejected by the analyzer's E025 before lowering runs).
fn lower_lvalue_container_chain(
    lvalue: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::Stmt>,
) -> Option<LvalueContainerChain> {
    let (root_expr, indices_hir) = match lvalue {
        hir::Expr::Index(idx) => flatten_index_chain(idx),
        hir::Expr::Path(_) => (lvalue, Vec::new()),
        _ => return None,
    };
    let root_target = super::stmts::lower_assign_target(root_expr, ctx)?;

    let idx_slots: Vec<(u16, brink_format::NameId)> = indices_hir
        .iter()
        .map(|e| {
            let v = lower_expr(e, ctx);
            declare_synthetic("__idx", v, ctx, out)
        })
        .collect();

    let mut c_slots: Vec<(u16, brink_format::NameId)> = vec![declare_synthetic(
        "__c",
        get_expr_for_target(&root_target),
        ctx,
        out,
    )];
    for k in 0..idx_slots.len() {
        let base = lir::Expr::GetTemp(c_slots[k].0, c_slots[k].1);
        let index = lir::Expr::GetTemp(idx_slots[k].0, idx_slots[k].1);
        let read = lir::Expr::Index {
            base: Box::new(base),
            index: Box::new(index),
        };
        c_slots.push(declare_synthetic("__c", read, ctx, out));
    }

    Some((root_target, idx_slots, c_slots))
}

/// Cascade the write-back for a mutated container chain built by
/// [`lower_lvalue_container_chain`]: `c_slots.last()` must already hold the
/// mutated value (the caller assigns it there before calling this) — this
/// writes it back up through an `IndexSet` at each index level and finally
/// into `root_target`, mirroring `lower_indexed_assignment`'s steps 5-6. A
/// bare-variable lvalue (`idx_slots` empty) skips straight to the root
/// write.
fn writeback_lvalue_container_chain(
    root_target: lir::AssignTarget,
    idx_slots: &[(u16, brink_format::NameId)],
    c_slots: &[(u16, brink_format::NameId)],
    out: &mut Vec<lir::Stmt>,
) {
    for k in (0..idx_slots.len()).rev() {
        let Some(&(slot, name)) = c_slots.get(k) else {
            return;
        };
        let Some(&(next_slot, next_name)) = c_slots.get(k + 1) else {
            return;
        };
        let base = lir::Expr::GetTemp(slot, name);
        let index = lir::Expr::GetTemp(idx_slots[k].0, idx_slots[k].1);
        let inner = lir::Expr::GetTemp(next_slot, next_name);
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
    let Some(&(root_slot, root_name)) = c_slots.first() else {
        return;
    };
    out.push(lir::Stmt::Assign {
        target: root_target,
        op: AssignOp::Set,
        value: lir::Expr::GetTemp(root_slot, root_name),
    });
}
