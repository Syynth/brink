//! **Lambda lifting** — turning a `hir::Expr::Lambda` into a real runtime
//! function value (issue #1709).
//!
//! Issue #1685 landed the HIR half: the native surface's `|x| expr` parses
//! and lowers to [`hir::Expr::Lambda`]. It stopped there — an anonymous
//! body had no runtime representation, so LIR lowering raised a targeted
//! `E052` codegen fence. That fence is what this module retires.
//!
//! ## The shape
//!
//! The runtime's only fn value is T1c's [`lir::Expr::MakeFnValue`] over a
//! *named* target: zero bound args codegen to `PushFnRef` (a `VAL_FN_REF`),
//! a bound prefix to `MakeClosure` (a `VAL_CLOSURE` carrying a
//! `{name, is_ref, payload}` captured environment). Lifting rides that
//! existing wire shape rather than inventing a parallel one:
//!
//! 1. the lambda's **free locals** — names its body reads that resolve to a
//!    temp/param slot in the *enclosing* frame — become the leading
//!    parameters of a synthesized function;
//! 2. the lambda's own params follow, in source order;
//! 3. the lambda expression itself becomes a `MakeFnValue` whose target is
//!    the synthesized function and whose bound row is one `GetTemp` read
//!    per capture.
//!
//! So a capture is bound exactly the way `#fn(f, a)` binds `a`: evaluated
//! **once, at the creation site**, in the enclosing frame, and carried in
//! the closure environment. That *is* the 2026-07-19 ruling's "capture is
//! BY-VALUE always" — there is no capture mode to choose, no `move`
//! keyword, and no ref capture: every bound entry is a
//! [`lir::CallArg::Value`], so the `is_ref` bit the runtime validates on
//! rehydration is `false` for every capture by construction. (Writing to a
//! captured binding is already rejected at HIR lowering — `E156`,
//! `hir::lower_native::lambda::check_capture_writes` — so a snapshot can
//! never be silently written through here.)
//!
//! ## Where the synthesized function lives
//!
//! As a top-level container, a sibling of the project's function knots —
//! not nested inside the frame that created it. A container is only ever
//! entered by an explicit `EnterContainer`/`Goto`/call, never by falling
//! off the end of a parent, so a top-level lifted function is unreachable
//! except through its own fn value. Its identity is content-derived
//! (`{enclosing scope path}.#lambda-{start offset}`), so a fresh
//! `IdAllocator` in a per-chunk salsa memo mints the same `DefinitionId`
//! the whole-project walk does (the FG-4d history-independence gate).
//!
//! ## What lifting still cannot express
//!
//! Effect rows. Lambdas are fn-colored always and rows compose through
//! captures (#872), but `Ty::Fn` carries no rows at all today (#1680) — so
//! a lifted lambda's row is unrepresentable, and the pure trio's
//! pure-required contract (`brink_analyzer::comparator_contract`'s E119)
//! stays unable to see through a fn value. That gate checks inline
//! `#fn(target)` callbacks only; a lambda callback is residual, exactly as
//! gradual typing intends, and the runtime's dev-mode write guard
//! (`vm::guard_comparator_write`) remains the backstop. Nothing here fakes
//! that enforcement.

use brink_format::CountingFlags;

use crate::hir;

use super::context::{LowerCtx, TempMap};
use super::lir;

