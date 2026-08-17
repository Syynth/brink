//! F27: condition-position `Option[T]` has no truthiness — E116
//! (`docs/stdlib-spec.md` §1.6, ruled 2026-07-19, issue #1120).
//!
//! The ruling: Option has **no** truthiness. A condition-position
//! `Option[T]` (an `if`/`while` condition, a `{cond: …}` conditional
//! branch, a choice guard, an `await` condition) is a compile error under
//! `types = strict` and a runtime fault under gradual
//! (`RuntimeError::OptionTruthiness`). Authors write `== none` /
//! `== some(x)`, or the `as`-binding (B1b, issue #1475 — see
//! `check_binding_condition` below, which owns its inverse check). This
//! supersedes NS-A1's shipped falsy-none truthiness.
//!
//! Strict-mode-only, mirroring `conversions::check`'s gating and
//! classification posture exactly: wired into `strict::check`, judging only
//! **statically classifiable** conditions through the same inference
//! substrate (`structs::classify_expr_ty` — a param/temp's finalized
//! `BodyTypes::locals`, a global's declaration-derived type, a resolved
//! callee's `InferredSig::return_ty`, an index into a known collection),
//! plus the two condition shapes that classification can't see but the
//! Option package owns outright: a direct call to an unresolved
//! Option-returning stdlib intrinsic (`{find(s, "x"): …}` — membership from
//! [`crate::infer::intrinsic_returns_option`], the same table
//! `infer::body::InferPass::infer_intrinsic`'s typing arms implement) and
//! the bare unresolved `none` literal. Whenever the resolved type is
//! `Unknown`/`Conflicted` or the shape isn't handled, the condition stays
//! silently unchecked — "Unknown never disagrees" — and the runtime fault
//! remains the backstop that still catches every case at execution time.
//!
//! `{expr: - val: …}` switch *case* values are compared with `==`, not
//! evaluated for truthiness — neither the scrutinee nor the case values are
//! condition positions themselves, so neither is ever routed through
//! `check_condition`/`check_condition_or_binding`. Both are still walked
//! for an embedded lambda literal, though (issue #2764) — a lambda's own
//! block body can hold a real condition even though the position it sits
//! in cannot.
//!
//! A condition is *reached* at any expression depth a lambda literal can
//! sit at — a VAR/CONST default, a temp initializer, an assignment, a
//! return value, a divert/tunnel/thread-start argument, a content
//! interpolation, a switch scrutinee/case value, or a condition expression
//! itself can all hold a lambda whose own `|…| { … }` block body has a
//! condition; [`walk_expr_for_lambdas`] finds every such lambda and hands
//! its block statements to [`check_block_stmt`] (issue #2764, mirroring
//! `protocols.rs`'s identical lambda-descent fix for E113, issue #1773).
//! Reaching it is not the same as *classifying* it, though: the same
//! **statically classifiable** gate above still applies, now sourced from
//! the enclosing def's locals with the lambda's own bindings pruned out
//! ([`pruned_locals_for_lambda`]) — a name the lambda itself binds (its own
//! param, or a name its block introduces) cannot be classified from that
//! lookup and stays silently unchecked, exactly like any other
//! unclassifiable shape; only a *captured* outer local/global/intrinsic
//! call inside the lambda body gets the identical check a top-level
//! condition gets.
//!
//! **Every container `HirFile` exposes that can hold a condition, walked
//! exhaustively (issue #2772, the third gap found in this same walk after
//! #2764/PR #2768's other two — `Expr` descent and the VAR/CONST scan):**
//! `hir.knots` (each `Knot`, function or not per `is_function`, plus every
//! `stitch` on it), `hir.variables`/`hir.constants` (initializer values,
//! for an embedded lambda body only — see above), and `hir.root_content`
//! (file-scope content before the first knot/stitch header — this fix).
//! The remaining `HirFile` fields (`lists`, `structs`, `externals`,
//! `includes`, `module`, `imports`, `visibility`, `was_directives`,
//! `allow_scopes`, `element_matches`, `cue_names`, `native`,
//! `claim_handlers`) carry no `Stmt`/`Expr` tree of their own and so can
//! never hold a condition or a lambda body — `native` is a bare `bool` and
//! `claim_handlers` is a `Vec<ClaimHandlerDecl>` whose own fields are only
//! `Name`/`TextRange`/`Vec<String>`/`String`/`bool`/`i64`/`Option<String>`
//! (`crates/internal/brink-ir/src/hir/types.rs`) — there is nothing left in
//! `HirFile` for this walk to reach.

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Choice, ChoiceSet, CondKind, Conditional, Content, ContentPart, Diagnostic,
    DiagnosticCode, ElseBranch, Expr, FileId, HirFile, IfStmt, LambdaBody, PrefixOp, ResolutionMap,
    Stmt, StringPart, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, Ty};
use crate::structs::{self, MistypeCtx};

