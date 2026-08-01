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
//! captures (#872). `Ty::Fn` has carried an effect row since #1680 step 3,
//! but that row names **creation targets** by `DefinitionId`, and a
//! lambda's id is minted right here in LIR — after inference — so a lifted
//! lambda's row is still unrepresentable (#1727), and the pure trio's
//! pure-required contract (`brink_analyzer::comparator_contract`'s E119)
//! stays unable to see through a fn value. That gate checks inline
//! `#fn(target)` callbacks only; a lambda callback is residual, exactly as
//! gradual typing intends, and the runtime's dev-mode write guard
//! (`vm::guard_comparator_write`) remains the backstop. Nothing here fakes
//! that enforcement.
//!
//! There is a second, *structural* half to that gap, and it lives here
//! rather than in the analyzer: the `DefinitionId` this module mints is
//! never a key in the shipped `DefinitionId → row` table. That table
//! (`StoryData::effect_rows`) is populated by `brink-db`'s
//! `populate_effect_rows` from `inferable_defs_from_index`, which is
//! `index.symbols` filtered to `SymbolKind::Knot | SymbolKind::Stitch`; a
//! lambda is an inline `hir::Expr::Lambda`, never an indexed symbol, so it
//! has no `DefKey`/SCC membership and no iteration of that set can ever
//! yield one — the obstacle is the keyspace, not the order the id and the
//! rows are minted in (`IdAllocator::alloc_lambda_address` has already run,
//! by construction, by the time `populate_effect_rows` executes later in
//! the same `story_data_query`). Consequence: effects-spec §7's "a live fn
//! value is a token; its row is a table lookup" has nothing to find for a
//! lambda token — that blocks the shipped-table/§7-narrowing path for
//! lambda tokens (§6 item 4, an optional host optimization) and,
//! conditionally, T1c item 4's row field if that is ruled to be an id
//! reference rather than an inline row. It did not block #1680's own
//! analyzer-side work (rows on `Ty::Fn`, the unifier row join, §6.1
//! row-polymorphism), all of which has landed. Sound today only because
//! `InferPass::infer_lambda`
//! absorbs the body's atoms into the enclosing definition's row
//! (over-reporting, spec §3). Pinned by
//! `brink-db/tests/issue_1680_lambda_effect_row_gap.rs`.

use brink_format::CountingFlags;
use rowan::TextRange;

use crate::hir;
use crate::symbols::SymbolKind;
use crate::{Diagnostic, DiagnosticCode};

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
            // A lifted lambda body is still the enclosing file's source —
            // it inherits the frontend that produced it.
            native: ctx.native,
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

/// The lambda's **captures**: every free name its body reads (as a bare
/// local, a call callee, or the head of a `base.field`/`base.method()`
/// chain) that resolves to a temp/param slot in the enclosing frame, in
/// first-occurrence source order (deterministic — never a hash order).
///
/// A name that is not a local of the enclosing frame is not a capture at
/// all: a module-level `var` is a durable cell reached by name, and a
/// function/knot name is a static reference. Both resolve identically from
/// inside the lifted function, so leaving them alone is correct, not a
/// simplification. This mirrors the rule
/// `hir::lower_native::lambda::check_capture_writes` already enforces
/// lexically for `E156`.
///
/// A free name that fails `ctx.temp_slot` is not automatically "not a
/// local" — the lambda's own not-yet-bound `let` name (a self/recursive
/// reference) resolves to a `Temp`/`Param` in the analyzer's own symbol
/// table but has no slot here, because lifting scans the initializer
/// *before* the enclosing `let` finishes binding. Falling through would
/// leave `lower_call` to target that temp's own `DefinitionId` as a
/// callee — a container that does not exist. `E158` refuses that case
/// loudly instead (issue #1709 review).
fn captured_locals(l: &hir::LambdaExpr, ctx: &mut LowerCtx<'_>) -> Vec<(String, u16)> {
    let mut scan = FreeScan {
        bound: vec![l.params.iter().map(|p| p.name.text.clone()).collect()],
        free: Vec::new(),
    };
    scan.body(&l.body);
    scan.free
        .into_iter()
        .filter_map(|(name, range)| {
            if let Some(slot) = ctx.temp_slot(&name) {
                return Some((name, slot));
            }
            reject_unliftable_capture(&name, range, ctx);
            None
        })
        .collect()
}

/// `E158`: a free name the analyzer resolved to a `Temp`/`Param` of the
/// enclosing frame, but which `ctx.temp_slot` cannot see at this lambda's
/// lifting point (its own not-yet-bound `let` name — recursion/self
/// reference). Everything else that misses `ctx.temp_slot` is a
/// legitimate non-local (a global `var` cell, a knot/function name) and is
/// left alone silently, per `captured_locals`'s own doc.
fn reject_unliftable_capture(name: &str, range: TextRange, ctx: &mut LowerCtx<'_>) {
    let Some(info) = ctx.resolve_path(range) else {
        return;
    };
    if !matches!(info.kind, SymbolKind::Temp | SymbolKind::Param) {
        return;
    }
    ctx.diagnostics.push(Diagnostic {
        file: ctx.file,
        range,
        message: format!(
            "{}: `{name}` is a local the lambda cannot capture here — most likely its own \
             `let` name, read before the `let` finishes binding (recursion is not supported)",
            DiagnosticCode::E158.title(),
        ),
        code: DiagnosticCode::E158,
    });
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
    /// Free names, first-occurrence order, deduped. The `TextRange` is
    /// where that first occurrence was read — `captured_locals` needs it
    /// to resolve an unliftable capture (`E158`) back to a source span.
    free: Vec<(String, TextRange)>,
}

impl FreeScan {
    fn is_bound(&self, name: &str) -> bool {
        self.bound.iter().any(|f| f.iter().any(|n| n == name))
    }

    fn read(&mut self, name: &str, range: TextRange) {
        if !self.is_bound(name) && !self.free.iter().any(|(n, _)| n == name) {
            self.free.push((name.to_string(), range));
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
                // The *head* segment is what can be a frame local,
                // regardless of how many segments follow: a bare
                // `ident.ident` chain (`p.x`, `items.len`) still lowers as
                // one `Expr::Path` — see `hir::Expr::FieldAccess`'s own doc
                // — so `p`/`items` is the name that must resolve, not the
                // path taken as a whole (issue #1709 review: this arm used
                // to require exactly one segment, silently dropping every
                // dotted-field or UFCS-shaped read of a captured local).
                if let Some(head) = p.segments.first() {
                    self.read(&head.text, p.range);
                }
            }
            hir::Expr::Prefix(_, inner) | hir::Expr::Postfix(inner, _) => self.expr(inner),
            hir::Expr::Infix(ie) => {
                self.expr(&ie.lhs);
                self.expr(&ie.rhs);
            }
            hir::Expr::Call(p, args) => {
                // The callee path's head segment is a frame local exactly
                // when the call goes through it: a single-segment callee
                // held in a temp/param (`lower_call`'s `CallVariableTemp`
                // branch — calling a captured fn value) or a multi-segment
                // UFCS callee whose head is the receiver local
                // (`lower_ufcs_call`). A static function/knot name head
                // fails `ctx.temp_slot` in `captured_locals` and is
                // dropped there, same as any other non-local — reading it
                // here is always safe (issue #1709 review: this arm used
                // to skip the callee entirely).
                if let Some(head) = p.segments.first() {
                    self.read(&head.text, p.range);
                }
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