/// Lower `|params| body` to a function value (issue #1709).
///
/// Synthesizes the lifted function into [`LowerCtx::lifted`] and returns
/// the creation-site expression — see the module doc for the shape.
pub(super) fn lower_lambda(l: &hir::LambdaExpr, ctx: &mut LowerCtx<'_>) -> lir::Expr {
    let captures = captured_locals(l, ctx);

    // The lifted param row: captures (the bound prefix `MakeClosure`
    // fills) then the lambda's own params, in source order.
    let mut param_names: Vec<String> = captures.iter().map(|(n, _)| n.clone()).collect();
    for p in &l.params {
        param_names.push(p.name.text.clone());
    }

    let mut temps = TempMap::new();
    for (i, name) in param_names.iter().enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a lambda's captures + params never approach u16::MAX"
        )]
        temps.insert(name.clone(), i as u16);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a lambda's captures + params never approach u16::MAX"
    )]
    let mut block_slot = param_names.len() as u16;

    let params: Vec<lir::Param> = param_names
        .iter()
        .enumerate()
        .map(|(i, name)| lir::Param {
            name: ctx.names.intern(name),
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a lambda's captures + params never approach u16::MAX"
            )]
            slot: i as u16,
            // By-value always (RULED 2026-07-19): no ref captures in v1,
            // and the native grammar accepts no `ref`/`->` lambda param.
            is_ref: false,
            is_divert: false,
        })
        .collect();

    let (id, path) = ctx.ids.alloc_lambda_address(&lambda_scope_path(l, ctx));

    let borrowed_names: Vec<&str> = param_names.iter().map(String::as_str).collect();
    let (body, children) = {
        let mut lctx = LowerCtx {
            file: ctx.file,
            resolutions: ctx.resolutions,
            index: ctx.index,
            temps: &temps,
            names: ctx.names,
            ids: ctx.ids,
            scope_path: path.clone(),
            is_root_content_scope: false,
            pending_children: Vec::new(),
            visible_temps: borrowed_names.iter().map(|s| (*s).to_string()).collect(),
            file_paths: ctx.file_paths,
            root_id: ctx.root_id,
            choice_gather_target: None,
            next_block_slot: &mut block_slot,
            block_scopes: Vec::new(),
            as_binding_slots: crate::determinism::LookupSet::new(),
            block_scoped_temp_names: crate::determinism::LookupSet::new(),
            diagnostics: ctx.diagnostics,
            loop_depth: 0,
            structs: ctx.structs,
            temp_shapes: crate::determinism::LookupMap::new(),
            tables: ctx.tables,
            lifted: ctx.lifted,
        };
        let body = lower_body(&l.body, &mut lctx);
        (body, core::mem::take(&mut lctx.pending_children))
    };

    ctx.lifted.push(lir::Container {
        id,
        name: Some(path),
        kind: lir::ContainerKind::Knot,
        params,
        body,
        children,
        counting_flags: CountingFlags::empty(),
        temp_slot_count: block_slot,
        labeled: false,
        inline: false,
        // A lifted lambda *is* a function: it is entered by a call, returns
        // a value, and (like every `== function ==` knot) its children are
        // stored without inklecate's implicit `.0` stitch prefix.
        is_function: true,
        local: false,
    });

    lir::Expr::MakeFnValue {
        target: id,
        bound: captures
            .into_iter()
            .map(|(name, slot)| lir::Expr::GetTemp(slot, ctx.names.intern(&name)))
            .map(lir::CallArg::Value)
            .collect(),
    }
}

/// The scope-relative path the lifted function is addressed by.
///
/// Content-derived: the enclosing knot/stitch path plus the lambda's own
/// source start offset. Two lambdas cannot start at the same offset in the
/// same file, and an enclosing scope path is unique project-wide (root
/// content is qualified per file by the allocator's own prefix — #1504), so
/// this is unique without any allocation-order counter, which is what keeps
/// a per-chunk salsa memo byte-identical to the whole-project walk.
fn lambda_scope_path(l: &hir::LambdaExpr, ctx: &LowerCtx<'_>) -> String {
    let offset = u32::from(l.ptr.text_range().start());
    if ctx.scope_path.is_empty() {
        format!("#lambda-{offset}")
    } else {
        format!("{}.#lambda-{offset}", ctx.scope_path)
    }
}