/// Strict-mode-only condition-position Option checks over every truthiness
/// condition in the project. Callers only reach this once
/// `strict::config_error` has confirmed `types = strict` + `dialect =
/// brink` (mirrors `conversions::check`'s entry condition).
#[must_use]
pub(crate) fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    // No manifest access, mirroring `conversions::check`'s own note — an
    // Option type never originates from a `Handle<K>` manifest vocabulary.
    let globals = crate::infer::collect_globals(files, index, None);
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        // File-scope `VAR`/`CONST` initializers (issue #2764): no enclosing
        // knot/stitch here, so no `locals` — mirrors `protocols.rs`'s own
        // VAR/CONST walk. A declaration's own value is never itself a
        // condition position, but it can hold a lambda literal whose own
        // block body is one (the issue's own repro shape:
        // `var f = |x| { if x { 0 } else { 1 } }`) — `check` never looked at
        // `hir.variables`/`hir.constants` at all before this fix, so this
        // position was unreachable regardless of lambda-body descent.
        let file_scope_ctx = MistypeCtx {
            index,
            globals: &globals,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            locals: None,
        };
        for var in &hir.variables {
            walk_expr_for_lambdas(&var.value, file, &file_scope_ctx, &mut out);
        }
        for cst in &hir.constants {
            walk_expr_for_lambdas(&cst.value, file, &file_scope_ctx, &mut out);
        }
        // File-scope root content (issue #2772): the statements sitting
        // before the first knot/stitch header. `check()` never walked
        // `hir.root_content` at all before this fix, mirroring
        // `protocols.rs::check_reserved_names`'s own
        // `walk_stmts(&hir.root_content.stmts, …)` call, which is the
        // template this walk is missing relative to. Unlike the VAR/CONST
        // walk above, root content is *not* a bare declaration value — it's
        // real executable content that can declare its own `~ temp` locals
        // and reference them in a condition (the issue's own repro shape),
        // so `locals: None` would be wrong here: it would leave every
        // root-scope temp unclassifiable. Instead this looks up root
        // content's own inferred locals via `infer::root_content_def_id`
        // (issue #1903, factored out for #2772 so this and `strict.rs`'s
        // own root-content lookups share one derivation rather than each
        // re-deriving the id inline) — `collect_defs` synthesizes that same
        // id for inference, so a body-level check must look it up under
        // the identical scheme.
        let root_locals = if hir.root_content.stmts.is_empty() {
            None
        } else {
            let synthetic_id = crate::infer::root_content_def_id(file);
            inference.bodies.get(&synthetic_id).map(|b| &b.locals)
        };
        let root_ctx = MistypeCtx {
            index,
            globals: &globals,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            locals: root_locals,
        };
        check_block(&hir.root_content, file, &root_ctx, &mut out);
        for knot in &hir.knots {
            let kind = knot.symbol_kind();
            let knot_locals = annotations::def_id_for(index, file, kind, &knot.name.text)
                .and_then(|id| inference.bodies.get(&id))
                .map(|b| &b.locals);
            let ctx = MistypeCtx {
                index,
                globals: &globals,
                signatures: &inference.signatures,
                resolution_by_range: &resolution_by_range,
                locals: knot_locals,
            };
            check_block(&knot.body, file, &ctx, &mut out);
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                let stitch_locals =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                        .and_then(|id| inference.bodies.get(&id))
                        .map(|b| &b.locals);
                let ctx = MistypeCtx {
                    index,
                    globals: &globals,
                    signatures: &inference.signatures,
                    resolution_by_range: &resolution_by_range,
                    locals: stitch_locals.or(knot_locals),
                };
                check_block(&stitch.body, file, &ctx, &mut out);
            }
        }
    }
    out
}

fn check_block(block: &Block, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    for stmt in &block.stmts {
        check_stmt(stmt, file, ctx, out);
    }
}

fn check_stmt(stmt: &Stmt, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    match stmt {
        Stmt::Content(c) => check_content(c, file, ctx, out),
        Stmt::Conditional(c) => check_conditional(c, file, ctx, out),
        Stmt::ChoiceSet(cs) => check_choice_set(cs, file, ctx, out),
        Stmt::LabeledBlock(b) => check_block(b, file, ctx, out),
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                check_block(&branch.body, file, ctx, out);
            }
        }
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                check_block_stmt(bs, file, ctx, out);
            }
        }
        // `~ await <cond>`: the runtime re-evaluates the condition for
        // truthiness to decide when to wake — condition position.
        Stmt::Await(a) => {
            if let Some(cond) = &a.condition {
                walk_expr_for_lambdas(cond, file, ctx, out);
                check_condition(cond, a.ptr.text_range(), file, ctx, out);
            }
        }
        // Issue #2764 (same family as #1773/#2762): none of these positions
        // is itself a condition, but each can hold a lambda literal whose
        // own block body has one — `check_stmt`/`check_block_stmt` never
        // inspected any `Expr` at all before this fix, so a condition
        // inside a lambda's own body was unreachable regardless of where
        // the lambda sat.
        Stmt::Divert(d) => {
            for arg in &d.target.args {
                walk_expr_for_lambdas(arg, file, ctx, out);
            }
        }
        Stmt::TunnelCall(tc) => {
            for target in &tc.targets {
                for arg in &target.args {
                    walk_expr_for_lambdas(arg, file, ctx, out);
                }
            }
        }
        Stmt::ThreadStart(ts) => {
            for arg in &ts.target.args {
                walk_expr_for_lambdas(arg, file, ctx, out);
            }
        }
        Stmt::TempDecl(t) => {
            if let Some(v) = &t.value {
                walk_expr_for_lambdas(v, file, ctx, out);
            }
        }
        Stmt::Assignment(a) => {
            walk_expr_for_lambdas(&a.target, file, ctx, out);
            walk_expr_for_lambdas(&a.value, file, ctx, out);
        }
        Stmt::Return(r) => {
            if let Some(v) = &r.value {
                walk_expr_for_lambdas(v, file, ctx, out);
            }
            for arg in &r.onwards_args {
                walk_expr_for_lambdas(arg, file, ctx, out);
            }
        }
        // Issue #2108: an attach handler's call is not a condition
        // position — no `Option[T]` truthiness check applies here, same as
        // `ExprStmt`. Still walked for an embedded lambda's own body.
        Stmt::ExprStmt(e) | Stmt::AttachElement(e) => walk_expr_for_lambdas(e, file, ctx, out),
        Stmt::EndOfLine | Stmt::EndElementRun => {}
    }
}

fn check_content(c: &Content, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    for part in &c.parts {
        check_content_part(part, file, ctx, out);
    }
}

fn check_content_part(
    part: &ContentPart,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    match part {
        ContentPart::InlineConditional(cond) => check_conditional(cond, file, ctx, out),
        ContentPart::InlineSequence(s) => {
            for branch in &s.branches {
                check_block(&branch.body, file, ctx, out);
            }
        }
        // A span can nest a conditional (§4.3's nesting doctrine — markup
        // and logic nest freely inside each other), so a mistyped
        // condition inside one is still reachable and must still be
        // checked.
        ContentPart::Span(span) => {
            for child in &span.children {
                check_content_part(child, file, ctx, out);
            }
        }
        // Issue #2764: not itself a condition position, but can hold a
        // lambda whose own block body has one.
        ContentPart::Interpolation(e) => walk_expr_for_lambdas(e, file, ctx, out),
        ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
    }
}

fn check_conditional(
    c: &Conditional,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    // A switch's case values are `==`-compared against the scrutinee, never
    // truthiness-evaluated (see the module doc) — only branch *bodies* are
    // recursed for truthiness-check purposes on a `CondKind::Switch`.
    let conditions_are_truthiness = !matches!(c.kind, CondKind::Switch(_));
    // Issue #2764 review finding: a switch's own scrutinee and each branch's
    // case-value expression are still reachable expression positions a
    // lambda literal can sit at, even though neither is ever
    // truthiness-checked itself — lambda descent only, never routed through
    // `check_condition`/`check_condition_or_binding` (case values are
    // `==`-compared, not truthiness-evaluated).
    if let CondKind::Switch(scrutinee) = &c.kind {
        walk_expr_for_lambdas(scrutinee, file, ctx, out);
    }
    for branch in &c.branches {
        if conditions_are_truthiness && let Some(cond) = &branch.condition {
            check_condition_or_binding(
                cond,
                branch.binding.as_ref(),
                c.ptr.text_range(),
                file,
                ctx,
                out,
            );
        } else if let Some(case_value) = &branch.condition {
            walk_expr_for_lambdas(case_value, file, ctx, out);
        }
        check_block(&branch.body, file, ctx, out);
    }
}

fn check_choice_set(cs: &ChoiceSet, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    for choice in &cs.choices {
        check_choice(choice, file, ctx, out);
    }
    check_block(&cs.continuation, file, ctx, out);
}

fn check_choice(choice: &Choice, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    if let Some(cond) = &choice.condition {
        // Same routing as `check_if`/`check_while`: a guard `as` binding
        // (issue #1508) makes an `Option[T]` condition exactly what the
        // author is supposed to write, so `E116` must not fire — `E147`
        // takes its place for a statically non-Option condition.
        check_condition_or_binding(
            cond,
            choice.binding.as_ref(),
            choice.ptr.text_range(),
            file,
            ctx,
            out,
        );
    }
    if let Some(c) = &choice.start_content {
        check_content(c, file, ctx, out);
    }
    if let Some(c) = &choice.bracket_content {
        check_content(c, file, ctx, out);
    }
    if let Some(c) = &choice.inner_content {
        check_content(c, file, ctx, out);
    }
    check_block(&choice.body, file, ctx, out);
}

fn check_block_stmt(bs: &BlockStmt, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    match bs {
        BlockStmt::If(i) => check_if(i, file, ctx, out),
        BlockStmt::While(w) => {
            // A plain `while` and a `while await` both truthiness-evaluate
            // their condition (the wake contract: waking IS condition-true).
            check_condition_or_binding(
                &w.condition,
                w.binding.as_ref(),
                w.ptr.text_range(),
                file,
                ctx,
                out,
            );
            for s in &w.body {
                check_block_stmt(s, file, ctx, out);
            }
        }
        BlockStmt::For(f) => {
            walk_expr_for_lambdas(&f.iterable, file, ctx, out);
            for s in &f.body {
                check_block_stmt(s, file, ctx, out);
            }
        }
        BlockStmt::Await(a) => {
            if let Some(cond) = &a.condition {
                walk_expr_for_lambdas(cond, file, ctx, out);
                check_condition(cond, a.ptr.text_range(), file, ctx, out);
            }
        }
        // Issue #2764: mirrors `check_stmt`'s identical wiring — none of
        // these is a condition position itself, but each can hold a lambda
        // whose own block body has one.
        BlockStmt::TempDecl(t) => {
            if let Some(v) = &t.value {
                walk_expr_for_lambdas(v, file, ctx, out);
            }
        }
        BlockStmt::Assignment(a) => {
            walk_expr_for_lambdas(&a.target, file, ctx, out);
            walk_expr_for_lambdas(&a.value, file, ctx, out);
        }
        BlockStmt::Return(r) => {
            if let Some(v) = &r.value {
                walk_expr_for_lambdas(v, file, ctx, out);
            }
            for arg in &r.onwards_args {
                walk_expr_for_lambdas(arg, file, ctx, out);
            }
        }
        BlockStmt::ExprStmt(e) => walk_expr_for_lambdas(e, file, ctx, out),
        BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
    }
}