/// Lower a lambda body into the lifted function's statement list.
///
/// Both ruled spellings end in a `Return` carrying the body's value — "last
/// expression is the value". A braced body whose block ends in a statement
/// has no trailing value expression; its value comes from an explicit
/// `return` inside the block (which leaves the lambda), so no synthetic
/// terminal `Return` is appended in that case.
fn lower_body(body: &hir::LambdaBody, ctx: &mut LowerCtx<'_>) -> Vec<lir::Stmt> {
    match body {
        hir::LambdaBody::Expr(e) => {
            let value = super::expr::lower_expr(e, ctx);
            vec![value_return(value)]
        }
        hir::LambdaBody::Block { stmts, tail } => {
            ctx.push_block_scope();
            let mut out = super::blocks::lower_block_stmt_list(stmts, ctx);
            if let Some(t) = tail {
                let value = super::expr::lower_expr(t, ctx);
                out.push(value_return(value));
            }
            ctx.pop_block_scope();
            out
        }
    }
}

fn value_return(value: lir::Expr) -> lir::Stmt {
    lir::Stmt::Return {
        value: Some(value),
        is_tunnel: false,
        args: Vec::new(),
    }
}

/// The lambda's **captures**: every free single-segment name its body reads
/// that resolves to a temp/param slot in the enclosing frame, in
/// first-occurrence source order (deterministic — never a hash order).
///
/// A name that is not a local of the enclosing frame is not a capture at
/// all: a module-level `var` is a durable cell reached by name, and a
/// function/knot name is a static reference. Both resolve identically from
/// inside the lifted function, so leaving them alone is correct, not a
/// simplification. This mirrors the rule
/// `hir::lower_native::lambda::check_capture_writes` already enforces
/// lexically for `E156`.
fn captured_locals(l: &hir::LambdaExpr, ctx: &LowerCtx<'_>) -> Vec<(String, u16)> {
    let mut scan = FreeScan {
        bound: vec![l.params.iter().map(|p| p.name.text.clone()).collect()],
        free: Vec::new(),
    };
    scan.body(&l.body);
    scan.free
        .into_iter()
        .filter_map(|name| ctx.temp_slot(&name).map(|slot| (name, slot)))
        .collect()
}

/// Free-name collection over a lambda body, with a binder scope stack.
///
/// Mirrors `hir::visit::walk_expr`'s descent, but tracks what each
/// construct *binds* so an inner `let`/`for`/`as`/nested-lambda param is
/// not mistaken for a capture of a same-named outer local. Both halves
/// matter: missing a binder invents a spurious capture parameter (and, with
/// it, a bogus shadow warning), while missing a read drops a real capture
/// and silently reads a global instead.
struct FreeScan {
    /// Innermost-last stack of binder frames.
    bound: Vec<Vec<String>>,
    /// Free names, first-occurrence order, deduped.
    free: Vec<String>,
}

impl FreeScan {
    fn is_bound(&self, name: &str) -> bool {
        self.bound.iter().any(|f| f.iter().any(|n| n == name))
    }

    fn read(&mut self, name: &str) {
        if !self.is_bound(name) && !self.free.iter().any(|n| n == name) {
            self.free.push(name.to_string());
        }
    }

    fn bind(&mut self, name: &str) {
        if let Some(frame) = self.bound.last_mut() {
            frame.push(name.to_string());
        }
    }

    fn body(&mut self, body: &hir::LambdaBody) {
        match body {
            hir::LambdaBody::Expr(e) => self.expr(e),
            hir::LambdaBody::Block { stmts, tail } => {
                self.bound.push(Vec::new());
                self.stmts(stmts);
                if let Some(t) = tail {
                    self.expr(t);
                }
                self.bound.pop();
            }
        }
    }