/// Issue #2764 (same family as #1773/#2762's `protocols.rs::walk_expr_for_lambdas`):
/// find every `Expr::Lambda` reachable from `expr` — including nested
/// arbitrarily deep inside another expression, and nested inside a
/// lambda's *own* body — and, for each one found whose body is a braced
/// block, check that block's own statements for Option-truthiness
/// conditions the exact same way [`check_block_stmt`] checks a top-level
/// knot/stitch body's statements. A single-expression lambda body
/// (`|x| x.next()`) has no statement to check, only further nested
/// expressions to search — handled via the plain recursive walk below via
/// `LambdaBody::Expr`'s own arm, not `LambdaBody::Block`'s.
///
/// Deliberately does not use `LambdaBody::all_exprs()` for the `Block`
/// case: that helper flattens an `if`/`while` condition down to a bare
/// `Expr`, losing the "this is a condition position" tag `check_block_stmt`
/// needs to route it through [`check_condition_or_binding`] rather than
/// treating it as an arbitrary expression. Handing the block's real
/// `BlockStmt`s to `check_block_stmt` keeps that position information —
/// and, since `check_block_stmt` now itself calls back into this function
/// for every expr-bearing position it visits (the wiring immediately
/// above), a lambda nested inside *this* lambda's own body is found too,
/// without a second, parallel recursion.
///
/// Mirrors the shape of `protocols.rs`'s own walker (and `hir::visit`'s
/// `walk_expr`) — every `Expr` variant that can hold a nested expression is
/// descended; the only ones skipped (`Int`/`Float`/`Bool`/`Null`/`Path`/
/// `DivertTarget`/`ListLiteral`) are leaves that can never contain a lambda
/// literal.
fn walk_expr_for_lambdas(
    expr: &Expr,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    match expr {
        Expr::Lambda(l) => match &l.body {
            LambdaBody::Block { stmts, tail } => {
                // Issue #2764 review finding: a lambda-own binding (a param,
                // or a name the block itself introduces via `TempDecl`/`for`/
                // an `if`/`while` `as` binding) that shadows a same-named
                // outer local must not be typed as the *outer* binding while
                // this block's own statements are checked —
                // `structs::resolved_symbol_ty` resolves a Param/Temp by
                // bare name out of `ctx.locals`, with no shadowing frame of
                // its own, so an unpruned `ctx` here would misclassify the
                // inner name as whatever the enclosing def's finalized
                // `BodyTypes::locals` says the outer name is. Pruning those
                // names out of the ctx handed to this block's own checks
                // makes them fall back to "Unknown never disagrees" instead,
                // same as any other unclassifiable shape.
                let pruned_locals = pruned_locals_for_lambda(l, stmts, ctx);
                let pruned_ctx = MistypeCtx {
                    index: ctx.index,
                    globals: ctx.globals,
                    signatures: ctx.signatures,
                    resolution_by_range: ctx.resolution_by_range,
                    locals: Some(&pruned_locals),
                };
                for bs in stmts {
                    check_block_stmt(bs, file, &pruned_ctx, out);
                }
                if let Some(t) = tail {
                    walk_expr_for_lambdas(t, file, &pruned_ctx, out);
                }
            }
            LambdaBody::Expr(e) => walk_expr_for_lambdas(e, file, ctx, out),
        },
        Expr::Call(_path, args) => {
            for arg in args {
                walk_expr_for_lambdas(arg, file, ctx, out);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => {
            walk_expr_for_lambdas(inner, file, ctx, out);
        }
        Expr::Infix(ie) => {
            walk_expr_for_lambdas(&ie.lhs, file, ctx, out);
            walk_expr_for_lambdas(&ie.rhs, file, ctx, out);
        }
        Expr::String(s) => {
            for part in &s.parts {
                if let StringPart::Interpolation(e) = part {
                    walk_expr_for_lambdas(e, file, ctx, out);
                }
            }
        }
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                walk_expr_for_lambdas(e, file, ctx, out);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, v) in &m.entries {
                walk_expr_for_lambdas(k, file, ctx, out);
                walk_expr_for_lambdas(v, file, ctx, out);
            }
        }
        Expr::Index(idx) => {
            walk_expr_for_lambdas(&idx.base, file, ctx, out);
            walk_expr_for_lambdas(&idx.index, file, ctx, out);
        }
        Expr::StructLiteral(sl) => {
            for (_name, val) in &sl.fields {
                walk_expr_for_lambdas(val, file, ctx, out);
            }
        }
        Expr::FieldAccess(fa) => walk_expr_for_lambdas(&fa.base, file, ctx, out),
        // T1c `#fn(target, args…)`: the target is a static path, not an
        // `Expr` child (same shape as `Call`'s path) — only bound args
        // descend.
        Expr::FnLiteral(fl) => {
            for arg in &fl.args {
                walk_expr_for_lambdas(arg, file, ctx, out);
            }
        }
        Expr::RefArg(ra) => walk_expr_for_lambdas(&ra.operand, file, ctx, out),
        Expr::Range(r) => {
            walk_expr_for_lambdas(&r.start, file, ctx, out);
            walk_expr_for_lambdas(&r.end, file, ctx, out);
        }
        // Block-capture fragment (issue #1839): not constructible from
        // surface syntax, but it embeds real `Stmt`s the ordinary weave
        // walk already knows how to visit — reuse `check_stmt` rather than
        // growing a second statement vocabulary here.
        Expr::Fragment(stmts) => {
            for s in stmts {
                check_stmt(s, file, ctx, out);
            }
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => {}
    }
}

/// Issue #2764 review finding: the name set to prune from `ctx.locals`
/// before checking a lambda's own block body — its own param names, plus
/// every name the block itself binds (`TempDecl`, a `for` loop's var/val, an
/// `if`/`while` `as` binding), recursed through nested `if`/`while`/`for` the
/// same way [`crate::infer::lambda_own_bindings`] already does for
/// `infer_lambda`'s own shadow set.
///
/// Issue #2782: also seeds the lambda's own explicitly `: T`-annotated
/// param types directly into the returned map — mirrors
/// `infer::body::InferPass::infer_lambda`'s own `self.annotated` seed
/// (issue #1941), just written into the bare-name-keyed locals map this
/// check's classification (`resolved_symbol_ty`'s `ctx.locals?.get(…)`)
/// actually reads, instead of the parallel `self.annotated` map that seed
/// feeds. Without it, a lambda param's written annotation never reached
/// E116's truthiness check: only a body-inferred type did (the
/// ordinary-`fn`-param half of the same gap is fixed at its source, in
/// `infer::body::infer_def_body`, since a lambda has no `BodyTypes` entry
/// of its own for this check to read in the first place). A param name the
/// lambda's own block *re-binds* (a fresh `TempDecl`/`if`/`while`/`for`
/// binding of the same spelling) is excluded from the seed — `body_names`
/// is collected before the param loop below adds param names to `own_names`,
/// so it reflects only the block's own re-binds, exactly like
/// `infer_lambda`'s identical `body_bound_names.contains` guard.
///
/// No longer bottoms out to nothing for a file-scope lambda (`ctx.locals`
/// itself `None`, e.g. a `var f = |x: Option<int>| { … }` initializer): an
/// annotated param must still be classifiable there, so this always
/// returns a real map, falling back to an empty pruned base when there was
/// no enclosing `locals` to prune from — the caller's `ctx.locals?` (via
/// `Some(&map)`) treats an empty map and `None` identically for any name
/// absent from it, so this is not a behavior change for the pre-existing
/// pruning path. Plain `BTreeMap` return (not `Option`-wrapped): every path
/// now produces a real map, so wrapping it would just be
/// `clippy::unnecessary_wraps`.
fn pruned_locals_for_lambda(
    l: &brink_ir::LambdaExpr,
    stmts: &[BlockStmt],
    ctx: &MistypeCtx<'_>,
) -> BTreeMap<String, Ty> {
    let mut body_names: BTreeMap<String, (TextRange, Option<brink_ir::TypeExpr>)> = BTreeMap::new();
    crate::infer::lambda_own_bindings(stmts, &mut body_names);
    let body_bound_names: BTreeSet<String> = body_names.keys().cloned().collect();

    let mut own_names = body_names;
    for p in &l.params {
        own_names
            .entry(p.name.text.clone())
            .or_insert((p.name.range, None));
    }

    let mut pruned: BTreeMap<String, Ty> = ctx.locals.map_or_else(BTreeMap::new, |locals| {
        locals
            .iter()
            .filter(|(name, _)| !own_names.contains_key(*name))
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect()
    });

    let type_names = annotations::TypeNames::new(ctx.index, None);
    for p in &l.params {
        if body_bound_names.contains(&p.name.text) {
            continue;
        }
        if let Some(te) = &p.annotation
            && let Some(ty) = annotations::resolve(te, &type_names)
        {
            pruned.insert(p.name.text.clone(), ty);
        }
    }

    pruned
}

fn check_if(i: &IfStmt, file: FileId, ctx: &MistypeCtx<'_>, out: &mut Vec<Diagnostic>) {
    check_condition_or_binding(
        &i.condition,
        i.binding.as_ref(),
        i.ptr.text_range(),
        file,
        ctx,
        out,
    );
    for s in &i.body {
        check_block_stmt(s, file, ctx, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => check_if(inner, file, ctx, out),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                check_block_stmt(s, file, ctx, out);
            }
        }
        None => {}
    }
}

/// Route a condition to the right check: with an `as` binding (B1b, issue
/// #1475) an `Option[T]` condition is exactly what the author is *supposed*
/// to write, so F27's `E116` must not fire — the binding is the third
/// explicit spelling the F27 ruling named, next to `== none`/`== some(x)`.
/// The inverse check takes its place: an `as` over a statically known
/// **non**-Option has nothing to unwrap (`E147`).
fn check_condition_or_binding(
    cond: &Expr,
    binding: Option<&brink_ir::Name>,
    fallback_range: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    // Issue #2764: every condition position funnels through here (or
    // through the two `Await` call sites, which walk it themselves) — one
    // walk finds a lambda embedded *within* the condition expression itself
    // (independent of the binding/non-binding classification below).
    walk_expr_for_lambdas(cond, file, ctx, out);
    match binding {
        Some(_) => check_binding_condition(cond, fallback_range, file, ctx, out),
        None => check_condition(cond, fallback_range, file, ctx, out),
    }
}

/// `E147`: an `as` binding whose condition is statically classifiable and
/// is not an `Option[T]`.
///
/// Gated exactly like `check_condition`'s `E116`: only a *classifiable*
/// type is judged. `Unknown`/`Conflicted` — and any shape the
/// classification substrate can't see — stay silent ("Unknown never
/// disagrees"), with `RuntimeError::AsBindingNotOption` as the backstop.
fn check_binding_condition(
    cond: &Expr,
    fallback_range: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    if condition_is_option(cond, ctx) {
        return;
    }
    let Some(ty) = structs::classify_expr_ty(cond, ctx) else {
        return;
    };
    if matches!(ty, Ty::Unknown | Ty::Conflicted) {
        return;
    }
    out.push(Diagnostic {
        file,
        range: expr_anchor(cond).unwrap_or(fallback_range),
        message: format!(
            "{}: the `as` binding unwraps an `Option[T]`, but this condition is `{}`",
            DiagnosticCode::E147.title(),
            ty.display(),
        ),
        code: DiagnosticCode::E147,
    });
}

/// Check one truthiness condition. `fallback_range` anchors the diagnostic
/// when the condition expression carries no own range (most `Expr` shapes
/// don't) — the enclosing construct's span, same posture as
/// `await_purity`'s E105 anchor.
///
/// `not <cond>` recurses: the VM's `Not` opcode truthiness-evaluates its
/// operand through the same `is_truthy` path, so `{not r: …}` over an
/// Option `r` is the identical fault shape.
fn check_condition(
    cond: &Expr,
    fallback_range: TextRange,
    file: FileId,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    if let Expr::Prefix(PrefixOp::Not, inner) = cond {
        check_condition(inner, fallback_range, file, ctx, out);
        return;
    }
    if !condition_is_option(cond, ctx) {
        return;
    }
    out.push(Diagnostic {
        file,
        range: expr_anchor(cond).unwrap_or(fallback_range),
        // `DiagnosticCode::E116.title()` already carries the full boilerplate
        // ("an `Option[T]` has no truthiness — test `== none` / `==
        // some(x)`") — this format! supplies only the diagnostic-specific
        // detail beyond that (the F27/spec citation), not a repeat of the
        // same sentence (issue #2774: it used to repeat verbatim right after
        // the title, doubling the whole message).
        message: format!(
            "{} (F27, docs/stdlib-spec.md §1.6)",
            DiagnosticCode::E116.title()
        ),
        code: DiagnosticCode::E116,
    });
}

/// Whether `cond`'s type is statically known to be `Option[T]` — the
/// inference-substrate classification first, then the two shapes it can't
/// see (see the module doc): an unresolved (builtin, not author-shadowed)
/// call to an Option-returning intrinsic, and the bare unresolved `none`
/// literal.
fn condition_is_option(cond: &Expr, ctx: &MistypeCtx<'_>) -> bool {
    match cond {
        Expr::Call(path, _) => {
            if let [seg] = path.segments.as_slice()
                && !ctx.resolution_by_range.contains_key(&range_key(path.range))
            {
                return crate::infer::intrinsic_returns_option(&seg.text);
            }
            matches!(structs::classify_expr_ty(cond, ctx), Some(Ty::Option(_)))
        }
        Expr::Path(p) => {
            if let [seg] = p.segments.as_slice()
                && seg.text == "none"
                && !ctx.resolution_by_range.contains_key(&range_key(p.range))
            {
                return true;
            }
            matches!(structs::classify_expr_ty(cond, ctx), Some(Ty::Option(_)))
        }
        _ => matches!(structs::classify_expr_ty(cond, ctx), Some(Ty::Option(_))),
    }
}

/// A best-effort own-range for a condition expression, for diagnostic
/// anchoring — the shapes that carry a source range (a path, a call's
/// callee path, and the roots reachable through unary/index/field
/// wrappers). `None` falls back to the enclosing construct's span.
fn expr_anchor(expr: &Expr) -> Option<TextRange> {
    match expr {
        Expr::Path(p) => Some(p.range),
        Expr::Call(path, _) => Some(path.range),
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => expr_anchor(inner),
        Expr::Index(idx) => expr_anchor(&idx.base),
        Expr::FieldAccess(fa) => expr_anchor(&fa.base),
        _ => None,
    }
}

fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `conversions::resolution_index`.
fn resolution_index(
    resolutions: &ResolutionMap,
    file: FileId,
) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| (range_key(r.range), r.target))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;

    /// Real resolutions + a whole-project [`InferenceResult`] — mirrors
    /// `conversions::tests::build_with_inference`.
    fn check_all(src: &str) -> Vec<Diagnostic> {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    #[test]
    fn option_temp_in_inline_conditional_guard_is_e116() {
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r: found.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Issue #2774: `DiagnosticCode::E116.title()` already carries "an
    /// `Option[T]` has no truthiness ... test `== none` / `== some(x)`", and
    /// `check_condition`'s own `format!` used to repeat that exact phrase
    /// verbatim right after it, doubling the whole message. Reproduces on
    /// this same top-level positive-control fixture as
    /// [`option_temp_in_inline_conditional_guard_is_e116`] above — pinned
    /// separately here so a regression that reintroduces the duplicate
    /// phrase fails on message content, not just on `.code`.
    #[test]
    fn e116_message_is_not_doubled() {
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r: found.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        let message = &diags[0].message;
        assert_eq!(
            message.matches("has no truthiness").count(),
            1,
            "E116 message repeats its core sentence: {message:?}"
        );
        assert_eq!(
            message.matches("== none").count(),
            1,
            "E116 message repeats the `== none` idiom: {message:?}"
        );
    }

    #[test]
    fn direct_option_intrinsic_call_in_condition_is_e116() {
        let diags = check_all("=== main ===\n{find(\"ab\", \"b\"): found.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Issue #2772: third gap in this same walk. `option_conditions::check`
    /// never visited `hir.root_content` at all, so a condition on an
    /// `Option[T]` value sitting in file-scope content *before the first
    /// knot* got no E116, while the byte-identical condition sitting
    /// inside a knot (see
    /// [`option_temp_in_inline_conditional_guard_is_e116`], which this
    /// mirrors exactly, just with the `~ temp`/conditional pair moved
    /// above the first `=== main ===` header) fired correctly. Positive
    /// control: root content must now fire E116 exactly like knot content
    /// does.
    #[test]
    fn root_content_condition_on_option_is_e116() {
        let diags =
            check_all("~ temp r = find(\"ab\", \"b\")\n{r: found.}\n=== main ===\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Negative control alongside
    /// [`root_content_condition_on_option_is_e116`] (mirrors
    /// `int_truthiness_idiom_stays_clean`): a non-Option root-content
    /// condition must not start firing just because the walk now reaches
    /// it.
    #[test]
    fn root_content_non_option_condition_stays_clean() {
        let diags = check_all("~ temp n = 3\n{n: nonzero.}\n=== main ===\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Issue #2772's own literal repro shape (review finding): a
    /// **file-scope global** `VAR … : Option<T> = none` conditioned on in
    /// root content, not a `~ temp`. [`root_content_condition_on_option_is_e116`]
    /// above only exercises the `root_locals` half of this fix (a local
    /// declared *within* root content); a condition on a project-wide
    /// global reads through `ctx.globals` instead (`condition_is_option`'s
    /// `structs::classify_expr_ty` fallback), which was already reachable
    /// before this fix via `infer::collect_globals` — but `check()` never
    /// walked `hir.root_content` at all, so the condition *statement*
    /// itself was unreachable regardless of which lookup would have
    /// classified it. Pins that the globals-through-root-content path gets
    /// covered too, not just the new `root_locals` path.
    #[test]
    fn root_content_global_option_condition_is_e116() {
        let diags =
            check_all("VAR opt: Option<int> = none\n{opt: has value.}\n=== main ===\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn option_temp_in_choice_guard_is_e116() {
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n* {r} [go] Went.\n- -> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn option_temp_in_block_if_condition_is_e116() {
        let diags = check_all(
            "=== main ===\n~ {\n    temp r = find(\"ab\", \"b\")\n    if r {\n        \
             return\n    }\n}\nHi.\n-> END\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn negated_option_condition_is_e116() {
        // `not r` truthiness-evaluates `r` through the same VM path.
        let diags =
            check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n{not r: absent.}\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn explicit_none_comparison_is_clean() {
        let diags = check_all(
            "=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r == none: absent.}\n\
             {r == some(1): at one.}\n-> END\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_truthiness_idiom_stays_clean() {
        // The `{visited_knot: …}` idiom survives — F27 bans Option only.
        let diags = check_all("=== main ===\n~ temp n = 3\n{n: nonzero.}\n-> END\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unknown_typed_condition_stays_silent() {
        // An unclassifiable condition (an unused param) never flags —
        // "Unknown never disagrees"; the runtime fault is the backstop.
        let diags = check_all("=== main(r) ===\n{r: yes.}\n-> END\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn switch_case_values_are_not_condition_positions() {
        // `{n: - 1: one - else: other}` case values are `==`-compared, not
        // truthiness-evaluated — never flagged, even with Option around.
        let diags = check_all(
            "=== main ===\n~ temp n = 2\n{n:\n- 1: one\n- 2: two\n- else: other\n}\n-> END\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn option_returning_user_function_call_in_condition_is_e116() {
        let diags = check_all(
            "=== function probe() ===\n~ return find(\"ab\", \"b\")\n\
             === main ===\n{probe(): found.}\n-> END\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn await_condition_of_option_type_is_e116() {
        let diags = check_all("=== main ===\n~ temp r = find(\"ab\", \"b\")\n~ await r\n-> END\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    fn check_all_native(src: &str) -> Vec<Diagnostic> {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, manifest, _diag) = brink_ir::hir::lower_native::lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    /// Issue #2764: same family as #1773/#2762, in this file's own E116
    /// walk — an `if` condition on an `Option[T]`-returning intrinsic call
    /// sitting inside a lambda's own block body was never reached, unlike
    /// the identical condition at top level. Positive control alongside
    /// [`lambda_body_condition_on_option_is_e116`]/
    /// [`var_lambda_body_condition_on_option_is_e116`]: the top-level
    /// version must keep firing exactly as before.
    #[test]
    fn top_level_condition_on_option_is_e116_native() {
        let diags = check_all_native(
            "fn heal(x) {\n  if find(\"ab\", \"b\") {\n    return 0;\n  } else {\n    return 1;\n  }\n}\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// The lambda sits inside a knot/fn body's own `~ temp`-equivalent
    /// (`let`) — reached via [`check_block_stmt`]'s `TempDecl` arm now
    /// descending into the value expression to find the lambda, then
    /// checking its block body's own statements the same way a top-level
    /// body is checked.
    #[test]
    fn lambda_body_condition_on_option_is_e116() {
        let diags = check_all_native(
            "fn heal(x) {\n  let f = |y| {\n    if find(\"ab\", \"b\") {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// The issue's own literal repro shape: a *file-scope* `var`'s lambda
    /// default. `check` didn't even walk `hir.variables`/`hir.constants`
    /// before this fix — the lambda-descent fix alone doesn't reach this
    /// position without also driving `walk_expr_for_lambdas` over each
    /// declaration's initializer.
    #[test]
    fn var_lambda_body_condition_on_option_is_e116() {
        let diags = check_all_native(
            "var f = |y| {\n  if find(\"ab\", \"b\") {\n    0\n  } else {\n    1\n  }\n};\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Negative control (mirrors `int_truthiness_idiom_stays_clean`): a
    /// non-Option condition inside a lambda body must not start firing just
    /// because the walk now reaches it.
    #[test]
    fn lambda_body_non_option_condition_stays_clean() {
        let diags = check_all_native(
            "fn heal(x) {\n  let f = |y| {\n    if y {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Issue #2764 review finding (BLOCKING false positive): a lambda PARAM
    /// that shadows an outer same-named `Option[T]` local must stay clean —
    /// `resolved_symbol_ty` resolves a Param/Temp by *bare name* out of the
    /// enclosing def's finalized `BodyTypes::locals`, and `infer_lambda`
    /// shadows-then-restores every lambda-own name, so an unpruned ctx would
    /// misclassify the inner `r` (an untyped int-ish param) as the outer
    /// `r`'s `Option[T]`. Reproduced pre-fix: this yielded a hard E116 at
    /// PR #2768 head (`07740e1b`) over the explicitly-legal truthiness
    /// idiom (`int_truthiness_idiom_stays_clean`'s own shape, just on a
    /// shadowing param instead of a plain int temp).
    #[test]
    fn lambda_param_shadowing_outer_option_stays_clean() {
        let diags = check_all_native(
            "fn heal(x) {\n  let r = some(3);\n  let f = |r| {\n    if r {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Same family as
    /// [`lambda_param_shadowing_outer_option_stays_clean`], but the
    /// shadowing binding is a `TempDecl` the lambda's own block introduces
    /// (not a param) — pinned separately because `pruned_locals_for_lambda`
    /// sources param names and [`crate::infer::lambda_own_bindings`]'s
    /// block-introduced names from two different places.
    #[test]
    fn lambda_own_temp_shadowing_outer_option_stays_clean() {
        let diags = check_all_native(
            "fn heal(x) {\n  let r = some(3);\n  let f = |q| {\n    let r = 5;\n    if r {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Issue #2764 review finding: `check_conditional` never walked a
    /// switch's own scrutinee or case-value expressions for an embedded
    /// lambda literal before this fix — `conditions_are_truthiness`
    /// short-circuited before any descent into either position. Both are
    /// hand-built HIR fixtures (`Provenance::synthetic`, sanctioned by
    /// [`brink_ir::LambdaExpr::container_id`]'s own doc for exactly this
    /// case) rather than parsed native source: the native `{match …}`
    /// grammar is the only source-level way to *produce* a
    /// `CondKind::Switch` at all (ink has no switch syntax and no lambda
    /// syntax — see [`Expr::Lambda`]'s own doc), and its bare-expression
    /// arm-body grammar (`arm.bare_expr()`, `family.rs::match_arm`) has an
    /// unrelated pre-existing parse-error quirk on prose-punctuated arm
    /// bodies that would make a source-parsed fixture fragile evidence for
    /// *this* fix specifically.
    ///
    /// A single fixture lambda — `|q| { if find("ab", "b") { return } }` —
    /// is reused in the scrutinee position by one test and the case-value
    /// position by another, isolating which position the walk reaches.
    fn option_lambda_in_condition_position() -> Expr {
        let range = TextRange::new(0.into(), 1.into());
        let find_call = Expr::Call(
            brink_ir::Path {
                segments: vec![brink_ir::Name {
                    text: "find".to_string(),
                    range,
                }],
                range,
                crosses_module_wall: false,
            },
            Vec::new(),
        );
        let if_stmt = IfStmt {
            ptr: brink_ir::Provenance::synthetic(brink_ir::NodeClass::If, range),
            condition: find_call,
            binding: None,
            body: vec![BlockStmt::Return(brink_ir::Return {
                ptr: None,
                kind: brink_ir::ReturnKind::Explicit,
                value: None,
                onwards_args: Vec::new(),
            })],
            else_branch: None,
        };
        Expr::Lambda(Box::new(brink_ir::LambdaExpr {
            ptr: brink_ir::Provenance::synthetic(brink_ir::NodeClass::Lambda, range),
            params: Vec::new(),
            return_type: None,
            body: LambdaBody::Block {
                stmts: vec![BlockStmt::If(if_stmt)],
                tail: None,
            },
            container_id: None,
        }))
    }

    /// Owned backing storage for an empty `MistypeCtx` — enough for
    /// [`option_lambda_in_condition_position`]'s fixture, whose `find(...)`
    /// call classifies as `Option[T]` through `condition_is_option`'s
    /// unresolved-intrinsic shape (`index`/`resolution_by_range` never in
    /// play), same as the parsed-source tests' `find(...)` calls. A named
    /// struct (rather than a four-tuple) so clippy's `type_complexity`
    /// lint stays clean at the call sites below.
    struct EmptyCtxParts {
        index: std::sync::Arc<SymbolIndex>,
        globals: BTreeMap<DefinitionId, Ty>,
        signatures: BTreeMap<DefinitionId, crate::infer::InferredSig>,
        resolution_by_range: BTreeMap<(u32, u32), DefinitionId>,
    }

    fn empty_ctx() -> EmptyCtxParts {
        let (index, _diag) = crate::symbol_index(&[]);
        EmptyCtxParts {
            index,
            globals: BTreeMap::new(),
            signatures: BTreeMap::new(),
            resolution_by_range: BTreeMap::new(),
        }
    }

    #[test]
    fn switch_scrutinee_lambda_condition_is_e116() {
        let parts = empty_ctx();
        let ctx = MistypeCtx {
            index: parts.index.as_ref(),
            globals: &parts.globals,
            signatures: &parts.signatures,
            resolution_by_range: &parts.resolution_by_range,
            locals: None,
        };
        let range = TextRange::new(0.into(), 1.into());
        let conditional = Conditional {
            ptr: brink_ir::Provenance::synthetic(brink_ir::NodeClass::Conditional, range),
            kind: CondKind::Switch(option_lambda_in_condition_position()),
            branches: Vec::new(),
        };
        let mut out = Vec::new();
        check_conditional(&conditional, FileId(0), &ctx, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].code, DiagnosticCode::E116);
    }

    #[test]
    fn switch_case_value_lambda_condition_is_e116() {
        let parts = empty_ctx();
        let ctx = MistypeCtx {
            index: parts.index.as_ref(),
            globals: &parts.globals,
            signatures: &parts.signatures,
            resolution_by_range: &parts.resolution_by_range,
            locals: None,
        };
        let range = TextRange::new(0.into(), 1.into());
        let branch = brink_ir::CondBranch {
            ptr: brink_ir::Provenance::synthetic(brink_ir::NodeClass::Conditional, range),
            condition: Some(option_lambda_in_condition_position()),
            binding: None,
            body: Block::from_stmts(Vec::new()),
            container_id: None,
        };
        let conditional = Conditional {
            ptr: brink_ir::Provenance::synthetic(brink_ir::NodeClass::Conditional, range),
            // A non-lambda scrutinee — isolates that the diagnostic below
            // came from the case-value walk, not the scrutinee walk
            // [`switch_scrutinee_lambda_condition_is_e116`] already covers.
            kind: CondKind::Switch(Expr::Int(0)),
            branches: vec![branch],
        };
        let mut out = Vec::new();
        check_conditional(&conditional, FileId(0), &ctx, &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].code, DiagnosticCode::E116);
    }

    // ── Issue #2782: an explicitly `: Option<T>`-annotated param ─────────
    //
    // Only an *inference-derived* `Option[T]` (the tests above — `find(...)`
    // results, `some(...)` locals) reached E116's classification before this
    // fix. A param whose type came from a *written* annotation instead
    // never did, for both an ordinary `fn` param
    // (`infer::body::infer_def_body`'s `pass.locals` never got the
    // annotation overlay `param_types` already applied to `InferredSig`)
    // and a lambda's own param (`pruned_locals_for_lambda` pruned it out of
    // the enclosing scope but never seeded it back in from the lambda's own
    // annotation). Confirmed as the live bug pre-fix: both
    // `annotated_fn_param_option_condition_is_e116`'s and
    // `annotated_lambda_param_option_condition_is_e116`'s fixtures produced
    // zero diagnostics at this issue's filing.

    #[test]
    fn annotated_fn_param_option_condition_is_e116() {
        let diags = check_all_native(
            "fn heal(x: Option<int>): int {\n  if x {\n    return 0;\n  } else {\n    return 1;\n  }\n}\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Negative control alongside
    /// [`annotated_fn_param_option_condition_is_e116`]: an annotated
    /// **non**-Option param must not start firing just because annotated
    /// params now reach classification at all.
    #[test]
    fn annotated_fn_param_non_option_condition_stays_clean() {
        let diags = check_all_native(
            "fn heal(x: int): int {\n  if x {\n    return 0;\n  } else {\n    return 1;\n  }\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// The `fn`-param half of the #2773-hazard family, mirroring
    /// [`lambda_param_own_annotation_re_bound_by_body_stays_clean`]: an
    /// ordinary `fn` param's own written annotation must NOT be seeded onto
    /// a *different* binding the body re-introduces via a fresh same-spelled
    /// `let`. `infer::body::infer_def_body`'s `pass.locals` overlay used to
    /// key on the stored type being `Unknown` — which can't distinguish "no
    /// entry was ever written for this name" from "the re-bound temp's own
    /// entry is legitimately `Unknown`" — so it clobbered the re-bound `x`
    /// (a plain `int` copy of `y`) with the outer param's `Option<int>`
    /// annotation, firing a false E116. Fixed to key on `contains_key`
    /// instead, matching docs/typed-mode-spec.md §2's RULED #1912 firewall.
    #[test]
    fn fn_param_own_annotation_re_bound_by_body_stays_clean() {
        let diags = check_all_native(
            "fn heal(x: Option<int>, y: int): int {\n  let x = y;\n  if x {\n    return 0;\n  } else {\n    return 1;\n  }\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// The lambda-param half of issue #2782 — not lambda-specific in cause,
    /// but a separate fix site (`pruned_locals_for_lambda`) since a lambda
    /// has no `BodyTypes` entry of its own for the enclosing-def fix above
    /// to reach.
    #[test]
    fn annotated_lambda_param_option_condition_is_e116() {
        let diags = check_all_native(
            "fn heal(n: int): int {\n  let f = |x: Option<int>| {\n    if x {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return n;\n}\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Negative control alongside
    /// [`annotated_lambda_param_option_condition_is_e116`].
    #[test]
    fn annotated_lambda_param_non_option_condition_stays_clean() {
        let diags = check_all_native(
            "fn heal(n: int): int {\n  let f = |x: int| {\n    if x {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return n;\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Issue #2773's own hazard, re-checked against this fix specifically:
    /// `pruned_locals_for_lambda` now writes annotation-derived types into
    /// the same bare-name-keyed map #2773 warned is shadowing-unsafe. This
    /// pins that a lambda param's *own* annotation — Option here — is what
    /// governs its body, never an outer same-named local's own (different,
    /// non-Option) type: the outer `r` is a plain `int` temp, the inner `r`
    /// is an annotated `Option<int>` lambda param that shadows it, and the
    /// condition must classify against the inner annotation and fire E116
    /// — the pre-fix pruning already stopped the outer `int` from leaking
    /// in (see `lambda_param_shadowing_outer_option_stays_clean`, the
    /// mirror-image direction), so this pins that the *new* annotation seed
    /// added by this fix doesn't accidentally skip the shadowing param
    /// entirely and leave it unclassified instead.
    #[test]
    fn lambda_param_own_annotation_shadowing_outer_non_option_local_is_e116() {
        let diags = check_all_native(
            "fn heal(x) {\n  let r = 5;\n  let f = |r: Option<int>| {\n    if r {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// Same #2773-hazard family, the exclusion half: a lambda param's own
    /// annotation must NOT be seeded for a name the lambda's own block body
    /// re-binds via a fresh same-spelled `let` — mirrors
    /// `infer::body::InferPass::infer_lambda`'s identical
    /// `body_bound_names.contains` guard on its own `self.annotated` seed.
    /// Without this exclusion, the annotated Option param's type would leak
    /// into the *re-bound* `r` (a plain `int` local the block introduces
    /// itself), producing a false E116 on a condition that is actually
    /// unclassified (any annotation this check doesn't independently prove
    /// governs the fresh binding).
    #[test]
    fn lambda_param_own_annotation_re_bound_by_body_stays_clean() {
        let diags = check_all_native(
            "fn heal(x) {\n  let f = |r: Option<int>| {\n    let r = 5;\n    if r {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// `pruned_locals_for_lambda` was changed from `Option<BTreeMap<..>>`
    /// to a plain map specifically so a *file-scope* lambda — one whose
    /// enclosing `ctx.locals` is itself `None`, e.g. this top-level `var`'s
    /// default — stays classifiable from its own annotation. All the other
    /// tests in this #2782 family nest the lambda inside a `fn` body,
    /// exercising only the pruning path (`ctx.locals: Some(..)`); this pins
    /// the `ctx.locals: None` path the pruning fix's `map_or_else` branch
    /// covers, mirroring [`var_lambda_body_condition_on_option_is_e116`].
    #[test]
    fn var_lambda_own_annotation_option_condition_is_e116() {
        let diags = check_all_native(
            "var f = |x: Option<int>| {\n  if x {\n    0\n  } else {\n    1\n  }\n};\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }

    /// The genuine capture case, pinned so the pruning fix above doesn't
    /// overreach: a lambda that reads an outer `Option[T]` local *without*
    /// shadowing it (no param/temp/loop/`as`-binding of the same name in the
    /// lambda's own body) must still fire E116 exactly once.
    #[test]
    fn lambda_captured_outer_option_condition_is_e116() {
        let diags = check_all_native(
            "fn heal(x) {\n  let r = some(3);\n  let f = |q| {\n    if r {\n      return 0;\n    } else {\n      return 1;\n    }\n  };\n  return 0;\n}\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E116);
    }
}