    fn stmts(&mut self, stmts: &[hir::BlockStmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    /// A nested block (an `if`/`while`/`for` body) opens its own binder
    /// frame — a `let` inside it stops shadowing once the block closes.
    fn nested(&mut self, stmts: &[hir::BlockStmt]) {
        self.bound.push(Vec::new());
        self.stmts(stmts);
        self.bound.pop();
    }

    fn stmt(&mut self, stmt: &hir::BlockStmt) {
        match stmt {
            hir::BlockStmt::TempDecl(t) => {
                // The initializer is evaluated *before* the name binds, so
                // `let x = x` reads the outer `x`.
                if let Some(e) = &t.value {
                    self.expr(e);
                }
                self.bind(&t.name.text);
            }
            hir::BlockStmt::Assignment(a) => {
                self.expr(&a.target);
                self.expr(&a.value);
            }
            hir::BlockStmt::Return(r) => {
                if let Some(e) = &r.value {
                    self.expr(e);
                }
                for a in &r.onwards_args {
                    self.expr(a);
                }
            }
            hir::BlockStmt::If(i) => self.if_stmt(i),
            hir::BlockStmt::While(w) => {
                self.expr(&w.condition);
                self.bound.push(Vec::new());
                if let Some(b) = &w.binding {
                    self.bind(&b.text);
                }
                self.stmts(&w.body);
                self.bound.pop();
            }
            hir::BlockStmt::For(f) => {
                self.expr(&f.iterable);
                self.bound.push(Vec::new());
                self.bind(&f.var_name.text);
                if let Some(v) = &f.val_name {
                    self.bind(&v.text);
                }
                self.stmts(&f.body);
                self.bound.pop();
            }
            hir::BlockStmt::ExprStmt(e) => self.expr(e),
            hir::BlockStmt::Await(a) => {
                if let Some(c) = &a.condition {
                    self.expr(c);
                }
            }
            hir::BlockStmt::Break(_) | hir::BlockStmt::Continue(_) => {}
        }
    }

    fn if_stmt(&mut self, i: &hir::IfStmt) {
        self.expr(&i.condition);
        self.bound.push(Vec::new());
        if let Some(b) = &i.binding {
            self.bind(&b.text);
        }
        self.stmts(&i.body);
        self.bound.pop();
        match &i.else_branch {
            Some(hir::ElseBranch::ElseIf(nested)) => self.if_stmt(nested),
            Some(hir::ElseBranch::Else(stmts)) => self.nested(stmts),
            None => {}
        }
    }

    fn expr(&mut self, e: &hir::Expr) {
        match e {
            hir::Expr::Path(p) => {
                // A multi-segment path (`knot.stitch`, `list.item`) is a
                // static reference, never a frame local.
                if p.segments.len() == 1 {
                    self.read(&p.segments[0].text);
                }
            }
            hir::Expr::Prefix(_, inner) | hir::Expr::Postfix(inner, _) => self.expr(inner),
            hir::Expr::Infix(ie) => {
                self.expr(&ie.lhs);
                self.expr(&ie.rhs);
            }
            hir::Expr::Call(_, args) => {
                for a in args {
                    self.expr(a);
                }
            }
            hir::Expr::String(s) => {
                for part in &s.parts {
                    if let hir::StringPart::Interpolation(inner) = part {
                        self.expr(inner);
                    }
                }
            }
            hir::Expr::ArrayLiteral(a) => {
                for el in &a.elements {
                    self.expr(el);
                }
            }
            hir::Expr::MapLiteral(m) => {
                for (k, v) in &m.entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            hir::Expr::Index(idx) => {
                self.expr(&idx.base);
                self.expr(&idx.index);
            }
            hir::Expr::StructLiteral(sl) => {
                for (_, v) in &sl.fields {
                    self.expr(v);
                }
            }
            hir::Expr::FieldAccess(fa) => self.expr(&fa.base),
            hir::Expr::FnLiteral(fl) => {
                for a in &fl.args {
                    self.expr(a);
                }
            }
            hir::Expr::RefArg(ra) => self.expr(&ra.operand),
            // A nested lambda's own params bind inside it; everything it
            // reads beyond them is a read of *this* lambda's frame, and so
            // transitively a capture of this one too.
            hir::Expr::Lambda(inner) => {
                self.bound
                    .push(inner.params.iter().map(|p| p.name.text.clone()).collect());
                self.body(&inner.body);
                self.bound.pop();
            }
            hir::Expr::Range(r) => {
                self.expr(&r.start);
                self.expr(&r.end);
            }
            hir::Expr::Int(_)
            | hir::Expr::Float(_)
            | hir::Expr::Bool(_)
            | hir::Expr::Null
            | hir::Expr::DivertTarget(_)
            | hir::Expr::ListLiteral(_) => {}
        }
    }
}
